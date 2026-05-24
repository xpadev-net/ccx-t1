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
  - Fall back to headless only before CLI mode is established, including when the CLI is unavailable or the initial CLI call fails.
  - Propagate runtime CLI errors after CLI mode has been established instead of reporting a mixed headless success.
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
- 2026-05-25: Addressed final timeout/probe follow-up by making CLI availability a non-executing executable check and only treating SIGKILL from the timeout path as a timeout.
- 2026-05-25: Addressed final timer-thread discriminator feedback by killing only on `RecvTimeoutError::Timeout`.
- 2026-05-25: Addressed process-control feedback by replacing `wait_with_output` and external `kill` with capped stream readers and `Child::kill`, and clarified fallback acceptance criteria.
- 2026-05-25: Addressed orphan-process feedback by killing the child before process monitor error returns.
- 2026-05-25: Addressed waiter polling feedback by switching the process monitor to `waitpid`.
- 2026-05-25: Addressed fallback lock feedback by using an `Establishing` state with a condition variable, so only initial mode selection waits while established CLI calls run without holding the mode lock.
- 2026-05-25: Addressed response-shape feedback by returning `Null` for result-less JSON-RPC envelopes while preserving raw CLI result objects, and documented the `waitpid`/`Child::kill` split.
- 2026-05-25: Addressed condvar panic feedback by adding an establishing guard, and avoided double-reaping by letting the waiter own reaping after `Child::kill`.
- 2026-05-25: Addressed recycled-PID feedback by making `kill_child` a no-op when `try_wait` reports the waiter already reaped the process.

## Decision Log

- 2026-05-25: Keep CLI fallback best-effort only until CLI mode is established. After a successful CLI operation, later CLI runtime failures propagate as errors to avoid mixing real cmux workspace state with headless tab state.
