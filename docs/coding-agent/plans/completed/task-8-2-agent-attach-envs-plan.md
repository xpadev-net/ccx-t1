# Task 8.2 Agent Attach Env Injection Plan

## Context

Task 8.2 has tmux session creation and heartbeat implemented. The remaining mechanically verifiable item is injecting the expected nine CCX environment variables when `agent attach` launches a child process.

Repository rule context: `docs/coding-agent/rules/` is absent, so this plan uses harness default validation.

Research waived: `env_builder.rs` already defines the expected nine variables and unit tests; `launch.rs` already passes `LaunchSpec.envs` into `tmux.create_session`; `agent attach` is currently a skeleton.

## Task_1

- type: impl
- owns:
  - `src/cli/agent.rs`
  - `z/tasks.md`
- depends_on: []
- acceptance:
  - `agent attach` resolves the work execution and project metadata needed for launch.
  - `agent attach` builds envs with `build_agent_envs` and passes all nine populated values into `LaunchSpec`.
  - `agent attach` preserves existing JSON/non-JSON output shape while returning real session/tab IDs from launch.
  - Unit tests verify env injection into tmux for attach.
  - Mark task 8.2 complete in `z/tasks.md`.
- validation:
  - required: true
    owner: orchestrator
    kind: unit
    detail: targeted Rust tests for `agent attach` env injection
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
- 2026-05-25: Implemented attach env injection through `build_agent_envs` and `launch_agent`.
- 2026-05-25: Validated with `rtk rustfmt --check src/cli/agent.rs`, `rtk cargo test attach_injects_all_agent_envs_into_tmux_launch`, and `rtk cargo test env_builder`.
- 2026-05-25: Harness reviewer returned APPROVED with no findings.

## Decision Log

- 2026-05-25: Keep the change narrow by wiring `agent attach` through existing `build_agent_envs` and `launch_agent` rather than changing tmux/env semantics.
