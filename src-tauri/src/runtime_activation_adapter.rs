//! Runtime lifecycle adapter boundary and a concrete command-host adapter.
//!
//! The command adapter uses a fixed executable and fixed arguments supplied at
//! registration time. Execution intent is serialized to standard input; it is
//! never interpolated into a shell command.

use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, RwLock},
};

use serde::Serialize;
use thiserror::Error;

use crate::{
    runtime_adapter::{RuntimeAdapter, RuntimeAdapterError},
    runtime_domain::{
        CapabilitySupport, RuntimeAvailability, RuntimeCapabilityStatus, RuntimeDescriptor,
        RuntimeId, RuntimeProbe,
    },
    runtime_execution::{
        RuntimeExecutionAdapter, RuntimeExecutionError, RuntimeInvocation, RuntimeInvocationOutput,
    },
    runtime_instance_domain::{RuntimeInstance, RuntimeInstanceId, RuntimeInstanceLifecycle},
    runtime_session::{
        RuntimeSessionAdapter, RuntimeSessionError, RuntimeSessionHandle, RuntimeSessionId,
    },
};

const DEFAULT_MAX_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum RuntimeActivationAdapterError {
    #[error(transparent)]
    Adapter(#[from] RuntimeAdapterError),
    #[error("Runtime activation adapter is already registered: {0}")]
    AlreadyRegistered(RuntimeId),
    #[error("Runtime activation adapter is not registered: {0}")]
    NotRegistered(RuntimeId),
    #[error("Runtime activation adapter registry lock failed: {0}")]
    RegistryLock(String),
    #[error("Runtime instance {instance_id} does not match adapter Runtime {runtime_id}")]
    RuntimeMismatch {
        instance_id: RuntimeInstanceId,
        runtime_id: RuntimeId,
    },
    #[error("Runtime instance must be Activating before adapter activation")]
    InvalidActivationState,
    #[error("Runtime instance is not active in adapter: {0}")]
    InstanceNotActive(RuntimeInstanceId),
    #[error("Runtime {runtime_id} is already activated as instance {instance_id}")]
    AlreadyActive {
        runtime_id: RuntimeId,
        instance_id: RuntimeInstanceId,
    },
    #[error("Runtime {0} is unavailable for activation")]
    RuntimeUnavailable(RuntimeId),
    #[error("Command Runtime configuration is invalid: {0}")]
    InvalidCommandSpec(String),
    #[error("Command Runtime host failed: {0}")]
    Host(String),
}

pub trait RuntimeLifecycleAdapter: RuntimeExecutionAdapter {
    fn activate(
        &self,
        instance: &RuntimeInstance,
    ) -> Result<RuntimeProbe, RuntimeActivationAdapterError>;
    fn health(
        &self,
        instance_id: &RuntimeInstanceId,
    ) -> Result<RuntimeProbe, RuntimeActivationAdapterError>;
    fn deactivate(
        &self,
        instance_id: &RuntimeInstanceId,
    ) -> Result<(), RuntimeActivationAdapterError>;
}

pub trait RuntimeLifecycleAdapterRepository: Send + Sync {
    fn register(
        &self,
        adapter: Arc<dyn RuntimeLifecycleAdapter>,
    ) -> Result<(), RuntimeActivationAdapterError>;
    fn get(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Option<Arc<dyn RuntimeLifecycleAdapter>>, RuntimeActivationAdapterError>;
}

#[derive(Clone, Default)]
pub struct InMemoryRuntimeLifecycleAdapterRepository {
    adapters: Arc<RwLock<HashMap<RuntimeId, Arc<dyn RuntimeLifecycleAdapter>>>>,
}

impl RuntimeLifecycleAdapterRepository for InMemoryRuntimeLifecycleAdapterRepository {
    fn register(
        &self,
        adapter: Arc<dyn RuntimeLifecycleAdapter>,
    ) -> Result<(), RuntimeActivationAdapterError> {
        adapter
            .descriptor()
            .validate()
            .map_err(RuntimeAdapterError::from)?;
        let runtime_id = adapter.descriptor().runtime_id().clone();
        let mut adapters = self
            .adapters
            .write()
            .map_err(|error| RuntimeActivationAdapterError::RegistryLock(error.to_string()))?;
        if adapters.contains_key(&runtime_id) {
            return Err(RuntimeActivationAdapterError::AlreadyRegistered(runtime_id));
        }
        adapters.insert(runtime_id, adapter);
        Ok(())
    }

    fn get(
        &self,
        runtime_id: &RuntimeId,
    ) -> Result<Option<Arc<dyn RuntimeLifecycleAdapter>>, RuntimeActivationAdapterError> {
        let adapters = self
            .adapters
            .read()
            .map_err(|error| RuntimeActivationAdapterError::RegistryLock(error.to_string()))?;
        Ok(adapters.get(runtime_id).cloned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRuntimeSpec {
    executable: PathBuf,
    execution_args: Vec<String>,
    probe_args: Vec<String>,
    working_directory: Option<PathBuf>,
    inherited_environment_keys: Vec<String>,
    max_output_bytes: usize,
}

impl CommandRuntimeSpec {
    pub fn new(
        executable: PathBuf,
        execution_args: Vec<String>,
        probe_args: Vec<String>,
        working_directory: Option<PathBuf>,
        inherited_environment_keys: Vec<String>,
        max_output_bytes: Option<usize>,
    ) -> Result<Self, RuntimeActivationAdapterError> {
        if executable.as_os_str().is_empty() || !executable.is_absolute() {
            return Err(RuntimeActivationAdapterError::InvalidCommandSpec(
                "executable must be an absolute path".into(),
            ));
        }
        if execution_args.iter().any(|arg| arg.contains('\0'))
            || probe_args.iter().any(|arg| arg.contains('\0'))
        {
            return Err(RuntimeActivationAdapterError::InvalidCommandSpec(
                "argument contains a null byte".into(),
            ));
        }
        let mut environment_keys = HashSet::new();
        for key in &inherited_environment_keys {
            if key.trim().is_empty()
                || key.contains('=')
                || key.contains('\0')
                || !environment_keys.insert(key)
            {
                return Err(RuntimeActivationAdapterError::InvalidCommandSpec(
                    "inherited environment key is invalid or duplicated".into(),
                ));
            }
        }
        let max_output_bytes = max_output_bytes.unwrap_or(DEFAULT_MAX_OUTPUT_BYTES);
        if max_output_bytes == 0 || max_output_bytes > MAX_OUTPUT_BYTES {
            return Err(RuntimeActivationAdapterError::InvalidCommandSpec(format!(
                "max output bytes must be between 1 and {MAX_OUTPUT_BYTES}"
            )));
        }
        Ok(Self {
            executable,
            execution_args,
            probe_args,
            working_directory,
            inherited_environment_keys,
            max_output_bytes,
        })
    }

    pub fn executable(&self) -> &PathBuf {
        &self.executable
    }

    pub fn execution_args(&self) -> &[String] {
        &self.execution_args
    }

    pub fn probe_args(&self) -> &[String] {
        &self.probe_args
    }

    pub fn working_directory(&self) -> Option<&PathBuf> {
        self.working_directory.as_ref()
    }

    pub fn inherited_environment_keys(&self) -> &[String] {
        &self.inherited_environment_keys
    }

    pub fn max_output_bytes(&self) -> usize {
        self.max_output_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRuntimeProbe {
    pub availability: RuntimeAvailability,
    pub runtime_version: Option<String>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRuntimeInput {
    pub contract_version: u16,
    pub execution_id: String,
    pub agent_id: String,
    pub objective: String,
    pub context_references: Vec<String>,
    pub model_id: String,
    pub provider_id: Option<String>,
    pub admission_receipt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRuntimeOutput {
    pub summary: String,
    pub artifact_references: Vec<String>,
}

pub trait CommandRuntimeHost: Send + Sync {
    fn probe(
        &self,
        spec: &CommandRuntimeSpec,
    ) -> Result<CommandRuntimeProbe, RuntimeActivationAdapterError>;
    fn execute(
        &self,
        spec: &CommandRuntimeSpec,
        input: &CommandRuntimeInput,
    ) -> Result<CommandRuntimeOutput, RuntimeActivationAdapterError>;
}

#[derive(Debug, Default)]
pub struct SystemCommandRuntimeHost;

impl SystemCommandRuntimeHost {
    fn command(spec: &CommandRuntimeSpec) -> Command {
        let mut command = Command::new(spec.executable());
        command.env_clear();
        for key in spec.inherited_environment_keys() {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        if let Some(directory) = spec.working_directory() {
            command.current_dir(directory);
        }
        command
    }

    fn bounded_text(bytes: &[u8], max: usize) -> Result<String, RuntimeActivationAdapterError> {
        if bytes.len() > max {
            return Err(RuntimeActivationAdapterError::Host(format!(
                "Runtime output exceeded {max} bytes"
            )));
        }
        Ok(String::from_utf8_lossy(bytes).trim().to_string())
    }
}

impl CommandRuntimeHost for SystemCommandRuntimeHost {
    fn probe(
        &self,
        spec: &CommandRuntimeSpec,
    ) -> Result<CommandRuntimeProbe, RuntimeActivationAdapterError> {
        if let Some(directory) = spec.working_directory() {
            if !directory.is_dir() {
                return Ok(CommandRuntimeProbe {
                    availability: RuntimeAvailability::RequiresConfiguration,
                    runtime_version: None,
                    diagnostics: vec!["Configured working directory is unavailable".into()],
                });
            }
        }
        let output = match Self::command(spec).args(spec.probe_args()).output() {
            Ok(output) => output,
            Err(_) => {
                return Ok(CommandRuntimeProbe {
                    availability: RuntimeAvailability::Unavailable,
                    runtime_version: None,
                    diagnostics: vec!["Runtime executable is unavailable".into()],
                })
            }
        };
        let stdout = Self::bounded_text(&output.stdout, spec.max_output_bytes())?;
        Self::bounded_text(&output.stderr, spec.max_output_bytes())?;
        let diagnostic = if output.status.success() {
            Vec::new()
        } else {
            vec![format!("Runtime probe exited with {}", output.status)]
        };
        Ok(CommandRuntimeProbe {
            availability: if output.status.success() {
                RuntimeAvailability::Ready
            } else {
                RuntimeAvailability::Degraded
            },
            runtime_version: (!stdout.is_empty()).then_some(stdout),
            diagnostics: diagnostic,
        })
    }

    fn execute(
        &self,
        spec: &CommandRuntimeSpec,
        input: &CommandRuntimeInput,
    ) -> Result<CommandRuntimeOutput, RuntimeActivationAdapterError> {
        let payload = serde_json::to_vec(input)
            .map_err(|error| RuntimeActivationAdapterError::Host(error.to_string()))?;
        let mut child = Self::command(spec)
            .args(spec.execution_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| RuntimeActivationAdapterError::Host(error.to_string()))?;
        child
            .stdin
            .take()
            .ok_or_else(|| {
                RuntimeActivationAdapterError::Host("Runtime stdin is unavailable".into())
            })?
            .write_all(&payload)
            .map_err(|error| RuntimeActivationAdapterError::Host(error.to_string()))?;
        let output = child
            .wait_with_output()
            .map_err(|error| RuntimeActivationAdapterError::Host(error.to_string()))?;
        let stdout = Self::bounded_text(&output.stdout, spec.max_output_bytes())?;
        Self::bounded_text(&output.stderr, spec.max_output_bytes())?;
        if !output.status.success() {
            return Err(RuntimeActivationAdapterError::Host(format!(
                "Runtime exited with {}",
                output.status
            )));
        }
        if stdout.is_empty() {
            return Err(RuntimeActivationAdapterError::Host(
                "Runtime completed without normalized output".into(),
            ));
        }
        Ok(CommandRuntimeOutput {
            summary: stdout,
            artifact_references: Vec::new(),
        })
    }
}

pub struct CommandRuntimeAdapter<H> {
    descriptor: RuntimeDescriptor,
    spec: CommandRuntimeSpec,
    host: Arc<H>,
    active_instance: RwLock<Option<RuntimeInstanceId>>,
    active_sessions: RwLock<HashMap<RuntimeSessionId, (RuntimeInstanceId, String)>>,
}

impl<H> CommandRuntimeAdapter<H>
where
    H: CommandRuntimeHost,
{
    pub fn new(
        descriptor: RuntimeDescriptor,
        spec: CommandRuntimeSpec,
        host: Arc<H>,
    ) -> Result<Self, RuntimeActivationAdapterError> {
        descriptor.validate().map_err(RuntimeAdapterError::from)?;
        Ok(Self {
            descriptor,
            spec,
            host,
            active_instance: RwLock::new(None),
            active_sessions: RwLock::new(HashMap::new()),
        })
    }

    fn probe_host(&self) -> Result<RuntimeProbe, RuntimeActivationAdapterError> {
        let probe = self.host.probe(&self.spec)?;
        let support = if probe.availability.can_prepare() {
            CapabilitySupport::Supported
        } else {
            CapabilitySupport::RequiresConfiguration
        };
        let normalized = RuntimeProbe {
            runtime_id: self.descriptor.runtime_id().clone(),
            availability: probe.availability,
            runtime_version: probe.runtime_version,
            capabilities: self
                .descriptor
                .capabilities()
                .iter()
                .cloned()
                .map(|capability| RuntimeCapabilityStatus {
                    capability,
                    support,
                })
                .collect(),
            diagnostics: probe.diagnostics,
        };
        normalized.validate().map_err(RuntimeAdapterError::from)?;
        Ok(normalized)
    }
}

impl<H> RuntimeSessionAdapter for CommandRuntimeAdapter<H>
where
    H: CommandRuntimeHost,
{
    fn open_session(
        &self,
        instance_id: &RuntimeInstanceId,
        session_id: &RuntimeSessionId,
    ) -> Result<RuntimeSessionHandle, RuntimeSessionError> {
        let active = self
            .active_instance
            .read()
            .map_err(|error| RuntimeActivationAdapterError::RegistryLock(error.to_string()))?;
        if active.as_ref() != Some(instance_id) {
            return Err(
                RuntimeActivationAdapterError::InstanceNotActive(instance_id.clone()).into(),
            );
        }
        drop(active);

        let handle =
            RuntimeSessionHandle::new(format!("command-runtime-session:{}", session_id.as_str()))?;
        let mut sessions = self
            .active_sessions
            .write()
            .map_err(|error| RuntimeSessionError::RegistryLock(error.to_string()))?;
        if sessions.contains_key(session_id) {
            return Err(RuntimeSessionError::AlreadyRegistered(session_id.clone()));
        }
        sessions.insert(
            session_id.clone(),
            (instance_id.clone(), handle.session_ref().to_string()),
        );
        Ok(handle)
    }

    fn probe_session(
        &self,
        instance_id: &RuntimeInstanceId,
        handle: &RuntimeSessionHandle,
    ) -> Result<RuntimeProbe, RuntimeSessionError> {
        let active = self
            .active_instance
            .read()
            .map_err(|error| RuntimeActivationAdapterError::RegistryLock(error.to_string()))?;
        if active.as_ref() != Some(instance_id) {
            return Err(
                RuntimeActivationAdapterError::InstanceNotActive(instance_id.clone()).into(),
            );
        }
        drop(active);

        let sessions = self
            .active_sessions
            .read()
            .map_err(|error| RuntimeSessionError::RegistryLock(error.to_string()))?;
        if !sessions
            .values()
            .any(|(registered_instance_id, session_ref)| {
                registered_instance_id == instance_id && session_ref == handle.session_ref()
            })
        {
            return Err(RuntimeSessionError::InvalidSessionReference);
        }
        drop(sessions);
        Ok(self.probe_host()?)
    }

    fn close_session(
        &self,
        instance_id: &RuntimeInstanceId,
        handle: &RuntimeSessionHandle,
    ) -> Result<(), RuntimeSessionError> {
        let mut sessions = self
            .active_sessions
            .write()
            .map_err(|error| RuntimeSessionError::RegistryLock(error.to_string()))?;
        let session_id = sessions
            .iter()
            .find_map(|(session_id, (registered_instance_id, session_ref))| {
                (registered_instance_id == instance_id && session_ref == handle.session_ref())
                    .then(|| session_id.clone())
            })
            .ok_or(RuntimeSessionError::InvalidSessionReference)?;
        sessions.remove(&session_id);
        Ok(())
    }
}

impl<H> RuntimeAdapter for CommandRuntimeAdapter<H>
where
    H: CommandRuntimeHost,
{
    fn descriptor(&self) -> &RuntimeDescriptor {
        &self.descriptor
    }

    fn probe(&self) -> Result<RuntimeProbe, RuntimeAdapterError> {
        self.probe_host()
            .map_err(|error| RuntimeAdapterError::ProbeFailed {
                runtime_id: self.descriptor.runtime_id().clone(),
                message: error.to_string(),
            })
    }
}

impl<H> RuntimeExecutionAdapter for CommandRuntimeAdapter<H>
where
    H: CommandRuntimeHost,
{
    fn invoke(
        &self,
        invocation: &RuntimeInvocation,
    ) -> Result<RuntimeInvocationOutput, RuntimeExecutionError> {
        let active = self
            .active_instance
            .read()
            .map_err(|error| RuntimeExecutionError::InvocationFailed(error.to_string()))?;
        if active.is_none() {
            return Err(RuntimeExecutionError::InvocationFailed(
                "Runtime is not activated".into(),
            ));
        }
        let request = invocation.request();
        let output = self
            .host
            .execute(
                &self.spec,
                &CommandRuntimeInput {
                    contract_version: self.descriptor.contract_version(),
                    execution_id: request.execution_id().as_str().to_string(),
                    agent_id: request.context().binding().agent_id().to_string(),
                    objective: request.objective().to_string(),
                    context_references: request.context().context_references().to_vec(),
                    model_id: request.model_binding().model_id().as_str().to_string(),
                    provider_id: request
                        .model_binding()
                        .provider_id()
                        .map(|id| id.as_str().to_string()),
                    admission_receipt: invocation.admission().receipt_ref().to_string(),
                },
            )
            .map_err(|error| RuntimeExecutionError::InvocationFailed(error.to_string()))?;
        Ok(RuntimeInvocationOutput {
            summary: output.summary,
            artifact_references: output.artifact_references,
        })
    }
}

impl<H> RuntimeLifecycleAdapter for CommandRuntimeAdapter<H>
where
    H: CommandRuntimeHost,
{
    fn activate(
        &self,
        instance: &RuntimeInstance,
    ) -> Result<RuntimeProbe, RuntimeActivationAdapterError> {
        if instance.runtime_id() != self.descriptor.runtime_id()
            || instance.adapter_id() != self.descriptor.adapter_id()
        {
            return Err(RuntimeActivationAdapterError::RuntimeMismatch {
                instance_id: instance.id().clone(),
                runtime_id: self.descriptor.runtime_id().clone(),
            });
        }
        if instance.lifecycle() != RuntimeInstanceLifecycle::Activating {
            return Err(RuntimeActivationAdapterError::InvalidActivationState);
        }
        let probe = self.probe_host()?;
        if !probe.availability.can_prepare() {
            return Err(RuntimeActivationAdapterError::RuntimeUnavailable(
                self.descriptor.runtime_id().clone(),
            ));
        }
        let mut active = self
            .active_instance
            .write()
            .map_err(|error| RuntimeActivationAdapterError::RegistryLock(error.to_string()))?;
        if let Some(active_id) = active.as_ref() {
            if active_id != instance.id() {
                return Err(RuntimeActivationAdapterError::AlreadyActive {
                    runtime_id: self.descriptor.runtime_id().clone(),
                    instance_id: active_id.clone(),
                });
            }
        }
        *active = Some(instance.id().clone());
        Ok(probe)
    }

    fn health(
        &self,
        instance_id: &RuntimeInstanceId,
    ) -> Result<RuntimeProbe, RuntimeActivationAdapterError> {
        let active = self
            .active_instance
            .read()
            .map_err(|error| RuntimeActivationAdapterError::RegistryLock(error.to_string()))?;
        if active.as_ref() != Some(instance_id) {
            return Err(RuntimeActivationAdapterError::InstanceNotActive(
                instance_id.clone(),
            ));
        }
        drop(active);
        self.probe_host()
    }

    fn deactivate(
        &self,
        instance_id: &RuntimeInstanceId,
    ) -> Result<(), RuntimeActivationAdapterError> {
        let mut active = self
            .active_instance
            .write()
            .map_err(|error| RuntimeActivationAdapterError::RegistryLock(error.to_string()))?;
        if active.as_ref() != Some(instance_id) {
            return Err(RuntimeActivationAdapterError::InstanceNotActive(
                instance_id.clone(),
            ));
        }
        *active = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_host_executes_without_shell_interpolation() {
        let executable = std::env::current_exe().expect("test executable path");
        let spec = CommandRuntimeSpec::new(
            executable,
            vec!["--list".into()],
            vec!["--list".into()],
            None,
            Vec::new(),
            Some(1024 * 1024),
        )
        .unwrap();
        let host = SystemCommandRuntimeHost;
        let probe = host.probe(&spec).unwrap();
        assert!(probe.availability.can_prepare());
        let output = host
            .execute(
                &spec,
                &CommandRuntimeInput {
                    contract_version: 1,
                    execution_id: "execution:test".into(),
                    agent_id: "agent:test".into(),
                    objective: "$(must-not-execute)".into(),
                    context_references: vec![],
                    model_id: "model:test".into(),
                    provider_id: None,
                    admission_receipt: "admission:test".into(),
                },
            )
            .unwrap();
        assert!(!output.summary.is_empty());
    }

    #[test]
    fn command_spec_rejects_relative_paths_and_duplicate_environment_inheritance() {
        assert!(CommandRuntimeSpec::new(
            PathBuf::from("runtime"),
            Vec::new(),
            Vec::new(),
            None,
            Vec::new(),
            None,
        )
        .is_err());
        assert!(CommandRuntimeSpec::new(
            std::env::current_exe().unwrap(),
            Vec::new(),
            Vec::new(),
            None,
            vec!["HOME".into(), "HOME".into()],
            None,
        )
        .is_err());
    }
}
