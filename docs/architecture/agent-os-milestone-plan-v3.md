# Agent OS Milestone Plan v3

## Phase 3: Operational Platform

## Milestone 11: Operational Management

Status: Completed (2026-08-18). Evidence: [Milestone 11 Operational Management Evidence](../reviews/milestone-011-operational-management-evidence.md).

Goal: provide management and observation capabilities.

Includes:

- Workflow management
- Team management
- Agent operations
- Execution monitoring

Excludes:

- Runtime execution changes
- Provider routing
- Model selection

Delivered:

- Durable Team organization repository adapter with auditable lifecycle state
- Bounded Team management query and desktop presentation
- Existing Workflow management, Agent lifecycle operations, and Execution monitoring verified as one operational surface
- No Runtime execution, Provider routing, or Model selection changes

## Milestone 12: Runtime Activation

Status: Completed (2026-08-18). COD-025 activated Runtime Adapters, COD-026 activated Provider Adapters, COD-027 established explicit Model resolution, and COD-028 completed the non-executable controlled preparation environment. Evidence: [COD-028 Controlled Execution Environment Evidence](../reviews/cod-028-controlled-execution-environment-evidence.md).

Goal: connect abstract runtime boundaries with controlled implementations.

Includes:

- Runtime adapters
- Provider adapters
- Model resolution boundary
- Execution environment management

Delivered in COD-025:

- Concrete command-host adapter remains behind the existing `RuntimeAdapter` contract
- Revisioned Runtime Session lifecycle with opaque adapter-owned session references
- Session-scoped capability, health, and availability probing
- Additive repository and service boundaries with fail-closed lifecycle handling

Delivered in COD-026:

- Independent, revisioned Provider adapter instance lifecycle
- Lifecycle adapter and registry extensions over the existing Agent OS Provider contract
- Read-only activation and health probes through the legacy Provider compatibility boundary
- Provider instance repository and activation service without Model routing or credential ownership

Delivered in COD-027:

- Reuse of the independent Model descriptor, capability, availability, and registry foundation
- Explicit Model resolution request/result contract with no inferred identities or fallback
- Validation across active Runtime Adapter and Provider Adapter instance boundaries
- Immutable resolution evidence repository and application service boundary
- Capability and availability validation without automatic selection, cost, token, or prompt routing

Delivered in COD-028:

- Independent controlled execution environment and isolation identities
- Immutable composition of one existing Execution Request and one explicit Model resolution
- Validation against matching active Runtime and Provider adapter instances
- Preparation-only isolation evidence with no invocation, tool, filesystem, network, or model-call operation
- Immutable environment evidence repository and application service boundary

Excludes:

- Agent identity changes
- Direct provider coupling

## Milestone 13: Enterprise Governance

Goal: operational governance.

Includes:

- Audit management
- Policy operations
- Organization boundaries

## Milestone 14: Agent Product Platform

Goal: user-facing agent platform capabilities.

Includes:

- Agent templates
- Collaboration features
- Marketplace foundations

## Delivery Rules

Every milestone must:

- Preserve Agent OS architecture boundaries
- Avoid duplicate domains
- Maintain compatibility
- Include tests
- Commit to main
- Verify remote state
