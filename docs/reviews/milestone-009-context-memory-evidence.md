# Milestone 9 Context and Memory Evidence

- **Status:** Completed
- **Milestone:** Agent OS Phase 2 Milestone 9 — Context and Memory
- **Tasks:** COD-021 Context Memory; COD-021 Context Memory Foundation
- **Reviewed by:** Codex, acting as Staff Engineer
- **Review date:** 2026-08-18
- **Remote baseline:** LoftySeas/cc-switch main at bb287d5bcd51a8e93f52b8825348f2768e43196e

## Purpose

This record traces the third Phase 2 productization milestone to implementation
and validation evidence. The implementation gives long-running Agents governed
Memory and Knowledge references while keeping every Context Package bounded to
one Execution and preserving Memory as a separate identity.

## Scope traceability

| Requirement | Implementation evidence |
| --- | --- |
| Context lifecycle | ContextPackage has its own ID and revisioned Draft, Resolved, Sealed, Expired and Revoked lifecycle. References are unavailable until sealed and unavailable after expiry. |
| Execution scope | The database enforces one Context Package per Execution ID. A sealed package emits only package, Memory and Knowledge references for the existing Runtime-neutral ExecutionContext. |
| Memory domain | MemoryEntry records independent identity, Agent reference, kind, bounded content, sensitivity, optional source Execution, lifecycle, revision and mandatory expiration. |
| Memory is not identity | Agent records are unchanged. MemoryEntryId is distinct from Agent ID, and cross-Agent Memory selection is denied. |
| Memory is not Execution History | Source Execution ID is optional provenance on an independently identified MemoryEntry. It neither owns the Memory lifecycle nor stores or replaces Execution events. |
| Secret handling | OpaqueSecret sensitivity accepts only MemoryContent::OpaqueReference. Durable raw secret text is rejected by domain validation. |
| Knowledge references | KnowledgeReference records a locator, source kind, optional Agent scope, trust, optional source Execution, lifecycle, revision and mandatory expiration without fetching or embedding source content. |
| Least-privilege resolution | ContextPolicy constrains allowed Memory kinds, Knowledge source kinds, source counts, maximum sensitivity, required verification and maximum package lifetime. |
| Context Manager | ContextMemoryService verifies policy identity, Agent scope, source availability, expiry, trust, sensitivity and counts before resolving or sealing a package. It does not grant Permission or select Runtime, Provider or Model. |
| Controlled persistence | Schema v21 adds durable Memory, Knowledge and Context tables. Every record has mandatory expiration, immutable identity/retention columns and monotonic revisions; physical deletion is rejected so lifecycle evidence remains auditable. |
| Expiration safety | Memory and Knowledge become unavailable at their expiration timestamp even before an explicit lifecycle cleanup transition runs. |

## Boundary and compatibility verification

- Memory ID, Context Package ID and Knowledge Reference ID are never Agent IDs.
- Existing Agent records and lifecycle are unchanged and contain no Memory content.
- Context Package references one Execution but does not change Execution or
  Workflow state.
- Context resolution has no Runtime Adapter, Provider API, Model routing, Role or
  Permission service dependency.
- Context Policy narrows information visibility and cannot grant Capability or
  Permission.
- Context packages contain bounded references, not unrestricted chat history.
- Existing Hermes memory files and other native runtime/session storage remain
  unchanged and are not silently migrated into Agent OS Memory.
- Existing Provider, proxy, configuration, session, usage and frontend behavior
  is unchanged.
- No Tauri command or frontend UI was added because those belong to Milestone 10.
- The later, expanded `COD-021-context-memory-foundation.md` specification is
  reconciled to this implementation rather than creating a duplicate domain or
  storage boundary.

## Validation evidence

| Check | Result |
| --- | --- |
| M9 foundation tests | 10 tests cover secret isolation, mandatory retention, independent Memory/Agent/Execution identity, Execution provenance, Memory lifecycle/revisions, Context resolve/seal/expiry, policy source/trust/sensitivity limits, durable repository restoration, stale revisions, deletion guards, least-privilege ExecutionContext integration, cross-Agent denial and unavailable-source denial. |
| cargo fmt --all -- --check | Passed. |
| cargo clippy --all-targets -- -D warnings | Passed with warnings denied. |
| cargo test --all-targets --quiet | Passed: 2,726 tests passed, 5 ignored, 0 failed. |
| pnpm format:check | Passed. |
| pnpm typecheck | Passed. |
| pnpm exec vitest run --reporter=dot --maxWorkers=4 --minWorkers=1 | Passed: 126 test files and 883 tests. One unrelated PiProviderForm test timed out during an initial run concurrent with a cold Rust Clippy build; it passed alone and the subsequent serial full run passed. Existing test diagnostics remain non-blocking. |
| pnpm build:renderer | Passed. Existing dependency-data freshness, mixed dynamic/static import and bundle-size warnings remain non-blocking. |
| Database backup compatibility | Passed by inspection: SQL backup dynamically enumerates the new tables, indexes and triggers. |

## Conclusion

Milestone 9 supplies controlled long-running Context and Memory without making
Memory an Agent identity, copying complete conversations or weakening governance.
Product management APIs, execution visibility and user-facing controls were
subsequently delivered by Milestone 10.
