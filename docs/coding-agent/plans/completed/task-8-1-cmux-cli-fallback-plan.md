# Task 8.1 Cmux CLI Fallback Plan

## Context

`z/tasks.md` task 8.1 has the `CmuxAdapter` trait, Unix socket implementation, and headless fallback complete. The remaining mechanically verifiable item is the `cmux` CLI fallback implementation.

Repository rule context: `docs/coding-agent/rules/` is absent, so this plan uses harness default validation.

Research waived: existing `cmux_adapter.rs` and `gui/CLI/cmux.swift` confirm `cmux rpc <method> [json-params]` prints JSON returned by the v2 RPC client.

## Task_1

- type: impl
- owns:
  - `src/agent_runtime/cmux_adapter.rs`
  - `z/tasks.md`
- depends_on: []
- acceptance:
  - Add a `CliCmuxAdapter` that invokes `cmux rpc <method> <json-params>` and parses JSON responses.
  - Wire adapter factory fallback order as socket, CLI, then headless.
  - Preserve headless behavior when the CLI is unavailable or the CLI call fails at runtime.
  - Add unit tests covering successful CLI RPC calls and CLI-to-headless fallback.
  - Mark task 8.1 complete in `z/tasks.md`.
- validation:
  - required: true
    owner: orchestrator
    kind: unit
    detail: `rtk cargo test cmux_adapter`
  - required: true
    owner: orchestrator
    kind: format
    detail: `rtk rustfmt --check src/agent_runtime/cmux_adapter.rs`
  - required: true
    owner: reviewer
    kind: review
    detail: independent subagent review of touched files and validation evidence

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-25: Plan created. Implementing Task_1 directly under user-requested automation loop.
- 2026-05-25: Implemented CLI fallback, updated `z/tasks.md`, and validated with `rtk rustfmt --check src/agent_runtime/cmux_adapter.rs` plus `rtk cargo test cmux_adapter`.
- 2026-05-25: Harness reviewer returned APPROVED with no findings.
- 2026-05-25: Addressed `gh-review-hook` findings by adding a CLI subprocess timeout, avoiding headless success after CLI state is established, and replacing the fragile `help rpc` probe with `--version`.
- 2026-05-25: Addressed follow-up `gh-review-hook` findings by serializing fallback mode transitions under one lock and replacing timeout polling with a deadline helper thread.

## Decision Log

- 2026-05-25: Keep CLI fallback best-effort by wrapping it with headless fallback, so a present but unusable `cmux` binary does not break headless operation.
