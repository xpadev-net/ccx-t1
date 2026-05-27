import Foundation
import Observation

@MainActor
@Observable
final class CCXTaskSourceStore {
    typealias CLIProvider = () -> Result<CCXControllerCLI, CCXControllerCLIError>

    private(set) var snapshot: CCXTaskSourceSnapshot?
    private var _draftContent = ""
    var draftContent: String {
        get { _draftContent }
        set {
            _draftContent = newValue
            scheduleWorkItemCandidateParse(for: newValue)
        }
    }
    private(set) var isLoading = false
    private(set) var isSaving = false
    private(set) var isComposing = false
    private(set) var errorMessage: String?
    private(set) var conflictMessage: String?
    private(set) var composerErrorMessage: String?
    private(set) var composerValidationMessage: String?
    private(set) var composerRefreshToken: UUID?
    private(set) var composerStatusMessage: String?
    private(set) var sourceChangeMessage: String?
    private(set) var workCreateErrorMessage: String?
    private(set) var workCreateStatusMessage: String?
    private(set) var workItemCandidates: [CCXTaskSourceWorkItemCandidate] = []
    private(set) var isCreatingWork = false
    private var pendingSourceChangeHash: String?
    private(set) var lastCreatedWorkExecutionId: String?
    private let workExecutionCreator = CCXWorkExecutionCreator()
    var composerInput = "" {
        didSet {
            composerValidationMessage = Self.validateComposerInput(composerInput)
        }
    }
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

    init(
        projectId: String,
        cliProvider: @escaping CLIProvider = { CCXControllerCLI.make() }
    ) {
        self.projectId = projectId
        self.cliProvider = cliProvider
        self.composerValidationMessage = Self.validateComposerInput(composerInput)
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
        composerValidationMessage == nil
            && !composerInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && !isComposing
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
            await apply(snapshot: loaded)
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
            stopComposerReflectionWatch()
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

    func awaitComposerReflectionTimeout(for token: UUID) async {
        guard composerRefreshToken == token else { return }
        do {
            try await Task.sleep(for: .seconds(15))
        } catch {
            return
        }
        guard composerRefreshToken == token else { return }
        composerErrorMessage = String(
            localized: "ccx.tasks.composer.error.noReflection",
            defaultValue: "No task source update was observed yet. Reopen the task source or retry if the composition was not applied."
        )
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
        composerErrorMessage = nil
        composerValidationMessage = Self.validateComposerInput(composerInput)
    }

    func createWorkExecutionFromSelection(project: CCXProjectSummary) async {
        guard canCreateWorkExecution, let candidate = selectedWorkItemCandidate else { return }
        isCreatingWork = true
        workCreateErrorMessage = nil
        defer { isCreatingWork = false }

        let result = await workExecutionCreator.create(
            project: project,
            candidate: candidate,
            cli: cli
        )
        workCreateStatusMessage = result.statusMessage
        workCreateErrorMessage = result.errorMessage
        if let createdId = result.lastCreatedWorkExecutionId {
            lastCreatedWorkExecutionId = createdId
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
            let updatedSnapshot = CCXTaskSourceSnapshot(
                projectId: result.projectId,
                path: result.path,
                content: draftContent,
                hash: result.hash,
                mtime: result.mtime,
                warning: result.warning
            )
            await apply(snapshot: updatedSnapshot)
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
        if let validation = Self.validateComposerInput(request, isSubmitting: true) {
            composerValidationMessage = validation
            composerErrorMessage = validation
            return
        }
        composerValidationMessage = nil
        guard !isComposing else { return }
        isComposing = true
        composerErrorMessage = nil
        composerStatusMessage = nil
        defer { isComposing = false }

        do {
            let cli = try cli()
            let sessionId: String = if let orchestratorSessionId,
                !orchestratorSessionId.isEmpty {
                orchestratorSessionId
            } else {
                do {
                    try await cli.startOrchestrator(projectId: project.projectId).agentSessionId
                } catch {
                    composerErrorMessage = CCXTaskComposerSupport.startOrchestratorErrorMessage(for: error)
                    return
                }
            }
            let prompt = CCXTaskComposerSupport.prompt(
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
            startComposerReflectionWatch()
        } catch {
            composerErrorMessage = CCXTaskComposerSupport.message(for: error)
        }
    }

    private func startComposerReflectionWatch() {
        composerRefreshToken = UUID()
    }

    private func stopComposerReflectionWatch() {
        composerRefreshToken = nil
    }

    private static func validateComposerInput(
        _ input: String,
        isSubmitting: Bool = false
    ) -> String? {
        let normalized = input.trimmingCharacters(in: .whitespacesAndNewlines)
        if normalized.isEmpty {
            guard isSubmitting else { return nil }
            return String(
                localized: "ccx.tasks.composer.validation.empty",
                defaultValue: "Task request is empty. Please enter details before sending."
            )
        }
        if normalized.utf8.count > 8_192 {
            return String(
                localized: "ccx.tasks.composer.validation.tooLong",
                defaultValue: "Task request is too long. Please shorten it."
            )
        }
        if normalized.contains(where: { char in
            let scalar = char.unicodeScalars.first?.value ?? 0
            return scalar < 0x09 || (scalar >= 0x0E && scalar < 0x20)
        }) {
            return String(
                localized: "ccx.tasks.composer.validation.invalidCharacters",
                defaultValue: "Task request contains unsupported characters."
            )
        }
        if !hasBalancedCodeFences(normalized) || hasMismatchedYamlFrontMatter(normalized) {
            return String(
                localized: "ccx.tasks.composer.validation.invalidMarkdown",
                defaultValue: "Task request text is not valid markdown-like input."
            )
        }
        return nil
    }

    private static func hasBalancedCodeFences(_ input: String) -> Bool {
        let lines = input.split(separator: "\n", omittingEmptySubsequences: false)
        let fenceLines = lines.filter { line in
            line.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("```")
        }
        return fenceLines.count.isMultiple(of: 2)
    }

    private static func hasMismatchedYamlFrontMatter(_ input: String) -> Bool {
        let lines = input.split(separator: "\n", omittingEmptySubsequences: false)
        let firstNonEmpty = lines.first { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
        guard let first = firstNonEmpty, first.trimmingCharacters(in: .whitespacesAndNewlines) == "---" else {
            return false
        }
        let markerCount = lines.dropFirst().reduce(0) { count, line in
            count + (line.trimmingCharacters(in: .whitespacesAndNewlines) == "---" ? 1 : 0)
        }
        return markerCount != 1
    }

    static func workItemCandidates(in markdown: String) -> [CCXTaskSourceWorkItemCandidate] {
        CCXWorkItemCandidateParser.parse(markdown)
    }

    private func apply(snapshot: CCXTaskSourceSnapshot) async {
        guard !Task.isCancelled else { return }
        self.snapshot = snapshot
        stopComposerReflectionWatch()
        _draftContent = snapshot.content
        let parseTask = scheduleWorkItemCandidateParse(for: snapshot.content, prunePendingAttempts: true)
        await withTaskCancellationHandler {
            await parseTask.value
        } onCancel: {
            parseTask.cancel()
        }
        guard !Task.isCancelled else {
            scheduleWorkItemCandidateParse(for: snapshot.content, prunePendingAttempts: true)
            return
        }
    }

    @discardableResult
    private func scheduleWorkItemCandidateParse(
        for markdown: String,
        prunePendingAttempts: Bool = false
    ) -> Task<Void, Never> {
        workItemCandidatesParseTask?.cancel()
        let task = Task { [weak self, markdown, prunePendingAttempts] in
            let parseTask = Task.detached(priority: .userInitiated) {
                CCXWorkItemCandidateParser.parse(markdown)
            }
            let candidates = await withTaskCancellationHandler {
                await parseTask.value
            } onCancel: {
                parseTask.cancel()
            }
            guard !Task.isCancelled else { return }
            self?.updateWorkItemCandidates(candidates, for: markdown, prunePendingAttempts: prunePendingAttempts)
        }
        workItemCandidatesParseTask = task
        return task
    }

    private func updateWorkItemCandidates(
        _ candidates: [CCXTaskSourceWorkItemCandidate],
        for markdown: String,
        prunePendingAttempts: Bool = false
    ) {
        guard draftContent == markdown else { return }
        workItemCandidatesParseTask?.cancel()
        workItemCandidatesParseTask = nil
        if prunePendingAttempts {
            workExecutionCreator.discardPendingAttemptsMissing(from: candidates)
        }
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

}
