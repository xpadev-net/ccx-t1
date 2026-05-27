# Phase 16.4 Merge failure state transition plan

## Context

Next unchecked portion in `z/tasks.md` requires end-to-end completion handling for worker merge outcomes. The smallest safe increment is to ensure merge failures are durably reflected as `failed` state transitions so downstream orchestrator observers can treat merge outcome as an explicit terminal state.

## Task_1

- type: impl
- owns:
  - `src/git/github.rs`
  - `src/domain/transition.rs`
- depends_on: []
- acceptance:
  - `execute_merge` emits `WorkExecutionStateChanged` to `failed` only for failures occurring after `merging` starts.
  - Merge failures that occur before entering `merging` continue to emit existing `MergeFailed` without rewriting work-execution state.
  - `WorkExecutionState` transition rules allow `merging -> failed` as an explicit interruption transition.
  - `z/tasks.md` is updated only if this work scope is now complete.
- validation:
  - required: true
    owner: orchestrator
    kind: static
    detail: `rtk rustfmt --check src/git/github.rs src/domain/transition.rs`
  - required: true
    owner: orchestrator
    kind: test
    detail: `rtk cargo test abort_merge --lib` (or equivalent targeted equivalent for `src/git` domain tests)

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-27: Created plan for minimal phase 16.4 completion-state increment.

## Decision Log

- 2026-05-27: Limit scope to state transition durability first so orchestrator-side listeners can deterministically branch on terminal merge outcomes.
