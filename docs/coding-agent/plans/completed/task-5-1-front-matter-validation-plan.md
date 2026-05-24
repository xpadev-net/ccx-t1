# Task 5.1 Front Matter Validation Plan

## Approval

User requested iterative execution from `z/tasks.md`; Orchestrator waived separate plan approval for this narrow mechanical task.

## Tasks

### Task_1

- type: impl
- owns:
  - `src/watcher/front_matter.rs`
  - `src/watcher/task_watcher.rs`
  - `z/tasks.md`
- depends_on: []
- acceptance:
  - Front matter parsing only accepts known task.md status values from the task ledger.
  - Missing, empty, non-string, or unknown status values are handled defensively without panics.
  - Malformed YAML behavior remains compatible with existing best-effort watcher handling.
  - `z/tasks.md` marks task 5.1 complete once validation passes.
- validation:
  - required: true
    owner: worker
    kind: test
    detail: `cargo test front_matter task_watcher`
  - required: true
    owner: reviewer
    kind: review
    detail: Independent subagent review of the implementation and validation evidence.

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-24: Plan created; Task_1 started.
- 2026-05-24: Added status allow-list validation for task.md front matter and watcher coverage for invalid status values.
- 2026-05-24: Worker validation passed: `rustfmt --check src/watcher/front_matter.rs src/watcher/task_watcher.rs`; `cargo test front_matter`; `cargo test task_watcher`.
- 2026-05-24: Reviewer validation passed with `APPROVED` and no findings.

## Decision Log

- 2026-05-24: Scoped validation to parser sanitization because front matter parsing already exists and the ledger's remaining unchecked item is defensive validation.
