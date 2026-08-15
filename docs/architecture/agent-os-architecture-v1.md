# CC Switch Agent OS Architecture v1

## 1. Vision

CC Switch Agent OS evolves from a model switcher into an Agent orchestration platform. The architecture treats Agent as a first-class domain object that composes identity, capability, permission, execution environment, provider resources and governance.

## 2. Core Domain Model

```
Agent
 ├── Identity
 ├── Profile
 ├── Role Assignment
 ├── Capability Binding
 ├── Permission Policy
 ├── Runtime Binding
 ├── Provider Binding
 ├── Model Policy
 └── Execution History
```

## 3. Domain Principles

### Agent Identity

Agent is a stable identity. It is not a model, provider, runtime, prompt or role.

An Agent may migrate between models and providers while preserving identity.

### Role

Role defines responsibility and behavior scope.

Examples:
- Coding Agent
- Review Agent
- Research Agent
- Deployment Agent

Role assignment is explicit and scoped.

### Capability

Capability answers: "What can this Agent technically do?"

Examples:
- Code generation
- File editing
- Repository analysis
- Tool invocation

### Permission

Permission answers: "What is this Agent allowed to do?"

Capability and permission remain separate to support enterprise governance.

## 4. Execution Architecture

```
User Request
    ↓
Agent Resolver
    ↓
Policy Engine
    ↓
Capability Check
    ↓
Permission Check
    ↓
Runtime Selector
    ↓
Provider Adapter
    ↓
Model Execution
    ↓
Audit Event Store
```

## 5. Runtime Layer

Runtime abstraction isolates execution environments.

Supported future runtimes:
- Claude Code
- Codex
- Gemini CLI
- Local Agent Runtime
- Remote Agent Runtime

Runtime selection must not modify Agent identity.

## 6. Provider Layer

Provider abstraction manages model sources.

Provider responsibilities:
- authentication
- endpoint management
- quota management
- health status
- routing policy

Provider must be replaceable without domain migration.

## 7. Model Management

Model is an execution capability resource.

Model selection considers:
- capability requirements
- cost policy
- latency
- availability
- user preference

## 8. Governance

Every execution records immutable evidence:

- Agent ID
- Role snapshot
- Capability snapshot
- Permission result
- Runtime
- Provider
- Model
- Timestamp
- Usage metrics

## 9. Storage Recommendation

Logical entities:

- agents
- agent_roles
- capabilities
- permissions
- providers
- runtimes
- model_bindings
- execution_records
- audit_events

## 10. Migration Strategy

Existing CC Switch concepts remain compatible:

- Provider remains provider
- Configuration remains configuration
- Session remains session
- Usage tracking remains usage tracking

Agent OS adds composition instead of replacing existing concepts.

## 11. Roadmap

Phase 1:
- Domain model
- Persistence schema
- Agent lifecycle

Phase 2:
- Permission engine
- Capability registry
- Runtime abstraction

Phase 3:
- Multi-agent collaboration
- Workflow orchestration
- Enterprise governance
