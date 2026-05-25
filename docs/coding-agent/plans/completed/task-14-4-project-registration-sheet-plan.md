# Task 14.4 Project Registration Sheet Plan

- status: completed
- owner: orchestrator
- repo_rules: `docs/coding-agent/rules/` is absent in this checkout; using `AGENTS.md` and harness instructions.

## Task Breakdown

### Task_1

- owns:
  - `gui/Sources/CCX/CCXProjectPickerView.swift`
  - `gui/Resources/Localizable.xcstrings`
  - `gui/cmuxTests/CCXProjectPickerTests.swift`
  - `z/tasks.md`
  - `docs/coding-agent/plans/active/task-14-4-project-registration-sheet-plan.md`
- depends_on: []
- acceptance:
  - The Add Project action opens a registration sheet instead of placeholder copy.
  - The sheet lets users choose a repository directory and `*.md` task source file with `NSOpenPanel`.
  - Validation rejects missing repository paths, repositories without a `.git` directory, missing task source files, directories, and non-`.md` task sources before invoking the CLI.
  - Successful submit runs `CCXControllerCLI.register(canonicalRepo:taskSourceFile:)`, then opens the registered project through the existing picker callback.
  - Unit tests cover validation and CLI submit behavior.
- validation:
  - required: true
    owner: orchestrator
    kind: lint
    detail: JSON parse for `gui/Resources/Localizable.xcstrings` plus Xcode `xcstringstool` dry-run through targeted build.
  - required: true
    owner: orchestrator
    kind: lint
    detail: `git diff --check`
  - required: true
    owner: orchestrator
    kind: test
    detail: Targeted `xcodebuild` for `cmuxTests/CCXProjectPickerTests`, with checkout dependency caveat recorded if `gui/ghostty` remains absent.
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
- 2026-05-25: Replaced the Add Project placeholder with a registration sheet, validation model, `NSOpenPanel` selectors, CLI submit path, localized strings, and unit tests for validation/submit behavior.
- 2026-05-25: Validation passed: `git diff --check`; JSON parse for `gui/Resources/Localizable.xcstrings`. Targeted `xcodebuild` for `cmuxTests/CCXProjectPickerTests` compiled touched Swift/test files and ran `xcstringstool` dry-run, then stopped before test execution on the existing missing `gui/ghostty` checkout dependency.
- 2026-05-25: Aligned task wording with the completed registration sheet and made validation text prefer the current form state over stale submit errors.
- 2026-05-25: Reviewer subagent approved the change set.
- 2026-05-25: `gh-review-hook` requested safe user-facing CLI failure copy, `@Observable` state, non-blocking `NSOpenPanel.begin`, duplicate-submit protection, and submit-time dismissal guards. Fixed each item and added duplicate-submit test coverage.
- 2026-05-25: Hook-fix validation passed: `git diff --check`; JSON parse for `gui/Resources/Localizable.xcstrings`. Targeted `xcodebuild` compiled touched Swift/test files and ran `xcstringstool` dry-run, then stopped before test execution on the existing missing `gui/ghostty` checkout dependency.
- 2026-05-25: `gh-review-hook` requested clearing stale submit errors on sheet re-open and splitting registration model types out of the picker view file. Added `CCXProjectRegistrationModel.swift`, wired it into the Xcode project, and added clear-error test coverage.
- 2026-05-25: Second hook-fix validation passed: `git diff --check`; JSON parse for `gui/Resources/Localizable.xcstrings`; `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`; `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`. Targeted `xcodebuild` compiled touched Swift/test files and again stopped before test execution on the existing missing `gui/ghostty` checkout dependency.
- 2026-05-25: `gh-review-hook` requested tying file-picker tasks to the sheet lifecycle. Stored the open-panel task, cancelled it on Cancel/disappear, and wrapped `NSOpenPanel.begin` in a cancellation-aware continuation that cancels the panel and resumes with `.cancel`.
- 2026-05-25: Third hook-fix validation passed: `git diff --check`; JSON parse for `gui/Resources/Localizable.xcstrings`; `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`; `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`. Targeted `xcodebuild` compiled touched Swift/test files and again stopped before test execution on the existing missing `gui/ghostty` checkout dependency.

## Decision Log

- 2026-05-25: Keep repository/task source validation in a small value type so the mechanically verifiable rules can be tested without driving AppKit panels.
