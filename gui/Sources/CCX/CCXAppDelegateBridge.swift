import AppKit
import Foundation

/// Glue between the `ccx project open <id>` CLI and cmux's existing
/// main-window / workspace plumbing. Called from
/// `AppDelegate.applicationDidFinishLaunching` as a single line:
///
/// ```swift
/// CCXAppDelegateBridge.presentDashboardIfRequested(on: self)
/// ```
///
/// `ccx project open` invokes `open -a <bundle> --args --project-id <id>`,
/// which leaves the id in `CommandLine.arguments`. `CCXLaunchArguments.parse()`
/// extracts it; if a project id was supplied, we route through
/// `openCCXDashboardInPreferredMainWindow`, which mirrors the existing
/// `openFilePreviewInPreferredMainWindow` flow — pick an existing main-window
/// context if there is one, otherwise create a fresh window and adopt the
/// dashboard into its workspace.
@MainActor
enum CCXAppDelegateBridge {
    static func presentDashboardIfRequested(on appDelegate: AppDelegate) {
        let args = CCXLaunchArguments.parse()
        guard args.isCCXLaunch else { return }
        appDelegate.openCCXDashboardInPreferredMainWindow(
            projectId: args.projectId?.isEmpty == false ? args.projectId : nil,
            debugSource: "ccxLaunchArgs"
        )
    }
}

extension AppDelegate {
    /// Open a CCX dashboard tab for `projectId` in a main window. Creates one
    /// if none exists. Returns `true` on success. Failure branches log via
    /// `NSLog` (production-visible) and `cmuxDebugLog` (DEBUG-only) so a user
    /// running `ccx project open <id>` who sees no dashboard can find a
    /// diagnostic in the unified log.
    @discardableResult
    func openCCXDashboardInPreferredMainWindow(
        projectId: String?,
        debugSource: String = "unspecified"
    ) -> Bool {
        let context: MainWindowContext? = {
            if let existing = mainWindowContexts.values.first {
                return existing
            }
            let windowId = ensureInitialMainWindowIfNeeded(shouldActivate: true)
            return mainWindowContexts.values.first { $0.windowId == windowId }
        }()
        guard let context else {
            NSLog("ccx.dashboardOpen.failed reason=no_main_window_context source=%@ projectId=%@",
                  debugSource, projectId ?? "<picker>")
#if DEBUG
            cmuxDebugLog("ccx.dashboardOpen.failed reason=no_main_window_context source=\(debugSource) projectId=\(projectId ?? "<picker>")")
#endif
            return false
        }

        if let window = context.window ?? windowForMainWindowId(context.windowId) {
            setActiveMainWindow(window)
        }

        let workspace = context.tabManager.selectedWorkspace
            ?? context.tabManager.addWorkspace(workingDirectory: nil, select: true)
        guard let paneId = workspace.bonsplitController.focusedPaneId
            ?? workspace.bonsplitController.allPaneIds.first else {
            NSLog("ccx.dashboardOpen.failed reason=no_pane source=%@ projectId=%@ workspace=%@",
                  debugSource, projectId ?? "<picker>", workspace.id.uuidString)
#if DEBUG
            cmuxDebugLog("ccx.dashboardOpen.failed reason=no_pane source=\(debugSource) projectId=\(projectId ?? "<picker>") workspace=\(workspace.id.uuidString)")
#endif
            return false
        }

#if DEBUG
        cmuxDebugLog("ccx.dashboardOpen source=\(debugSource) projectId=\(projectId ?? "<picker>")")
#endif
        return workspace.newCCXDashboardSurface(
            inPane: paneId,
            projectId: projectId,
            focus: true
        ) != nil
    }
}
