# Milestone 3 Provider and Model Architecture Evidence

- **Status:** Completed
- **Milestone:** Agent OS Milestone 3 — Provider and Model Architecture
- **Tasks:** COD-009 Provider Boundary Foundation; COD-010 Model Architecture Foundation
- **Reviewed by:** Codex, acting as Staff Engineer
- **Review date:** 2026-08-17
- **Remote baseline:** `LoftySeas/cc-switch` `main` at `4c44fc4537d03b4555b8426e632104eb187bc144`

## Purpose

This record traces Milestone 3 requirements to implementation and validation
evidence. It does not introduce a replacement architecture. The source of truth
remains the existing Agent OS architecture, ADRs, milestone plan, and COD-009 and
COD-010 task specifications.

## Scope traceability

| Requirement | Implementation evidence |
| --- | --- |
| Provider identity and descriptor | `AgentProviderId`, `AgentProviderAdapterId`, and `AgentProviderDescriptor` are validated identities and metadata contracts in `src-tauri/src/agent_provider_domain.rs`. |
| Provider capability metadata | `ProviderCapability` carries versioned, validated declarations without Permission semantics. |
| Provider adapter and registry | `AgentProviderAdapter`, `AgentProviderAdapterRepository`, and `InMemoryAgentProviderAdapterRepository` provide replaceable read-only boundaries. |
| Legacy Provider compatibility | `LegacyProviderSource` and `LegacyProviderCompatibilityAdapter` query existing Provider registration through a non-secret summary while legacy storage remains authoritative. |
| Model identity and descriptor | `ModelId`, `ModelDescriptor`, and `ModelMetadata` define Model catalog identity without Agent or Provider fields. |
| Model capability declaration | `ModelCapability` records versioned capability metadata without authorization or routing behavior. |
| Model registry | `ModelRegistry` and `InMemoryModelRegistry` register Model descriptors and explicit availability records. |
| Provider–Model relationship | `ModelAvailability` has its own identity and references distinct Provider and Model identities plus the provider-native catalog reference. |
| Application services | `AgentProviderService` and `ModelCatalogService` expose registration, lookup, probing, and relationship validation only. |

## Compatibility and boundary verification

- Existing `Provider`, Provider DAO, `ProviderService`, native configuration,
  proxy, session, usage, and frontend behavior are unchanged.
- The compatibility adapter reads existing records but never writes them, copies
  `settingsConfig`, resolves credentials, or invokes an upstream API.
- Agent OS Provider identity is distinct from the referenced legacy Provider ID.
- Model descriptors contain no Provider or Agent identity. Availability is a
  separate relationship, so one Model may be declared by multiple Providers.
- Provider and Model capability declarations do not grant Permission.
- No Model selection, token routing, prompt execution, billing migration,
  Provider API request, Runtime execution, Role, Permission, or Workflow behavior
  was introduced.
- No database migration, Tauri IPC command, or UI was required by Milestone 3.

## Validation evidence

| Check | Result |
| --- | --- |
| Provider and Model domain/boundary tests | 15 new tests passed across identity, descriptor, adapter, compatibility, registry, availability, and service behavior. |
| `cargo fmt --check` | Passed. |
| `cargo test --all-targets --quiet` | 2,656 tests passed; 5 ignored; 0 failed. |
| `cargo clippy --all-targets -- -D warnings` | Passed with warnings denied. |
| `pnpm format:check` | Passed. |
| `pnpm typecheck` | Passed. |
| `pnpm exec vitest run --reporter=dot --maxWorkers=4 --minWorkers=1` | 881 tests in 124 files passed. |
| `pnpm build:renderer` | Production renderer build passed. |
| Documentation-link validation | All links added by this milestone resolve to repository files. |
| Forbidden-boundary source scan | No execution, routing, Provider API, Permission, Role Assignment, or Workflow operation exists in the new foundation sources. |

The delivery report records the remotely verified completion commit. Milestone 4
execution behavior has not started as part of this delivery.

## Conclusion

COD-009 and COD-010 establish additive Provider and Model catalog boundaries
without replacing current CC Switch Provider/configuration capabilities or
coupling Agent, Runtime, Provider, and Model identities. Milestone 3 is ready to
serve as input to the separately governed Execution Platform milestone.
