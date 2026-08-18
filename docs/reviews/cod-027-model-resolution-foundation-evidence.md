# COD-027 Model Resolution Foundation Evidence

- **Status:** Implemented; Phase 3 Milestone 12 remains in progress
- **Task:** COD-027 Model Resolution Foundation
- **Reviewed by:** Codex, acting as Staff Architect and implementation engineer
- **Review date:** 2026-08-18
- **Source baseline:** `LoftySeas/cc-switch` `main@de773d2a9e6d2c821777f7ac0b44e693064c6128`

## Scope traceability

| Requirement | Evidence |
| --- | --- |
| Independent Model Domain | Existing `ModelId`, `ModelDescriptor`, `ModelCapability`, `ModelAvailability`, and `ModelRegistry` remain canonical. COD-027 does not copy legacy Model configuration. |
| Registry | The existing Model Registry remains the source of descriptor and explicit Provider–Model availability facts. A separate immutable resolution repository records completed validations. |
| Resolution contract | `ModelResolutionRequest` requires the caller to name Runtime instance, Provider instance, Model, and availability identities. `ResolvedModel` captures validated descriptor and availability snapshots. |
| Service boundary | `ModelResolutionService` coordinates read-only validation across activated Runtime, activated Provider, and Model Registry boundaries. |
| Capability validation | Explicit minimum versions and metadata constraints are matched against the requested Model descriptor; capabilities do not grant Permission. |
| No automatic routing | The service never lists, ranks, substitutes, or falls back to another Model, Provider, or Runtime. Existing legacy/model-routing behavior is unchanged and is not called by COD-027. |

## Architecture verification

- Agent, Runtime, Provider, Model, Execution, Role, Permission, and Workflow identities remain distinct.
- Runtime and Provider instances must already be Ready or Degraded and must match registered lifecycle adapter descriptors.
- Model availability must explicitly match the requested Model and the activated Provider and must have `Declared` status.
- The resolution result is immutable: duplicate resolution IDs are rejected and the repository provides no update operation.
- No Agent binding, execution invocation, Provider API request, credential lookup, legacy Model conversion, cost optimization, token routing, prompt routing, Permission change, or Workflow change was introduced.
- Existing CC Switch Model/configuration, Provider, proxy, and UI behavior remains unchanged.
- Phase 3 Milestone 12 remains in progress because controlled execution-environment work is outside COD-027.

## Test coverage

- Resolution requests serialize without Agent, Execution, or Role identity.
- Duplicate capability requirements and cross-boundary identity collisions fail validation.
- Explicit resolution succeeds only across matching active Runtime/Provider adapters and declared Model availability.
- An unsatisfied capability fails closed even when another Model exists in the registry; no fallback occurs.
- Successful resolutions are recorded as immutable evidence.

## Validation

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --all-targets -- -D warnings` | Passed with warnings denied. |
| `cargo test --all-targets --quiet` | Passed: 2,743 tests passed, 5 ignored, 0 failed across all targets. |
| `pnpm format:check` | Passed. |
| `pnpm typecheck` | Passed. |
| `pnpm exec vitest run --reporter=dot --maxWorkers=4 --minWorkers=1 --testTimeout=15000` | Passed: 126 files, 883 tests. The default 5-second run first exposed five existing `PiProviderForm` timing failures; the isolated file passed 46/46 and the complete retry passed 883/883 with a 15-second ceiling. Existing React `act(...)`, MSW handler, and expected error-path diagnostics remain non-blocking. |
| `pnpm build:renderer` | Passed. Existing dependency-data freshness, mixed import, and bundle-size warnings remain non-blocking. |
