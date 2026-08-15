# Change Management Protocol

## Purpose

Define how changes are classified and reviewed in CC Switch Agent OS.

## Change Types

### Documentation Change

Examples:

- guides
- commands
- reviews
- context updates

Requires:

- correct location
- validation

### Architecture Change

Examples:

- module boundaries
- runtime abstractions
- data models

Requires:

- architecture document update
- ADR when decision is durable
- review before implementation

### Product Code Change

Examples:

- Rust backend
- React frontend
- runtime implementation

Requires:

- task definition
- validation
- delivery status

## Agent Rules

Agents must not mix unrelated changes.

Large architectural decisions should happen before implementation.

## Review Requirement

The larger the impact, the stronger the review requirement.

- Low impact: self validation
- Medium impact: agent review
- High impact: architecture review
