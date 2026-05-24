import SwiftUI

/// A compact sidebar surface that summarizes the current project's CCX state.
/// Designed to be embedded inside cmux's right-sidebar panel system as a custom
/// `PanelType` case (see `Sources/Panels/Panel.swift` for the existing enum and
/// `gui/Sources/CCX/README.md` for the wiring steps).
public struct CCXSidebarPanel: View {
    @ObservedObject var store: CCXProjectStore

    public init(store: CCXProjectStore) {
        self.store = store
    }

    public var body: some View {
        let snapshot = store.workExecutions
        let recent = store.recentEvents
        VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(String(localized: "ccx.sidebar.title", defaultValue: "CCX project"))
                    .font(.headline)
                if let project = store.project {
                    Text(project.displaySlug)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
            }

            statsBlock(workExecutions: snapshot)

            VStack(alignment: .leading, spacing: 4) {
                Text(String(localized: "ccx.sidebar.recentEvents", defaultValue: "Recent events"))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                if recent.isEmpty {
                    Text(String(localized: "ccx.sidebar.noEvents",
                                defaultValue: "No events recorded yet."))
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(recent.prefix(10)) { ev in
                        HStack(alignment: .firstTextBaseline, spacing: 6) {
                            Text(ev.kind)
                                .font(.system(.caption2, design: .monospaced))
                                .lineLimit(1)
                            Spacer()
                            Text(shortTimestamp(ev.timestamp))
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }

            Spacer()
        }
        .padding(12)
        .onAppear { store.start() }
    }

    private func statsBlock(workExecutions: [CCXWorkExecution]) -> some View {
        let totals = CCXOverviewPanel.StateTotals(workExecutions: workExecutions)
        return Grid(alignment: .leading, horizontalSpacing: 12, verticalSpacing: 6) {
            GridRow {
                statCell(String(localized: "ccx.sidebar.running", defaultValue: "Running"),
                         totals.running)
                statCell(String(localized: "ccx.sidebar.prOpen", defaultValue: "PR open"),
                         totals.prOpen)
            }
            GridRow {
                statCell(String(localized: "ccx.sidebar.merged", defaultValue: "Merged"),
                         totals.merged)
                statCell(String(localized: "ccx.sidebar.blocked", defaultValue: "Blocked"),
                         totals.blocked)
            }
        }
    }

    private func statCell(_ label: String, _ count: Int) -> some View {
        HStack(spacing: 4) {
            Text("\(count)").font(.callout).fontWeight(.semibold)
            Text(label).font(.caption).foregroundStyle(.secondary)
        }
    }

    private func shortTimestamp(_ ts: String) -> String {
        // Inputs are RFC3339; show only the time portion when present.
        if let tIdx = ts.firstIndex(of: "T") {
            let after = ts.index(after: tIdx)
            return String(ts[after...].prefix(8))
        }
        return String(ts.prefix(8))
    }
}
