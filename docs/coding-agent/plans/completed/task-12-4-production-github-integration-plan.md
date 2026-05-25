# Task 12.4 Production GitHub Integration Verification

## Scope

Verify the production GitHub loop against the real repository rather than fake `gh` fixtures:

- create a real pull request
- run `gh-review-hook`
- fix review findings
- commit and push follow-up changes
- rerun `gh-review-hook` until it exits 0
- merge the pull request
- return `master` to a clean, up-to-date state

## Evidence

- 2026-05-25: PR #40 (`codex/task-14-8-device-verification`) was created against `xpadev-net/ccx-t1`.
- `gh-review-hook` initially returned exit 2 with two actionable Greptile review issues in `gui/Sources/CCX/CCXDashboardView.swift`.
- The review issues were fixed in follow-up commit `1391565f` (`Clarify CCX dashboard store lifecycle`), then pushed to the PR branch.
- A reviewer subagent approved the hook-fix diff with no actionable findings.
- `gh-review-hook` was rerun after the push and exited 0 after Greptile completed successfully and CodeRabbit was already successful.
- PR #40 was merged successfully with `gh pr merge 40 --squash --delete-branch`.
- Local `master` was checked out by the merge command, fast-forwarded to `origin/master`, and `git pull --ff-only` reported `Already up to date`.

## Validation

- `git diff --check`
- `rtk plutil -lint gui/cmux.xcodeproj/project.pbxproj`
- `rtk bash gui/scripts/lint-pbxproj-test-wiring.sh --repo-root gui`
- Targeted `xcodebuild test` for the P14.8 PR passed before PR creation for `cmuxTests/CCXProjectPickerTests` and `cmuxTests/WorkspaceCCXDashboardSwitchTests` with `CMUX_SKIP_ZIG_BUILD=1`.
- Hook-fix targeted `xcodebuild test` reached Swift compilation and linked `cmuxTests`; rerun stopped before test connection with a local test-host bootstrap termination, while the reviewer subagent independently confirmed no CCX compile diagnostics before its unrelated Zig-helper validation stop.

## Result

Production GitHub PR creation, review-hook gating, review-fix push, successful hook rerun, merge, and `master` refresh were verified.
