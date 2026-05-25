import AppKit
import SwiftUI
import UniformTypeIdentifiers

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
    let store: CCXProjectsStore
    let onOpenProject: (CCXProjectSummary) -> Void
    @State private var isAddProjectPresented = false
    @StateObject private var registrationViewModel = CCXProjectRegistrationViewModel()

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
            CCXProjectRegistrationSheet(
                viewModel: registrationViewModel,
                onCancel: { isAddProjectPresented = false },
                onRegistered: { project in
                    isAddProjectPresented = false
                    onOpenProject(project)
                }
            )
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

}

public struct CCXProjectRegistrationFormState: Equatable {
    public var repositoryPath: String
    public var taskSourceFilePath: String

    public init(repositoryPath: String = "", taskSourceFilePath: String = "") {
        self.repositoryPath = repositoryPath
        self.taskSourceFilePath = taskSourceFilePath
    }

    public var trimmedRepositoryPath: String {
        repositoryPath.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    public var trimmedTaskSourceFilePath: String {
        taskSourceFilePath.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    public func validationError(fileManager: FileManager = .default) -> CCXProjectRegistrationValidationError? {
        let repository = trimmedRepositoryPath
        guard !repository.isEmpty else { return .emptyRepository }
        var isDirectory = ObjCBool(false)
        guard fileManager.fileExists(atPath: repository, isDirectory: &isDirectory), isDirectory.boolValue else {
            return .repositoryNotDirectory
        }
        let gitDirectory = URL(fileURLWithPath: repository, isDirectory: true).appendingPathComponent(".git", isDirectory: true)
        isDirectory = ObjCBool(false)
        guard fileManager.fileExists(atPath: gitDirectory.path, isDirectory: &isDirectory), isDirectory.boolValue else {
            return .repositoryMissingGitDirectory
        }

        let taskSource = trimmedTaskSourceFilePath
        guard !taskSource.isEmpty else { return .emptyTaskSourceFile }
        isDirectory = ObjCBool(false)
        guard fileManager.fileExists(atPath: taskSource, isDirectory: &isDirectory) else {
            return .taskSourceFileMissing
        }
        guard !isDirectory.boolValue else { return .taskSourceFileIsDirectory }
        guard URL(fileURLWithPath: taskSource).pathExtension.lowercased() == "md" else {
            return .taskSourceFileNotMarkdown
        }

        return nil
    }
}

public enum CCXProjectRegistrationValidationError: Error, Equatable, LocalizedError {
    case emptyRepository
    case repositoryNotDirectory
    case repositoryMissingGitDirectory
    case emptyTaskSourceFile
    case taskSourceFileMissing
    case taskSourceFileIsDirectory
    case taskSourceFileNotMarkdown

    public var errorDescription: String? {
        switch self {
        case .emptyRepository:
            return String(localized: "ccx.projectRegistration.error.emptyRepository",
                          defaultValue: "Choose a repository folder.")
        case .repositoryNotDirectory:
            return String(localized: "ccx.projectRegistration.error.repositoryNotDirectory",
                          defaultValue: "Repository path must be an existing folder.")
        case .repositoryMissingGitDirectory:
            return String(localized: "ccx.projectRegistration.error.repositoryMissingGitDirectory",
                          defaultValue: "Repository folder must contain a .git directory.")
        case .emptyTaskSourceFile:
            return String(localized: "ccx.projectRegistration.error.emptyTaskSourceFile",
                          defaultValue: "Choose a task source Markdown file.")
        case .taskSourceFileMissing:
            return String(localized: "ccx.projectRegistration.error.taskSourceFileMissing",
                          defaultValue: "Task source file does not exist.")
        case .taskSourceFileIsDirectory:
            return String(localized: "ccx.projectRegistration.error.taskSourceFileIsDirectory",
                          defaultValue: "Task source must be a file.")
        case .taskSourceFileNotMarkdown:
            return String(localized: "ccx.projectRegistration.error.taskSourceFileNotMarkdown",
                          defaultValue: "Task source file must use the .md extension.")
        }
    }
}

@MainActor
final class CCXProjectRegistrationViewModel: ObservableObject {
    typealias CLIProvider = () -> Result<CCXControllerCLI, CCXControllerCLIError>

    @Published var form: CCXProjectRegistrationFormState
    @Published private(set) var isSubmitting = false
    @Published private(set) var errorMessage: String?

    private let fileManager: FileManager
    private let cliProvider: CLIProvider

    init(
        form: CCXProjectRegistrationFormState = CCXProjectRegistrationFormState(),
        fileManager: FileManager = .default,
        cliProvider: @escaping CLIProvider = { CCXControllerCLI.make() }
    ) {
        self.form = form
        self.fileManager = fileManager
        self.cliProvider = cliProvider
    }

    var validationError: CCXProjectRegistrationValidationError? {
        form.validationError(fileManager: fileManager)
    }

    var canSubmit: Bool {
        validationError == nil && !isSubmitting
    }

    func submit() async -> CCXProjectSummary? {
        if validationError != nil {
            return nil
        }

        let cli: CCXControllerCLI
        switch cliProvider() {
        case .success(let resolvedCLI):
            cli = resolvedCLI
        case .failure(let error):
            errorMessage = error.localizedDescription
            return nil
        }

        isSubmitting = true
        errorMessage = nil
        defer { isSubmitting = false }

        do {
            return try await cli.register(
                canonicalRepo: URL(fileURLWithPath: form.trimmedRepositoryPath, isDirectory: true),
                taskSourceFile: URL(fileURLWithPath: form.trimmedTaskSourceFilePath)
            )
        } catch {
            errorMessage = Self.message(for: error)
            return nil
        }
    }

    private static func message(for error: Error) -> String {
        if case CCXControllerCLIError.processFailed(_, _, let stderr) = error {
            let trimmed = stderr.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty {
                return trimmed
            }
        }
        return error.localizedDescription
    }
}

private struct CCXProjectRegistrationSheet: View {
    @ObservedObject var viewModel: CCXProjectRegistrationViewModel
    let onCancel: () -> Void
    let onRegistered: (CCXProjectSummary) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(String(localized: "ccx.projectRegistration.title", defaultValue: "Add Project"))
                .font(.title2)
                .fontWeight(.semibold)

            VStack(alignment: .leading, spacing: 10) {
                pathField(
                    title: String(localized: "ccx.projectRegistration.repository", defaultValue: "Repository"),
                    text: $viewModel.form.repositoryPath,
                    action: chooseRepository
                )
                pathField(
                    title: String(localized: "ccx.projectRegistration.taskSource", defaultValue: "Task source"),
                    text: $viewModel.form.taskSourceFilePath,
                    action: chooseTaskSourceFile
                )
            }

            if let message = viewModel.validationError?.localizedDescription ?? viewModel.errorMessage {
                Text(message)
                    .font(.caption)
                    .foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }

            HStack {
                Spacer()
                Button(String(localized: "ccx.common.cancel", defaultValue: "Cancel")) {
                    onCancel()
                }
                Button {
                    Task {
                        if let project = await viewModel.submit() {
                            onRegistered(project)
                        }
                    }
                } label: {
                    if viewModel.isSubmitting {
                        ProgressView()
                            .controlSize(.small)
                    } else {
                        Text(String(localized: "ccx.projectRegistration.register", defaultValue: "Register"))
                    }
                }
                .disabled(!viewModel.canSubmit)
            }
        }
        .frame(width: 520, alignment: .leading)
        .padding(20)
    }

    private func pathField(title: String, text: Binding<String>, action: @escaping () -> Void) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(.caption)
                .foregroundStyle(.secondary)
            HStack(spacing: 8) {
                TextField(title, text: text)
                    .textFieldStyle(.roundedBorder)
                Button(action: action) {
                    Image(systemName: "folder")
                }
                .help(String(localized: "ccx.projectRegistration.choose", defaultValue: "Choose"))
            }
        }
    }

    private func chooseRepository() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = String(localized: "ccx.projectRegistration.choose", defaultValue: "Choose")
        if panel.runModal() == .OK, let url = panel.url {
            viewModel.form.repositoryPath = url.path
        }
    }

    private func chooseTaskSourceFile() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [UTType(filenameExtension: "md") ?? .text]
        panel.directoryURL = URL(fileURLWithPath: viewModel.form.trimmedRepositoryPath, isDirectory: true)
        panel.prompt = String(localized: "ccx.projectRegistration.choose", defaultValue: "Choose")
        if panel.runModal() == .OK, let url = panel.url {
            viewModel.form.taskSourceFilePath = url.path
        }
    }
}
