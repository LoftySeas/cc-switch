# COD-029 Audit Evidence and Execution Readiness Evidence

- **Status:** Implemented
- **Task:** COD-029 Audit Evidence and Execution Readiness Hardening
- **Milestone:** Phase 3 M13 Enterprise Governance
- **Reviewed by:** Codex, acting as Staff Architect and implementation engineer
- **Review date:** 2026-08-20
- **Source baseline:** `LoftySeas/cc-switch` `main@8eb175358ba2e2b7d534057e67eb48b83879b1e9`

## Scope traceability

| Requirement | Evidence |
| --- | --- |
| Exact Runtime activation evidence | `RuntimeActivationSnapshot` freezes the Runtime Instance ID and revision, Runtime and adapter identities, lifecycle, health status, health observation time, instance update time, and snapshot time. |
| Exact Provider activation evidence | `ProviderActivationSnapshot` freezes the Provider Instance ID and revision, Provider and adapter identities, lifecycle, availability, independent probe observation time, instance update time, and snapshot time. |
| Trusted final time | `TrustedClock` is injected into preparation, revalidation, and audit services. `SystemTrustedClock` uses Unix milliseconds, matching existing Agent OS timestamps; tests use `FixedTrustedClock`. |
| Complete time validation | Resolution now preserves its request time. Environment validation enforces the non-decreasing execution, resolution, request, snapshot, isolation, preparation sequence and an explicit maximum evidence age in milliseconds. Audit streams reject time regression. |
| Validated environment persistence | Direct environment deserialization, repository insert, SQLite load, and list paths validate nested Domain invariants. SQLite loads also verify every indexed identity and timestamp against immutable JSON evidence. |
| Non-executable revalidation | `ControlledExecutionEnvironmentRevalidator` compares exact current Runtime, Provider, adapter, lifecycle, health/availability, observation age, and Model Resolution evidence and returns typed `Ready` or `Stale` results. |
| Fail-closed readiness | Missing evidence, changed revisions, adapter mismatch, lifecycle/status mismatch, expired observations, unavailable boundaries, and a changed or missing resolution all require a newly prepared environment. |
| Governance audit repository | In-memory and SQLite repositories provide ordered append, duplicate rejection, deterministic SHA-256 digest chains, bounded queries, and validated loads. Bounded retries preserve a contiguous stream during concurrent writers. |
| Database immutability | Schema v24 adds immutable controlled-environment and governance-audit tables. Triggers prohibit update/delete and reject sequence gaps, wrong predecessor digests, time regression, and JSON/index-column mismatch. |
| Audit data safety | Metadata uses a fixed operational-key allowlist, bounded entry/value/byte limits, restricted code-like values, and secret/payload signature rejection. Actor, subject, and correlation references are bounded opaque references. Services record stable reason codes rather than raw errors. |
| Acceptance-before-persistence ordering | Final acceptance audit is recorded before environment insertion. An audit failure cannot leave a durable environment without final acceptance evidence; a subsequent persistence failure is recorded as rejection. |

## Compatibility

- Agent, Runtime, Provider, Model, Execution, Memory, Workflow, Capability, Permission, and Role identity meanings are unchanged.
- Provider Instance persistence gains an optional `lastProbeObservedAt` field. Existing records without it remain deserializable, but execution preparation fails closed until a fresh Provider probe supplies trusted observation time.
- Existing Execution Request, Model Resolution, Runtime Instance, Provider Instance, and adapter repositories remain their respective sources of operational truth.
- Existing IPC and UI contracts are unchanged.
- Audit evidence remains separate from Execution History, Memory, and mutable operational records.

## Security and forbidden-boundary verification

- Preparation and revalidation perform repository and adapter-descriptor lookups only.
- Runtime lifecycle, Provider lifecycle, and Runtime invocation counters remain zero in success and failure tests.
- No Runtime invocation, Provider or Model API call, tool execution, network execution, filesystem execution, autonomous loop, Workflow scheduling, automatic Model selection/fallback, Permission grant, policy publication, or organization tenancy is exposed.
- Audit records contain only typed identifiers, normalized outcomes, stable operational reason codes, revisions, and adapter identities. Provider diagnostics, configuration, credentials, environment variables, Memory content, Prompt content, model output, and file content are never copied.

## Persistence migration

- Schema version: `23 -> 24`.
- Added `agent_os_controlled_execution_environments`.
- Added `agent_os_governance_audit_events`.
- Both tables use additive creation and retain historical evidence; neither exposes a physical delete operation.
- Governance audit metadata and query size are bounded. Retention/archive deletion is intentionally not introduced because M13 requires historical Audit Events to remain immutable and the COD-029 database contract explicitly forbids delete.

## Test coverage

- Runtime and Provider snapshot revisions and adapter identities are frozen.
- Adapter identity, stale revision, unavailable lifecycle, missing resolution, and expired evidence fail closed without adapter invocation.
- Provider lifecycle changes do not rewrite the original probe observation time.
- Trusted time order and Unix-millisecond clock scale are enforced.
- Invalid nested typed IDs, invalid environment deserialization, and indexed-column/JSON mismatch are rejected.
- Final audit failure leaves no persisted environment.
- Audit metadata rejects secret-like keys and values and unapproved free-form fields.
- Audit streams reject duplicate/broken sequence and digest chains, time regression, update, and delete.
- Concurrent audit writers produce one contiguous stream through bounded conflict retry.
- SQLite v23-to-v24 migration verifies immutable tables, insert-chain guards, time guards, and environment mutation guards.

## Validation

| Check | Result |
| --- | --- |
| COD-029 Domain, Service, Audit, Repository, and migration tests | Passed. |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | Passed. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | Passed with no warnings. |
| `cargo test --manifest-path src-tauri/Cargo.toml --all-targets --quiet` | Passed: 2,769 tests; 5 ignored; 0 failed. |
| Architecture forbidden-operation scan | Passed. Matches were limited to reading the immutable Runtime health value; no lifecycle adapter or execution operation was called. |
| `git diff --check` | Passed. |
| Independent COD-029 release-blocker review | Approved; no remaining release blocker. |
