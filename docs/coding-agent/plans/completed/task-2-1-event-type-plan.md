# Task 2.1 EventType Plan

## Approval

User requested iterative execution from `z/tasks.md`; Orchestrator waived separate plan approval for this narrow mechanical task.

## Tasks

### Task_1

- type: impl
- owns:
  - `src/domain/event.rs`
  - `z/tasks.md`
- depends_on: []
- acceptance:
  - `src/domain/event.rs` defines an explicit `EventType` enum matching the task ledger's 3.1 list.
  - `EventData` can expose its corresponding `EventType` without changing the existing JSON event shape.
  - Tests verify `EventType` serde naming and `EventData` to `EventType` mapping.
  - `z/tasks.md` marks task 2.1 complete once validation passes.
- validation:
  - required: true
    owner: worker
    kind: test
    detail: `cargo test event`
  - required: true
    owner: reviewer
    kind: review
    detail: Independent subagent review of the implementation and validation evidence.

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-25: Plan created; Task_1 started.
- 2026-05-25: Added `EventType`, `EventData::event_type()`, serde/mapping tests, and marked `z/tasks.md` task 2.1 complete.
- 2026-05-25: Worker validation passed: `rustfmt --check src/domain/event.rs`; `cargo test event`. Repository-wide `cargo fmt --check` was not used as required evidence because pre-existing formatting drift outside the task scope fails it.
- 2026-05-25: Reviewer validation passed with `APPROVED` and no findings.

## Decision Log

- 2026-05-25: Kept the task local and narrow because the ledger says `EventData` already exists and only the explicit enum is missing.
