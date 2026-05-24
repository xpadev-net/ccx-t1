import AppKit
import Combine
import SwiftUI

/// Concrete `Panel` for the CCX dashboard. Mounts a `CCXDashboardView` driven
/// by a `CCXProjectStore` for the project id passed in at launch time
/// (`--project-id <id>`). Internal access matches the sibling concrete panel
/// types in this target (RightSidebarToolPanel etc.).
@MainActor
final class CCXDashboardPanel: Panel, ObservableObject {
    let id: UUID
    let panelType: PanelType = .ccxDashboard
    let store: CCXProjectStore

    @Published private(set) var focusFlashToken: Int = 0
    @Published private(set) var titleOverride: String?

    private var storeChangeCancellable: AnyCancellable?

    init(projectId: String, ccxHome: URL? = nil) {
        self.id = UUID()
        self.store = CCXProjectStore(projectId: projectId, ccxHome: ccxHome)
        // `displayTitle` reads through to `store.project?.displaySlug`. Without
        // forwarding the store's change publisher, SwiftUI views observing
        // this panel never re-evaluate the title after the project config
        // loads asynchronously, so the tab gets stuck on the "CCX" fallback.
        let panelObjectWillChange = self.objectWillChange
        self.storeChangeCancellable = store.objectWillChange.sink { _ in
            Task { @MainActor in
                panelObjectWillChange.send()
            }
        }
    }

    var displayTitle: String {
        if let titleOverride { return titleOverride }
        if let slug = store.project?.displaySlug, !slug.isEmpty {
            return slug
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

    init(panel: CCXDashboardPanel) {
        self.panel = panel
    }

    var body: some View {
        CCXDashboardView(store: panel.store)
    }
}
