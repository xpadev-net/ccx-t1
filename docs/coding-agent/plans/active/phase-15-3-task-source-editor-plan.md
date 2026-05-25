# Phase 15.3 Task Source Editor Plan

## Context

Implement the next unchecked item in `z/tasks.md`: `15.3 GUI 手動編集ビュー`.

Repository rule suite status: `docs/coding-agent/rules/` is absent on `master`; proceeding under harness defaults with this gap recorded.

Quality routing note:
- Routing level: L2
- In-scope docs: orchestration harness, plan-format, engineering language routing/testing, Rust baseline for CLI contract awareness, backend/frontend cross-boundary baseline for CLI-to-Swift contract.
- Out-of-scope docs: web framework/browser E2E references because this is a macOS Swift/AppKit UI, not a web UI.
- Top risks: data integrity and concurrency via expected-hash saves; contract compatibility between Rust JSON and Swift decoding; UI state drift.

## Task_1

- type: impl
- owns:
  - `gui/Sources/CCX/CCXControllerCLI.swift`
  - `gui/Sources/CCX/CCXTaskSourceStore.swift`
  - `gui/Sources/CCX/CCXTasksView.swift`
  - `gui/Resources/Localizable.xcstrings`
  - `gui/Sources/CCX/README.md`
  - `gui/cmux.xcodeproj/project.pbxproj`
  - `gui/cmuxTests/CCXControllerCLITests.swift`
  - `gui/cmuxTests/CCXProjectPickerTests.swift`
  - `z/tasks.md`
  - `docs/coding-agent/plans/active/phase-15-3-task-source-editor-plan.md`
- depends_on: []
- acceptance:
  - `CCXControllerCLI` exposes typed task-source read/write/append wrappers matching the Rust JSON contract and supports stdin payloads for write/append.
  - `CCXTaskSourceStore` loads content/hash/mtime, tracks draft edits, saves with `expected-hash`, supports reload/discard, and reports hash conflicts without overwriting local draft.
  - Tasks tab shows a Markdown editing surface plus Save, Reload, and Discard controls wired to the store.
  - Save conflict surfaces a clear non-destructive message and leaves draft content intact for user resolution.
  - This PR includes the `z/tasks.md` 15.3 checklist update after validation and subagent review pass.
- validation:
  - required: true
    owner: orchestrator
    kind: static
    detail: `rtk git diff --check`, `rtk jq empty gui/Resources/Localizable.xcstrings`, and `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`.
  - required: true
    owner: orchestrator
    kind: test
    detail: Focused Xcode `build-for-testing` or equivalent targeted unit test command covering `CCXControllerCLITests` and task-source store tests.
  - required: true
    owner: reviewer
    kind: review
    detail: Independent harness reviewer verifies implementation, tests, data integrity behavior, and validation evidence before PR creation.

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-26: Created plan after merging Phase 15.2 PR and starting branch `codex/task-source-gui-editor`.

## Decision Log

- 2026-05-26: Treating 15.3 as non-trivial because it adds GUI state, CLI stdin/JSON contract wrappers, and optimistic-concurrency save behavior.
