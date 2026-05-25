import Bonsplit
import Foundation

extension Workspace {
    @discardableResult
    func ensureCCXDashboardSurface(projectId: String, origin: String = "ccx_project_workspace") -> CCXDashboardPanel? {
        let trimmedProjectId = projectId.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedProjectId.isEmpty else { return nil }
        if let panel = focusCCXDashboardSurface(projectId: trimmedProjectId) {
            return panel
        }
        guard let paneId = bonsplitController.focusedPaneId ?? bonsplitController.allPaneIds.first else {
            return nil
        }
        return newCCXDashboardSurface(
            inPane: paneId,
            projectId: trimmedProjectId,
            focus: true,
            origin: origin
        )
    }

    @discardableResult
    func focusCCXDashboardSurface(projectId: String) -> CCXDashboardPanel? {
        let trimmedProjectId = projectId.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedProjectId.isEmpty else { return nil }
        guard let panelId = ccxDashboardPanelId(projectId: trimmedProjectId),
              let panel = panels[panelId] as? CCXDashboardPanel else {
            return nil
        }
        focusPanel(panelId)
        return panel
    }

    func hasCCXProject(projectId: String) -> Bool {
        ccxDashboardPanelId(projectId: projectId) != nil
    }

    private func ccxDashboardPanelId(projectId: String) -> UUID? {
        let trimmedProjectId = projectId.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedProjectId.isEmpty else { return nil }
        for panelId in panels.keys {
            guard let panel = panels[panelId] as? CCXDashboardPanel,
                  panel.projectStore?.projectId == trimmedProjectId else {
                continue
            }
            return panelId
        }
        return nil
    }

    @discardableResult
    func switchToCCXDashboard(projectId: String, origin: String = "ccx_project_picker") -> CCXDashboardPanel? {
        let trimmedProjectId = projectId.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedProjectId.isEmpty else { return nil }

        guard let existingPanelId = ccxDashboardPanelIdForSwitch(),
              let existingPanel = panels[existingPanelId] as? CCXDashboardPanel,
              let existingTabId = surfaceIdFromPanelId(existingPanelId),
              let paneId = paneId(forPanelId: existingPanelId) else {
            guard let paneId = bonsplitController.focusedPaneId ?? bonsplitController.allPaneIds.first else {
                return nil
            }
            return newCCXDashboardSurface(
                inPane: paneId,
                projectId: trimmedProjectId,
                focus: true,
                origin: origin
            )
        }

        if existingPanel.projectStore?.projectId == trimmedProjectId {
            focusPanel(existingPanelId)
            return existingPanel
        }

        let wasFocused = focusedPanelId == existingPanelId
        discardClosedPanelLifecycleState(
            panelId: existingPanelId,
            tabId: existingTabId,
            paneId: paneId,
            panel: existingPanel,
            origin: origin,
            closePanel: true,
            publishSurfaceClosedEvent: true,
            clearSurfaceNotifications: true,
            requestTransferredRemoteCleanup: false,
            cleanupControllerSurfaceState: false
        )

        let panel = CCXDashboardPanel(projectId: trimmedProjectId, projectsStore: ccxProjectsStore)
        panels[panel.id] = panel
        panelTitles[panel.id] = panel.displayTitle
        surfaceIdToPanelId[existingTabId] = panel.id
        bonsplitController.updateTab(
            existingTabId,
            title: panel.displayTitle,
            icon: .some(panel.displayIcon),
            iconImageData: .some(nil),
            kind: .some(SurfaceKind.ccxDashboard),
            hasCustomTitle: false,
            isDirty: false,
            showsNotificationBadge: false,
            isLoading: false,
            isPinned: false
        )
        publishCmuxSurfaceCreated(
            existingTabId.uuid,
            paneId: paneId,
            kind: Self.cmuxEventSurfaceKind(panel),
            origin: origin,
            focused: wasFocused
        )
        if wasFocused {
            focusPanel(panel.id)
        }
        return panel
    }

    private func ccxDashboardPanelIdForSwitch() -> UUID? {
        if let focusedPanelId, panels[focusedPanelId] is CCXDashboardPanel {
            return focusedPanelId
        }
        if let focusedPane = bonsplitController.focusedPaneId {
            for tab in bonsplitController.tabs(inPane: focusedPane) {
                if let panelId = panelIdFromSurfaceId(tab.id),
                   panels[panelId] is CCXDashboardPanel {
                    return panelId
                }
            }
        }
        for paneId in bonsplitController.allPaneIds {
            if paneId == bonsplitController.focusedPaneId { continue }
            for tab in bonsplitController.tabs(inPane: paneId) {
                if let panelId = panelIdFromSurfaceId(tab.id),
                   panels[panelId] is CCXDashboardPanel {
                    return panelId
                }
            }
        }
        return nil
    }
}

extension TabManager {
    @discardableResult
    func openCCXProjectWorkspace(
        project: CCXProjectSummary,
        origin: String = "ccx_project_picker"
    ) -> Workspace? {
        openCCXProjectWorkspace(
            projectId: project.projectId,
            title: Self.ccxWorkspaceTitle(for: project),
            workingDirectory: Self.ccxWorkspaceDirectory(for: project),
            origin: origin
        )
    }

    @discardableResult
    func openCCXProjectWorkspace(
        projectId: String,
        origin: String = "ccx_launch_args"
    ) -> Workspace? {
        let trimmedProjectId = projectId.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedProjectId.isEmpty else { return nil }
        return openCCXProjectWorkspace(
            projectId: trimmedProjectId,
            title: "CCX \(trimmedProjectId)",
            workingDirectory: nil,
            origin: origin
        )
    }

    private func openCCXProjectWorkspace(
        projectId: String,
        title: String,
        workingDirectory: String?,
        origin: String
    ) -> Workspace? {
        let trimmedProjectId = projectId.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedProjectId.isEmpty else { return nil }

        if let existing = tabs.first(where: { $0.hasCCXProject(projectId: trimmedProjectId) }) {
            selectWorkspace(existing)
            existing.setCustomTitle(title)
            _ = existing.ensureCCXDashboardSurface(projectId: trimmedProjectId, origin: origin)
            return existing
        }

        let workspace = addWorkspace(
            title: title,
            workingDirectory: workingDirectory,
            inheritWorkingDirectory: workingDirectory == nil,
            select: true,
            autoWelcomeIfNeeded: false
        )
        guard workspace.ensureCCXDashboardSurface(projectId: trimmedProjectId, origin: origin) != nil else {
            closeWorkspace(workspace)
            return nil
        }
        return workspace
    }

    private static func ccxWorkspaceTitle(for project: CCXProjectSummary) -> String {
        if !project.displaySlug.isEmpty { return project.displaySlug }
        if !project.canonicalRepo.isEmpty {
            let url = URL(fileURLWithPath: project.canonicalRepo)
            return url.lastPathComponent.isEmpty ? project.canonicalRepo : url.lastPathComponent
        }
        return project.projectId
    }

    private static func ccxWorkspaceDirectory(for project: CCXProjectSummary) -> String? {
        let trimmed = project.canonicalRepo.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
