# COD-028 Controlled Execution Environment Evidence

- **Status:** Implemented; Phase 3 Milestone 12 completed
- **Task:** COD-028 Controlled Execution Environment
- **Reviewed by:** Codex, acting as Staff Architect and implementation engineer
- **Review date:** 2026-08-18
- **Source baseline:** `LoftySeas/cc-switch` `main@9dd1319e44b06ab19c31f4219152f6aae4cac96d`

## Scope traceability

| Requirement | Evidence |
| --- | --- |
| Environment Domain | `ControlledExecutionEnvironmentId`, `ExecutionIsolationId`, preparation request, isolation evidence, and immutable environment snapshot are independent identities and records. |
| Runtime / Provider / Model consumption | The service consumes an existing COD-027 `ResolvedModel`, verifies its exact active Runtime and Provider instances, and requires matching registered lifecycle adapters. |
| Execution Request consumption | The environment snapshots an existing immutable `ExecutionRequest` and verifies its Runtime, Provider, Model, and availability references against resolution evidence. |
| Preparation Contract | `ControlledExecutionPreparationContract` exposes preparation only and returns a validated environment; it has no start or invoke operation. |
| Isolation boundary | `ExecutionIsolationBoundary` can produce only `PreparationOnly` evidence. Its in-memory implementation has no filesystem, network, tool, Provider, Model, or Runtime execution operation. |
| Repository / Service | An append-only repository rejects duplicate environment identities; the application service composes existing boundaries without owning or mutating them. |

## Architecture verification

- Agent, Runtime, Provider, Model, Execution, Memory, Capability, Permission, Role, and Workflow identities remain independent.
- Existing `ExecutionActivationPlan` and policy Model routing remain compatibility surfaces and were not modified or called by COD-028.
- Runtime and Provider adapter descriptors must match their activated instance identities; an instance alone cannot bypass the adapter registry.
- Model resolution is consumed by immutable resolution ID and is never recomputed, ranked, substituted, or bypassed.
- Governance evidence remains an opaque part of the existing Execution Request. Environment preparation neither grants nor broadens Permission.
- Context references are snapshotted without loading or mutating Memory.
- No Runtime invocation, Provider API call, Model API call, autonomous loop, tool execution, workflow scheduling, prompt routing, cost optimization, credential access, or UI behavior was introduced.
- Phase 3 Milestone 12 is complete at the controlled preparation boundary; real model execution remains explicitly outside COD-028.

## Test coverage

- Environment preparation succeeds only when Execution Request, Runtime, Provider, Model, and availability identities match.
- Mismatched explicit Model or Runtime evidence fails closed and records no environment.
- Prepared serialization contains preparation-only isolation evidence and no credential, model-invocation, or tool-execution field.
- Duplicate environment identity is rejected and existing evidence remains immutable.
- Instrumented Runtime lifecycle, Provider lifecycle, and Runtime invocation adapter methods remain at zero calls during preparation.

## Validation

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed. |
| `cargo clippy --all-targets -- -D warnings` | Passed with no warnings. |
| `cargo test --all-targets --quiet` | Passed: 2,748 tests; 5 ignored; 0 failed. |
| `pnpm format:check` | Passed. |
| `pnpm typecheck` | Passed. |
| `pnpm exec vitest run --reporter=dot --maxWorkers=4 --minWorkers=1 --testTimeout=15000` | Passed: 126 files; 883 tests. |
| `pnpm build:renderer` | Passed. Existing dependency-age, mixed-import, and chunk-size advisories remain non-blocking. |
