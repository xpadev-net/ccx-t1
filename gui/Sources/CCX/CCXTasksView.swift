import AppKit
import SwiftUI

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
                                           defaultValue: "Loading task source..."))
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
