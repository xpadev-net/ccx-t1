import Foundation
import Observation

@MainActor
@Observable
final class CCXTaskSourceStore {
    typealias CLIProvider = () -> Result<CCXControllerCLI, CCXControllerCLIError>

    private(set) var snapshot: CCXTaskSourceSnapshot?
    var draftContent = "" {
        didSet {
            scheduleWorkItemCandidateParse(for: draftContent)
        }
    }
    private(set) var isLoading = false
    private(set) var isSaving = false
    private(set) var isComposing = false
    private(set) var errorMessage: String?
    private(set) var conflictMessage: String?
    private(set) var composerErrorMessage: String?
    private(set) var composerStatusMessage: String?
    private(set) var sourceChangeMessage: String?
    private(set) var workCreateErrorMessage: String?
    private(set) var workCreateStatusMessage: String?
    private(set) var workItemCandidates: [CCXTaskSourceWorkItemCandidate] = []
    private(set) var isCreatingWork = false
    private var pendingSourceChangeHash: String?
    private var pendingWorkCreateAttempts: [String: PendingWorkCreateAttempt] = [:]
    private var retainedPartialWorkCreateStatusMessages: [String] = []
    private(set) var lastCreatedWorkExecutionId: String?
    var composerInput = ""
    var selectedWorkItemCandidateId: String?
    var desiredTaskFormat = String(
        localized: "ccx.defaultTaskFormat",
        defaultValue: "- [ ] <actionable task title>\n  - context: <why this matters>\n  - acceptance: <how to verify it>"
    )

    @ObservationIgnored
    private let projectId: String
    @ObservationIgnored
    private let cliProvider: CLIProvider
    @ObservationIgnored
    private var workItemCandidatesParseTask: Task<Void, Never>?

    private struct PendingWorkCreateAttempt {
        var result: CCXWorkCreateResult
        var workerSessionId: String?
    }

    init(
        projectId: String,
        cliProvider: @escaping CLIProvider = { CCXControllerCLI.make() }
    ) {
        self.projectId = projectId
        self.cliProvider = cliProvider
    }

    deinit {
        workItemCandidatesParseTask?.cancel()
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

    var selectedWorkItemCandidate: CCXTaskSourceWorkItemCandidate? {
        selectedWorkItemCandidate(in: workItemCandidates)
    }

    func selectedWorkItemCandidate(in candidates: [CCXTaskSourceWorkItemCandidate]) -> CCXTaskSourceWorkItemCandidate? {
        if let selectedWorkItemCandidateId,
           let selected = candidates.first(where: { $0.id == selectedWorkItemCandidateId }) {
            return selected
        }
        return candidates.first
    }

    var canCreateWorkExecution: Bool {
        canCreateWorkExecution(selectedCandidate: selectedWorkItemCandidate)
    }

    func canCreateWorkExecution(selectedCandidate: CCXTaskSourceWorkItemCandidate?) -> Bool {
        snapshot != nil
            && selectedCandidate != nil
            && !isDirty
            && !isLoading
            && !isSaving
            && !isCreatingWork
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

        do {
            let loaded = try await cli().readTaskSource(projectId: projectId)
            apply(snapshot: loaded)
            sourceChangeMessage = nil
        } catch {
            errorMessage = Self.message(for: error)
        }
        isLoading = false
        if let pendingSourceChangeHash {
            self.pendingSourceChangeHash = nil
            await handleTaskSourceChanged(newHash: pendingSourceChangeHash)
        }
    }

    func reload() async {
        guard !isDirty else { return }
        await load()
    }

    func handleTaskSourceChanged(newHash: String?) async {
        if isLoading {
            pendingSourceChangeHash = newHash
            return
        }
        if let newHash, newHash == snapshot?.hash {
            sourceChangeMessage = nil
            return
        }
        if isDirty {
            sourceChangeMessage = String(
                localized: "ccx.tasks.source.changedDirty",
                defaultValue: "Task source changed on disk. Reload after saving or discarding your local edits."
            )
            return
        }
        await load()
    }

    func discardChanges() {
        guard let snapshot else { return }
        draftContent = snapshot.content
        conflictMessage = nil
        errorMessage = nil
        if sourceChangeMessage != nil {
            sourceChangeMessage = String(
                localized: "ccx.tasks.source.reloadAvailable",
                defaultValue: "Task source changed on disk. Reload to show the latest content."
            )
        }
    }

    func clearComposerStatusMessage() {
        composerStatusMessage = nil
    }

    func createWorkExecutionFromSelection(project: CCXProjectSummary) async {
        guard canCreateWorkExecution, let candidate = selectedWorkItemCandidate else { return }
        isCreatingWork = true
        retainPendingWorkCreateStatusIfChangingSelection(to: candidate)
        workCreateErrorMessage = nil
        workCreateStatusMessage = combinedWorkCreateStatus(current: nil)
        defer { isCreatingWork = false }

        do {
            let cli = try cli()
            let created: CCXWorkCreateResult
            if let pendingAttempt = pendingWorkCreateAttempts[candidate.id] {
                created = pendingAttempt.result
            } else {
                created = try await cli.createWork(
                    projectId: project.projectId,
                    sourcePath: project.taskSourceFile,
                    selectorType: candidate.selectorType,
                    selectorValue: candidate.selectorValue,
                    displayText: candidate.displayText
                )
                pendingWorkCreateAttempts[candidate.id] = PendingWorkCreateAttempt(
                    result: created,
                    workerSessionId: nil
                )
                lastCreatedWorkExecutionId = created.workExecutionId
            }

            let sessionId: String
            if let pendingWorkerSessionId = pendingWorkCreateAttempts[candidate.id]?.workerSessionId {
                sessionId = pendingWorkerSessionId
            } else {
                do {
                    let attached = try await cli.attachAgent(
                        projectId: project.projectId,
                        workExecutionId: created.workExecutionId,
                        role: "worker",
                        mode: "writer"
                    )
                    sessionId = attached.agentSessionId
                    pendingWorkCreateAttempts[candidate.id]?.workerSessionId = sessionId
                } catch {
                    workCreateStatusMessage = combinedWorkCreateStatus(current: Self.workCreatePartialStatus(created: created))
                    workCreateErrorMessage = Self.workCreateAttachMessage(for: error)
                    return
                }
            }

            do {
                _ = try await cli.promptAgent(
                    sessionId: sessionId,
                    message: Self.workerPrompt(created: created, candidate: candidate)
                )
            } catch {
                workCreateStatusMessage = combinedWorkCreateStatus(current: Self.workCreatePartialStatus(created: created))
                workCreateErrorMessage = Self.workCreatePromptMessage(for: error)
                return
            }

            pendingWorkCreateAttempts[candidate.id] = nil
            retainedPartialWorkCreateStatusMessages = []
            workCreateStatusMessage = combinedWorkCreateStatus(current: String(
                localized: "ccx.tasks.workCreate.sent",
                defaultValue: "Created WorkExecution and prompted a Worker."
            ))
        } catch {
            workCreateErrorMessage = Self.workCreateMessage(for: error)
        }
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
            updateWorkItemCandidates(Self.workItemCandidates(in: draftContent), for: draftContent)
            sourceChangeMessage = nil
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
            } else {
                sessionId = try await cli.startOrchestrator(projectId: project.projectId).agentSessionId
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
            - Prefer `ccx task-source append` or `ccx task-source write` so the controller records the reflection event.
            - Preserve the GUI original request in the task source entry or nearby context.
            - Do not overwrite unrelated task source content.
        """
    }

    static func workerPrompt(
        created: CCXWorkCreateResult,
        candidate: CCXTaskSourceWorkItemCandidate
    ) -> String {
        """
        WorkExecution \(created.workExecutionId) has been created from the task source.

        Selected item:
        \(candidate.displayText)

        Task file:
        \(created.taskFilePath)

        Worktree:
        \(created.worktreePath)

        Please read the task file, inspect the repository, and begin implementation within the worktree.
        """
    }

    static func workItemCandidates(in markdown: String) -> [CCXTaskSourceWorkItemCandidate] {
        CCXWorkItemCandidateParser.parse(markdown)
    }

    private func apply(snapshot: CCXTaskSourceSnapshot) {
        self.snapshot = snapshot
        self.draftContent = snapshot.content
        let candidates = Self.workItemCandidates(in: snapshot.content)
        discardPendingWorkCreateAttemptsMissing(from: candidates)
        updateWorkItemCandidates(candidates, for: snapshot.content)
    }

    private func scheduleWorkItemCandidateParse(for markdown: String) {
        workItemCandidatesParseTask?.cancel()
        workItemCandidatesParseTask = Task { [weak self, markdown] in
            let parseTask = Task.detached(priority: .userInitiated) {
                CCXWorkItemCandidateParser.parse(markdown)
            }
            let candidates = await withTaskCancellationHandler {
                await parseTask.value
            } onCancel: {
                parseTask.cancel()
            }
            guard !Task.isCancelled else { return }
            self?.updateWorkItemCandidates(candidates, for: markdown)
        }
    }

    private func updateWorkItemCandidates(_ candidates: [CCXTaskSourceWorkItemCandidate], for markdown: String) {
        workItemCandidatesParseTask?.cancel()
        workItemCandidatesParseTask = nil
        guard draftContent == markdown else { return }
        workItemCandidates = candidates
        clearMissingWorkItemSelection(in: candidates)
    }

    private func clearMissingWorkItemSelection(in candidates: [CCXTaskSourceWorkItemCandidate]) {
        guard let selectedWorkItemCandidateId else { return }
        let candidateIds = Set(candidates.map(\.id))
        if !candidateIds.contains(selectedWorkItemCandidateId) {
            self.selectedWorkItemCandidateId = nil
        }
    }

    private func discardPendingWorkCreateAttemptsMissing(from candidates: [CCXTaskSourceWorkItemCandidate]) {
        let candidateIds = Set(candidates.map(\.id))
        let previousCount = pendingWorkCreateAttempts.count
        pendingWorkCreateAttempts = pendingWorkCreateAttempts.filter { candidateIds.contains($0.key) }
        if pendingWorkCreateAttempts.count != previousCount {
            retainedPartialWorkCreateStatusMessages = []
        }
    }

    private func retainPendingWorkCreateStatusIfChangingSelection(to candidate: CCXTaskSourceWorkItemCandidate) {
        for (candidateId, pendingAttempt) in pendingWorkCreateAttempts where candidateId != candidate.id {
            let status = Self.workCreatePartialStatus(created: pendingAttempt.result)
            if !retainedPartialWorkCreateStatusMessages.contains(status) {
                retainedPartialWorkCreateStatusMessages.append(status)
            }
        }
    }

    private func combinedWorkCreateStatus(current: String?) -> String? {
        let messages = retainedPartialWorkCreateStatusMessages + [current].compactMap { $0 }
        guard !messages.isEmpty else { return nil }
        return messages.joined(separator: "\n")
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

    private static func workCreateMessage(for error: Error) -> String {
        if let cliError = error as? CCXControllerCLIError {
            switch cliError {
            case .executableNotFound, .notExecutable, .launchFailed:
                return String(localized: "ccx.tasks.editor.error.cliUnavailable",
                              defaultValue: "CCX controller CLI is not available. Check the CCX installation, then try again.")
            case .processFailed, .invalidJSON, .timedOut, .cancelled:
                return String(localized: "ccx.tasks.workCreate.error.generic",
                              defaultValue: "Could not create the WorkExecution. Check the selected task and try again.")
            }
        }
        return String(localized: "ccx.tasks.workCreate.error.generic",
                      defaultValue: "Could not create the WorkExecution. Check the selected task and try again.")
    }

    private static func workCreatePartialStatus(created: CCXWorkCreateResult) -> String {
        let format = String(
            localized: "ccx.tasks.workCreate.partial.created",
            defaultValue: "Created WorkExecution. Attach or prompt the Worker manually if retry does not recover: %@",
            bundle: .main,
            comment: "WorkExecution created but subsequent step failed; placeholder is the WorkExecution ID."
        )
        return String(format: format, locale: .current, created.workExecutionId)
    }

    private static func workCreateAttachMessage(for error: Error) -> String {
        let format = String(
            localized: "ccx.tasks.workCreate.error.attach",
            defaultValue: "WorkExecution was created, but the Worker could not be attached. %@",
            bundle: .main,
            comment: "WorkExecution created but Worker attach failed; placeholder is recovery guidance."
        )
        return String(format: format, locale: .current, Self.workCreateRecoveryMessage(for: error))
    }

    private static func workCreatePromptMessage(for error: Error) -> String {
        let format = String(
            localized: "ccx.tasks.workCreate.error.prompt",
            defaultValue: "WorkExecution was created and a Worker was attached, but the prompt could not be sent. %@",
            bundle: .main,
            comment: "WorkExecution created and Worker attached but prompt failed; placeholder is recovery guidance."
        )
        return String(format: format, locale: .current, Self.workCreateRecoveryMessage(for: error))
    }

    private static func workCreateRecoveryMessage(for error: Error) -> String {
        if let cliError = error as? CCXControllerCLIError {
            switch cliError {
            case .executableNotFound, .notExecutable, .launchFailed:
                return String(localized: "ccx.tasks.editor.error.cliUnavailable",
                              defaultValue: "CCX controller CLI is not available. Check the CCX installation, then try again.")
            case .processFailed, .invalidJSON, .timedOut, .cancelled:
                break
            }
        }
        return String(localized: "ccx.tasks.workCreate.error.recover",
                      defaultValue: "Retry to continue from the created WorkExecution, or attach and prompt a Worker manually.")
    }
}
