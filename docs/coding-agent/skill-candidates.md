# Skill Candidates

## Harness Migration Candidates

### HMC-XCODE-EXPLICIT-TEST-ACTION

- category: validation
- proposed_home: `engineering-quality-baselines` or a Swift/Xcode validation reference
- generalized_rule: When invoking a wrapper script that defaults to `test` only with zero arguments, include the explicit `test` action before target selectors such as `-only-testing`.
- trigger: Targeted Xcode or XCTest validation through repository wrapper scripts.
- evidence_from_repo: `gui/scripts/test-unit.sh -only-testing:cmuxTests/CCXProjectPickerTests` exited 0 after building only, while `gui/scripts/test-unit.sh test -only-testing:cmuxTests/CCXProjectPickerTests` revealed test-bundle compilation errors.
- rationale: Prevents false-positive validation evidence for Swift/Xcode repositories.
- suggested_change: Add a validation checklist item requiring explicit actions when wrapper defaults are bypassed by additional arguments.
