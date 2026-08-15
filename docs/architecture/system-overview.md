# CC Switch Agent OS System Overview

## Current State

CC Switch currently provides AI runtime and provider management capabilities.

## Target Architecture

```
CC Switch Agent OS

+---------------------------+
| User Interface             |
+---------------------------+

+---------------------------+
| Workflow Layer             |
+---------------------------+

+---------------------------+
| Agent Organization Layer   |
| Agent / Role / Team        |
+---------------------------+

+---------------------------+
| Agent Runtime Layer        |
| Claude / Codex / Gemini    |
+---------------------------+

+---------------------------+
| Provider and Configuration |
+---------------------------+
```

## Design Principles

- Runtime is separated from Role.
- Models are replaceable resources.
- Workflow controls collaboration.
- Context is explicitly managed.
- Expensive models are reserved for high-value decisions.

## Evolution Strategy

Preserve existing CC Switch functionality and incrementally add Agent OS capabilities.
