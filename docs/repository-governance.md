# Documentation governance model

## Purpose

This policy keeps CC Switch Agent OS documentation trustworthy over a long-lived,
multi-agent development program. Documentation is part of the engineering system:
it defines intent, constrains implementation, records decisions, and supplies the
evidence used to approve progress.

This change establishes governance only. It does not approve an architecture,
schedule product features, or authorize implementation.

## Governing principles

1. **One canonical home per concern.** Vision, current architecture, decisions,
   plans, commands, reviews, and development procedures remain distinct.
2. **History is append-oriented.** Supersede durable decisions; do not rewrite
   accepted history. Preserve old milestone and review records.
3. **Status is explicit.** A document without an approved status is not authority
   to implement a product change.
4. **Claims require evidence.** Reviews and milestone completion cite tests,
   measurements, diffs, or other reproducible evidence.
5. **Agents propose; accountable humans approve.** Codex and other agents may
   draft any artifact, but may not self-approve vision, ADRs, or milestones.
6. **Documentation changes accompany behavior changes.** A code change that
   alters an approved interface, trust boundary, workflow, or invariant updates
   its canonical documentation in the same change set.

## Directory ownership and lifecycle

| Area                      | Canonical content                          | Primary approver                   | Lifecycle                                  |
| ------------------------- | ------------------------------------------ | ---------------------------------- | ------------------------------------------ |
| `vision/`                 | Mission, principles, boundaries, non-goals | Product and architecture owners    | Draft → Approved → Superseded              |
| `architecture/`           | Current system design and contracts        | Staff architect or delegate        | Draft → Approved → Deprecated              |
| `architecture-decisions/` | One durable decision per ADR               | Architecture owner(s) named in ADR | Proposed → Accepted/Rejected → Superseded  |
| `roadmap/`                | Ordered milestones and exit criteria       | Product and engineering owners     | Proposed → Active → Completed/Cancelled    |
| `commands/`               | Repeatable agent task contracts            | Owning engineering team            | Draft → Active → Deprecated                |
| `reviews/`                | Point-in-time findings and evidence        | Review requester or named approver | Draft → Final; never retroactively updated |
| `development/`            | Contributor workflows                      | Owning engineering team            | Draft → Active → Deprecated                |

Approvals must be visible in normal repository history through reviewed commits or
pull requests. A status field changed by the author alone does not constitute
approval where a separate approver is required.

## Required metadata

Governed artifacts should begin with a compact metadata block containing the
fields relevant to their type:

- `Status`
- `Owner`
- `Created`
- `Last updated`
- `Approvers`
- `Related` links

Templates define additional required fields. Use ISO 8601 dates (`YYYY-MM-DD`),
repository-relative Markdown links, and stable identifiers where applicable.

## Naming rules

- Use lowercase kebab-case Markdown filenames.
- ADRs use `NNNN-short-decision-title.md`, with zero-padded, never-reused numbers.
- Milestones use `YYYY-MM-short-outcome.md` when time-bound, or a stable milestone
  identifier when dates are intentionally undecided.
- Commands use an imperative outcome, such as `validate-provider-boundary.md`.
- Reviews include their subject and date when multiple reviews may exist.
- Templates retain `template` in the filename and are never used as live records.

## Change workflow

1. Identify the canonical directory and read its active documents.
2. Link the proposed change to the relevant vision, architecture, ADR, or milestone.
3. Use the directory template and set the initial status to `Draft` or `Proposed`.
4. Keep observations, decisions, and future work visibly separate.
5. Obtain the required review and record approval through repository history.
6. Update indexes and related links in the same change.
7. When replacing an artifact, mark it superseded or deprecated and link both ways.

Moving a document is a content-preserving operation. Update inbound links, retain
Git history where practical, and add a redirect note at a widely referenced old
path when consumers may not migrate atomically. Never discard planning content
because it is obsolete; mark its status and preserve it.

## Agent operating rules

Before acting, Codex or another agent must state which authoritative documents it
read and distinguish missing context from assumptions. During work, it must:

- remain within the active milestone and accepted architecture;
- surface conflicts instead of selecting a convenient source silently;
- create a proposed ADR for a new durable or cross-cutting decision;
- avoid changing an accepted ADR in place, except for non-semantic corrections;
- avoid representing generated text as approved;
- record commands, evidence, and unresolved risks needed for reproducibility; and
- preserve user-authored and agent-authored history.

The required pre-execution context alignment, discovery, validation, and reporting
protocol is defined in
[`development/agent-operation-guidelines.md`](./development/agent-operation-guidelines.md).

Agent-generated content must name the generating agent in the artifact metadata or
review record. Human edits remain normal Git-authored changes; no special label is
required.

## Review and freshness

Architecture and development documents should be reviewed when their referenced
code or dependencies materially change. Active roadmap milestones are reviewed at
their checkpoints. Commands are tested when their required tools, inputs, or
expected outputs change. Final reviews remain immutable point-in-time records; a
new assessment produces a new review file.

Broken links, contradictory status, missing owners, and unreferenced active
documents are governance defects and should be corrected like code defects.

## Existing-content migration

Existing product manuals, guides, release notes, and Pi documents remain in place;
their information and link structure are preserved. They should move only in a
dedicated documentation migration with a reviewed mapping and link validation.

The original [`agent-os-blueprint.md`](./agent-os-blueprint.md) is preserved as the
compatibility source for Agent OS planning. Because it combines Vision,
architecture, roadmap, risks, and MVP scope, its information should be progressively
classified into governed directories. During that migration, the owner must:

1. preserve the original file, path compatibility, and commit history;
2. map product intent to `vision/`, stable design to `architecture/`, durable
   choices to ADRs, and sequencing to `roadmap/`;
3. copy every unresolved item into an owned roadmap or review artifact;
4. verify that no original heading or decision is lost; and
5. replace the old path only with a redirect after inbound links are updated.

## Compliance checklist

A documentation change is ready to merge when:

- it has one canonical location and a valid status;
- owners and approvers are identified where required;
- links are repository-relative and resolve;
- superseded artifacts link to their replacements;
- implementation claims include reproducible evidence;
- indexes include new active artifacts; and
- no planning or decision content was deleted during reorganization.
