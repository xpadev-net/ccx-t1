import AppKit
import Observation
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
    @State private var registrationViewModel = CCXProjectRegistrationViewModel()
    @State private var unregistrationViewModel = CCXProjectUnregistrationViewModel()

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
                    .contextMenu {
                        Button(role: .destructive) {
                            unregistrationViewModel.request(project)
                        } label: {
                            Label(
                                String(localized: "ccx.projectPicker.unregister", defaultValue: "Unregister project"),
                                systemImage: "trash"
                            )
                        }
                        .disabled(unregistrationViewModel.isSubmitting)
                    }
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
        .confirmationDialog(
            String(localized: "ccx.projectUnregister.title", defaultValue: "Unregister project?"),
            isPresented: Binding(
                get: { unregistrationViewModel.pendingProject != nil },
                set: { isPresented in
                    if !isPresented {
                        unregistrationViewModel.cancel()
                    }
                }
            ),
            titleVisibility: .visible
        ) {
            Button(
                String(localized: "ccx.projectPicker.unregister", defaultValue: "Unregister project"),
                role: .destructive
            ) {
                if let project = unregistrationViewModel.claimPendingProjectForConfirmation() {
                    Task {
                        if await unregistrationViewModel.finishConfirmation(for: project) {
                            store.refresh()
                        }
                    }
                }
            }
            Button(String(localized: "ccx.common.cancel", defaultValue: "Cancel"), role: .cancel) {
                unregistrationViewModel.cancel()
            }
        } message: {
            Text(String(localized: "ccx.projectUnregister.message", defaultValue: "This removes the project from the CCX project list. Project data is kept on disk."))
        }
        .sheet(isPresented: $isAddProjectPresented) {
            CCXProjectRegistrationSheet(
                viewModel: registrationViewModel,
                onCancel: { isAddProjectPresented = false },
                onRegistered: { project in
                    isAddProjectPresented = false
                    onOpenProject(project)
                }
            )
            .interactiveDismissDisabled(registrationViewModel.isSubmitting)
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
            if let error = unregistrationViewModel.errorMessage {
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

@MainActor
@Observable
final class CCXProjectUnregistrationViewModel {
    var pendingProject: CCXProjectSummary?
    private(set) var isSubmitting = false
    private(set) var errorMessage: String?

    private let cliProvider: () -> Result<CCXControllerCLI, CCXControllerCLIError>

    init(cliProvider: @escaping () -> Result<CCXControllerCLI, CCXControllerCLIError> = {
        CCXControllerCLI.make()
    }) {
        self.cliProvider = cliProvider
    }

    func request(_ project: CCXProjectSummary) {
        guard !isSubmitting else { return }
        errorMessage = nil
        pendingProject = project
    }

    func cancel() {
        guard !isSubmitting else { return }
        pendingProject = nil
    }

    func confirm(purge: Bool = false) async -> Bool {
        guard let project = claimPendingProjectForConfirmation() else { return false }
        return await finishConfirmation(for: project, purge: purge)
    }

    func claimPendingProjectForConfirmation() -> CCXProjectSummary? {
        guard !isSubmitting, let project = pendingProject else { return nil }
        isSubmitting = true
        errorMessage = nil
        return project
    }

    func finishConfirmation(for project: CCXProjectSummary, purge: Bool = false) async -> Bool {
        defer { isSubmitting = false }
        do {
            let cli = try cliProvider().get()
            try await cli.unregister(projectId: project.projectId, purge: purge)
            pendingProject = nil
            return true
        } catch let error as CCXControllerCLIError {
            errorMessage = Self.safeErrorMessage(for: error)
            return false
        } catch {
            errorMessage = Self.genericErrorMessage()
            return false
        }
    }

    private static func safeErrorMessage(for error: CCXControllerCLIError) -> String {
        switch error {
        case .executableNotFound, .notExecutable:
            return String(
                localized: "ccx.projectUnregister.error.cliUnavailable",
                defaultValue: "CCX controller CLI is not available. Check the CCX installation, then try again."
            )
        default:
            return genericErrorMessage()
        }
    }

    private static func genericErrorMessage() -> String {
        String(
            localized: "ccx.projectUnregister.error.generic",
            defaultValue: "Could not unregister the project. Check the project list, then try again."
        )
    }
}

private struct CCXProjectRegistrationSheet: View {
    @Bindable var viewModel: CCXProjectRegistrationViewModel
    let onCancel: () -> Void
    let onRegistered: (CCXProjectSummary) -> Void
    @State private var openPanelTask: Task<Void, Never>?

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
                    cancelOpenPanelTask()
                    onCancel()
                }
                .disabled(viewModel.isSubmitting)
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
        .onAppear {
            viewModel.clearSubmitError()
        }
        .onDisappear {
            cancelOpenPanelTask()
        }
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
        cancelOpenPanelTask()
        openPanelTask = Task { @MainActor in
            await chooseRepositoryURL()
        }
    }

    private func chooseTaskSourceFile() {
        cancelOpenPanelTask()
        openPanelTask = Task { @MainActor in
            await chooseTaskSourceFileURL()
        }
    }

    private func cancelOpenPanelTask() {
        openPanelTask?.cancel()
        openPanelTask = nil
    }

    @MainActor
    private func chooseRepositoryURL() async {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        panel.prompt = String(localized: "ccx.projectRegistration.choose", defaultValue: "Choose")
        if await runOpenPanel(panel) == .OK, let url = panel.url {
            viewModel.form.repositoryPath = url.path
            viewModel.validate()
        }
    }

    @MainActor
    private func chooseTaskSourceFileURL() async {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.allowedContentTypes = [UTType(filenameExtension: "md") ?? .text]
        let repositoryPath = viewModel.form.trimmedRepositoryPath
        if !repositoryPath.isEmpty {
            panel.directoryURL = URL(fileURLWithPath: repositoryPath, isDirectory: true)
        }
        panel.prompt = String(localized: "ccx.projectRegistration.choose", defaultValue: "Choose")
        if await runOpenPanel(panel) == .OK, let url = panel.url {
            viewModel.form.taskSourceFilePath = url.path
            viewModel.validate()
        }
    }

    private func runOpenPanel(_ panel: NSOpenPanel) async -> NSApplication.ModalResponse {
        await CCXOpenPanelContinuation(panel: panel).run()
    }
}

@MainActor
private final class CCXOpenPanelContinuation {
    private let panel: NSOpenPanel
    private var continuation: CheckedContinuation<NSApplication.ModalResponse, Never>?
    private var didResume = false

    init(panel: NSOpenPanel) {
        self.panel = panel
    }

    func run() async -> NSApplication.ModalResponse {
        await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                if didResume {
                    continuation.resume(returning: .cancel)
                    return
                }
                self.continuation = continuation
                panel.begin { response in
                    Task { @MainActor in
                        self.resume(response)
                    }
                }
            }
        } onCancel: {
            Task { @MainActor in
                self.cancel()
            }
        }
    }

    private func cancel() {
        panel.cancel(nil)
        resume(.cancel)
    }

    private func resume(_ response: NSApplication.ModalResponse) {
        guard !didResume else { return }
        didResume = true
        let continuation = continuation
        self.continuation = nil
        continuation?.resume(returning: response)
    }
}
