# Task 5.2 Orchestrator Notify Plan

## Approval

User requested iterative execution from `z/tasks.md`; Orchestrator waived separate plan approval for this narrow mechanical subtask.

## Tasks

### Task_1

- type: impl
- owns:
  - `src/watcher/task_watcher.rs`
  - `z/tasks.md`
- depends_on: []
- acceptance:
  - After appending and projecting a task-file-changed event, the watcher best-effort notifies the active Orchestrator Agent when one exists.
  - Notification level reflects event priority: normal changes use info, low-priority unchanged-status changes use a lower/noisy-safe level.
  - Notification failures do not fail or stop the watcher.
  - `z/tasks.md` marks task 5.2 complete while leaving later tasks untouched.
- validation:
  - required: true
    owner: worker
    kind: test
    detail: `cargo test task_watcher`
  - required: true
    owner: reviewer
    kind: review
    detail: Independent subagent review of the implementation and validation evidence.

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-24: Plan created; Task_1 started.
- 2026-05-24: Added best-effort active orchestrator notification after task-file-changed append/projection, skipping low-priority events.
- 2026-05-24: Worker validation passed: `rustfmt --check src/watcher/task_watcher.rs`; `cargo test task_watcher`.
- 2026-05-24: Reviewer validation passed with `APPROVED` and no findings.

## Decision Log

- 2026-05-24: Used existing `CmuxAdapter::notify_user` and active orchestrator session rows as the channel contract because no separate watcher-specific notification channel exists.
