import AppKit
import Observation
import SwiftUI

public struct CCXTasksView: View {
    let project: CCXProjectSummary?
    let workExecutions: [CCXWorkExecution]
    let agentSessions: [CCXAgentSession]
    let recentEvents: [CCXEventEntry]

    public init(
        project: CCXProjectSummary?,
        workExecutions: [CCXWorkExecution] = [],
        agentSessions: [CCXAgentSession] = [],
        recentEvents: [CCXEventEntry] = []
    ) {
        self.project = project
        self.workExecutions = workExecutions
        self.agentSessions = agentSessions
        self.recentEvents = recentEvents
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                if let project {
                    CCXTaskSourcePanel(
                        project: project,
                        workExecutions: workExecutions,
                        orchestratorSessionId: Self.activeOrchestratorSessionId(agentSessions),
                        sourceChangeToken: Self.latestTaskSourceChangeToken(
                            recentEvents,
                            project: project
                        )
                    )
                        .id(project.projectId)
                } else {
                    placeholderView(String(localized: "ccx.tasks.loading",
                                           defaultValue: "Loading task source..."))
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
        }
    }

    private static func activeOrchestratorSessionId(_ sessions: [CCXAgentSession]) -> String? {
        sessions.first { session in
            session.role == "orchestrator"
                && ["starting", "running", "idle"].contains(session.state)
        }?.agentSessionId
    }

    private static func latestTaskSourceChangeToken(
        _ events: [CCXEventEntry],
        project: CCXProjectSummary
    ) -> String? {
        events.first { event in
            event.projectId == project.projectId
                && event.kind == "task_source_file_changed"
                && event.taskSourceFile == project.taskSourceFile
        }.map { event in
            "\(event.eventId):\(event.newHash ?? "")"
        }
    }
}

private struct CCXTaskSourcePanel: View {
    let project: CCXProjectSummary
    let workExecutions: [CCXWorkExecution]
    let orchestratorSessionId: String?
    let sourceChangeToken: String?
    @State private var status: CCXTaskSourceFileStatus
    @State private var sourceStore: CCXTaskSourceStore

    init(
        project: CCXProjectSummary,
        workExecutions: [CCXWorkExecution],
        orchestratorSessionId: String?,
        sourceChangeToken: String?
    ) {
        self.project = project
        self.workExecutions = workExecutions
        self.orchestratorSessionId = orchestratorSessionId
        self.sourceChangeToken = sourceChangeToken
        self._status = State(initialValue: CCXTaskSourceFileStatus.checking(path: project.taskSourceFile))
        self._sourceStore = State(initialValue: CCXTaskSourceStore(projectId: project.projectId))
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

            if let warning = sourceStore.warningMessage {
                Text(warning)
                    .font(.callout)
                    .foregroundStyle(.orange)
                    .textSelection(.enabled)
            }

            if let sourceChange = sourceStore.sourceChangeMessage {
                Text(sourceChange)
                    .font(.callout)
                    .foregroundStyle(.orange)
                    .textSelection(.enabled)
            }

            if let error = sourceStore.errorMessage {
                Text(error)
                    .font(.callout)
                    .foregroundStyle(.red)
                    .textSelection(.enabled)
            }

            if let conflict = sourceStore.conflictMessage {
                Text(conflict)
                    .font(.callout)
                    .foregroundStyle(.orange)
                    .textSelection(.enabled)
            }

            VStack(alignment: .leading, spacing: 6) {
                HStack(alignment: .firstTextBaseline) {
                    Text(String(localized: "ccx.tasks.editor.title", defaultValue: "Markdown"))
                        .font(.headline)
                    Spacer(minLength: 12)
                    if sourceStore.isDirty {
                        Text(String(localized: "ccx.tasks.editor.unsaved", defaultValue: "Unsaved"))
                            .font(.caption)
                            .foregroundStyle(.orange)
                    }
                }
                TextEditor(text: $sourceStore.draftContent)
                    .font(.system(.body, design: .monospaced))
                    .scrollContentBackground(.hidden)
                    .frame(minHeight: 320)
                    .background(.background, in: RoundedRectangle(cornerRadius: 6))
                    .overlay(
                        RoundedRectangle(cornerRadius: 6)
                            .stroke(.separator.opacity(0.7))
                    )
                    .disabled(sourceStore.isLoading || sourceStore.isSaving || !status.canOpen)
            }

            VStack(alignment: .leading, spacing: 8) {
                Text(String(localized: "ccx.tasks.composer.title", defaultValue: "Add task with Orchestrator"))
                    .font(.headline)
                TextEditor(text: $sourceStore.composerInput)
                    .font(.body)
                    .scrollContentBackground(.hidden)
                    .frame(minHeight: 96)
                    .background(.background, in: RoundedRectangle(cornerRadius: 6))
                    .overlay(
                        RoundedRectangle(cornerRadius: 6)
                            .stroke(.separator.opacity(0.7))
                    )
                    .disabled(sourceStore.isComposing || !status.canOpen)
                    .onChange(of: sourceStore.composerInput) { _, newValue in
                        if !newValue.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                            sourceStore.clearComposerStatusMessage()
                        }
                    }
                if let status = sourceStore.composerStatusMessage {
                    Text(status)
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                if let error = sourceStore.composerErrorMessage {
                    Text(error)
                        .font(.callout)
                        .foregroundStyle(.red)
                }
                Button {
                    Task {
                        await sourceStore.submitNaturalLanguageTask(
                            project: project,
                            workExecutions: workExecutions,
                            orchestratorSessionId: orchestratorSessionId
                        )
                    }
                } label: {
                    Label(String(localized: "ccx.tasks.composer.submit", defaultValue: "Refine with LLM and Add"),
                          systemImage: "sparkles")
                }
                .disabled(!sourceStore.canSubmitComposer || sourceStore.isComposing || !status.canOpen)
            }

            HStack(spacing: 8) {
                Button {
                    Task { await sourceStore.save() }
                } label: {
                    Label(String(localized: "ccx.tasks.action.save", defaultValue: "Save"),
                          systemImage: "square.and.arrow.down")
                }
                .disabled(!sourceStore.canSave || !status.canOpen)

                Button {
                    Task { await sourceStore.reload() }
                } label: {
                    Label(String(localized: "ccx.tasks.action.reload", defaultValue: "Reload"),
                          systemImage: "arrow.clockwise")
                }
                .disabled(sourceStore.isDirty || sourceStore.isLoading || sourceStore.isSaving || !status.canOpen)

                Button {
                    sourceStore.discardChanges()
                } label: {
                    Label(String(localized: "ccx.tasks.action.discard", defaultValue: "Discard"),
                          systemImage: "xmark.circle")
                }
                .disabled(!sourceStore.isDirty || sourceStore.isLoading || sourceStore.isSaving)

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
            if status.canOpen {
                await sourceStore.load()
            }
        }
        .task(id: sourceChangeToken) {
            guard sourceChangeToken != nil, status.canOpen else { return }
            await refreshStatus(for: project.taskSourceFile)
            await sourceStore.handleTaskSourceChanged()
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
        if status.existsOnDisk {
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

private func placeholderView(_ text: String) -> some View {
    VStack {
        Spacer()
        Text(text)
            .foregroundStyle(.secondary)
        Spacer()
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity)
}
