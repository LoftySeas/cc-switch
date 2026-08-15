# Agent Evidence Protocol

## Purpose

Agent completion reports must be supported by verifiable evidence.

## Required Evidence

### File Changes

Report:

- file paths
- change type
- validation result

### Git Commit

Report:

- commit SHA
- commit message
- branch

### Remote Delivery

Report:

- push status
- remote branch verification
- remote commit SHA

### Tests

Report:

- command executed
- result
- logs or artifacts when available

## Rule

A statement such as "completed" is insufficient without evidence.

The final status must distinguish:

- Workspace Modified
- Commit Created
- Pushed Remote
- Remote Verified
