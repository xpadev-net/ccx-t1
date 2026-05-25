# Task 14.2 CCX Projects Store Plan

## Context

- Repository rule files under `docs/coding-agent/rules/` are absent, so this plan uses the orchestration harness defaults.
- Research waived: 14.1 established the CCX Swift file layout, pbxproj wiring pattern, and local GUI validation constraints.

## Task_1

- type: impl
- owns:
  - `gui/Sources/CCX/CCXProjectsStore.swift`
  - `gui/cmuxTests/CCXProjectsStoreTests.swift`
  - `gui/cmux.xcodeproj/project.pbxproj`
  - `z/tasks.md`
  - `docs/coding-agent/plans/active/task-14-2-ccx-projects-store-plan.md`
- depends_on: []
- acceptance:
  - `CCXProjectsStore` publishes all registered projects as `[CCXProjectSummary]`.
  - The store reads `$CCX_HOME/projects.json` and supplements entries from per-project `project.json` when available.
  - Disk I/O happens on a background queue and `@Published` updates happen on the main actor.
  - The store watches the CCX home with `FSEventStream` and coalesces refreshes using the existing single-project store pattern.
  - Unit tests cover snapshot loading, fallback summaries, and invalid index handling.
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
    detail: targeted `xcodebuild` for `cmuxTests/CCXProjectsStoreTests`, with checkout dependency caveat recorded if `gui/ghostty` remains absent.
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
- 2026-05-25: Implemented `CCXProjectsStore`, unit coverage, pbxproj wiring, localization, and task checklist updates.
- 2026-05-25: Validation passed: `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`; `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`; `git diff --check`; localization JSON parse; Swift typecheck for `CCXModels`, `CCXProjectStore`, and `CCXProjectsStore`.
- 2026-05-25: Targeted `xcodebuild` for `cmuxTests/CCXProjectsStoreTests` is blocked before test execution by the existing checkout dependency error: `Ghostty submodule is missing at /Users/xpadev/IdeaProjects/ccx-t1/gui/ghostty`.
- 2026-05-25: Reviewer subagent approved the change set with the same `gui/ghostty` validation caveat.
- 2026-05-25: `gh-review-hook` requested replacing the new store's Combine/DispatchQueue refresh path with a tracked task lifecycle. Updated `CCXProjectsStore` to `@Observable`, offloaded snapshot reads through a cancellable detached task, and kept state writes on the main actor.

## Decision Log

- 2026-05-25: Implemented a read-only store; GUI mutations remain routed through the controller CLI per P14 note.
