# Task 5.2 Status Priority Plan

## Approval

User requested iterative execution from `z/tasks.md`; Orchestrator waived separate plan approval for this narrow mechanical subtask.

## Tasks

### Task_1

- type: impl
- owns:
  - `src/domain/event.rs`
  - `src/watcher/task_watcher.rs`
  - `z/tasks.md`
- depends_on: []
- acceptance:
  - `work_execution_task_file_changed` events include machine-readable status-change and notification-priority metadata.
  - The watcher marks unchanged status updates as low priority while retaining audit events for changed content.
  - Existing JSONL replay remains tolerant of older events without the new metadata.
  - `z/tasks.md` marks only the completed 5.2 priority-filtering subtask complete.
- validation:
  - required: true
    owner: worker
    kind: test
    detail: `cargo test task_watcher event`
  - required: true
    owner: reviewer
    kind: review
    detail: Independent subagent review of the implementation and validation evidence.

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-24: Plan created; Task_1 started.
- 2026-05-24: Added status-change and notification-priority metadata to task file change events, with watcher state tracking for unchanged status changes.
- 2026-05-24: Worker validation passed: `rustfmt --check src/domain/event.rs src/watcher/task_watcher.rs`; `cargo test task_watcher`; `cargo test event`.
- 2026-05-24: Reviewer validation passed with `APPROVED` and no findings.

## Decision Log

- 2026-05-24: Scoped this pass to the first unchecked 5.2 subtask because full orchestrator notification delivery is a separate integration task.
