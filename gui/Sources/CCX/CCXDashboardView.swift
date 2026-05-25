import AppKit
import os
import SwiftUI

private let ccxTaskSourceLogger = Logger(
    subsystem: "com.cmuxterm.ccx",
    category: "CCXTaskSource"
)

/// Top-level CCX dashboard. Hosts the project's overview, work executions,
/// review/PR state, and artifacts in a tabbed layout. The dashboard reads
/// from a `CCXProjectStore` that mirrors the Rust controller's read model.
public struct CCXDashboardView: View {
    @ObservedObject var store: CCXProjectStore
    let projectsStore: CCXProjectsStore?
    let onOpenProject: (CCXProjectSummary) -> Void
    @State private var selection: Tab = .overview

    public enum Tab: String, CaseIterable, Identifiable {
        case overview, workExecutions, reviews, tasks, artifacts
        public var id: String { rawValue }

        var label: String {
            switch self {
            case .overview:
                return String(localized: "ccx.dashboard.tab.overview", defaultValue: "Overview")
            case .workExecutions:
                return String(localized: "ccx.dashboard.tab.workExecutions", defaultValue: "Work executions")
            case .reviews:
                return String(localized: "ccx.dashboard.tab.reviews", defaultValue: "Reviews")
            case .tasks:
                return String(localized: "ccx.dashboard.tab.tasks", defaultValue: "Tasks")
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
                case .tasks:
                    CCXTasksView(project: store.project)
                case .artifacts:
                    CCXArtifactsView(store: store)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .onAppear {
            startDashboardStores()
        }
        .onChange(of: ObjectIdentifier(store)) { _, _ in
            store.start()
        }
    }

    private func startDashboardStores() {
        store.start()
        projectsStore?.start()
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
    let projectsStore: CCXProjectsStore
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

public struct CCXTasksView: View {
    let project: CCXProjectSummary?

    public init(project: CCXProjectSummary?) {
        self.project = project
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                if let project {
                    CCXTaskSourcePanel(project: project)
                } else {
                    placeholderView(String(localized: "ccx.tasks.loading",
                                           defaultValue: "Loading task source…"))
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
        }
    }
}

private struct CCXTaskSourcePanel: View {
    let project: CCXProjectSummary
    @State private var status: CCXTaskSourceFileStatus

    init(project: CCXProjectSummary) {
        self.project = project
        self._status = State(initialValue: CCXTaskSourceFileStatus.checking(path: project.taskSourceFile))
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline) {
                Text(String(localized: "ccx.tasks.source.title", defaultValue: "Task source file"))
                    .font(.headline)
                Spacer(minLength: 12)
                Text(status.badgeLabel)
                    .font(.caption)
                    .padding(.horizontal, 7)
                    .padding(.vertical, 3)
                    .background(status.badgeTint.opacity(0.16), in: Capsule())
                    .foregroundStyle(status.badgeTint)
            }

            labelled(String(localized: "ccx.tasks.source.path", defaultValue: "Path"),
                     status.displayPath)
            labelled(String(localized: "ccx.tasks.source.lastRead", defaultValue: "Last read"),
                     status.checkedAt.formatted(date: .abbreviated, time: .standard))

            if let modifiedAt = status.modifiedAt {
                labelled(String(localized: "ccx.tasks.source.modified", defaultValue: "Modified"),
                         modifiedAt.formatted(date: .abbreviated, time: .standard))
            }

            if let message = status.message {
                Text(message)
                    .font(.callout)
                    .foregroundStyle(status.isReady ? Color.secondary : Color.orange)
                    .textSelection(.enabled)
            }

            HStack(spacing: 8) {
                Button {
                    openTaskSource()
                } label: {
                    Label(String(localized: "ccx.tasks.action.open", defaultValue: "Open in Editor"),
                          systemImage: "square.and.pencil")
                }
                .disabled(!status.canOpen)

                Button {
                    revealTaskSource()
                } label: {
                    Label(String(localized: "ccx.tasks.action.reveal", defaultValue: "Reveal in Finder"),
                          systemImage: "folder")
                }
                .disabled(!status.hasPath)

                Button {
                    copyTaskSourcePath()
                } label: {
                    Label(String(localized: "ccx.tasks.action.copyPath", defaultValue: "Copy Path"),
                          systemImage: "doc.on.doc")
                }
                .disabled(!status.hasPath)
            }
            .buttonStyle(.bordered)
        }
        .padding(12)
        .background(.quaternary.opacity(0.3), in: RoundedRectangle(cornerRadius: 8))
        .task(id: project.taskSourceFile) {
            await refreshStatus(for: project.taskSourceFile)
        }
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

    private func openTaskSource() {
        guard let url = status.fileURL else { return }
        NSWorkspace.shared.open(url)
    }

    private func revealTaskSource() {
        guard let url = status.revealURL else { return }
        if status.canOpen {
            NSWorkspace.shared.activateFileViewerSelecting([url])
        } else {
            NSWorkspace.shared.activateFileViewerSelecting([url.deletingLastPathComponent()])
        }
    }

    private func copyTaskSourcePath() {
        guard status.hasPath else { return }
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString(status.path, forType: .string)
    }

    private func refreshStatus(for path: String) async {
        status = CCXTaskSourceFileStatus.checking(path: path)
        let nextStatus = await Task.detached(priority: .utility) {
            CCXTaskSourceFileStatus(path: path)
        }.value
        guard !Task.isCancelled else { return }
        status = nextStatus
    }
}

struct CCXTaskSourceFileStatus: Equatable, Sendable {
    enum Kind: Equatable, Sendable {
        case checking
        case missingPath
        case missingFile
        case directory
        case notMarkdown
        case ready
        case unreadable(String)
    }

    let path: String
    let kind: Kind
    let checkedAt: Date
    let modifiedAt: Date?

    private init(path: String, kind: Kind, checkedAt: Date, modifiedAt: Date?) {
        self.path = path.trimmingCharacters(in: .whitespacesAndNewlines)
        self.kind = kind
        self.checkedAt = checkedAt
        self.modifiedAt = modifiedAt
    }

    static func checking(path: String, checkedAt: Date = Date()) -> CCXTaskSourceFileStatus {
        CCXTaskSourceFileStatus(path: path, kind: .checking, checkedAt: checkedAt, modifiedAt: nil)
    }

    init(
        path: String,
        fileManager: FileManager = .default,
        checkedAt: Date = Date()
    ) {
        let trimmedPath = path.trimmingCharacters(in: .whitespacesAndNewlines)
        self.path = trimmedPath
        self.checkedAt = checkedAt

        guard !trimmedPath.isEmpty else {
            self.kind = .missingPath
            self.modifiedAt = nil
            return
        }

        var isDirectory: ObjCBool = false
        guard fileManager.fileExists(atPath: trimmedPath, isDirectory: &isDirectory) else {
            self.kind = .missingFile
            self.modifiedAt = nil
            return
        }

        guard !isDirectory.boolValue else {
            self.kind = .directory
            self.modifiedAt = nil
            return
        }

        let markdownExtensions = ["md", "markdown"]
        guard markdownExtensions.contains(URL(fileURLWithPath: trimmedPath).pathExtension.lowercased()) else {
            self.kind = .notMarkdown
            self.modifiedAt = nil
            return
        }

        do {
            _ = try Data(contentsOf: URL(fileURLWithPath: trimmedPath), options: [.mappedIfSafe])
            let attributes = try fileManager.attributesOfItem(atPath: trimmedPath)
            self.kind = .ready
            self.modifiedAt = attributes[.modificationDate] as? Date
        } catch {
            ccxTaskSourceLogger.warning(
                "Could not read task source file at \(trimmedPath, privacy: .private): \(error.localizedDescription, privacy: .public)"
            )
            self.kind = .unreadable(error.localizedDescription)
            self.modifiedAt = nil
        }
    }

    var fileURL: URL? {
        hasPath ? URL(fileURLWithPath: path) : nil
    }

    var revealURL: URL? {
        fileURL
    }

    var hasPath: Bool {
        !path.isEmpty
    }

    var canOpen: Bool {
        kind == .ready
    }

    var isReady: Bool {
        kind == .ready
    }

    var displayPath: String {
        hasPath ? path : String(localized: "ccx.tasks.source.path.empty", defaultValue: "Not configured")
    }

    var badgeLabel: String {
        switch kind {
        case .checking:
            return String(localized: "ccx.tasks.status.checking", defaultValue: "Checking")
        case .ready:
            return String(localized: "ccx.tasks.status.ready", defaultValue: "Ready")
        case .missingPath:
            return String(localized: "ccx.tasks.status.notConfigured", defaultValue: "Not configured")
        default:
            return String(localized: "ccx.tasks.status.needsAttention", defaultValue: "Needs attention")
        }
    }

    var badgeTint: Color {
        switch kind {
        case .ready:
            return .green
        case .checking, .missingPath:
            return .secondary
        default:
            return .orange
        }
    }

    var message: String? {
        switch kind {
        case .checking:
            return String(localized: "ccx.tasks.message.checking",
                          defaultValue: "Checking the registered task source file.")
        case .ready:
            return String(localized: "ccx.tasks.message.ready",
                          defaultValue: "The registered Markdown task source is available.")
        case .missingPath:
            return String(localized: "ccx.tasks.message.missingPath",
                          defaultValue: "This project does not have a task source file configured.")
        case .missingFile:
            return String(localized: "ccx.tasks.message.missingFile",
                          defaultValue: "The configured task source file does not exist.")
        case .directory:
            return String(localized: "ccx.tasks.message.directory",
                          defaultValue: "The configured task source path points to a directory.")
        case .notMarkdown:
            return String(localized: "ccx.tasks.message.notMarkdown",
                          defaultValue: "The configured task source file is not a Markdown file.")
        case .unreadable:
            return String(localized: "ccx.tasks.message.unreadable",
                          defaultValue: "The task source file could not be read. Check file permissions and try again.")
        }
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
