import Foundation

@MainActor
final class CCXTaskSourceStore: ObservableObject {
    typealias CLIProvider = () -> Result<CCXControllerCLI, CCXControllerCLIError>

    @Published private(set) var snapshot: CCXTaskSourceSnapshot?
    @Published var draftContent = ""
    @Published private(set) var isLoading = false
    @Published private(set) var isSaving = false
    @Published private(set) var errorMessage: String?
    @Published private(set) var conflictMessage: String?

    private let projectId: String
    private let cliProvider: CLIProvider

    init(
        projectId: String,
        cliProvider: @escaping CLIProvider = { CCXControllerCLI.make() }
    ) {
        self.projectId = projectId
        self.cliProvider = cliProvider
    }

    var isDirty: Bool {
        guard let snapshot else { return false }
        return draftContent != snapshot.content
    }

    var canSave: Bool {
        snapshot != nil && isDirty && !isLoading && !isSaving
    }

    var loadedHash: String? {
        snapshot?.hash
    }

    var loadedMtime: String? {
        snapshot?.mtime
    }

    var warningMessage: String? {
        snapshot?.warning?.message
    }

    func load() async {
        guard !isLoading else { return }
        isLoading = true
        errorMessage = nil
        conflictMessage = nil
        defer { isLoading = false }

        do {
            let loaded = try await cli().readTaskSource(projectId: projectId)
            apply(snapshot: loaded)
        } catch {
            errorMessage = Self.message(for: error)
        }
    }

    func reload() async {
        await load()
    }

    func discardChanges() {
        guard let snapshot else { return }
        draftContent = snapshot.content
        conflictMessage = nil
        errorMessage = nil
    }

    func save() async {
        guard canSave, let expectedHash = snapshot?.hash else { return }
        isSaving = true
        errorMessage = nil
        conflictMessage = nil
        defer { isSaving = false }

        do {
            let result = try await cli().writeTaskSource(
                projectId: projectId,
                expectedHash: expectedHash,
                content: draftContent
            )
            snapshot = CCXTaskSourceSnapshot(
                projectId: result.projectId,
                path: result.path,
                content: draftContent,
                hash: result.hash,
                mtime: result.mtime,
                warning: result.warning
            )
        } catch {
            if Self.isConflict(error) {
                conflictMessage = String(
                    localized: "ccx.tasks.editor.conflict",
                    defaultValue: "The task source changed on disk. Reload before saving, or discard your draft."
                )
            } else {
                errorMessage = Self.message(for: error)
            }
        }
    }

    private func apply(snapshot: CCXTaskSourceSnapshot) {
        self.snapshot = snapshot
        self.draftContent = snapshot.content
    }

    private func cli() throws -> CCXControllerCLI {
        switch cliProvider() {
        case .success(let cli):
            return cli
        case .failure(let error):
            throw error
        }
    }

    private static func isConflict(_ error: Error) -> Bool {
        guard case let CCXControllerCLIError.processFailed(_, _, stderr) = error else {
            return false
        }
        return stderr.contains("task source conflict")
    }

    private static func message(for error: Error) -> String {
        if let cliError = error as? CCXControllerCLIError {
            switch cliError {
            case .executableNotFound, .notExecutable, .launchFailed:
                return String(localized: "ccx.tasks.editor.error.cliUnavailable",
                              defaultValue: "CCX controller CLI is not available.")
            case .processFailed, .invalidJSON, .timedOut, .cancelled:
                return String(localized: "ccx.tasks.editor.error.generic",
                              defaultValue: "Could not update the task source.")
            }
        }
        return String(localized: "ccx.tasks.editor.error.generic",
                      defaultValue: "Could not update the task source.")
    }
}
