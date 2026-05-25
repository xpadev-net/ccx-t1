import SwiftUI

/// Top-level CCX dashboard. Hosts the project's overview, work executions,
/// review/PR state, and artifacts in a tabbed layout. The dashboard reads
/// from a `CCXProjectStore` that mirrors the Rust controller's read model.
public struct CCXDashboardView: View {
    @ObservedObject var store: CCXProjectStore
    let projectsStore: CCXProjectsStore?
    let onOpenProject: (CCXProjectSummary) -> Void
    @State private var selection: Tab = .overview

    public enum Tab: String, CaseIterable, Identifiable {
        case overview, workExecutions, reviews, artifacts
        public var id: String { rawValue }

        var label: String {
            switch self {
            case .overview:
                return String(localized: "ccx.dashboard.tab.overview", defaultValue: "Overview")
            case .workExecutions:
                return String(localized: "ccx.dashboard.tab.workExecutions", defaultValue: "Work executions")
            case .reviews:
                return String(localized: "ccx.dashboard.tab.reviews", defaultValue: "Reviews")
            case .artifacts:
                return String(localized: "ccx.dashboard.tab.artifacts", defaultValue: "Artifacts")
            }
        }
    }

    public init(
        store: CCXProjectStore,
        projectsStore: CCXProjectsStore? = nil,
        onOpenProject: @escaping (CCXProjectSummary) -> Void = { _ in }
    ) {
        self.store = store
        self.projectsStore = projectsStore
        self.onOpenProject = onOpenProject
    }

    public var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            Picker("", selection: $selection) {
                ForEach(Tab.allCases) { tab in
                    Text(tab.label).tag(tab)
                }
            }
            .pickerStyle(.segmented)
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

            Group {
                switch selection {
                case .overview:
                    CCXOverviewPanel(store: store)
                case .workExecutions:
                    CCXWorkExecutionsView(store: store)
                case .reviews:
                    CCXReviewsView(store: store)
                case .artifacts:
                    CCXArtifactsView(store: store)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .onAppear {
            store.start()
            projectsStore?.start()
        }
    }

    private var header: some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                if let project = store.project {
                    Text(projectTitle(project))
                        .font(.title2)
                        .fontWeight(.semibold)
                    Text(project.canonicalRepo)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    Text(String(localized: "ccx.dashboard.loading", defaultValue: "Loading project…"))
                        .font(.title2)
                        .foregroundStyle(.secondary)
                }
                if let err = store.lastRefreshError {
                    Text(err)
                        .font(.caption)
                        .foregroundStyle(.orange)
                }
            }
            Spacer(minLength: 12)
            if let projectsStore {
                CCXProjectSwitchMenu(
                    projectsStore: projectsStore,
                    currentProjectId: store.projectId,
                    onOpenProject: onOpenProject
                )
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
    }

    private func projectTitle(_ project: CCXProjectSummary) -> String {
        if !project.displaySlug.isEmpty { return project.displaySlug }
        if !project.canonicalRepo.isEmpty { return project.canonicalRepo }
        return project.projectId
    }
}

private struct CCXProjectSwitchMenu: View {
    @Bindable var projectsStore: CCXProjectsStore
    let currentProjectId: String?
    let onOpenProject: (CCXProjectSummary) -> Void

    var body: some View {
        Menu {
            if let error = projectsStore.lastRefreshError {
                Text(error)
            } else if projectsStore.projects.isEmpty {
                Text(String(localized: "ccx.projectPicker.empty", defaultValue: "No CCX projects registered."))
            } else {
                ForEach(projectsStore.projects) { project in
                    Button {
                        onOpenProject(project)
                    } label: {
                        Text(project.displaySlug.isEmpty ? project.projectId : project.displaySlug)
                    }
                    .disabled(project.projectId == currentProjectId)
                }
            }
        } label: {
            Label(
                String(localized: "ccx.dashboard.switchProject", defaultValue: "Switch project"),
                systemImage: "rectangle.2.swap"
            )
        }
        .menuStyle(.button)
    }
}

public struct CCXOverviewPanel: View {
    @ObservedObject var store: CCXProjectStore

    public init(store: CCXProjectStore) {
        self.store = store
    }

    public var body: some View {
        let totals = StateTotals(workExecutions: store.workExecutions)
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                summaryRow(totals: totals)
                if let project = store.project {
                    VStack(alignment: .leading, spacing: 4) {
                        labelled(String(localized: "ccx.overview.projectId", defaultValue: "Project ID"),
                                 project.projectId)
                        labelled(String(localized: "ccx.overview.taskFile", defaultValue: "Task source"),
                                 project.taskSourceFile)
                        labelled(String(localized: "ccx.overview.created", defaultValue: "Created"),
                                 project.createdAt)
                    }
                    .padding(12)
                    .background(.quaternary.opacity(0.3), in: RoundedRectangle(cornerRadius: 8))
                }
            }
            .padding(12)
        }
    }

    private func summaryRow(totals: StateTotals) -> some View {
        HStack(spacing: 12) {
            statTile(String(localized: "ccx.overview.running", defaultValue: "Running"),
                     totals.running)
            statTile(String(localized: "ccx.overview.prOpen", defaultValue: "PR open"),
                     totals.prOpen)
            statTile(String(localized: "ccx.overview.merged", defaultValue: "Merged"),
                     totals.merged)
            statTile(String(localized: "ccx.overview.blocked", defaultValue: "Blocked"),
                     totals.blocked)
        }
    }

    private func statTile(_ label: String, _ count: Int) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text("\(count)")
                .font(.title)
                .fontWeight(.semibold)
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
        .background(.quaternary.opacity(0.3), in: RoundedRectangle(cornerRadius: 8))
    }

    private func labelled(_ label: String, _ value: String) -> some View {
        HStack(alignment: .firstTextBaseline) {
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(width: 96, alignment: .leading)
            Text(value)
                .font(.callout)
                .textSelection(.enabled)
        }
    }

    struct StateTotals {
        let running: Int
        let prOpen: Int
        let merged: Int
        let blocked: Int

        init(workExecutions: [CCXWorkExecution]) {
            var r = 0, p = 0, m = 0, b = 0
            for w in workExecutions {
                switch w.state {
                case "running", "dispatched", "review_fixing", "merging":
                    r += 1
                case "pr_open", "gate_check", "merge_ready":
                    p += 1
                case "merged":
                    m += 1
                case "blocked", "failed":
                    b += 1
                default:
                    break
                }
            }
            self.running = r
            self.prOpen = p
            self.merged = m
            self.blocked = b
        }
    }
}

public struct CCXWorkExecutionsView: View {
    @ObservedObject var store: CCXProjectStore

    public init(store: CCXProjectStore) {
        self.store = store
    }

    public var body: some View {
        // Snapshot the array into a value type before handing to the row builder,
        // following the cmux snapshot-boundary rule (no ObservableObject in row
        // builders).
        let items = store.workExecutions
        List(items) { item in
            CCXWorkExecutionRow(item: item)
        }
    }
}

private struct CCXWorkExecutionRow: View {
    let item: CCXWorkExecution

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(item.displayText ?? item.workExecutionId)
                    .font(.callout)
                    .lineLimit(1)
                Spacer()
                Text(stateLabel)
                    .font(.caption)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(.quaternary.opacity(0.4), in: Capsule())
            }
            if let branch = item.branchName {
                Text(branch)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .padding(.vertical, 2)
    }

    private var stateLabel: String {
        CCXWorkExecutionState(rawValue: item.state)?.localizedLabel ?? item.state
    }
}

public struct CCXReviewsView: View {
    @ObservedObject var store: CCXProjectStore

    public init(store: CCXProjectStore) {
        self.store = store
    }

    public var body: some View {
        let items = store.workExecutions.filter {
            $0.state == "pr_open" || $0.state == "gate_check"
                || $0.state == "review_fixing" || $0.state == "merge_ready"
        }
        if items.isEmpty {
            placeholderView(String(localized: "ccx.reviews.empty",
                                   defaultValue: "No work executions are in review."))
        } else {
            List(items) { item in
                VStack(alignment: .leading, spacing: 2) {
                    Text(item.displayText ?? item.workExecutionId)
                        .font(.callout)
                    if let prUrl = item.prUrl {
                        Text(prUrl)
                            .font(.caption)
                            .foregroundStyle(.blue)
                            .textSelection(.enabled)
                    }
                    Text(item.state)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }
}

public struct CCXArtifactsView: View {
    @ObservedObject var store: CCXProjectStore

    public init(store: CCXProjectStore) {
        self.store = store
    }

    public var body: some View {
        let items = store.workExecutions.filter { $0.worktreePath != nil }
        if items.isEmpty {
            placeholderView(String(localized: "ccx.artifacts.empty",
                                   defaultValue: "No worktrees have been created yet."))
        } else {
            List(items) { item in
                VStack(alignment: .leading, spacing: 2) {
                    Text(item.worktreePath ?? "")
                        .font(.callout)
                        .lineLimit(1)
                        .textSelection(.enabled)
                    HStack(spacing: 8) {
                        if let head = item.headCommit {
                            Text(head.prefix(8))
                                .font(.system(.caption, design: .monospaced))
                                .foregroundStyle(.secondary)
                        }
                        if let state = item.artifactState {
                            Text(state)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
        }
    }
}

@ViewBuilder
private func placeholderView(_ text: String) -> some View {
    VStack {
        Spacer()
        Text(text)
            .foregroundStyle(.secondary)
        Spacer()
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
}
