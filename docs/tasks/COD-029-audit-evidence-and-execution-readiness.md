# COD-029 Audit Evidence and Execution Readiness Hardening

- **Milestone:** M13 Enterprise Governance
- **Status:** Approved
- **Depends on:** COD-028 Controlled Execution Environment
- **Architecture:** [Agent OS Enterprise Governance Architecture v1](../architecture/agent-os-enterprise-governance-v1.md)

## Goal

Establish the enterprise audit evidence foundation and close the execution-readiness evidence gaps identified after COD-028, without enabling real execution.

The task must make every prepared environment independently auditable: it must prove exactly which Runtime and Provider instance revisions and adapter identities were validated, when they were observed, which explicit Model Resolution was consumed, and whether those facts remain current.

## Existing Boundaries to Reuse

Reuse, do not duplicate:

- `ExecutionRequest`
- `ControlledExecutionEnvironment`
- Runtime Instance and Runtime lifecycle adapter repositories
- Provider Adapter Instance and Provider lifecycle adapter repositories
- `ResolvedModel` and Model Resolution repository
- existing Execution audit/history concepts where compatible

The new governance audit domain is cross-cutting evidence. It must not replace Execution History or operational domain state.

## Required Domain Additions

### Runtime activation snapshot

Introduce an immutable `RuntimeActivationSnapshot` containing at minimum:

- Runtime Instance ID
- Runtime Instance revision
- Runtime ID
- Runtime Adapter ID
- Runtime lifecycle
- health status
- health observation timestamp
- instance updated timestamp
- snapshot timestamp

### Provider activation snapshot

Introduce an immutable `ProviderActivationSnapshot` containing at minimum:

- Provider Instance ID
- Provider Instance revision
- Provider ID
- Provider Adapter ID
- Provider lifecycle
- latest probe/health observation timestamp when available
- instance updated timestamp
- snapshot timestamp

### Governance audit event

Introduce typed identities and a bounded audit envelope:

- Audit Event ID
- Audit Stream ID
- sequence number
- event kind
- normalized outcome
- actor/provenance reference
- subject type and subject reference
- correlation references
- occurrence timestamp
- previous digest
- current deterministic digest
- sanitized metadata

Required event kinds include at least:

- controlled environment preparation requested
- Runtime snapshot captured
- Provider snapshot captured
- environment preparation accepted
- environment preparation rejected
- environment revalidation accepted
- environment revalidation rejected as stale or mismatched

## Controlled Environment Hardening

Extend the Controlled Execution Environment evidence so it freezes:

- Runtime activation snapshot
- Provider activation snapshot
- explicit Model Resolution ID
- Execution ID
- isolation evidence
- preparation timestamp

The environment must remain independent from the current mutable Runtime and Provider instances.

Add a public domain validation boundary or validated persistence DTO conversion. Direct deserialization must not create an invalid domain object. Repository insert and load paths must validate all invariants.

## Trusted Time

Introduce an injectable trusted clock used by the service for final snapshot, isolation, environment, and audit timestamps. Ordinary callers may provide request timestamps only where already required; they must not control final governance evidence time.

Enforce this order:

```text
execution.accepted_at
  <= resolution.requested_at
  <= resolution.resolved_at
  <= environment.requested_at
  <= runtime.health.observed_at
  <= runtime.snapshot_at
  <= provider observation time
  <= provider.snapshot_at
  <= isolation.prepared_at
  <= environment.prepared_at
  <= audit.occurred_at
```

Where Runtime or Provider observations predate the environment request, the snapshot must still record their actual observation time and the service must enforce an explicit maximum evidence-age policy or reject stale evidence. Do not silently rewrite timestamps.

## Revalidation Contract

Add a non-executable `ControlledExecutionEnvironmentRevalidator` contract that:

- loads the immutable prepared environment
- loads current Runtime and Provider instances
- compares current instance IDs, revisions, domain IDs, adapter IDs, lifecycle, and availability against the snapshots
- verifies the explicit Model Resolution evidence still matches
- returns a typed Ready or Stale result
- records an audit event
- exposes no invoke, start, network, tool, filesystem, Provider-call, or Model-call operation

A changed revision, adapter identity, unavailable lifecycle, missing resolution, or cross-boundary mismatch must fail closed and require a newly prepared environment.

## Repository and Persistence

Implement:

- `GovernanceAuditRepository`
- append-only in-memory adapter for tests
- additive SQLite adapter and migration
- deterministic ordered query by stream and sequence
- duplicate identity and sequence rejection
- digest-chain validation
- database triggers preventing update and delete

Audit events must contain no secrets, raw Provider configuration, API keys, environment variables, raw Memory content, full prompts, or model outputs.

## Compatibility

- Do not change Agent identity or lifecycle.
- Do not change Execution lifecycle semantics.
- Do not change Runtime, Provider, or Model identity meanings.
- Do not turn audit data into Memory or Execution History.
- Preserve existing IPC and UI behavior unless a bounded read-only audit query is added.

## Forbidden

Do not implement:

- Runtime invocation
- Provider or Model API calls
- tool execution
- network or filesystem execution
- autonomous loops
- Workflow scheduling
- automatic Model selection or fallback
- Permission grants
- policy publication; that is COD-030
- organization tenancy; that is COD-031

## Tests

Add tests covering at minimum:

1. exact Runtime and Provider revisions are frozen in the environment
2. adapter identity mismatch fails closed
3. stale revision revalidation fails closed
4. unavailable Runtime or Provider revalidation fails closed
5. trusted time order is enforced
6. stale observation age is rejected according to policy
7. invalid deserialization or persistence loading is rejected
8. audit events are append-only
9. duplicate sequence and broken digest chain are rejected
10. secret-like fields are absent from serialized audit evidence
11. Runtime lifecycle, Provider lifecycle, and invocation call counters remain zero during preparation and revalidation
12. existing COD-028 behavior remains compatible

## Acceptance Criteria

COD-029 is complete when:

- activation snapshots are immutable and persisted with controlled environments or linked immutable evidence
- trusted-time ordering is enforced
- prepared environments can be revalidated without execution
- stale environments fail closed
- audit streams are durable, ordered, append-only, and tamper-evident
- all new load and insert paths validate domain invariants
- full repository tests and architecture scans pass
- evidence is recorded in `docs/reviews/cod-029-audit-evidence-and-execution-readiness-evidence.md`
- `main` is pushed and remote verified
