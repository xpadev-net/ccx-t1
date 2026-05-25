# cmux One Workspace Per CCX Project Plan

## Objective

Change CCX project opening in cmux so each CCX project owns a dedicated workspace, and opening a project creates/selects that workspace with a separate management dashboard tab.

Research waived: the user requested immediate continuation, and the relevant CCX workspace/picker/dashboard entry points were locally inspected before editing.

## Task_1

- type: implementation
- owns:
  - `gui/Sources/CCX/CCXAppDelegateBridge.swift`
  - `gui/Sources/Workspace+CCXDashboard.swift`
  - `gui/Sources/WorkspaceContentView.swift`
  - `gui/cmuxTests/WorkspaceUnitTests.swift`
- depends_on: []
- acceptance:
  - Opening a concrete CCX project from launch args, picker, registration, or switch menu creates/selects one workspace for that project instead of replacing the current workspace's dashboard.
  - Reopening the same project selects the existing project workspace and focuses its management dashboard tab.
  - A project workspace contains a separate management dashboard tab and does not reuse the picker tab as the project dashboard.
  - Picker mode remains available for no-project launches.
- validation:
  - kind: static
    required: true
    owner: orchestrator
    detail: `git diff --check`
  - kind: unit
    required: true
    owner: orchestrator
    detail: targeted `xcodebuild test` for CCX workspace/picker tests where local dependencies allow
  - kind: review
    required: true
    owner: reviewer
    detail: independent subagent review of behavior and lifecycle risk

## Notes

- Repository rule files under `docs/coding-agent/rules` were not present.
- Keep the change scoped to CCX project workspace routing; do not alter generic cmux workspace behavior.

## Evidence

- 2026-05-25: Implemented `TabManager.openCCXProjectWorkspace` to create or reuse a dedicated workspace per CCX project.
- Project opens from launch args now route concrete project IDs into the dedicated project workspace flow; picker-only launches still open the picker in the current/main workspace.
- Picker row selection, registration success, and dashboard switch-menu callbacks now route through the workspace's owning `TabManager` so they open/select a project workspace instead of replacing the picker/dashboard panel in place.
- Added unit coverage for creating a dedicated project workspace with a separate CCX dashboard tab and for reusing the existing workspace on repeat opens.

## Validation

- `git diff --check`
- `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`
- `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`
- `CMUX_SKIP_ZIG_BUILD=1 xcodebuild test -project gui/cmux.xcodeproj -scheme cmux-unit -destination 'platform=macOS' -only-testing:cmuxTests/WorkspaceCCXDashboardSwitchTests -only-testing:cmuxTests/CCXProjectPickerTests`
  - Result: `TEST SUCCEEDED`, 40 tests, 0 failures.
- Reviewer subagent `019e5f40-6f57-7bd0-8943-e913d806e834` returned APPROVED with no actionable findings.
