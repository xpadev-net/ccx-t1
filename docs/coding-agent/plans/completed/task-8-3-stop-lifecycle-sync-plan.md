# Task 8.3 Stop Lifecycle Sync Plan

## Context

Task 8.3 has the two-stage launch sequence complete. The remaining mechanically verifiable part is ensuring `ccx agent stop` synchronizes lifecycle cleanup by stopping tmux and closing the cmux tab.

Repository rule context: `docs/coding-agent/rules/` is absent, so this plan uses harness default validation.

Research waived: `src/cli/agent.rs` already implements stop behavior using `ShellTmuxAdapter` and `make_adapter`; the narrow gap is testability and ledger completion.

## Task_1

- type: test
- owns:
  - `src/cli/agent.rs`
  - `z/tasks.md`
- depends_on: []
- acceptance:
  - `agent stop` can be exercised with injected tmux/cmux adapters.
  - A unit test verifies the stop path appends `AgentSessionStopped`, calls `tmux.kill_session`, and calls `cmux.close_tab`.
  - Existing public `stop` behavior remains unchanged.
  - Mark task 8.3 complete in `z/tasks.md`.
- validation:
  - required: true
    owner: orchestrator
    kind: unit
    detail: targeted Rust test for `agent stop` lifecycle sync
  - required: true
    owner: orchestrator
    kind: format
    detail: `rtk rustfmt --check src/cli/agent.rs`
  - required: true
    owner: reviewer
    kind: review
    detail: independent subagent review of touched files and validation evidence

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-25: Plan created. Implementing Task_1 directly under user-requested automation loop.
- 2026-05-25: Extracted stop path for adapter injection and added lifecycle sync test.
- 2026-05-25: Validated with `rtk rustfmt --check src/cli/agent.rs` and `rtk cargo test stop_kills_tmux_closes_cmux_and_records_event`.
- 2026-05-25: Harness reviewer returned APPROVED with no findings.

## Decision Log

- 2026-05-25: Treat cmux tab-closed detection as outside the mechanically verifiable scope here; the task wording allows the `ccx agent stop` path, and that path is testable.
