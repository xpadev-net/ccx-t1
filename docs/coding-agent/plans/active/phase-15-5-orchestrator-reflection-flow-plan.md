# Phase 15.5 Orchestrator Reflection Flow Plan

## Context

Implement the next unchecked item in `z/tasks.md`: `15.5 Orchestrator 反映フロー`.

Repository rule suite status: `docs/coding-agent/rules/` is absent on `master`; proceeding under harness defaults with this gap recorded.

Quality routing note:
- Routing level: L2
- In-scope docs: orchestration harness, plan-format, Rust event/projection patterns, Swift GUI state/localization patterns.
- Out-of-scope docs: browser E2E references because this is a native macOS Swift/AppKit UI.
- Top risks: silently missing external task-source updates, overwriting dirty GUI drafts on reload, and ambiguous composer reflection failures.

## Task_1

- type: impl
- owns:
  - `src/cli/task_source.rs`
  - `src/cli/mod.rs`
  - `src/watcher/source_watcher.rs`
  - `gui/Sources/CCX/CCXControllerCLI.swift`
  - `gui/Sources/CCX/CCXModels.swift`
  - `gui/Sources/CCX/CCXDashboardView.swift`
  - `gui/Sources/CCX/CCXProjectStore.swift`
  - `gui/Sources/CCX/CCXTaskSourceStore.swift`
  - `gui/Sources/CCX/CCXTasksView.swift`
  - `gui/Resources/Localizable.xcstrings`
  - `gui/cmuxTests/CCXControllerCLITests.swift`
  - `gui/cmuxTests/CCXProjectsStoreTests.swift`
  - `gui/cmuxTests/CCXTaskSourceStoreTests.swift`
  - `tests/e2e_fake_commands.rs`
  - `z/tasks.md`
  - `docs/coding-agent/plans/active/phase-15-5-orchestrator-reflection-flow-plan.md`
- depends_on: []
- acceptance:
  - CLI task-source updates used by the Orchestrator record `task_source_file_changed` with the updated file hash.
  - GUI recent-event loading decodes Rust `event_type` and `occurred_at` fields, including `task_source_file_changed`.
  - Tasks tab treats a task-source change event for the current project/file as a reload signal: clean drafts reload automatically, dirty drafts preserve edits and show a reload-required message.
  - Reloaded task-source content displays appended headings, checkboxes, and anchors in the existing Markdown editor.
  - Composer/reflection failures keep a dedicated Orchestrator error surface instead of task-source write copy.
  - This PR includes the `z/tasks.md` 15.5 checklist update after validation and subagent review pass.
- validation:
  - required: true
    owner: orchestrator
    kind: static
    detail: `rtk rustfmt --check src/cli/task_source.rs src/cli/mod.rs src/watcher/source_watcher.rs tests/e2e_fake_commands.rs`, `rtk git diff --check`, `rtk jq empty gui/Resources/Localizable.xcstrings`, and `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`.
  - required: true
    owner: orchestrator
    kind: static
    detail: `rtk cargo clippy --bin ccx` completes with 0 errors; existing warnings are outside this task scope.
  - required: true
    owner: orchestrator
    kind: test
    detail: `rtk cargo test task_source` and `rtk cargo test source_watcher` pass.
  - required: true
    owner: orchestrator
    kind: test
    detail: `rtk cargo test task_source --test e2e_fake_commands` passes for CLI-mediated reflection E2E coverage.
  - required: true
    owner: orchestrator
    kind: test
    detail: Focused Xcode tests for `CCXControllerCLITests`, `CCXProjectsStoreTests`, and `CCXTaskSourceStoreTests` pass.
  - required: true
    owner: reviewer
    kind: review
    detail: Independent harness reviewer verifies implementation, tests, reflection/reload behavior, and validation evidence before PR creation.

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-26: Created plan after merging Phase 15.4 PR and starting branch `codex/task-source-orchestrator-reflection`.

## Decision Log

- 2026-05-26: Scope the mechanically verifiable 15.5 work to CLI-mediated Orchestrator task-source updates and GUI event-backed reload behavior. A fully autonomous long-lived source watcher daemon remains a later hardening path unless required by review.
