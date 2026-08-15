# Governance baseline integration

- **Status:** Final
- **Owner:** CC Switch Agent OS maintainers
- **Created:** 2026-08-15
- **Last updated:** 2026-08-15
- **Reviewer:** Codex, Staff Architect / Staff Engineer
- **Requested by:** COD-001.11
- **Integration branch:** `agent/codex/governance-baseline-integration`
- **GitHub base:** `origin/main` at
  `05e5817fbd931c13defbad2aee582dab5f0ef8ff`
- **Integration commit:** The commit containing this document. Its authoritative
  SHA is reported by `git rev-parse HEAD` and in the COD-001.11 delivery report;
  it is not embedded here because a commit cannot contain its own content-derived
  SHA.
- **Related:** [Repository governance](../repository-governance.md),
  [Agent operation guidelines](../development/agent-operation-guidelines.md),
  [Governance foundation finalization](./governance-foundation-final.md)

## Outcome

The Agent OS governance foundation has been integrated on a dedicated Agent
branch based directly on the verified GitHub `main` baseline. The integration is
documentation-only and is prepared as one atomic commit. It has not been pushed
or merged.

## Integrated files

### Repository bootstrap and tracking

- `.gitignore` no longer excludes root `AGENTS.md`.
- `AGENTS.md` and `CONTEXT.md` are inherited unchanged from the verified GitHub
  baseline and are therefore present in the resulting repository baseline.

### Governance and discovery

- `docs/README.md`
- `docs/repository-governance.md`
- `docs/architecture/README.md`
- `docs/architecture-decisions/README.md`
- `docs/commands/README.md`
- `docs/development/README.md`
- `docs/reviews/README.md`
- `docs/roadmap/README.md`
- `docs/vision/README.md`

### Operating protocols and templates

- `docs/development/agent-operation-guidelines.md`
- `docs/architecture-decisions/0000-template.md`
- `docs/commands/command-template.md`
- `docs/reviews/agent-review-template.md`
- `docs/roadmap/development-milestone-template.md`

The five supporting development protocols already present on GitHub `main` were
verified and retained unchanged:

- `docs/development/local-repository-usage.md`
- `docs/development/agent-evidence-protocol.md`
- `docs/development/command-publication-protocol.md`
- `docs/development/change-management.md`
- `docs/development/codex-collaboration-protocol.md`

### Preserved planning and evidence

- `docs/vision/agent-os-blueprint.md`
- `docs/reviews/governance-synchronization-2026-08-15.md`
- `docs/reviews/governance-foundation-final.md`
- `docs/reviews/governance-baseline-integration.md`

The existing `docs/agent-os-blueprint.md`, architecture index, system overview,
terminology, command specifications, and product documentation from GitHub
`main` were preserved rather than replaced.

## Consistency validation

- Root `AGENTS.md` and `CONTEXT.md` exist at the integration base.
- `AGENTS.md` is not ignored and remains tracked from GitHub `main`.
- Every development protocol named by the development index exists.
- Relative links in the governance indexes and reviews resolve to repository
  files or directories.
- The existing architecture index files remain present; no Agent Runtime design
  document or product implementation was added by this integration.
- The staged change set contains only documentation and `.gitignore` changes.

## Branch and delivery status

The integration branch is intentionally based on the latest verified
`origin/main`, rather than on the older `agent/ccswitch-agent-usage-api` branch
where COD-001 through COD-001.10 were prepared. This prevents unrelated feature
history and earlier architecture work from entering the governance baseline.

At review time:

- **Commit:** created as the atomic commit containing this record; use Git history
  for the immutable SHA.
- **Push:** not performed.
- **Remote verification:** `origin/main` was verified at the base SHA above, and
  no remote integration branch existed at final verification.
- **Merge:** not performed and not authorized.

## Remaining risks

1. The integration is not available to other agents until it is pushed and
   reviewed through GitHub.
2. GitHub `main` remains incomplete until the integration commit is merged and
   remotely verified.
3. The existing architecture ADR index names future decisions without linking to
   concrete ADR files. This pre-existing condition was preserved because
   COD-001.11 prohibits starting Agent Runtime design; it should be handled by a
   separate architecture-governance task.
4. The older local Agent branch still contains its original commits and
   uncommitted COD-001.9/COD-001.10 workspace overlay. It should not be used as
   the merge source for this baseline.

## Recommended integration strategy

Review and push only `agent/codex/governance-baseline-integration`, then open a
documentation-only pull request targeting `main`. Require a human governance
review, merge through the repository's normal protected-branch process, and
verify the merged paths and index links on GitHub. Do not merge the older feature
branch as a substitute for this dedicated integration branch.
