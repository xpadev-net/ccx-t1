# Phase 15.4 Task Source Composer Plan

## Context

Implement the next unchecked item in `z/tasks.md`: `15.4 自然言語 Task 追加 Composer`.

Repository rule suite status: `docs/coding-agent/rules/` is absent on `master`; proceeding under harness defaults with this gap recorded.

Quality routing note:
- Routing level: L2
- In-scope docs: orchestration harness, plan-format, Swift UI state/layout baseline, localization baseline, CLI-to-Swift contract validation.
- Out-of-scope docs: browser E2E references because this is a native macOS Swift/AppKit UI.
- Top risks: prompt contract drift, accidentally bypassing Orchestrator ownership of task-source updates, stale project/session state, and user-facing error localization.

## Task_1

- type: impl
- owns:
  - `gui/Sources/CCX/CCXControllerCLI.swift`
  - `gui/Sources/CCX/CCXModels.swift`
  - `gui/Sources/CCX/CCXTaskSourceStore.swift`
  - `gui/Sources/CCX/CCXTasksView.swift`
  - `gui/Sources/CCX/CCXDashboardView.swift`
  - `src/cli/agent.rs` (`agent start-orchestrator`, fd-lock admission guard, active session detection, project config loading)
  - `gui/Resources/Localizable.xcstrings`
  - `gui/cmuxTests/CCXControllerCLITests.swift`
  - `gui/cmuxTests/CCXTaskSourceStoreTests.swift`
  - `z/tasks.md`
  - `docs/coding-agent/plans/active/phase-15-4-task-composer-plan.md`
- depends_on: []
- acceptance:
  - Tasks tab provides a natural-language task composer with a localized submit action.
  - Composer builds an Orchestrator prompt containing the task source file path, canonical repo, current WorkExecution state summary, desired append format, and the user's original request.
  - Composer sends the prompt to an active Orchestrator session when available; otherwise it starts one before prompting.
  - `agent start-orchestrator` loads project config, detects an existing active Orchestrator session, serializes concurrent admission with fd-lock, and returns a usable session before `agent prompt` is called.
  - Prompt text explicitly instructs the Orchestrator to inspect code, split/detail tasks when useful, update the task source file, and preserve the GUI original request.
  - This PR includes the `z/tasks.md` 15.4 checklist update after validation and subagent review pass.
- validation:
  - required: true
    owner: orchestrator
    kind: static
    detail: `rtk git diff --check`, `rtk jq empty gui/Resources/Localizable.xcstrings`, and `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`.
  - required: true
    owner: orchestrator
    kind: test
    detail: Focused Xcode test command covering `CCXControllerCLITests` and `CCXTaskSourceStoreTests`.
  - required: true
    owner: orchestrator
    kind: static
    detail: `rtk cargo clippy --bin ccx` completes with 0 errors for Rust CLI changes; warning cleanup is outside this task scope.
  - required: true
    owner: orchestrator
    kind: test
    detail: `rtk cargo test start_orchestrator` passes, including the concurrent fd-lock/session-detection regression.
  - required: true
    owner: orchestrator
    kind: test
    detail: `rtk cargo test prompt_sends` and `rtk cargo test task_source --test e2e_fake_commands` pass for related CLI contracts.
  - required: true
    owner: reviewer
    kind: review
    detail: Independent harness reviewer verifies implementation, tests, prompt contract, and validation evidence before PR creation.
  - required: true
    owner: tester
    kind: review
    detail: Verify recorded command output/logs show the required Swift, Rust, static, and orchestrator admission checks passed.

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-26: Created plan after merging Phase 15.3 PR and starting branch `codex/task-source-composer`.

## Decision Log

- 2026-05-26: Keep GUI writes indirect for natural-language additions: the GUI sends an Orchestrator prompt instead of appending directly to the task source.
