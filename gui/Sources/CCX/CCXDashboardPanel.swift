import AppKit
import Combine
import SwiftUI

/// Concrete `Panel` for the CCX dashboard. Mounts a `CCXDashboardView` driven
/// by a `CCXProjectStore` when a project id is available, or a project picker
/// backed by `CCXProjectsStore` when launched without one.
@MainActor
final class CCXDashboardPanel: Panel, ObservableObject {
    let id: UUID
    let panelType: PanelType = .ccxDashboard
    let projectStore: CCXProjectStore?
    let projectsStore: CCXProjectsStore

    @Published private(set) var focusFlashToken: Int = 0
    @Published private(set) var titleOverride: String?

    private var storeChangeCancellable: AnyCancellable?

    init(projectId: String? = nil, ccxHome: URL? = nil) {
        self.id = UUID()
        self.projectsStore = CCXProjectsStore(ccxHome: ccxHome)
        if let projectId, !projectId.isEmpty {
            self.projectStore = CCXProjectStore(projectId: projectId, ccxHome: ccxHome)
        } else {
            self.projectStore = nil
        }
        // `displayTitle` reads through to `store.project?.displaySlug`. Without
        // forwarding the store's change publisher, SwiftUI views observing
        // this panel never re-evaluate the title after the project config
        // loads asynchronously, so the tab gets stuck on the "CCX" fallback.
        let panelObjectWillChange = self.objectWillChange
        self.storeChangeCancellable = projectStore?.objectWillChange.sink { _ in
            Task { @MainActor in
                panelObjectWillChange.send()
            }
        }
    }

    var displayTitle: String {
        if let titleOverride { return titleOverride }
        if let slug = projectStore?.project?.displaySlug, !slug.isEmpty {
            return slug
        }
        if projectStore == nil {
            return String(localized: "ccx.projectPicker.title", defaultValue: "CCX Projects")
        }
        return String(localized: "ccx.panel.titleFallback", defaultValue: "CCX")
    }

    var displayIcon: String? { "rectangle.3.group" }

    deinit {
        storeChangeCancellable?.cancel()
        storeChangeCancellable = nil
    }

    func close() {
        // CCXProjectStore tears down its FSEventStream in deinit.
    }

    func focus() {
        focusFlashToken &+= 1
    }

    func unfocus() {}

    func triggerFlash(reason: WorkspaceAttentionFlashReason) {
        focusFlashToken &+= 1
    }
}

/// SwiftUI host that renders a `CCXDashboardPanel` inside cmux's panel system.
struct CCXDashboardPanelView: View {
    @ObservedObject var panel: CCXDashboardPanel
    let onOpenProject: (CCXProjectSummary) -> Void

    init(panel: CCXDashboardPanel, onOpenProject: @escaping (CCXProjectSummary) -> Void = { _ in }) {
        self.panel = panel
        self.onOpenProject = onOpenProject
    }

    var body: some View {
        if let projectStore = panel.projectStore {
            CCXDashboardView(
                store: projectStore,
                projectsStore: panel.projectsStore,
                onOpenProject: onOpenProject
            )
        } else {
            CCXProjectPickerView(
                store: panel.projectsStore,
                onOpenProject: onOpenProject
            )
        }
    }
}
