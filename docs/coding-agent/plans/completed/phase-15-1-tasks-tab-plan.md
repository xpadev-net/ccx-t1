# Phase 15.1 Tasks Tab Plan

## Scope

Implement the first unchecked `z/tasks.md` item, `15.1 現状導線の明示と Tasks タブ追加`.

Research waived: local targeted discovery already identified the dashboard and project snapshot files needed for this bounded UI change.

## Tasks

### Task_1

- type: impl
- owns:
  - `gui/Sources/CCX/CCXDashboardView.swift`
  - `gui/Sources/CCX/README.md`
  - `gui/cmuxTests/*CCX*Tests.swift`
  - `z/tasks.md`
- depends_on: []
- acceptance:
  - `CCXDashboardView` exposes a `Tasks` tab in the existing segmented tab control.
  - The tab shows the registered `taskSourceFile` path and a last-read timestamp or a clear unavailable state.
  - Missing, empty, directory, and non-Markdown task source paths render explicit empty/error states.
  - The tab provides `Open in Editor`, `Reveal in Finder`, and `Copy Path` actions for valid paths.
  - `z/tasks.md` marks only `15.1` complete after implementation and validation.
- validation:
  - kind: command
    required: true
    owner: orchestrator
    detail: Run targeted Swift tests or a narrow build/test command covering CCX dashboard code.
  - kind: review
    required: true
    owner: reviewer
    detail: Independent subagent review of the final diff.

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-26: Plan created for `15.1`.
- 2026-05-26: Implemented Tasks tab, task source status UI/actions, focused status tests, and README/task ledger updates.
- 2026-05-26: Initial validation mistake found by Reviewer: `test-unit.sh -only-testing:...` built without running XCTest because the `test` action was omitted.
- 2026-05-26: Fixed test helper overloads and recorded the validation lesson in `docs/coding-agent/lessons.md` plus `docs/coding-agent/skill-candidates.md`.
- 2026-05-26: Corrected validation passed: `rtk git diff --check`, `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`, `CMUX_SKIP_ZIG_BUILD=1 rtk bash gui/scripts/test-unit.sh build-for-testing -only-testing:cmuxTests/CCXProjectPickerTests`.
- 2026-05-26: Corrected XCTest execution attempted with `CMUX_SKIP_ZIG_BUILD=1 rtk bash gui/scripts/test-unit.sh test -only-testing:cmuxTests/CCXProjectPickerTests` and a single test selector; both reached the host app but failed before XCTest connection with `Early unexpected exit ... Test crashed with signal term before establishing connection`.
- 2026-05-26: Reviewer re-review returned `APPROVED`; residual XCTest runtime failure recorded as host-app/bootstrap environment risk.

## Decision Log

- 2026-05-26: Keep implementation in the dashboard file because `CCXProjectStore` already exposes `CCXProjectSummary.taskSourceFile`; no CLI changes are needed for `15.1`.
