# ADR-001: Establish the Agent Runtime Boundary

- **Status:** Proposed
- **Date:** 2026-08-16
- **Owners:** Staff Architect
- **Approvers:** Architecture owner and repository maintainer
- **Related:** [Agent OS blueprint](../agent-os-blueprint.md), [current-state architecture](../architecture/current-state.md), [Agent Runtime boundary](../architecture/agent-runtime-boundary.md)
- **Supersedes:** None
- **Superseded by:** None
- **Command:** COD-003.1 Agent Runtime ADR Review

## Context

CC Switch currently provides a mature desktop control plane for model providers
and external AI clients. It manages provider profiles, native configuration,
terminal launch, sessions, a local proxy, failover, and usage telemetry across
Claude, Codex, Gemini, OpenCode, and other clients.

The current-state audit found that these capabilities are organized around
provider and application types rather than a first-class agent execution model.
Runtime knowledge is distributed across Rust `AppType`, frontend `AppId`, native
configuration modules, session dispatch, provider forms, and capability lists.
There is no common runtime lifecycle, capability negotiation, permission grant,
execution event contract, or agent identity.

Agent OS must orchestrate different external and local runtimes without embedding
runtime-specific branches in workflow logic. It must also preserve the Vision
invariants that roles are independent from models and runtimes, workflows are
explicit, cost is policy-controlled, and humans retain authority over permissions
and approval points.

The proposed Agent Runtime Boundary design defines a language-neutral contract for
runtime discovery, capabilities, asynchronous execution, input and output,
structured errors, and permissions. A durable decision is required before schemas,
storage, IPC, Rust interfaces, React surfaces, or runtime adapters are designed.

### Evidence

- `docs/architecture/current-state.md` records the existing Tauri, Rust, React,
  provider, proxy, persistence, session, and IPC architecture.
- `docs/architecture/agent-runtime-boundary.md` records the reviewed COD-003
  boundary proposal and its lifecycle, capability, execution, error, permission,
  identity, and extension contracts.
- Existing proxy `ProviderAdapter` behavior addresses upstream model HTTP
  translation, not agent lifecycle or workflow execution.
- Existing provider and native-configuration services contain valuable behavior
  that must remain compatible during Agent OS migration.

## Decision drivers

- Preserve current provider switching, native configuration, proxy, session, and
  usage behavior.
- Support Claude Code, Codex CLI, Gemini CLI, OpenCode, and local-model agents
  through one orchestration-facing contract.
- Keep Agent, Runtime, Provider, Model, and Role identities semantically stable and
  independently changeable.
- Prevent runtime-specific details from entering team and workflow logic.
- Make unsupported capabilities, execution state, failure, cancellation, and lost
  observation explicit.
- Enforce least privilege, human approvals, cost limits, and auditable execution.
- Allow future runtimes to be added without modifying core orchestration state
  machines.
- Avoid premature commitment to Rust traits, Tauri IPC, serialization, sandboxing,
  or plugin transport.

## Considered options

### Option A: Extend `AppType` and `Provider` into Agent OS entities

This option would add agent fields and execution behavior directly to the current
application and provider models.

It has a low initial implementation cost and can reuse existing switches and
forms. However, it makes provider credentials, runtime installation, model choice,
agent identity, and organizational role parts of one changing entity. Every new
runtime would continue to require edits across closed enums, capability lists,
session dispatch, UI configuration, and orchestration branches.

This option is rejected because it preserves the current coupling and makes Agent
OS identity and workflow history unstable.

### Option B: Reuse the proxy `ProviderAdapter` as `AgentRuntime`

This option would expand the existing upstream model-protocol adapter to launch and
control agents.

It appears to reuse an abstraction already called an adapter. Its responsibility,
however, is HTTP authentication, endpoint routing, and request/response
transformation. Agent runtime integration additionally requires process and
session lifecycle, workspace context, permissions, cancellation, artifacts,
events, and recovery.

This option is rejected because the two boundaries have different consumers,
security models, state machines, and failure semantics.

### Option C: Require one universal CLI protocol

This option would require every runtime to implement an identical command-line and
output protocol before CC Switch can orchestrate it.

It could simplify one launch path, but existing runtimes have incompatible
installation, authentication, context, interactivity, output, cancellation, and
session behavior. A universal CLI would either discard native capabilities or
move runtime-specific exceptions back into orchestration.

This option is rejected as a prerequisite. A standardized adapter transport may
be evaluated later without changing the logical boundary.

### Option D: Add a capability-based Agent Runtime boundary

This option introduces a language-neutral contract above existing integration
modules. Runtime adapters normalize discovery, capabilities, lifecycle, events,
results, errors, and permission enforcement. Core orchestration selects adapters
through descriptors and capability negotiation.

This option preserves current services, makes differences explicit, and permits
incremental migration. It introduces new contract and conformance work, but that
cost is bounded and directly supports runtime independence.

This is the selected option.

### Option E: Replace the current runtime and provider control plane

This option would build Agent OS configuration, proxy, session, and telemetry
behavior from scratch around a new runtime model.

It offers a clean conceptual starting point but duplicates mature functionality
and creates high compatibility, credential, data migration, and regression risk.

This option is rejected. Migration must be additive.

## Decision

CC Switch Agent OS will introduce a capability-based `AgentRuntime` boundary
between workflow orchestration and runtime-specific integration.

The following rules are binding if this ADR is accepted:

1. **Core orchestration depends only on normalized runtime contracts.** It must not
   branch on Claude Code, Codex CLI, Gemini CLI, OpenCode, local-model-agent, or
   other runtime names to control lifecycle or interpret output.
2. **Runtime adapters own native translation.** An adapter discovers and probes a
   runtime, maps normalized execution requests to native behavior, observes state,
   and returns normalized events, results, and errors.
3. **Execution is asynchronous and stateful.** Start returns a stable execution
   handle. State changes, output, cancellation, loss of observation, and terminal
   results are represented explicitly rather than inferred from a blocking call or
   raw text.
4. **Capabilities are negotiated.** Required runtime behavior must be declared and
   verified before productive execution. Unsupported, unknown, and
   configuration-dependent capabilities are valid explicit states.
5. **Agent, Runtime, Provider, Model, and Role remain separate concepts.**
   - Runtime identifies how execution occurs.
   - Provider identifies an approved model service or account.
   - Model identifies the inference model used for an execution.
   - Agent identifies the stable team participant and execution-history owner.
   - Role identifies a bounded workflow responsibility.
6. **Bindings do not redefine identity.** Changing provider, model, or assigned
   role does not silently create a new agent. Historical executions retain their
   exact runtime, provider, model, permissions, and role bindings.
7. **Workflow orchestration is not embedded in runtime adapters.** Adapters must
   not assign roles, construct team graphs, choose workflow steps, decide review
   gates, route work by cost, or grant their own permissions. They execute one
   bounded orchestration request and report evidence.
8. **Permissions are deny-by-default and bounded.** Effective authority is the
   intersection of workflow policy, Agent Profile limits, runtime capabilities,
   workspace constraints, and explicit human approvals. An adapter must fail
   preparation when required restrictions cannot be enforced.
9. **Runtime extension uses versioned adapters and descriptors.** Adding a runtime
   may add an adapter and registration metadata, but must not require changes to
   workflow, team, role, or normalized result-processing logic.
10. **Migration preserves existing behavior.** Provider services, native
    configuration modules, proxy adapters, terminal launch, session readers, usage
    telemetry, and current IPC remain intact until separately approved migrations
    reach compatibility parity.

This ADR decides the logical boundary and invariants. It does not decide the
adapter hosting model, Rust API, serialization, database schema, IPC shape,
sandbox technology, or plugin distribution mechanism.

## Why workflow orchestration remains outside runtime adapters

Runtime adapters and workflow orchestration change for different reasons.
Adapters change when a native runtime changes its CLI, endpoint, output, session,
or authentication behavior. Workflows change when teams alter roles, sequencing,
approval policy, acceptance criteria, or cost strategy.

Keeping workflow policy outside adapters ensures:

- the same runtime can serve Architect, Developer, Reviewer, or Researcher roles;
- one workflow can substitute a compatible agent or runtime without rewriting its
  state machine;
- approval and cost policy remain human-controlled and auditable;
- runtime upgrades cannot silently alter team relationships or workflow order;
- adapter conformance can be tested independently from product workflows; and
- retries and handoffs preserve workflow history instead of becoming native
  runtime conversations.

An adapter may report an approval request or capability constraint. The
orchestrator decides how the workflow responds.

## Consequences

### Positive

- Core orchestration remains independent of runtime-specific CLI and session
  behavior.
- New runtimes can be added through a stable extension contract.
- Agent identity and team history survive provider, model, role, and permitted
  runtime changes.
- Capability differences become explicit instead of being scattered through
  application switches and fallback assumptions.
- Permission, approval, cost, and audit policy remain centralized and
  human-controlled.
- Existing CC Switch provider, proxy, configuration, session, and usage assets are
  reused rather than replaced.
- Normalized lifecycle and error semantics enable reliable cancellation, retry,
  recovery, and review evidence.

### Negative

- The project must maintain a versioned contract and adapter conformance suite.
- Runtime-specific features require capability extensions and may not be available
  through the first common contract.
- Existing runtime knowledge remains duplicated until each integration is
  incrementally wrapped or migrated.
- Normalizing asynchronous output and native sessions adds state-management and
  observability complexity.
- Permission enforcement may require host controls when native runtimes cannot
  enforce a grant themselves.
- Adapter registration is a new trust and compatibility boundary.

### Migration impact

Migration is incremental and compatibility-first:

1. Inventory current client capabilities against the normalized vocabulary.
2. Introduce read-only runtime discovery and readiness probes without changing
   provider or native configuration behavior.
3. Wrap existing terminal launch and session observation behind adapters while
   retaining current commands and UI behavior.
4. Add normalized execution events, errors, permission grants, and conformance
   tests before workflow orchestration depends on adapters.
5. Migrate one runtime at a time behind compatibility gates; allow current paths
   to coexist until parity is demonstrated.
6. Change IPC, persistence, or UI only through separately reviewed decisions and
   milestones.

No existing runtime is required to support every capability. A runtime enters an
Agent OS workflow only when its adapter proves the required capabilities and
permission enforcement.

### Risks and mitigations

| Risk                                                      | Mitigation                                                                                       |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Contract collapses to the lowest common denominator       | Use versioned optional capabilities and explicit runtime extensions                              |
| Core orchestration accumulates runtime-name exceptions    | Enforce adapter conformance tests and reject runtime-specific branches in orchestration review   |
| Capability drift after runtime upgrades                   | Probe effective capabilities with native-version evidence before execution                       |
| Permission grant cannot be enforced                       | Fail preparation or require an approved host enforcement mechanism                               |
| Lost observation causes duplicate side effects            | Preserve stable execution IDs, explicit `Lost` state, reconciliation, and non-retryable defaults |
| Agent identity is conflated with provider or model        | Use separate stable IDs and immutable per-execution binding records                              |
| Cost estimates differ across providers and local runtimes | Carry source and confidence; enforce hard policy budgets independently from cost class           |
| Native session semantics do not support handoff           | Keep native references opaque and require explicit resume/checkpoint capabilities                |
| Adapter registration becomes an untrusted plugin surface  | Define trust, signing, isolation, and compatibility policy before external adapter loading       |
| Migration regresses existing provider or proxy behavior   | Use additive wrappers and regression gates; do not replace current paths before parity           |

## Future extension considerations

The boundary must allow, without changing its identity rules:

- additional CLI, desktop, local-process, or remote-service runtimes;
- multiple discovered installations of one runtime family;
- local and offline inference;
- runtime-native tools and versioned optional capabilities;
- resumable sessions and checkpoints where explicitly supported;
- richer artifact and structured-output contracts;
- adapter isolation or remote hosting;
- signed third-party adapters;
- organization-level policy and remote execution; and
- improved cost, usage, and sustainability telemetry.

Future work must not treat a transport, plugin ABI, model API, or native session
format as the Agent Runtime identity.

## Validation

The architecture owner is responsible for approving the boundary. The owning
engineering team is responsible for implementation conformance.

Acceptance evidence must include:

- a capability matrix for Claude Code, Codex CLI, Gemini CLI, OpenCode, and at
  least one local-model-agent scenario;
- lifecycle scenarios for success, failure, input waiting, cancellation, timeout,
  lost observation, reconciliation, and resume;
- permission scenarios for denial, approval escalation, unenforceable grants, and
  secret redaction;
- contract examples for request, event, result, error, and immutable execution
  bindings;
- proof that core orchestration conformance tests contain no runtime-name-specific
  behavior; and
- regression evidence for provider switching, native configuration, proxy,
  session, usage, and existing IPC behavior.

An implementation is non-conforming if it bypasses capability negotiation,
silently broadens permissions, embeds workflow decisions in an adapter, or uses
provider/model/role identifiers as agent identity.

## Follow-up work

- [ ] Approve or reject this ADR through human architecture review.
- [ ] Define and review the runtime capability matrix.
- [ ] Define normalized lifecycle, request, event, result, and error contract
      examples.
- [ ] Decide adapter hosting, discovery, isolation, and trust through a separate
      ADR.
- [ ] Decide persistence, event delivery, checkpoint retention, and secret-broker
      boundaries through separate ADRs.
- [ ] Create conformance and existing-behavior regression criteria before product
      implementation.
- [ ] Plan the first read-only runtime discovery milestone.

## Revisit triggers

Revisit this decision when any of the following occurs:

- two or more runtime families cannot represent required lifecycle behavior
  without core orchestration exceptions;
- a required permission cannot be enforced or audited at either adapter or host
  boundary;
- adapter version negotiation cannot preserve compatibility across supported
  runtime releases;
- remote or distributed execution introduces identity, trust, or delivery
  semantics not expressible by this boundary;
- runtime-native workflows must interoperate with Agent OS workflows rather than
  execute as one bounded task;
- migration evidence shows material regression in existing CC Switch behavior; or
- an accepted ADR supersedes the identity separation, execution model, or
  extension strategy defined here.
