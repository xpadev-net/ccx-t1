import AppKit
import Foundation
import os

nonisolated private let ccxDashboardOpenLogger = Logger(
    subsystem: "com.cmuxterm.app",
    category: "CCXDashboardOpen"
)

/// Glue between the `ccx project open <id>` CLI and cmux's existing
/// main-window / workspace plumbing. Called from
/// `AppDelegate.applicationDidFinishLaunching` as a single line:
///
/// ```swift
/// CCXAppDelegateBridge.presentDashboardIfRequested(on: self)
/// ```
///
/// CCX launches pass either `--project-id <id>` for a direct dashboard or a
/// picker request/default-project environment for project selection.
/// `CCXLaunchArguments.parse()` extracts that policy; when a CCX launch was
/// requested, we route through
/// `openCCXDashboardInPreferredMainWindow`, which mirrors the existing
/// `openFilePreviewInPreferredMainWindow` flow — pick an existing main-window
/// context if there is one, otherwise create a fresh window and adopt the
/// dashboard into its workspace.
@MainActor
enum CCXAppDelegateBridge {
    static func presentDashboardIfRequested(on appDelegate: AppDelegate) {
        let args = CCXLaunchArguments.parse()
        let resolvedProjectId = args.projectId ?? autoResolveProjectId()
        appDelegate.openCCXDashboardInPreferredMainWindow(
            projectId: resolvedProjectId,
            debugSource: "ccxLaunchArgs"
        )
    }

    private static func autoResolveProjectId() -> String? {
        let indexURL = CCXProjectStore.defaultCCXHome()
            .appendingPathComponent("projects.json")
        guard let data = try? Data(contentsOf: indexURL),
              let entries = try? JSONDecoder().decode([ProjectIndexEntry].self, from: data),
              entries.count == 1 else {
            return nil
        }
        return entries[0].projectId
    }

    private struct ProjectIndexEntry: Decodable {
        let projectId: String
        private enum CodingKeys: String, CodingKey {
            case projectId = "project_id"
        }
    }
}

extension AppDelegate {
    /// Open a CCX dashboard tab for `projectId` in a main window. Creates one
    /// if none exists. Returns `true` on success.
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
            ccxDashboardOpenLogger.error("ccx.dashboardOpen.failed reason=no_main_window_context source=\(debugSource, privacy: .private(mask: .hash)) projectId=\(projectId ?? "<picker>", privacy: .private(mask: .hash))")
#if DEBUG
            cmuxDebugLog("ccx.dashboardOpen.failed reason=no_main_window_context source=\(debugSource) projectId=\(projectId ?? "<picker>")")
#endif
            return false
        }

        if let window = context.window ?? windowForMainWindowId(context.windowId) {
            setActiveMainWindow(window)
        }

        if let projectId {
#if DEBUG
            cmuxDebugLog("ccx.dashboardOpen source=\(debugSource) projectId=\(projectId)")
#endif
            return context.tabManager.openCCXProjectWorkspace(
                projectId: projectId,
                origin: debugSource
            ) != nil
        }

        let workspace = context.tabManager.selectedWorkspace
            ?? context.tabManager.addWorkspace(workingDirectory: nil, select: true)
        guard let paneId = workspace.bonsplitController.focusedPaneId
            ?? workspace.bonsplitController.allPaneIds.first else {
            ccxDashboardOpenLogger.error("ccx.dashboardOpen.failed reason=no_pane source=\(debugSource, privacy: .private(mask: .hash)) projectId=\(projectId ?? "<picker>", privacy: .private(mask: .hash)) workspace=\(workspace.id.uuidString, privacy: .private(mask: .hash))")
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
