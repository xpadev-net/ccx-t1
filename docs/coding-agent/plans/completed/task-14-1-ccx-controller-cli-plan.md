# Task 14.1 CCX Controller CLI Plan

## Context

- Repository rule files under `docs/coding-agent/rules/` are absent, so this plan uses the orchestration harness defaults.
- Research waived: prior P14 exploration already identified the Swift CCX model/store files, pbxproj wiring pattern, and relevant CLI JSON output.

## Task_1

- type: impl
- owns:
  - `gui/Sources/CCX/CCXControllerCLI.swift`
  - `gui/Sources/CCX/CCXModels.swift`
  - `gui/cmux.xcodeproj/project.pbxproj`
  - `gui/cmuxTests/CCXControllerCLITests.swift`
- depends_on: []
- acceptance:
  - `CCXControllerCLI` resolves `ccx` by `CCX_CLI`, then `$CCX_HOME/bin/ccx`, then `PATH`.
  - Missing or non-executable binaries return an error suitable for UI presentation.
  - `register(canonicalRepo:taskSourceFile:) async throws -> CCXProjectSummary` runs `ccx project register` and parses JSON output.
  - CLI failures retain stderr in the thrown error.
  - Unit tests cover path resolution, register parsing, and stderr propagation.
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
    detail: targeted `xcodebuild` for `cmuxTests/CCXControllerCLITests`
  - required: true
    owner: reviewer
    kind: review
    detail: independent subagent review before PR
  - required: true
    owner: orchestrator
    kind: gate
    detail: `gh-review-hook` must exit 0 before merge.

## Task Waves

- Wave 1: Task_1

## Progress Log

- 2026-05-25: Created plan and started Task_1 implementation.
- 2026-05-25: Added `CCXControllerCLI`, snake_case JSON decoding on `CCXProjectSummary`, unit tests, and Xcode project wiring.
- 2026-05-25: Passed `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`.
- 2026-05-25: Passed `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`.
- 2026-05-25: Passed `git diff --check`.
- 2026-05-25: Passed typecheck for `CCXModels.swift` and `CCXControllerCLI.swift` with module cache under `/private/tmp`.
- 2026-05-25: Targeted xcodebuild with `cmux` scheme was attempted, but `cmuxTests` is not a member of that scheme/test plan.
- 2026-05-25: Restored the tracked `gui/vendor/bonsplit` gitlink locally from `manaflow-ai/bonsplit` for validation only.
- 2026-05-25: Targeted xcodebuild with `cmux-unit` scheme compiled through the 14.1 Swift sources and current `bonsplit`, then failed in an existing app build phase because `gui/ghostty` is missing from this checkout.

## Decision Log

- 2026-05-25: Kept scope to the launcher abstraction and unit tests; later GUI list/picker work remains in 14.2+.
- 2026-05-25: Reviewer approved with a validation caveat for missing `gui/ghostty`; no task-scoped changes were requested.
