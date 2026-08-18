# COD-026 Provider Adapter Activation Evidence

- **Status:** Implemented; Phase 3 Milestone 12 remains in progress
- **Task:** COD-026 Provider Adapter Activation
- **Reviewed by:** Codex, acting as Staff Architect and implementation engineer
- **Review date:** 2026-08-18
- **Source baseline:** `LoftySeas/cc-switch` `main@613baa95f28a068f0879b2dd706d91313892eb05`

## Scope traceability

| Requirement | Evidence |
| --- | --- |
| Provider Domain | Existing `AgentProviderId`, `AgentProviderDescriptor`, and `ProviderCapability` remain canonical. COD-026 adds only `AgentProviderInstance`, an independent operational activation identity. |
| Adapter boundary | `AgentProviderLifecycleAdapter` extends the existing Provider adapter contract with activate, health, and deactivate operations that cannot select a Model or execute a request. |
| Concrete adapter | `LegacyProviderCompatibilityAdapter` activates by probing the existing non-secret compatibility source and retains only an in-memory active instance reference. |
| Lifecycle management | Provider adapter instances have revisioned Registered, Activating, Ready, Degraded, Stopping, Stopped, and Failed states with explicit probe evidence and timestamp guards. |
| Registry boundary | `AgentProviderLifecycleAdapterRepository` registers lifecycle-capable adapters by existing Agent OS Provider identity without changing the read-only catalog registry. |
| Instance repository | `AgentProviderInstanceRepository` enforces immutable identities, optimistic revisions, history retention, and one live adapter instance per Agent OS Provider. |
| Service boundary | `AgentProviderActivationService` coordinates registration, activation, health refresh, failure recording, and deactivation without accessing legacy Provider storage directly. |
| Existing Provider compatibility | The legacy `Provider` record and SQLite DAO remain authoritative. Activation reads only ID, display name, and category; configuration and credentials are neither copied nor mutated. |

## Architecture verification

- Agent OS Provider identity remains distinct from legacy Provider identity and Provider adapter instance identity.
- Runtime, Provider, and Model identities and responsibilities remain separate.
- No existing Provider Domain, DAO, `ProviderService`, proxy adapter, native configuration path, session path, usage path, IPC command, or UI behavior was replaced.
- No Model Registry, Model Routing policy, Model selection, prompt execution, Provider API call, credential migration, Permission, Role, or Workflow behavior was added.
- Provider capabilities remain technical declarations and do not grant Permission.
- Missing legacy Provider registration fails activation closed and records a Failed lifecycle.
- Phase 3 Milestone 12 remains in progress because Model resolution and controlled execution-environment work are outside COD-026.

## Test coverage

- Revisioned lifecycle and mandatory probe evidence before readiness.
- Cross-boundary identity collision rejection.
- One-live-instance repository enforcement with retained historical records.
- Real SQLite legacy Provider compatibility activation, health refresh, and deactivation.
- Secret-bearing legacy configuration remains unchanged and outside serialized activation evidence.
- Missing legacy Provider activation fails closed and persists Failed state.

## Validation

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --all-targets -- -D warnings` | Passed with warnings denied. |
| `cargo test --all-targets --quiet` | Passed: 2,738 tests passed, 5 ignored, 0 failed across all targets. |
| `pnpm format:check` | Passed. |
| `pnpm typecheck` | Passed. |
| `pnpm exec vitest run --reporter=dot --maxWorkers=4 --minWorkers=1` | Passed: 126 files, 883 tests. Existing React `act(...)`, MSW handler, and expected error-path diagnostics remain non-blocking. |
| `pnpm build:renderer` | Passed. Existing dependency-data freshness, mixed import, and bundle-size warnings remain non-blocking. |
