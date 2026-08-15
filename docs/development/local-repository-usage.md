# Local Repository Usage Protocol

## Purpose

Define how AI agents should use local repository access when available.

Local repository access is an execution and inspection capability. It is not the final source of truth.

## Source of Truth Priority

1. User explicitly specified repository, branch, commit, or PR.
2. Verified GitHub remote state.
3. Local committed repository state.
4. Local uncommitted workspace.
5. Agent memory.

## Before Using Local Repository

Agents must confirm:

- repository identity
- current branch
- current commit
- remote configuration
- workspace dirty state

Required checks:

- git remote -v
- git branch --show-current
- git status
- git log --oneline

## Local Changes

Uncommitted local changes must be treated as an overlay.

Agents must not:

- assume local files exist remotely
- overwrite unknown local modifications
- declare repository state based only on local files

## Remote Verification

Important conclusions must be verified against GitHub when possible.

Examples:

Incorrect:

"File does not exist."

Correct:

"File does not exist in the checked local branch. Remote verification required."

## Reporting

Agents should report:

- local repository
- branch
- commit
- dirty status
- remote synchronization status
