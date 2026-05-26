# Phase 15.6 WorkExecution Create Link Plan

## Context

Implement the next unchecked item in `z/tasks.md`: `15.6 WorkExecution 作成連携`.

Repository rule suite status: `docs/coding-agent/rules/` is absent on `master`; proceeding under harness defaults with this gap recorded.

Quality routing note:
- Routing level: L3
- In-scope docs: orchestration harness, plan-format, Rust event/projection/git worktree patterns, Swift GUI state/localization patterns.
- Out-of-scope docs: browser E2E references because this is a native macOS Swift/AppKit UI.
- Top risks: partial filesystem/git creation, event ordering, branch/worktree collisions, SwiftUI text-selection limits, and accidental destructive git behavior.

## Task_1

- type: impl
- owns:
  - `src/cli/work.rs`
  - `src/git/worktree.rs`
  - `tests/e2e_fake_commands.rs`
  - `gui/Sources/CCX/CCXControllerCLI.swift`
  - `gui/Sources/CCX/CCXModels.swift`
  - `gui/Sources/CCX/CCXTaskSourceStore.swift`
  - `gui/Sources/CCX/CCXTasksView.swift`
  - `gui/Sources/CCX/CCXDashboardView.swift`
  - `gui/Resources/Localizable.xcstrings`
  - `gui/cmuxTests/CCXControllerCLITests.swift`
  - `gui/cmuxTests/CCXTaskSourceStoreTests.swift`
  - `z/tasks.md`
  - `docs/coding-agent/plans/active/phase-15-6-work-execution-create-link-plan.md`
- depends_on: []
- acceptance:
  - `ccx work create` creates a real WorkExecution row through JSONL projection, writes `task.md`, creates branch/worktree, and returns real absolute paths.
  - Tasks tab can choose a parsed Markdown heading or checkbox task and pass it to `ccx work create`.
  - Created WorkExecution can be followed by Worker attach and prompt from the GUI path.
  - GUI exposes clear status/error feedback for create/attach/prompt actions.
  - This PR includes the `z/tasks.md` 15.6 checklist update after validation and subagent review pass.
- validation:
  - required: true
    owner: orchestrator
    kind: static
    detail: `rtk rustfmt --check src/cli/work.rs src/git/worktree.rs tests/e2e_fake_commands.rs`, `rtk git diff --check`, `rtk jq empty gui/Resources/Localizable.xcstrings`, and `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`.
  - required: true
    owner: orchestrator
    kind: test
    detail: `rtk cargo test work_create`, `rtk cargo test worktree`, and `rtk cargo test work_create --test e2e_fake_commands` pass.
  - required: true
    owner: orchestrator
    kind: test
    detail: Focused Swift tests for `CCXControllerCLITests` and `CCXTaskSourceStoreTests` pass.
  - required: true
    owner: reviewer
    kind: review
    detail: Independent harness reviewer verifies implementation, tests, event ordering, GUI create/attach path, and validation evidence before PR creation.

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-26: Created plan after merging Phase 15.5 PR and starting branch `codex/work-execution-create-link`.
- 2026-05-26: Addressed pre-PR reviewer findings by making WorkExecution event emission causal, serializing `task.md` front matter through YAML, and blocking GUI WorkExecution creation from dirty task-source drafts.
- 2026-05-26: Addressed re-review append-failure finding by rolling back failed event batches under lock and cleaning materialized artifacts only when the rollback is confirmed.

## Decision Log

- 2026-05-26: Use heading/checkbox candidate selection as the GUI MVP instead of raw `TextEditor` selection ranges, because SwiftUI `TextEditor` does not expose robust selection metadata.
- 2026-05-26: Keep git worktree creation free of event-log side effects; `work create` now appends the WorkExecution parent and artifact events together in the required order after materialization succeeds.
- 2026-05-26: Treat event append failures as either rolled-back or indeterminate; indeterminate failures leave material artifacts intact so JSONL cannot claim deleted files.
