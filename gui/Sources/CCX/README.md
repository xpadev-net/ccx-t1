# CCX integration

This directory contains the CCX-specific Swift source layered on top of the
forked cmux codebase (`gui/`). The Rust controller writes per-project artifacts
to `$CCX_HOME/projects/<projectId>/` (default `~/.ccx/projects/<id>/`):

| File             | Purpose                                                       |
| ---------------- | ------------------------------------------------------------- |
| `project.json`   | Project configuration written at registration.                |
| `events.jsonl`   | Append-only audit log. Source of truth for state.             |
| `state.sqlite`   | Denormalized read model the GUI queries directly (read-only). |
| `worktrees/`     | Git worktrees attached to work executions.                    |

## Files

| File                      | Role                                                                      |
| ------------------------- | ------------------------------------------------------------------------- |
| `CCXModels.swift`         | Value types mirroring the SQLite read model.                              |
| `CCXProjectStore.swift`   | `ObservableObject` SQLite reader + `FSEventStream` watcher.               |
| `CCXLaunchArguments.swift`| Parses `--project-id <id>` from the open-with-args invocation.            |
| `CCXDashboardView.swift`  | Top-level tabbed dashboard (Overview / Work / Reviews / Artifacts).       |
| `CCXSidebarPanel.swift`   | Compact summary surface for the cmux right-sidebar panel system.          |

## Xcode wiring

All files under `gui/Sources/CCX/` are now registered in
`gui/cmux.xcodeproj/project.pbxproj` (PBXFileReference / PBXBuildFile /
PBXGroup + main-app PBXSourcesBuildPhase entries). The `CCX` group lives
underneath the existing `Sources` group with a `CCXxxxxxxxxxxxxxxxxxx`
UUID prefix so it's easy to distinguish from the cmux upstream IDs.

Verify the wiring after editing the pbxproj:

```sh
plutil -lint gui/cmux.xcodeproj/project.pbxproj
xcodebuild -project gui/cmux.xcodeproj -list   # needs the ghostty submodule
```

## Hosting the dashboard

The `PanelType.ccxDashboard` case (added in `Sources/Panels/Panel.swift`) is
rendered by `Sources/Panels/PanelContentView.swift` via `CCXDashboardPanelView`.
To open a dashboard programmatically, parse the launch arguments and only
construct a panel when a project id was actually supplied:

```swift
let launchArgs = CCXLaunchArguments.parse()
guard let projectId = launchArgs.projectId else { return }
let projectsStore = CCXProjectsStore()
let panel = CCXDashboardPanel(projectId: projectId, projectsStore: projectsStore)
workspace.adopt(panel: panel)  // existing cmux helper
```

`CCXLaunchArguments.parse()` reads `--project-id <id>` from
`CommandLine.arguments`, which is what `ccx project open` passes via
`open -a <bundle> --args --project-id <id>`. Calling the snippet above from
`AppDelegate.applicationDidFinishLaunching` is the recommended hook point;
this single-line wiring is the remaining manual step and is tracked
separately from the file-level wiring above.

## Data flow

```
Rust controller                     ccx-cmux (Swift)
─────────────────                   ───────────────────
events.jsonl  ──append─►            FSEventStream ──► CCXProjectStore.refresh()
state.sqlite  ──write──►            SQLite3 read    ──► @Published snapshots
                                                       │
                                                       ▼
                                                 CCXDashboardView
                                                 CCXSidebarPanel
```

The Swift layer is intentionally **read-only**. Any mutation must go through
the controller CLI so the audit log remains the single source of truth.

## Launch from the controller CLI

`ccx project open <project_id>` shells out to:

```
open -a <bundle> --args --project-id <project_id>
```

The controller probes Launch Services for the following bundle names, in
order: `ccx-cmux`, `ccx-cmux DEV`, `cmux`, `cmux DEV`. The first installed
bundle wins. If none are present the controller prints a graceful error
pointing back to the build instructions in `gui/CLAUDE.md`.

The `PRODUCT_NAME` rename to `ccx-cmux` is deferred to Phase 13.6 because
`gui/scripts/reload*.sh` and `gui/CLAUDE.md` hardcode `cmux DEV`. Renaming the
bundle without updating those scripts breaks the cmux fork's reload loop.
