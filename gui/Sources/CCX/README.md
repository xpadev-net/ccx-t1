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

These files are not yet listed in `gui/cmux.xcodeproj/project.pbxproj`. To add
them to the main app target:

1. Open `gui/cmux.xcodeproj` in Xcode.
2. Right-click the `Sources` group, choose **Add Files to "cmux"…**, select the
   `Sources/CCX/` folder, and ensure the **ccx-cmux** target is checked.
3. Confirm the new files appear under a `Sources/Build Phases/Compile Sources`
   entry for that target.
4. Build the **ccx-cmux** scheme (Debug). The dashboard view can be presented
   from `AppDelegate` by constructing a `CCXProjectStore(projectId:)` driven
   by `CCXLaunchArguments.parse()`.

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
