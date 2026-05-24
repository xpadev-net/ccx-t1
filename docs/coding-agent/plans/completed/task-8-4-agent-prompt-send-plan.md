# Task 8.4 Agent Prompt Send Plan

## Context

`src/agent_runtime/prompt.rs` already implements text/file/stdin message reading and tmux buffer paste delivery. The remaining mechanically verifiable task is wiring `ccx agent prompt` to that implementation.

Repository rule context: `docs/coding-agent/rules/` is absent, so this plan uses harness default validation.

Research waived: the CLI prompt handler is a skeleton and can be connected directly to existing prompt runtime functions.

## Task_1

- type: impl
- owns:
  - `src/cli/agent.rs`
  - `z/tasks.md`
- depends_on: []
- acceptance:
  - `agent prompt --message` sends inline text through the prompt runtime.
  - `agent prompt --message-file` reads the file and sends its content.
  - `agent prompt --stdin` continues to use the existing stdin reader.
  - Unit tests verify inline and file sources without invoking real tmux.
  - Mark task 8.4 complete in `z/tasks.md`.
- validation:
  - required: true
    owner: orchestrator
    kind: unit
    detail: targeted Rust tests for `agent prompt`
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
- 2026-05-25: Wired `agent prompt` to `read_message` and `send_to_tmux`, with sender injection for tests.
- 2026-05-25: Validated with `rtk rustfmt --check src/cli/agent.rs`, `rtk cargo test prompt_sends_`, and `rtk cargo test read_message`.
- 2026-05-25: Harness reviewer returned APPROVED with no findings.
- 2026-05-25: Addressed `gh-review-hook` feedback by explicitly handling `--stdin` in source selection.

## Decision Log

- 2026-05-25: Keep tests tmux-free by injecting a prompt sender function into the CLI handler.
