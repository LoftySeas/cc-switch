# Agent operation guidelines

- **Status:** Active
- **Owner:** Staff Architect
- **Created:** 2026-08-15
- **Last updated:** 2026-08-15
- **Approved by:** Repository owner through tasks COD-001.5, COD-001.6, and COD-001.7
- **Related:** [Documentation governance](../repository-governance.md)

## Purpose

These guidelines define the mandatory context-alignment and execution protocol for
Codex and other agents working in the CC Switch repository. They prevent an agent
from treating a stale branch, an incomplete clone, or the local workspace as a
complete view of repository truth.

The key words **must**, **must not**, **should**, and **may** are normative. These
rules apply before documentation analysis, code analysis, review, or modification.

## 1. Repository Context Alignment

Before beginning substantive analysis or making any change, an agent must identify
and record the following context.

### Repository

Confirm the repository root rather than assuming the current working directory is
the root. Detect nested repositories, worktrees, submodules, or wrapper workspaces
that may contain a different Git repository from the directory named by the user.

Record:

- absolute repository root;
- repository identity or project name;
- whether the requested paths are inside that repository; and
- whether another repository boundary affects the task.

### Remote

Inspect configured fetch remotes and their URLs. Identify which remote the task
means by `origin`; do not infer that `upstream` or another similarly named remote
has the same owner, branches, or content.

When remote state is relevant, verify the remote directly with a read-only query or
an authorized fetch. A local `origin/main` reference is not proof that the remote
`main` branch has the same commit. If network or permission prevents verification,
the agent must state that limitation before relying on cached remote-tracking refs.

### Branch

Record:

- current branch or detached-HEAD state;
- configured upstream branch;
- task or pull-request branch, when different;
- intended base branch; and
- ahead/behind or merge-base information when relevant to the task.

An agent must not assume that the checked-out branch is the default branch or the
branch named by the task.

### Commit

Record the exact commit ID for the checked-out revision and for every remote or PR
revision used as evidence. Human-readable branch names are insufficient because
they may move during the task.

When freshness matters, record when and how the remote revision was verified. If
the remote moves during a long-running task, re-align before final validation.

### Source of truth

Declare the source of truth for the specific task before analysis. The declaration
must distinguish:

- the canonical baseline;
- the proposed-change branch, if any;
- the local execution workspace; and
- any explicit document, commit, release tag, or URL named by the user.

There is no safe universal assumption that “the currently visible files” are the
entire source of truth.

### Minimum alignment record

The agent's internal work log or user-facing update must be able to answer:

| Field           | Required evidence                                           |
| --------------- | ----------------------------------------------------------- |
| Repository      | Absolute Git root and repository identity                   |
| Remote          | Remote name, URL, and verified default branch               |
| Branch          | Current branch, upstream, PR branch, and base as applicable |
| Commit          | Exact local and relevant remote commit IDs                  |
| Workspace       | Staged, unstaged, untracked, and conflicting changes        |
| Source of truth | Explicit baseline and overlay order for this task           |

## 2. Source of Truth Rules

Source-of-truth priority depends on the question being answered. Agents must use
the following rules rather than applying one branch as a universal winner.

### Explicit task references

An explicit user-provided repository, remote, branch, PR, commit, tag, or document
is the first authority for task scope. If explicit references conflict with one
another, the agent must report the conflict and obtain clarification instead of
silently selecting one.

### `origin/main`

A freshly verified `origin/main` is the canonical baseline for repository-wide
statements about the current default branch, including whether a file exists in
the mainline project. A cached or stale local remote-tracking ref must be labelled
as cached and must not support an unqualified statement about current remote state.

`origin/main` does not override accepted changes that exist only on the designated
PR branch, and it does not authorize overwriting local user work.

### Pull-request or task branch

For a PR review or branch-scoped task, the designated PR branch is the authority
for the proposed change. It must be interpreted as a delta from its verified base,
normally `origin/main` or the PR's declared base branch.

The agent must inspect both sides when deciding whether content was added, removed,
moved, or modified. A file absent from the PR branch may still exist on the base,
and a file absent from the base may be introduced by the PR.

### Local workspace

The local workspace is the authority for the actual execution state: user edits,
agent edits, staged changes, unstaged changes, and untracked files. These changes
must be preserved unless the user explicitly authorizes their removal or
replacement.

The local workspace is an overlay, not proof of upstream repository state. Local
absence does not prove remote absence. Local presence does not prove that a file is
committed, approved, or present on the base or PR branch.

### Effective precedence

For normal PR or task work, reason about the effective state as:

1. explicit task references and constraints;
2. freshly verified canonical base (`origin/main` or declared base);
3. designated PR/task-branch changes relative to that base; and
4. local workspace changes overlaid for preservation and execution.

This order describes interpretation, not permission to overwrite. When layers
conflict, preserve the local workspace, identify the conflict, and request human
direction if intent cannot be established safely.

## 3. Document Discovery Rules

Before creating, moving, replacing, or declaring a document missing, an agent must
perform discovery across every source relevant to the task.

### Existence checks

Check:

- the exact requested path in the local workspace;
- tracked files in the current branch;
- untracked and ignored files when they may contain user work;
- the freshly verified base branch;
- the designated PR or task branch; and
- explicit remote repository paths or URLs supplied by the user.

Account for case differences, filename variants, translations, nested repository
roots, and documents whose title differs from their filename.

### Move and rename history

Inspect Git history for the requested path and likely predecessor names. Use rename
and follow-history evidence where available. Check whether a compatibility redirect
or inbound links identify a new canonical location.

A missing path may mean the document was moved, renamed, split, consolidated, or
removed. Those states must not be collapsed into “never existed.”

### Other versions

Search for:

- files with similar names or matching document titles;
- copies on the base and PR branches;
- versioned, localized, archived, or superseded documents;
- relevant release tags when the task names a released version; and
- remote versions when local refs are incomplete or stale.

When multiple versions exist, compare their commits, status metadata, and inbound
links before deciding which is canonical. Preserve non-canonical versions until
the documentation governance process explicitly migrates or supersedes them.

### Missing-document claims

An agent must not state “the file does not exist” without qualifying the checked
scope and evidence. Use precise statements such as:

- “The path is absent from the local workspace at commit `<id>`.”
- “The path is absent from the verified `origin/main` tree at commit `<id>`.”
- “The path was not found in the following checked refs: …”
- “Remote verification was unavailable, so repository-wide absence is unproven.”

An unqualified repository-wide absence claim requires verification of the current
source of truth and all task-relevant versions.

## 4. Execution Protocol

Every agent task follows four phases. A later phase must not erase unresolved
evidence or assumptions from an earlier phase.

### Phase 0: Context Alignment

Before substantive task work:

1. identify the repository root and repository identity;
2. inspect remotes and verify the relevant remote state;
3. record branch, upstream, base, and exact commits;
4. inspect the complete local workspace status;
5. declare the task-specific source of truth; and
6. report any freshness, permission, network, or repository-boundary limitation.

No existence claim or write should occur before Phase 0 is sufficient for the
task. If a missing remote check is material, pause and request access rather than
guessing.

### Phase 1: Analysis

1. read the requested governance, Vision, architecture, ADR, roadmap, and command
   documents that apply;
2. perform document or code discovery across the declared sources of truth;
3. identify existing local changes and ownership boundaries;
4. distinguish observed facts, assumptions, proposals, and unresolved conflicts;
   and
5. define the smallest change that satisfies the task while preserving existing
   information.

Analysis and review requests do not authorize implementation unless the task also
requests a change.

### Phase 2: Execution

1. operate only within the authorized scope;
2. preserve unrelated staged, unstaged, and untracked work;
3. retain existing information and compatibility paths during documentation
   migrations;
4. follow approved architecture and accepted ADRs;
5. record new durable decisions through the ADR process; and
6. stop when completion requires destructive action, new authority, or material
   scope expansion.

An agent must not use branch switching, reset, clean, checkout, or file replacement
as a shortcut around context misalignment.

### Phase 3: Validation

1. validate the changed artifacts in proportion to risk;
2. inspect the final diff and workspace status;
3. verify links, references, tests, formatting, or generated outputs as applicable;
4. confirm that no unrelated user content was removed or overwritten;
5. compare final work against the exact requested source and acceptance criteria;
   and
6. recheck remote or branch alignment when the source may have changed during the
   task.

Report validation that was not run, failed, or used stale evidence. Do not imply a
successful repository-wide check from a narrower local check.

## Git Delivery Protocol

Task execution state and Git delivery state are separate. A task may satisfy its
content requirements while still existing only as local workspace changes. Agents
must report the highest delivery state actually verified and must not use the word
“completed” as a substitute for delivery evidence.

The four delivery states are cumulative only when every earlier state has been
verified for the same intended change set.

### 1. Workspace Modified

**Workspace Modified** means that local files have been created, edited, moved, or
deleted in the current working tree.

This state does not mean that changes are staged, committed, pushed, visible on
GitHub, or available to another checkout. The agent must inspect the working tree
and distinguish staged, unstaged, and untracked files. It must also distinguish
task changes from pre-existing or unrelated workspace changes.

Required evidence:

- the changed-file list from the local workspace;
- the current branch or detached-HEAD state; and
- an explicit statement that no commit exists for the task change when that is the
  case.

### 2. Commit Created

**Commit Created** means that a Git commit containing the intended task changes has
been created successfully in the local repository.

Before committing, the agent must inspect the staged diff and ensure that unrelated
user changes are excluded. After committing, it must verify the commit contents
and record the full commit hash. A commit may exist only locally; this state does
not imply a push.

Required evidence:

- exact commit hash;
- commit subject;
- branch containing the commit;
- files included in the commit; and
- remaining workspace changes not included in the commit.

### 3. Pushed Remote

**Pushed Remote** means that Git reported a successful push of the intended commit
to a named remote and remote branch.

An agent may push only when the task explicitly requests or otherwise authorizes a
remote write. Creating a commit does not grant push permission. The report must
identify the remote and destination branch and distinguish success, rejection,
failure, not attempted, and not authorized.

A successful local commit or configured upstream does not prove this state.

Required evidence:

- remote name and URL or repository identity;
- destination branch;
- pushed commit hash; and
- push command result.

### 4. Remote Verified

**Remote Verified** means that, after any push, the agent performed a separate
read-only check through the GitHub API or Git remote and confirmed that the named
remote branch resolves to the expected commit.

Push command output alone does not establish Remote Verified. The verification
must query the remote state after the push and compare the returned commit hash
with the intended local commit. When GitHub UI, checks, or pull-request visibility
is part of the claim, the corresponding GitHub API state must also be verified.

Required evidence:

- verification method and time;
- remote repository and branch queried;
- expected commit hash;
- observed remote commit hash; and
- whether the hashes match.

### Delivery state transitions

| State              | Local files changed | Commit exists | Push confirmed | Remote independently verified |
| ------------------ | ------------------- | ------------- | -------------- | ----------------------------- |
| Workspace Modified | Yes                 | Not required  | No             | No                            |
| Commit Created     | Yes                 | Yes           | Not required   | No                            |
| Pushed Remote      | Yes                 | Yes           | Yes            | Not required                  |
| Remote Verified    | Yes                 | Yes           | Yes            | Yes                           |

If a push is not requested, a valid task handoff may stop at Workspace Modified or
Commit Created, but the report must say so explicitly. If remote verification
cannot be performed because of network, permission, or API limitations, report
the state as unverified rather than inferring success.

### Mandatory delivery report

Every final task report must include all of the following fields, even when the
value is `None`, `Not created`, `Not pushed`, `Not requested`, or `Not verified`:

- **Changed files:** task files created, modified, moved, or deleted;
- **Current branch:** exact local branch or detached-HEAD commit;
- **Commit hash:** full task commit hash, or an explicit statement that no commit
  was created;
- **Push status:** remote and branch with result, or why no push occurred; and
- **Remote verification status:** method, observed hash, and match result, or an
  explicit statement that verification was not performed.

The report should additionally state the highest verified delivery state and list
material workspace changes that remain outside the task commit.

### Prohibited delivery claims

Agents must not:

- use only “completed,” “done,” or similar language to describe task delivery;
- imply that modified files are committed without an exact verified commit hash;
- imply that a commit is on GitHub because it exists locally;
- treat a successful push message as independent remote verification;
- claim that a branch was pushed without naming the remote and destination branch;
- omit a rejected or failed push attempt; or
- describe the repository as clean when unrelated or untracked files remain.

## Branch Governance Protocol

Branches are bounded delivery contexts, not permanent workspaces or substitutes
for task ownership. Every branch must have a clear purpose, a known base, an
accountable owner, and an expected end state.

This protocol applies to branches created after COD-001.7 takes effect. Existing
branches with legacy names may finish their current task, but new work must not
copy a legacy naming pattern. An agent must not rename, delete, rebase, or replace
an existing branch merely to make its name compliant without explicit authority.

### 1. Branch types

#### `main`

`main` is the stable integration baseline for the repository.

- It represents reviewed mainline state.
- Development work must not be performed directly on `main`.
- Changes reach `main` through the repository's reviewed merge process.
- Direct commits and pushes are reserved for explicitly authorized repository
  administration or emergency procedures.
- An agent must treat protection rules and required checks as mandatory even when
  its credentials could bypass them.

#### Feature branch

Format:

```text
feature/<name>
```

A feature branch carries one coherent product or engineering outcome. `<name>`
must be short, lowercase kebab-case, and descriptive of the outcome, for example
`feature/agent-profile-import`.

Feature branches should start from the verified target base, remain limited to the
declared outcome, and end after merge or cancellation. Generic names such as
`feature/work`, `feature/misc`, or `feature/temp` are not acceptable.

#### Agent branch

Format:

```text
agent/<agent-name>/<task>
```

An agent branch identifies work executed primarily by a named agent for a bounded
task. `<agent-name>` and `<task>` must be lowercase kebab-case. The task segment
should include the command ID when one exists, for example
`agent/codex/cod-001-7-branch-governance`.

An agent branch does not grant the agent ownership of repository policy or
permission to push. It records execution provenance and task scope. Multiple
unrelated commands must not accumulate indefinitely on the same agent branch.

#### Review branch

Format:

```text
review/<topic>
```

A review branch contains a bounded review, audit, reproduction, or review artifact.
`<topic>` must be lowercase kebab-case and identify the subject, for example
`review/agent-runtime-security`.

A review branch must not silently become an implementation branch. Material fixes
identified during review move to an authorized feature or agent task unless the
review task explicitly includes remediation.

### Branch lifecycle

Before creating a branch, identify its base, owner, command or issue, and intended
merge target. Do not create a new branch when the task already has an appropriate
branch or when the task does not authorize repository mutation.

A branch should contain one reviewable outcome. After merge or cancellation, its
owner records the disposition and follows repository policy for remote cleanup.
Agents must not create placeholder, speculative, duplicate, or indefinitely
long-lived branches.

### 2. Agent working rules

By default, an agent:

- must not modify, commit on, or push directly to `main`;
- must work on the explicitly assigned task branch or request direction when the
  current branch is unsuitable;
- must not create or switch branches when doing so risks hiding or displacing local
  workspace changes;
- must not push automatically after creating a commit;
- must obtain explicit remote-write authorization before every push not already
  covered by an approved workflow;
- must not create a branch solely to make a report appear complete;
- must not reuse an unrelated or completed branch for convenient storage; and
- must not keep an agent branch alive as a general-purpose personal branch.

If an agent discovers that it is on `main`, a detached HEAD, a stale branch, or a
branch with unrelated changes, it must stop before writing, preserve the workspace,
and resolve the delivery context with the user or established workflow.

### 3. Commit rules

Every commit must communicate:

- **Purpose:** the outcome or reason for the change;
- **Scope:** the subsystem, document set, or concern affected; and
- **Related command ID:** the governing task identifier, such as `COD-001.7`.

Use a concise imperative subject with a meaningful scope:

```text
<type>(<scope>): <purpose>
```

Example:

```text
docs(agent-os): add runtime architecture

Purpose: Establish the reviewed runtime architecture baseline.
Scope: docs/architecture and related indexes.
Command: COD-003
```

The subject summarizes purpose; the body records scope and command traceability
when those details are not fully obvious from the subject. Placeholder messages
such as `update`, `changes`, `wip`, or an agent name alone are not acceptable for
handoff commits.

Commits must be atomic enough to review and must exclude unrelated user changes.
Before committing, inspect the staged diff. After committing, verify the exact hash,
subject, and included files. Agents must not amend, squash, rebase, or force-push
shared history without explicit authorization.

### 4. Handoff rules

Every agent handoff must include:

- **Branch:** exact local branch and intended base or merge target;
- **Commit:** full hash, or an explicit statement that no commit exists;
- **Changed files:** task files included in the commit or remaining workspace;
- **Next action:** the named review, push, merge, correction, or approval needed;
- **Push status:** remote and destination branch, or `Not pushed`; and
- **Remote verification:** observed remote hash and method, or `Not verified`.

The handoff must distinguish committed work from uncommitted workspace changes and
must identify unrelated changes that remain. If another agent will continue the
task, include the governing command ID, unresolved decisions, validation evidence,
and any branch freshness limitation needed to resume safely.

### 5. Merge strategy and responsibilities

The normal integration path is a reviewed pull request into the declared base
branch. A commit, push, or passing local validation does not itself authorize a
merge or release.

#### Review responsibility

The designated reviewer or owning team evaluates scope, correctness, architecture
alignment, risks, validation evidence, and branch freshness. The authoring agent
must not self-approve its own work. An agent review is advisory unless repository
policy explicitly grants that reviewer approval authority.

#### Merge responsibility

A repository maintainer or other human with merge authority owns the merge. The
merger confirms required approvals and checks, verifies the target branch and
commit, resolves policy-compliant integration strategy, and records any accepted
exceptions. Agents must not merge to `main` unless an explicit task and repository
policy authorize that action.

#### Release responsibility

The designated release owner owns versioning, release approval, tagging,
publication, rollback readiness, and release communication. Merging to `main` does
not mean a release occurred. An agent may prepare release evidence or artifacts but
must not publish a release without explicit release authorization.

#### Integration safety

Before merge, compare the branch with the current verified base and rerun required
checks when the base changed materially. Repository policy determines whether the
approved integration uses merge, squash, or rebase; an agent must not select a
history-rewriting strategy merely for convenience.

## 5. Reporting Rules

### Required reporting

Task updates and final reports must make the evidence boundary understandable.
Include, when relevant:

- repository and remote used;
- current branch and exact commit;
- verified base or PR commit;
- source-of-truth declaration;
- discovery scope, including refs and paths checked;
- material pre-existing workspace changes;
- files changed and compatibility impact;
- highest verified Git delivery state;
- commit hash, push status, and remote verification status;
- validation performed and its result; and
- limitations, unresolved conflicts, or checks not performed.

Reports should be concise, but omission must not create a false impression of
freshness, completeness, approval, or repository-wide coverage.

### Prohibited reporting

Agents must not:

- declare a file nonexistent based only on the checked-out branch or filesystem;
- call a remote-tracking ref current without direct verification or a recorded
  fresh fetch;
- describe a workspace as clean while ignoring untracked or staged content;
- represent a PR branch as canonical mainline state;
- represent local generated content as committed or approved;
- claim that all versions were checked when only one ref was searched; or
- omit a failed network, permission, or discovery check that materially limits a
  conclusion.

### Evidence language

Prefer bounded, reproducible language:

- identify exact paths and commit IDs;
- distinguish “not found” from “does not exist”;
- distinguish “local,” “base,” “PR,” and “remote” state;
- identify whether evidence was directly verified or cached; and
- state inference explicitly when a conclusion is not a direct observation.

The agent owns the accuracy of its reported scope even when the final task output
is intentionally brief.
