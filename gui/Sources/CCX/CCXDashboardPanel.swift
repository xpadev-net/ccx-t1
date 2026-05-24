import AppKit
import Combine
import SwiftUI

/// Concrete `Panel` for the CCX dashboard. Mounts a `CCXDashboardView` driven
/// by a `CCXProjectStore` for the project id passed in at launch time
/// (`--project-id <id>`). The panel is intentionally simple: cmux owns layout
/// and focus, the CCX view tree owns rendering.
@MainActor
public final class CCXDashboardPanel: Panel, ObservableObject {
    public let id: UUID
    public let panelType: PanelType = .ccxDashboard
    public let store: CCXProjectStore

    @Published public private(set) var focusFlashToken: Int = 0
    @Published public private(set) var titleOverride: String?

    private var storeChangeCancellable: AnyCancellable?

    public init(projectId: String, ccxHome: URL? = nil) {
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

    public var displayTitle: String {
        if let titleOverride { return titleOverride }
        if let slug = store.project?.displaySlug, !slug.isEmpty {
            return slug
        }
        return String(localized: "ccx.panel.titleFallback", defaultValue: "CCX")
    }

    public var displayIcon: String? { "rectangle.3.group" }

    deinit {
        storeChangeCancellable?.cancel()
        storeChangeCancellable = nil
    }

    public func close() {
        // CCXProjectStore tears down its FSEventStream in deinit.
    }

    public func focus() {
        focusFlashToken &+= 1
    }

    public func unfocus() {}

    public func triggerFlash(reason: WorkspaceAttentionFlashReason) {
        focusFlashToken &+= 1
    }
}

/// SwiftUI host that renders a `CCXDashboardPanel` inside cmux's panel system.
public struct CCXDashboardPanelView: View {
    @ObservedObject var panel: CCXDashboardPanel

    public init(panel: CCXDashboardPanel) {
        self.panel = panel
    }

    public var body: some View {
        CCXDashboardView(store: panel.store)
    }
}
