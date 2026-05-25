# Task 14.7 Launch Arguments Policy

## Objective

Clarify CCX launch policy so explicit project launches open dashboards, picker launches stay picker-only, and `CCX_DEFAULT_PROJECT_ID` can provide a default project when no project id is supplied.

## Scope

- Update `CCXLaunchArguments` parsing policy.
- Preserve ordinary cmux launches unless a CCX flag or default project environment variable is present.
- Add focused tests for picker fallback, explicit project precedence, explicit picker precedence, and environment defaults.
- Mark `z/tasks.md` 14.7 complete after validation.

## Validation

- `git diff --check`
- JSON parse for `gui/Resources/Localizable.xcstrings`
- `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`
- `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`
- Targeted `xcodebuild` for `cmuxTests/CCXProjectPickerTests`

## Progress

- 2026-05-25: Started from `master` after P14.6 merge.
- 2026-05-25: Added `CCX_DEFAULT_PROJECT_ID` parsing with explicit `--project-id` precedence and explicit picker override.
- 2026-05-25: Updated launch policy tests and CCX launch documentation.
- 2026-05-25: Marked `z/tasks.md` 14.7 complete.
- 2026-05-25: Validation passed: `git diff --check`; JSON parse for `gui/Resources/Localizable.xcstrings`; `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`; `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`. Targeted `xcodebuild` for `CCXProjectPickerTests` compiled touched Swift files/resources and stopped on the existing missing `gui/ghostty` checkout dependency.
- 2026-05-25: Subagent review requested stale comment/documentation cleanup. Updated `CCXLaunchArguments`, CCX README, and AppDelegate comments to distinguish `--ccx`, explicit picker launches, and `CCX_DEFAULT_PROJECT_ID`.
- 2026-05-25: Follow-up review found two remaining stale documentation lines. Updated the README file table and `CCXLaunchArguments` type comment to mention `--ccx`.
- 2026-05-25: `gh-review-hook` requested empty project id normalization and `--ccx` plus `CCX_DEFAULT_PROJECT_ID` coverage. Normalized empty project ids to `nil` at parse return and added focused tests.
- 2026-05-25: Second `gh-review-hook` pass requested whitespace project id normalization, removal of redundant bridge normalization, and a README fenced-code language tag. Applied those minimal fixes with regression coverage.
- 2026-05-25: Follow-up subagent review found blank explicit `--project-id` values could still fall back to `CCX_DEFAULT_PROJECT_ID`. Preserved explicit project-id argument presence separately so blank explicit ids normalize to picker mode instead of env fallback.
