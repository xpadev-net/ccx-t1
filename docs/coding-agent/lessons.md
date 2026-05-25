# Lessons Log (Coding Agent)

Purpose:
- capture recurring mistakes and the prevention mechanism
- enable "read once, don't repeat" improvements

## How to use

- Append a new entry after any user correction or significant miss.
- Keep entries short and actionable.
- Promote repeated/high-severity lessons into repo rules, harness migration candidates, troubleshooting notes, or accepted residual-risk records.

## Tags (recommended)

- planning
- validation
- delegation
- review
- ui-e2e
- tooling
- ci
- scope-owns

## Entries

### 2026-05-26 - Xcode Targeted Test Action

- tags: validation, tooling, review
- symptom: A targeted Swift validation command passed without compiling or running the selected test bundle.
- root cause: `gui/scripts/test-unit.sh` only defaults to the `test` action when no arguments are passed; passing only `-only-testing:...` turns the command into a build invocation.
- fix: Rerun targeted Swift tests with an explicit `test` action before selector flags.
- prevention: Any `gui/scripts/test-unit.sh` invocation with `-only-testing` must include `test -only-testing:...` in the command.
