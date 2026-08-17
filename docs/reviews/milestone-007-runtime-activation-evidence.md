# Milestone 7 Runtime Activation Evidence

- **Status:** Completed
- **Milestone:** Agent OS Phase 2 Milestone 7 — Runtime Activation
- **Tasks:** COD-017 Runtime Activation; COD-018 Provider Integration; COD-019 Model Routing
- **Reviewed by:** Codex, acting as Staff Engineer
- **Review date:** 2026-08-18
- **Remote baseline:** LoftySeas/cc-switch main at 294b3ae83e124f43b058934972d56d4c00b649df

## Purpose

This record traces the first Phase 2 productization milestone to implementation
and validation evidence. The implementation activates a real command-host Runtime
path while preserving the existing Agent, Runtime, Provider, Model, Capability,
Permission, Role, Execution and Workflow boundaries.

## Scope traceability

| Requirement | Implementation evidence |
| --- | --- |
| Runtime instance lifecycle | RuntimeInstance has an independent ID, Runtime and adapter references, revisioned Registered/Activating/Ready/Degraded/Stopping/Stopped/Failed lifecycle, and guarded timestamps. |
| Runtime health and availability | RuntimeHealthObservation records explicit Healthy/Degraded/Unavailable state and diagnostics. RuntimeActivationService refreshes health through the selected adapter and records lifecycle changes. |
| Concrete Runtime adapter | CommandRuntimeAdapter implements RuntimeAdapter, RuntimeLifecycleAdapter and RuntimeExecutionAdapter over a fixed CommandRuntimeSpec. SystemCommandRuntimeHost probes and executes a real process without a shell. |
| Controlled execution | Runtime objective and normalized references are serialized to stdin. User or model text is never interpolated into an executable or argument. Invocation is rejected until the Runtime instance is activated. |
| Provider adapter implementation | LegacyProviderCompatibilityAdapter prepares an opaque execution-scoped Provider binding after verifying the existing Provider record. It never copies credentials or calls a Model API. |
| Existing Provider compatibility | The existing Database Provider record remains authoritative and unchanged. Agent OS Provider identity stays distinct from the legacy Provider reference. |
| Model identity and descriptor | Existing independent ModelDescriptor and ModelAvailability contracts remain the routing inputs; no Model becomes Agent or Runtime identity. |
| Capability matching | ModelCapabilityRequirement matches explicit capability name, minimum version and required metadata. Missing required capability fails closed. |
| Routing policy boundary | ModelRoutingPolicy applies Model/Provider allow lists, deterministic preferences and maximum availability age. PolicyModelRoutingService also requires declared availability and a registered Provider probe. |
| Activation composition | ExecutionActivationPlan combines one ready Runtime instance, one resolved Model route and one prepared Provider binding for one Execution without invoking work or containing Agent identity. |

## Boundary and compatibility verification

- Agent does not contain Runtime, Provider, Model or activation state.
- Runtime instance, Runtime adapter and Execution retain independent identities and
  lifecycles.
- Runtime receives only the immutable Execution request and does not select a
  Provider or Model.
- Provider integration is performed through an adapter and returns a non-secret
  compatibility reference; the Runtime never calls a concrete Provider API.
- Model routing produces an immutable binding candidate and performs no direct
  inference or token-management behavior.
- Capability matching establishes technical eligibility only and never grants
  Permission.
- Role and Permission services are not modified.
- Existing Provider configuration, switching, proxy, sessions, usage and frontend
  behavior remain on their existing paths.
- No frontend or Tauri command was introduced because product UI and management
  APIs are assigned to Milestone 10.

## Validation evidence

| Check | Result |
| --- | --- |
| New M7 foundation tests | 12 tests cover Runtime lifecycle/health, one-live-instance repository enforcement, real process-host behavior, command/environment isolation, activation-gated governed execution, unavailable Runtime failure, cross-domain identity collision, Provider secret isolation, Model capability/freshness routing and cross-boundary activation planning. |
| cargo fmt --check | Passed. |
| cargo test --all-targets --quiet | Passed: 2,707 tests passed, 5 ignored, 0 failed. |
| cargo clippy --all-targets -- -D warnings | Passed with warnings denied. |
| pnpm format:check | Passed. |
| pnpm typecheck | Passed. |
| pnpm exec vitest run --reporter=dot --maxWorkers=4 --minWorkers=1 | Passed: 124 test files and 881 tests. |
| pnpm build:renderer | Passed. Existing dependency-data freshness, mixed dynamic/static import and bundle-size warnings remain non-blocking. |
| Documentation-link validation | Passed for all changed M7 Markdown documents. |
| Architecture-boundary source scan | Passed: no Provider API/proxy call, Tauri command, frontend IPC, Role/Permission service coupling or Workflow direct execution was introduced. |

## Conclusion

Milestone 7 moves the Phase 1 contracts into controlled Runtime activation,
Provider compatibility binding and policy-driven Model routing. It does not add
durable execution queues, retries or audit persistence, which remain Milestone 8
scope.
