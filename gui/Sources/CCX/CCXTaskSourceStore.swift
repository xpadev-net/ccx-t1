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
    private(set) var isComposing = false
    private(set) var errorMessage: String?
    private(set) var conflictMessage: String?
    private(set) var composerErrorMessage: String?
    private(set) var composerStatusMessage: String?
    var composerInput = ""
    var desiredTaskFormat = "- [ ] <actionable task title>\n  - context: <why this matters>\n  - acceptance: <how to verify it>"
    private var cachedOrchestratorSessionId: String?

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

    var canSubmitComposer: Bool {
        !composerInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !isComposing
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

    func submitNaturalLanguageTask(
        project: CCXProjectSummary,
        workExecutions: [CCXWorkExecution],
        orchestratorSessionId: String?
    ) async {
        let request = composerInput.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !request.isEmpty, !isComposing else { return }
        isComposing = true
        composerErrorMessage = nil
        composerStatusMessage = nil
        defer { isComposing = false }

        do {
            let cli = try cli()
            let sessionId: String
            if let orchestratorSessionId, !orchestratorSessionId.isEmpty {
                sessionId = orchestratorSessionId
                cachedOrchestratorSessionId = nil
            } else if let cachedOrchestratorSessionId {
                sessionId = cachedOrchestratorSessionId
            } else {
                sessionId = try await cli.startOrchestrator(projectId: project.projectId).agentSessionId
                cachedOrchestratorSessionId = sessionId
            }
            let prompt = Self.orchestratorPrompt(
                request: request,
                project: project,
                workExecutions: workExecutions,
                desiredTaskFormat: desiredTaskFormat
            )
            _ = try await cli.promptAgent(sessionId: sessionId, message: prompt)
            composerInput = ""
            composerStatusMessage = String(
                localized: "ccx.tasks.composer.sent",
                defaultValue: "Sent to Orchestrator."
            )
        } catch {
            cachedOrchestratorSessionId = nil
            composerErrorMessage = Self.composerMessage(for: error)
        }
    }

    static func orchestratorPrompt(
        request: String,
        project: CCXProjectSummary,
        workExecutions: [CCXWorkExecution],
        desiredTaskFormat: String
    ) -> String {
        let executionSummary = workExecutions.isEmpty
            ? "- none"
            : workExecutions.prefix(20).map { execution in
                "- \(execution.workExecutionId): state=\(execution.state), branch=\(execution.branchName ?? "none"), task=\(execution.displayText ?? "none")"
            }.joined(separator: "\n")

        return """
        GUI task intake request.

        Project:
        - project_id: \(project.projectId)
        - canonical_repo: \(project.canonicalRepo)
        - task_source_file: \(project.taskSourceFile)

        Current WorkExecution state:
        \(executionSummary)

        Desired task source append format:
        \(desiredTaskFormat)

        User original request:
        \(request)

        Instructions:
        - Inspect the repository code before changing the task source when code context is needed.
        - Split and detail the request into actionable task-source entries when useful.
        - Update the task source file with the refined task content.
        - Preserve the GUI original request in the task source entry or nearby context.
        - Do not overwrite unrelated task source content.
        """
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
                              defaultValue: "CCX controller CLI is not available. Check the CCX installation, then try again.")
            case .processFailed, .invalidJSON, .timedOut, .cancelled:
                return String(localized: "ccx.tasks.editor.error.generic",
                              defaultValue: "Could not update the task source. Reload, check the file, then try again.")
            }
        }
        return String(localized: "ccx.tasks.editor.error.generic",
                      defaultValue: "Could not update the task source. Reload, check the file, then try again.")
    }

    private static func composerMessage(for error: Error) -> String {
        if let cliError = error as? CCXControllerCLIError {
            switch cliError {
            case .executableNotFound, .notExecutable, .launchFailed:
                return String(localized: "ccx.tasks.editor.error.cliUnavailable",
                              defaultValue: "CCX controller CLI is not available. Check the CCX installation, then try again.")
            case .processFailed, .invalidJSON, .timedOut, .cancelled:
                return String(localized: "ccx.tasks.composer.error.generic",
                              defaultValue: "Could not send the request to Orchestrator. Check the agent session, then try again.")
            }
        }
        return String(localized: "ccx.tasks.composer.error.generic",
                      defaultValue: "Could not send the request to Orchestrator. Check the agent session, then try again.")
    }
}
