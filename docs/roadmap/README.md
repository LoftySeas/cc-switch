# Roadmap

This directory contains outcome-oriented Agent OS milestones, their dependencies,
and objective exit criteria. Roadmaps express intended sequence, not approved
architecture. Any durable technical choice discovered during planning belongs in
an ADR.

Create milestones from [`development-milestone-template.md`](./development-milestone-template.md).
Completed and cancelled milestones remain here as historical planning records.

## Purpose

Roadmap documents translate approved Vision into ordered, outcome-oriented
milestones. They explain what the program intends to achieve, why the sequence
matters, which dependencies constrain progress, and what evidence demonstrates
completion.

Roadmaps do not define implementation architecture. Durable technical choices
belong in ADRs, while architecture documents describe the approved system design.

## Source compatibility

The preserved [original Agent OS blueprint](../agent-os-blueprint.md) contains the
initial Development Roadmap, MVP, and future direction. Those sections remain
authoritative source material until their information is migrated into governed
milestones. A migration must retain every phase, goal, task, acceptance statement,
risk, and deferred item; it must not replace the source blueprint with this index.

## Required milestone content

Every milestone must use the repository template and identify:

- status, owner, approvers, and relevant dates;
- the outcome and its alignment with the approved Vision;
- explicit in-scope and out-of-scope boundaries;
- dependencies and accountable owners;
- objective exit criteria;
- risks and mitigations;
- links to governing architecture and ADRs; and
- evidence required for completion.

A feature list is not an outcome. A milestone states the user or engineering
condition that becomes true when the work succeeds.

## Lifecycle

1. **Proposed** — scope, alignment, dependencies, and exit criteria are under
   review. This status does not authorize implementation.
2. **Active** — product and engineering owners have approved the outcome and its
   boundaries.
3. **Completed** — exit criteria are satisfied and linked evidence has been
   reviewed.
4. **Cancelled** — the outcome will not be pursued under this milestone; the
   reason and disposition of unfinished work are recorded.

Material changes to outcome, boundaries, cost, risk, or dependencies require
renewed approval. Completed and cancelled records are not rewritten; create a
successor milestone when new work is needed.

## Naming and organization

Use lowercase kebab-case filenames. Prefer `YYYY-MM-short-outcome.md` for a
time-bound milestone and a stable milestone identifier when its date is undecided.
Keep milestones in this directory and link to task trackers instead of copying
volatile task-level detail.

## Human and agent responsibilities

Product and engineering owners approve activation, material scope changes, and
completion. The milestone owner keeps status, dependencies, risks, and evidence
current.

Codex and other agents may draft milestones, analyze dependencies, propose risks,
and collect evidence. Agents must not self-approve a milestone, silently expand
scope, invent architecture in a roadmap, or equate task closure with outcome
completion.

## Completion standard

A milestone is complete only when every exit criterion is satisfied or an
accountable approver explicitly records an exception. The completion record links
to reproducible evidence, identifies the approver, preserves open risks, and routes
remaining work to an owned successor milestone or issue.
