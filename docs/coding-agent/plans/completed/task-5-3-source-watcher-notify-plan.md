# Task 5.3 Source Watcher Notify Plan

## Approval

User requested iterative execution from `z/tasks.md`; Orchestrator waived separate plan approval for this narrow mechanical task.

## Tasks

### Task_1

- type: impl
- owns:
  - `src/watcher/source_watcher.rs`
  - `z/tasks.md`
- depends_on: []
- acceptance:
  - `SourceWatcher` continues detecting task source file content changes and appending `task_source_file_changed` events.
  - After successful append/projection, `SourceWatcher` best-effort notifies the active Orchestrator Agent when one exists.
  - Notification failures or missing orchestrator sessions do not fail or stop the watcher.
  - `z/tasks.md` marks task 5.3 complete.
- validation:
  - required: true
    owner: worker
    kind: test
    detail: `cargo test source_watcher`
  - required: true
    owner: reviewer
    kind: review
    detail: Independent subagent review of the implementation and validation evidence.

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-24: Plan created; Task_1 started.
- 2026-05-24: Added best-effort active orchestrator notification after task source file change events are appended and projected.
- 2026-05-24: Worker validation passed: `rustfmt --check src/watcher/source_watcher.rs`; `cargo test source_watcher`.
- 2026-05-24: Reviewer validation passed with `APPROVED` and no findings.

## Decision Log

- 2026-05-24: Used the existing cmux `ui.notify` path and active orchestrator session rows as the notification mechanism, matching task watcher behavior.
