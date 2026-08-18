# COD-022 Product Management Foundation

## Status

Completed

This expanded specification is delivered as an incremental extension of the
existing [`COD-022 Product UI`](COD-022-product-ui.md) baseline. It does not
replace that task or introduce a second product layer.

## Goal

Establish the first product management layer for Agent OS capabilities already implemented by the platform.

This phase exposes management and observability boundaries without changing the underlying Agent OS domain model.

## Scope

Implement:

- Agent management views and lifecycle visibility
- Execution history observation
- Context and Memory reference visibility
- Product-facing management service boundaries
- Read-only operational insights where appropriate

## Architecture Rules

Maintain:

- Agent != Runtime
- Runtime != Provider
- Provider != Model
- Memory != Identity
- Memory != Execution History
- Context != Runtime State

## Allowed

- UI presentation layer
- Query services
- Management APIs
- Read models
- Operational views

## Forbidden

Do not implement:

- Autonomous Agent execution
- Automatic Workflow creation
- Memory auto-learning
- RAG pipeline
- Provider routing UI
- Model execution controls
- Permission bypass

## Compatibility

Existing CC Switch provider, model, session, proxy and configuration features must remain unchanged.

## Validation

Required:

- Existing tests remain passing
- New management tests
- No domain boundary violations
- Production build verification

## Implementation Traceability

| Requirement | Delivered boundary |
| --- | --- |
| Agent management and lifecycle visibility | Existing revision-safe Agent Registry service, typed API, query hooks, and Agent view |
| Execution history observation | `ExecutionManagementView` read model projected from immutable Execution history |
| Context and Memory reference visibility | Read-only bounded references projected by the product service and rendered in the Execution view |
| Product-facing management services | `AgentOsProductService` owns query projection and bounded Workflow cancellation |
| Query boundary | Tauri commands return product read models; React never accesses repositories |
| Presentation boundary | Agent OS console renders Agent, Workflow, Execution, Context, and Memory evidence without Domain transition logic |

Detailed evidence is recorded in
[`Milestone 10 Product Layer Evidence`](../reviews/milestone-010-product-layer-evidence.md).
