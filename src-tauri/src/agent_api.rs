//! Loopback-only API for local coding agents to query configured provider usage.
//!
//! The API deliberately returns only normalized usage data. Provider credentials,
//! usage scripts, and account tokens never cross this boundary.

use crate::app_config::AppType;
use crate::commands::{query_provider_usage_inner, CopilotAuthState, XaiOAuthState};
use crate::provider::UsageResult;
use crate::services::ProviderService;
use crate::settings::{self, AgentApiSettings};
use crate::store::AppState;
use axum::{
    extract::{Query, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, Mutex, RwLock};

const CACHE_TTL: Duration = Duration::from_secs(30);
type UsageCacheKey = (String, String);
type UsageCacheEntry = (Instant, UsageResult);
type UsageCache = Arc<Mutex<HashMap<UsageCacheKey, UsageCacheEntry>>>;

#[derive(Clone)]
struct AgentApiState {
    app_state: AppState,
    copilot_state: CopilotAuthState,
    xai_state: XaiOAuthState,
    token: Arc<RwLock<String>>,
    cache: UsageCache,
}

struct RunningServer {
    port: u16,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

struct AgentApiInner {
    state: AgentApiState,
    running: Mutex<Option<RunningServer>>,
}

/// Managed Tauri state for the dedicated Agent API server.
#[derive(Clone)]
pub struct AgentApiService {
    inner: Arc<AgentApiInner>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentApiInfo {
    pub enabled: bool,
    pub running: bool,
    pub port: u16,
    pub url: String,
    pub token_configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl AgentApiService {
    pub fn new(
        app_state: AppState,
        copilot_state: CopilotAuthState,
        xai_state: XaiOAuthState,
    ) -> Self {
        Self {
            inner: Arc::new(AgentApiInner {
                state: AgentApiState {
                    app_state,
                    copilot_state,
                    xai_state,
                    token: Arc::new(RwLock::new(String::new())),
                    cache: Arc::new(Mutex::new(HashMap::new())),
                },
                running: Mutex::new(None),
            }),
        }
    }

    pub async fn start_from_settings(&self) -> Result<AgentApiInfo, String> {
        let config = effective_settings();
        if !config.enabled {
            return Ok(self.info(false, config.port, false, None).await);
        }
        self.start(&config).await?;
        Ok(self.info(true, config.port, true, None).await)
    }

    async fn start(&self, config: &AgentApiSettings) -> Result<(), String> {
        validate_config(config)?;
        let mut running = self.inner.running.lock().await;
        if let Some(server) = running.as_ref() {
            if server.port == config.port {
                *self.inner.state.token.write().await = config.token.clone();
                return Ok(());
            }
            return Err("Agent API is already running on a different port; stop it first".into());
        }

        *self.inner.state.token.write().await = config.token.clone();
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), config.port);
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|error| format!("Failed to bind Agent API to {address}: {error}"))?;
        let actual_port = listener
            .local_addr()
            .map_err(|error| format!("Failed to read Agent API address: {error}"))?
            .port();

        let router = Router::new()
            .route("/health", get(health))
            .route("/_ccswitch/v1/providers", get(list_providers))
            .route("/_ccswitch/v1/usage", get(query_usage))
            .with_state(self.inner.state.clone());
        let (shutdown, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let result = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
            if let Err(error) = result {
                log::error!("Agent API server stopped unexpectedly: {error}");
            }
        });

        log::info!("Agent API listening on 127.0.0.1:{actual_port}");
        *running = Some(RunningServer {
            port: actual_port,
            shutdown,
            task,
        });
        Ok(())
    }

    pub async fn stop(&self) {
        if let Some(server) = self.inner.running.lock().await.take() {
            let _ = server.shutdown.send(());
            if tokio::time::timeout(Duration::from_secs(3), server.task)
                .await
                .is_err()
            {
                log::warn!("Timed out waiting for Agent API server to stop");
            }
        }
    }

    async fn info(
        &self,
        enabled: bool,
        configured_port: u16,
        token_configured: bool,
        token: Option<String>,
    ) -> AgentApiInfo {
        let running = self.inner.running.lock().await;
        let port = running
            .as_ref()
            .map(|server| server.port)
            .unwrap_or(configured_port);
        AgentApiInfo {
            enabled,
            running: running.is_some(),
            port,
            url: format!("http://127.0.0.1:{port}"),
            token_configured,
            token,
        }
    }
}

fn effective_settings() -> AgentApiSettings {
    let mut config = settings::get_agent_api_settings();
    if let Ok(token) = std::env::var("CCSWITCH_AGENT_API_TOKEN") {
        if !token.trim().is_empty() {
            config.enabled = true;
            config.token = token;
        }
    }
    if let Ok(raw_port) = std::env::var("CCSWITCH_AGENT_API_PORT") {
        if let Ok(port) = raw_port.parse::<u16>() {
            config.port = port;
        }
    }
    config
}

fn validate_config(config: &AgentApiSettings) -> Result<(), String> {
    if config.port == 0 {
        return Err("Agent API port must be between 1 and 65535".into());
    }
    if config.token.len() < 32 {
        return Err("Agent API token must contain at least 32 bytes".into());
    }
    Ok(())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

async fn authorize(headers: &HeaderMap, state: &AgentApiState) -> bool {
    let Some(value) = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    else {
        return false;
    };
    let expected = state.token.read().await;
    constant_time_eq(value.as_bytes(), expected.as_bytes())
}

fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(serde_json::json!({ "success": false, "error": message.into() })),
    )
        .into_response()
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

#[derive(Debug, Deserialize)]
struct AppQuery {
    app: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderSummary {
    id: String,
    name: String,
    current: bool,
}

async fn list_providers(
    State(state): State<AgentApiState>,
    headers: HeaderMap,
    Query(query): Query<AppQuery>,
) -> Response {
    if !authorize(&headers, &state).await {
        return json_error(StatusCode::UNAUTHORIZED, "Missing or invalid bearer token");
    }
    let app_type = match AppType::from_str(&query.app) {
        Ok(value) => value,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error.to_string()),
    };
    let providers = match ProviderService::list(&state.app_state, app_type.clone()) {
        Ok(value) => value,
        Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    let current = ProviderService::current(&state.app_state, app_type).unwrap_or_default();
    let data: Vec<ProviderSummary> = providers
        .into_iter()
        .map(|(id, provider)| ProviderSummary {
            current: id == current,
            id,
            name: provider.name,
        })
        .collect();
    Json(serde_json::json!({ "success": true, "data": data })).into_response()
}

#[derive(Debug, Deserialize)]
struct UsageQuery {
    app: String,
    provider: Option<String>,
    #[serde(default)]
    refresh: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentUsageResponse {
    app: String,
    provider_id: String,
    queried_at: String,
    cached: bool,
    #[serde(flatten)]
    usage: UsageResult,
}

async fn query_usage(
    State(state): State<AgentApiState>,
    headers: HeaderMap,
    Query(query): Query<UsageQuery>,
) -> Response {
    if !authorize(&headers, &state).await {
        return json_error(StatusCode::UNAUTHORIZED, "Missing or invalid bearer token");
    }
    let app_type = match AppType::from_str(&query.app) {
        Ok(value) => value,
        Err(error) => return json_error(StatusCode::BAD_REQUEST, error.to_string()),
    };
    let provider_id = match query.provider.as_deref() {
        Some(value) if !value.trim().is_empty() && value != "active" => value.trim().to_string(),
        _ => match ProviderService::current(&state.app_state, app_type.clone()) {
            Ok(value) if !value.is_empty() => value,
            Ok(_) => {
                return json_error(
                    StatusCode::BAD_REQUEST,
                    "This app has no active provider; pass ?provider=<provider-id>",
                )
            }
            Err(error) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
        },
    };

    let cache_key = (app_type.as_str().to_string(), provider_id.clone());
    if !query.refresh {
        let cache = state.cache.lock().await;
        if let Some((stored_at, usage)) = cache.get(&cache_key) {
            if stored_at.elapsed() < CACHE_TTL {
                return Json(AgentUsageResponse {
                    app: app_type.as_str().to_string(),
                    provider_id,
                    queried_at: chrono::Utc::now().to_rfc3339(),
                    cached: true,
                    usage: usage.clone(),
                })
                .into_response();
            }
        }
    }

    let usage = match query_provider_usage_inner(
        &state.app_state,
        &state.copilot_state,
        &state.xai_state,
        app_type.clone(),
        &provider_id,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => return json_error(StatusCode::BAD_GATEWAY, error),
    };
    if usage.success {
        state
            .cache
            .lock()
            .await
            .insert(cache_key, (Instant::now(), usage.clone()));
    }

    Json(AgentUsageResponse {
        app: app_type.as_str().to_string(),
        provider_id,
        queried_at: chrono::Utc::now().to_rfc3339(),
        cached: false,
        usage,
    })
    .into_response()
}

#[tauri::command]
pub async fn get_agent_api_status(
    service: tauri::State<'_, AgentApiService>,
) -> Result<AgentApiInfo, String> {
    let config = effective_settings();
    Ok(service
        .info(config.enabled, config.port, !config.token.is_empty(), None)
        .await)
}

#[tauri::command]
pub async fn configure_agent_api(
    service: tauri::State<'_, AgentApiService>,
    enabled: bool,
    port: u16,
) -> Result<AgentApiInfo, String> {
    let mut config = settings::get_agent_api_settings();
    config.enabled = enabled;
    config.port = port;
    let mut newly_generated = None;
    if enabled && config.token.is_empty() {
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        config.token = token.clone();
        newly_generated = Some(token);
    }
    if enabled {
        validate_config(&config)?;
    }
    if enabled {
        // Rebinding is cheap and makes port changes deterministic.
        service.stop().await;
        service.start(&config).await?;
        if let Err(error) = settings::set_agent_api_settings(config.clone()) {
            service.stop().await;
            return Err(error.to_string());
        }
    } else {
        service.stop().await;
        settings::set_agent_api_settings(config.clone()).map_err(|error| error.to_string())?;
    }
    Ok(service
        .info(enabled, port, !config.token.is_empty(), newly_generated)
        .await)
}

#[tauri::command]
pub async fn rotate_agent_api_token(
    service: tauri::State<'_, AgentApiService>,
) -> Result<AgentApiInfo, String> {
    let mut config = settings::get_agent_api_settings();
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    config.token = token.clone();
    settings::set_agent_api_settings(config.clone()).map_err(|error| error.to_string())?;
    *service.inner.state.token.write().await = token.clone();
    Ok(service
        .info(config.enabled, config.port, true, Some(token))
        .await)
}

#[cfg(test)]
mod tests {
    use super::{constant_time_eq, validate_config};
    use crate::settings::AgentApiSettings;

    #[test]
    fn bearer_comparison_rejects_length_and_content_mismatches() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"samf"));
        assert!(!constant_time_eq(b"same", b"same-longer"));
    }

    #[test]
    fn enabled_api_requires_nonzero_port_and_long_token() {
        let mut config = AgentApiSettings {
            enabled: true,
            port: 15722,
            token: "x".repeat(32),
        };
        assert!(validate_config(&config).is_ok());
        config.port = 0;
        assert!(validate_config(&config).is_err());
        config.port = 15722;
        config.token = "short".into();
        assert!(validate_config(&config).is_err());
    }
}
