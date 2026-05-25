# Task 14.5 Project Switch Plan

## Task

Implement project switching for the CCX dashboard by adding `Workspace.switchToCCXDashboard(projectId:)` and routing project-picker selections through it.

## Scope

- Add a Workspace helper that replaces an existing `CCXDashboardPanel` with another project dashboard.
- Preserve the existing bonsplit tab identity when replacing the dashboard so switching does not create extra tabs or replacement terminals.
- Publish the old dashboard surface close event before publishing the new dashboard surface create event.
- Route project-picker selections through the helper.
- Mark `z/tasks.md` 14.5 complete.

## Validation

- `git diff --check`
- JSON parse for `gui/Resources/Localizable.xcstrings`
- `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`
- `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`
- Targeted `xcodebuild` for `cmuxTests/WorkspaceCCXDashboardSwitchTests` and `cmuxTests/CCXProjectPickerTests`

## Progress

- 2026-05-25: Added `Workspace.switchToCCXDashboard(projectId:origin:)`, reusing an existing dashboard tab while discarding the old dashboard panel lifecycle and publishing close before create.
- 2026-05-25: Routed project-picker selections through `switchToCCXDashboard(projectId:)`.
- 2026-05-25: Added unit coverage for replacing an existing dashboard panel and opening a dashboard when none exists.
- 2026-05-25: Marked `z/tasks.md` 14.5 complete.
- 2026-05-25: Validation passed: `git diff --check`; JSON parse for `gui/Resources/Localizable.xcstrings`; `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`; `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`. Targeted `xcodebuild` for `WorkspaceCCXDashboardSwitchTests` and `CCXProjectPickerTests` compiled touched Swift/test files and stopped before test execution on the existing missing `gui/ghostty` checkout dependency. Subagent review approved the lifecycle, tab reuse, fallback, and test coverage.
- 2026-05-25: `gh-review-hook` requested removing non-deterministic `Dictionary.first` fallback selection when multiple CCX dashboards exist. Replaced it with pane/tab-order traversal.
- 2026-05-25: Hook-fix validation passed: `git diff --check`; JSON parse for `gui/Resources/Localizable.xcstrings`; `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`; `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`. Subagent review approved the deterministic dashboard fallback selection.

## Notes

- Repository has no `docs/coding-agent/rules/` directory at the time of this task.
