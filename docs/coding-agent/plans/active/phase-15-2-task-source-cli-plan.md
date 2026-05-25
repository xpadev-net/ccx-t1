# Phase 15.2 Task Source CLI Plan

## Context

Implement the next unchecked item in `z/tasks.md`: `15.2 Task Source 読み書き CLI`.

Repository rule suite status: `docs/coding-agent/rules/` is absent on `master`; proceeding under harness defaults with this gap recorded.

## Task_1

- type: impl
- owns:
  - `src/**`
  - `tests/**`
  - `Cargo.toml`
  - `Cargo.lock`
  - `z/tasks.md`
  - `docs/coding-agent/plans/active/phase-15-2-task-source-cli-plan.md`
- depends_on: []
- acceptance:
  - `ccx task-source read --project-id <id> --json` returns task source path, content, hash, and mtime as JSON.
  - `ccx task-source write --project-id <id> --expected-hash <hash> --stdin --json` writes stdin only when the expected hash matches and returns explicit JSON output.
  - `ccx task-source append --project-id <id> --expected-hash <hash> --stdin --json` appends stdin only when the expected hash matches and returns append position plus updated hash.
  - Hash conflict and missing/unreadable task source errors exit non-zero with explicit error text or JSON according to existing CLI patterns.
  - When the task source file is inside the canonical repo, successful read/write/append JSON includes a dirty-state warning.
  - `z/tasks.md` marks 15.2 and its subitems complete only after validation and review pass.
- validation:
  - required: true
    owner: worker
    kind: test
    detail: Run focused Rust tests covering read/write/append, conflict, missing file, and dirty warning behavior.
  - required: true
    owner: orchestrator
    kind: static
    detail: Run `rtk cargo fmt --check` and a focused `rtk cargo test ...` command selected from repo patterns.
  - required: true
    owner: reviewer
    kind: review
    detail: Independent harness reviewer verifies implementation, tests, and task checklist before PR creation.

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-26: Created plan after merging Phase 15.1 PR and checking out updated `master`.
- 2026-05-26: Implemented `task-source read/write/append`, added compiled-binary E2E coverage, and validated with focused `cargo test task_source`. Repo-wide `cargo fmt --check` is blocked by pre-existing formatting drift, so targeted rustfmt is used for touched Rust files.

## Decision Log

- 2026-05-26: Treating 15.2 as non-trivial because it adds CLI behavior, file writes, conflict detection, and tests.
