import Foundation
import Observation

@MainActor
@Observable
final class CCXTaskSourceStore {
    typealias CLIProvider = () -> Result<CCXControllerCLI, CCXControllerCLIError>

    private(set) var snapshot: CCXTaskSourceSnapshot?
    var draftContent = ""
    private(set) var isLoading = false
    private(set) var isSaving = false
    private(set) var errorMessage: String?
    private(set) var conflictMessage: String?

    @ObservationIgnored
    private let projectId: String
    @ObservationIgnored
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
        guard let warning = snapshot?.warning else { return nil }
        switch warning.code {
        case "task_source_in_canonical_repo_dirty":
            return String(
                localized: "ccx.tasks.warning.canonicalRepoDirty",
                defaultValue: "Task source is inside the canonical repository and the working tree has uncommitted changes."
            )
        default:
            return String(
                localized: "ccx.tasks.warning.generic",
                defaultValue: "Task source returned a warning."
            )
        }
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
        guard !isDirty else { return }
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
                    defaultValue: "The task source changed on disk. Reload, then apply your edits again."
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
        guard case let CCXControllerCLIError.processFailed(exitCode, _, _) = error else {
            return false
        }
        return exitCode == 2
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
