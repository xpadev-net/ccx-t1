# Task 14.6 Project Unregister

## Objective

Implement CCX project unregister from both the controller CLI and the project picker UI, with audit logging, optional purge, and focused tests.

## Scope

- Add `ccx project unregister --project-id <id> [--purge]`.
- Append `project_unregistered` to the project event log.
- Remove the project from `$CCX_HOME/projects.json`.
- Delete `$CCX_HOME/projects/<id>/` only when `--purge` is set.
- Add project picker row context menu, confirmation dialog, and safe GUI error handling.
- Add Rust and Swift tests for CLI arguments, safe failures, index removal, purge, and event append.

## Validation

- `rustfmt --check src/cli/project.rs src/domain/event.rs src/persistence/projector.rs`
- `cargo test project`
- `git diff --check`
- JSON parse for `gui/Resources/Localizable.xcstrings`
- `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`
- `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`
- Targeted `xcodebuild` for `cmuxTests/CCXProjectPickerTests`

## Progress

- 2026-05-25: Added `ProjectUnregistered` event data and `ccx project unregister --project-id <id> [--purge]`.
- 2026-05-25: Added locked `projects.json` removal, non-purge retention, purge directory removal, and Rust coverage.
- 2026-05-25: Added `CCXControllerCLI.unregister(projectId:purge:)`, project picker context menu, confirmation dialog, and unregistration ViewModel with safe error messages.
- 2026-05-25: Added Swift coverage for unregister CLI arguments and unregistration ViewModel success/failure behavior.
- 2026-05-25: Marked `z/tasks.md` 14.6 complete.
- 2026-05-25: Validation passed: `rustfmt --check src/cli/project.rs src/domain/event.rs src/persistence/projector.rs`; `cargo test project`; `git diff --check`; JSON parse for `gui/Resources/Localizable.xcstrings`; `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`; `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`. Targeted `xcodebuild` for `CCXProjectPickerTests` compiled touched Swift files/tests and stopped before test execution on the existing missing `gui/ghostty` checkout dependency.
- 2026-05-25: Subagent review found the confirmation dialog could auto-dismiss and clear the pending unregister before the async CLI task started. Added a synchronous claim step before scheduling CLI execution, plus regression coverage. Re-validation passed: `rustfmt --check src/cli/project.rs src/domain/event.rs src/persistence/projector.rs`; `cargo test project`; `git diff --check`. Targeted `xcodebuild` again compiled touched Swift files/tests and stopped on the existing missing `gui/ghostty` checkout dependency.

## Notes

- Full `cargo fmt --check` still reports pre-existing formatting diffs outside this task's edited Rust files.
