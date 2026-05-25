import Bonsplit
import Foundation

extension Workspace {
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
