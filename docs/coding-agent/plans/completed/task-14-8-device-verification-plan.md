# Task 14.8 Device Verification

## Objective

Make the Phase 14 project-management GUI verification runnable from the local checkout, then record the mechanically verifiable evidence for the remaining device-verification checklist.

## Scope

- Fix any build/test configuration issue that prevents the CCX project-management GUI tests from running.
- Run the available mechanical validation for the app, CLI, project index, and database verification paths.
- Mark `z/tasks.md` 14.8 complete only if the verification checklist has evidence; otherwise record the remaining blocker.

## Task_1

- type: impl
- owns:
  - `gui/cmux.xcodeproj/project.pbxproj`
  - `gui/Sources/CCX/CCXDashboardView.swift`
  - `gui/cmuxTests/CCXProjectPickerTests.swift`
  - `docs/coding-agent/plans/active/task-14-8-device-verification-plan.md`
- depends_on: []
- acceptance:
  - Debug app builds as `ccx-cmux DEV.app` while exposing the historical `cmux_DEV` Swift module for tests.
  - Release app keeps exposing the historical `cmux` Swift module.
  - The project file remains plist-valid and test wiring lint still passes.
- validation:
  - required: true
    owner: orchestrator
    kind: command
    detail: `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`
  - required: true
    owner: orchestrator
    kind: command
    detail: `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`
  - required: true
    owner: orchestrator
    kind: command
    detail: Targeted `xcodebuild` for `cmuxTests/CCXProjectPickerTests`

## Task_2

- type: test
- owns:
  - `z/tasks.md`
  - `docs/coding-agent/plans/active/task-14-8-device-verification-plan.md`
- depends_on: [Task_1]
- acceptance:
  - Project Picker launch, add project, switch project, unregister, and CLI/GUI consistency checks have recorded evidence.
  - `ccx db verify` succeeds after the exercised operations.
  - Task ledger reflects the verified state.
- validation:
  - required: true
    owner: orchestrator
    kind: command
    detail: Build and launch `ccx-cmux DEV.app` with CCX picker/dashboard inputs.
  - required: true
    owner: orchestrator
    kind: command
    detail: Run `ccx db verify` for the exercised project.
  - required: true
    owner: reviewer
    kind: review
    detail: Independent review of the verification evidence and task ledger update.

## Task Waves

- Wave 1: Task_1
- Wave 2: Task_2

## Progress Log

- 2026-05-25: Started from `master` after P14.7 merge. Initial targeted `xcodebuild` was blocked by the app module being auto-derived as `ccx_cmux_DEV` while tests import `cmux_DEV`.
- 2026-05-25: Added explicit app module names to preserve `cmux_DEV`/`cmux`; targeted `xcodebuild` then advanced to a pre-existing `XCTUnwrap` error in `CCXProjectPickerTests`.
- 2026-05-25: Fixed the async test signature and made localized expectations locale-independent. Validation passed: `git diff --check`, `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`, `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`, and targeted `xcodebuild` for `cmuxTests/CCXProjectPickerTests` with `CMUX_SKIP_ZIG_BUILD=1` (35 tests, 0 failures).
- 2026-05-25: Manual UI verification found project switching could leave the dashboard stuck on loading because the replacement `CCXProjectStore` did not get started when SwiftUI reused the same view position.
- 2026-05-25: Fixed project switching by starting stores again when `store.projectId` changes. Validation passed: targeted `xcodebuild` for `cmuxTests/CCXProjectPickerTests` and `cmuxTests/WorkspaceCCXDashboardSwitchTests` with `CMUX_SKIP_ZIG_BUILD=1` (38 tests, 0 failures).
- 2026-05-25: Completed UI verification against `/tmp/ccx-p14-8.Xc6Iay`: picker launch logged `projectId=<picker>`; `+ Add Project` registered repo-three and navigated to its dashboard; project switch navigated from repo-three to repo-two; unregister removed repo-three from `projects.json`; `ccx db verify --json` returned `consistent: true` for active repo-two and unregistered repo-three after rebuild.
- 2026-05-25: Marked `z/tasks.md` 14.8 complete.

## Decision Log

- 2026-05-25: Research waived because the failure is already isolated to local Xcode build settings and the user requested continuing P14 directly.
