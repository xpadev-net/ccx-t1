# Task 14.3 CCX Project Picker Plan

## Context

- Repository rule files under `docs/coding-agent/rules/` are absent, so this plan uses the orchestration harness defaults.
- Research waived: 14.1 and 14.2 established CCX panel wiring, launch argument handling, and project index loading.

## Task_1

- type: impl
- owns:
  - `gui/Sources/CCX/CCXDashboardPanel.swift`
  - `gui/Sources/CCX/CCXDashboardView.swift`
  - `gui/Sources/CCX/CCXProjectPickerView.swift`
  - `gui/Sources/CCX/CCXAppDelegateBridge.swift`
  - `gui/Sources/Panels/PanelContentView.swift`
  - `gui/Sources/WorkspaceContentView.swift`
  - `gui/cmux.xcodeproj/project.pbxproj`
  - `gui/cmuxTests/CCXProjectPickerTests.swift`
  - `gui/Resources/Localizable.xcstrings`
  - `z/tasks.md`
  - `docs/coding-agent/plans/active/task-14-3-project-picker-plan.md`
- depends_on: []
- acceptance:
  - Launching without `--project-id` opens a CCX project picker panel instead of no-oping.
  - The picker lists projects from `CCXProjectsStore`, opens a selected project in a new `CCXDashboardPanel`, and exposes an Add Project row that presents the 14.4 placeholder sheet.
  - The dashboard header exposes a project switch menu populated from `CCXProjectsStore`; choosing a different project opens it in a new dashboard tab.
  - Unit tests cover the panel mode/title behavior and picker row view model behavior where mechanically testable.
- validation:
  - required: true
    owner: orchestrator
    kind: lint
    detail: `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`
  - required: true
    owner: orchestrator
    kind: lint
    detail: `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`
  - required: true
    owner: orchestrator
    kind: test
    detail: Swift typecheck for touched CCX files and targeted `xcodebuild` for `cmuxTests/CCXProjectPickerTests`, with checkout dependency caveat recorded if `gui/ghostty` remains absent.
  - required: true
    owner: reviewer
    kind: review
    detail: independent subagent review before PR.
  - required: true
    owner: orchestrator
    kind: gate
    detail: `gh-review-hook` must exit 0 before merge.

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-25: Created plan and started Task_1.
- 2026-05-25: Implemented picker-mode `CCXDashboardPanel`, `CCXProjectPickerView`, dashboard project switch menu, launch-without-project routing, tests, localization, pbxproj wiring, and task checklist updates.
- 2026-05-25: Validation passed: `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`; `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`; `git diff --check`; localization JSON parse.
- 2026-05-25: Targeted `xcodebuild` for `cmuxTests/CCXProjectPickerTests` compiled the touched Swift files but stopped before test execution with existing checkout dependency error: `Ghostty submodule is missing at /Users/xpadev/IdeaProjects/ccx-t1/gui/ghostty`.
- 2026-05-25: Reviewer requested explicit SwiftUI tracking for `CCXProjectsStore`; updated picker and project switch menu to hold the store with `@Bindable`.
- 2026-05-25: Reviewer subagent approved the updated change set.
- 2026-05-25: `gh-review-hook` requested preserving the launch intent guard and removing internal milestone copy. Added `CCXLaunchArguments.isCCXLaunch`, gated bridge presentation on CCX flags, covered parsing with tests, and changed placeholder text to user-facing copy.
- 2026-05-25: Hook-fix validation passed: `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`; `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`; localization JSON parse; `git diff --check`. Targeted `xcodebuild` again compiled touched Swift files and stopped before test execution on the existing missing `gui/ghostty` checkout dependency.
- 2026-05-25: `gh-review-hook` requested an empty `displaySlug` fallback in the project switch menu; updated the menu label to fall back to `projectId`. Reviewer subagent approved. `git diff --check` passed. Targeted `xcodebuild` compiled the touched Swift file and again stopped on the existing missing `gui/ghostty` checkout dependency.
- 2026-05-25: `gh-review-hook` requested avoiding per-panel `CCXProjectsStore` watchers; added workspace-level shared store injection into `CCXDashboardPanel` with unit coverage. Reviewer subagent approved. `git diff --check` passed. Targeted `xcodebuild` compiled touched Swift/test files and again stopped on the existing missing `gui/ghostty` checkout dependency.

## Decision Log

- 2026-05-25: Use a picker-mode `CCXDashboardPanel` to reuse cmux panel lifecycle and avoid adding a new panel type for the welcome screen.
