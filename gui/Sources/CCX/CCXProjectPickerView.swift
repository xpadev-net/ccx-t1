import SwiftUI

public struct CCXProjectPickerRowModel: Identifiable, Equatable {
    public let id: String
    public let title: String
    public let subtitle: String
    public let taskSourceFile: String

    public init(summary: CCXProjectSummary) {
        self.id = summary.projectId
        self.title = summary.displaySlug.isEmpty ? summary.projectId : summary.displaySlug
        self.subtitle = summary.canonicalRepo
        self.taskSourceFile = summary.taskSourceFile
    }
}

public struct CCXProjectPickerView: View {
    @Bindable var store: CCXProjectsStore
    let onOpenProject: (CCXProjectSummary) -> Void
    @State private var isAddProjectPresented = false

    public init(
        store: CCXProjectsStore,
        onOpenProject: @escaping (CCXProjectSummary) -> Void
    ) {
        self.store = store
        self.onOpenProject = onOpenProject
    }

    public var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            List {
                ForEach(store.projects) { project in
                    Button {
                        onOpenProject(project)
                    } label: {
                        projectRow(CCXProjectPickerRowModel(summary: project))
                    }
                    .buttonStyle(.plain)
                }
                if !store.projects.isEmpty {
                    addProjectButton
                }
            }
            .overlay {
                if store.projects.isEmpty {
                    emptyState
                }
            }
        }
        .onAppear { store.start() }
        .sheet(isPresented: $isAddProjectPresented) {
            addProjectPlaceholder
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(String(localized: "ccx.projectPicker.title", defaultValue: "CCX Projects"))
                .font(.title2)
                .fontWeight(.semibold)
            Text(String(localized: "ccx.projectPicker.subtitle", defaultValue: "Choose a registered project."))
                .font(.caption)
                .foregroundStyle(.secondary)
            if let error = store.lastRefreshError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.orange)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(12)
    }

    private func projectRow(_ model: CCXProjectPickerRowModel) -> some View {
        HStack(spacing: 10) {
            Image(systemName: "rectangle.3.group")
                .foregroundStyle(.secondary)
                .frame(width: 18)
            VStack(alignment: .leading, spacing: 2) {
                Text(model.title)
                    .font(.callout)
                    .lineLimit(1)
                Text(model.subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                if !model.taskSourceFile.isEmpty {
                    Text(model.taskSourceFile)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                }
            }
            Spacer()
        }
        .padding(.vertical, 4)
    }

    private var addProjectButton: some View {
        Button {
            isAddProjectPresented = true
        } label: {
            Label(
                String(localized: "ccx.projectPicker.addProject", defaultValue: "Add Project"),
                systemImage: "plus"
            )
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.vertical, 6)
        }
        .buttonStyle(.plain)
    }

    private var emptyState: some View {
        VStack(spacing: 8) {
            Image(systemName: "tray")
                .font(.title2)
                .foregroundStyle(.secondary)
            Text(String(localized: "ccx.projectPicker.empty", defaultValue: "No CCX projects registered."))
                .font(.callout)
                .foregroundStyle(.secondary)
            addProjectButton
                .frame(width: 180)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var addProjectPlaceholder: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(String(localized: "ccx.projectPicker.addProject", defaultValue: "Add Project"))
                .font(.title2)
                .fontWeight(.semibold)
            Text(String(localized: "ccx.projectPicker.addProjectPlaceholder",
                        defaultValue: "Project registration is coming soon."))
                .foregroundStyle(.secondary)
            Button(String(localized: "ccx.common.close", defaultValue: "Close")) {
                isAddProjectPresented = false
            }
        }
        .frame(width: 360, alignment: .leading)
        .padding(20)
    }
}
