# COD-026 Provider Adapter Activation

## Goal

建立 Agent OS Provider Adapter 层，使 Runtime 与 Provider 之间拥有稳定边界。

## Scope

实现：

- Provider Identity
- Provider Descriptor
- Provider Capability Contract
- Provider Adapter Interface
- Provider Lifecycle Boundary
- Provider Registry Boundary
- Compatibility boundary with existing CC Switch Provider system

## Architecture Rules

Must preserve:

- Agent != Runtime
- Runtime != Provider
- Provider != Model
- Execution != Provider Call
- Capability != Permission
- Role != Permission

## Compatibility Requirements

Existing Provider functionality is a compatibility source, not the new Agent OS Provider Domain.

Do not:

- replace existing Provider data model
- migrate existing provider configuration directly into Agent OS Provider
- bind Agent directly to Provider
- bypass Runtime Adapter boundary

## Forbidden

Do not implement:

- Model routing
- Model selection policy
- API key ownership migration
- Direct provider execution from Agent
- Workflow coupling
- Permission bypass

## Deliverables

- Domain implementation
- Adapter boundary
- Repository/service boundary if required
- Tests
- Documentation evidence

## Acceptance

- Existing CC Switch provider features remain compatible
- Provider remains independent from Agent and Model
- Runtime boundary remains intact
- Full test suite passes
- Commit and push to main with remote verification
