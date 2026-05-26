import Foundation

@MainActor
final class CCXWorkExecutionCreator {
    typealias CLIResolver = () throws -> CCXControllerCLI

    struct CreateResult {
        var statusMessage: String?
        var errorMessage: String?
        var lastCreatedWorkExecutionId: String?
    }

    private struct PendingAttempt {
        var result: CCXWorkCreateResult
        var workerSessionId: String?
        var signature: CandidateSignature
    }

    private struct CandidateSignature: Hashable {
        let selectorType: String
        let displayText: String

        init(candidate: CCXTaskSourceWorkItemCandidate) {
            self.selectorType = candidate.selectorType
            self.displayText = candidate.displayText
        }
    }

    private var pendingAttempts: [String: PendingAttempt] = [:]
    private var retainedPartialStatusMessages: [String] = []

    func create(
        project: CCXProjectSummary,
        candidate: CCXTaskSourceWorkItemCandidate,
        cli: CLIResolver
    ) async -> CreateResult {
        retainPendingStatusIfChangingSelection(to: candidate)
        var statusMessage = combinedStatus(current: nil)

        do {
            let cli = try cli()
            let created: CCXWorkCreateResult
            var lastCreatedWorkExecutionId: String?
            if let pendingAttempt = pendingAttempts[candidate.id] {
                created = pendingAttempt.result
            } else {
                created = try await cli.createWork(
                    projectId: project.projectId,
                    sourcePath: project.taskSourceFile,
                    selectorType: candidate.selectorType,
                    selectorValue: candidate.selectorValue,
                    displayText: candidate.displayText
                )
                pendingAttempts[candidate.id] = PendingAttempt(
                    result: created,
                    workerSessionId: nil,
                    signature: CandidateSignature(candidate: candidate)
                )
                lastCreatedWorkExecutionId = created.workExecutionId
            }

            let sessionId: String
            if let pendingWorkerSessionId = pendingAttempts[candidate.id]?.workerSessionId {
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
                    pendingAttempts[candidate.id]?.workerSessionId = sessionId
                } catch {
                    statusMessage = combinedStatus(current: Self.partialStatus(created: created))
                    return CreateResult(
                        statusMessage: statusMessage,
                        errorMessage: Self.attachMessage(for: error),
                        lastCreatedWorkExecutionId: lastCreatedWorkExecutionId
                    )
                }
            }

            do {
                _ = try await cli.promptAgent(
                    sessionId: sessionId,
                    message: Self.workerPrompt(created: created, candidate: candidate)
                )
            } catch {
                statusMessage = combinedStatus(current: Self.partialStatus(created: created))
                return CreateResult(
                    statusMessage: statusMessage,
                    errorMessage: Self.promptMessage(for: error),
                    lastCreatedWorkExecutionId: lastCreatedWorkExecutionId
                )
            }

            pendingAttempts[candidate.id] = nil
            retainedPartialStatusMessages = []
            statusMessage = combinedStatus(current: String(
                localized: "ccx.tasks.workCreate.sent",
                defaultValue: "Created WorkExecution and prompted a Worker."
            ))
            return CreateResult(
                statusMessage: statusMessage,
                errorMessage: nil,
                lastCreatedWorkExecutionId: lastCreatedWorkExecutionId
            )
        } catch {
            return CreateResult(
                statusMessage: statusMessage,
                errorMessage: Self.message(for: error),
                lastCreatedWorkExecutionId: nil
            )
        }
    }

    func discardPendingAttemptsMissing(from candidates: [CCXTaskSourceWorkItemCandidate]) {
        let candidateIds = Set(candidates.map(\.id))
        let previousCount = pendingAttempts.count
        var updatedAttempts: [String: PendingAttempt] = [:]
        var usedCandidateIds = Set<String>()
        for (candidateId, pendingAttempt) in pendingAttempts {
            if candidateIds.contains(candidateId) {
                updatedAttempts[candidateId] = pendingAttempt
                usedCandidateIds.insert(candidateId)
                continue
            }
            if let remappedCandidate = candidates.first(where: {
                !usedCandidateIds.contains($0.id)
                    && CandidateSignature(candidate: $0) == pendingAttempt.signature
            }) {
                updatedAttempts[remappedCandidate.id] = pendingAttempt
                usedCandidateIds.insert(remappedCandidate.id)
            }
        }
        pendingAttempts = updatedAttempts
        if pendingAttempts.count != previousCount {
            retainedPartialStatusMessages = []
        }
    }

    private func retainPendingStatusIfChangingSelection(to candidate: CCXTaskSourceWorkItemCandidate) {
        for (candidateId, pendingAttempt) in pendingAttempts where candidateId != candidate.id {
            let status = Self.partialStatus(created: pendingAttempt.result)
            if !retainedPartialStatusMessages.contains(status) {
                retainedPartialStatusMessages.append(status)
            }
        }
    }

    private func combinedStatus(current: String?) -> String? {
        let messages = retainedPartialStatusMessages + [current].compactMap { $0 }
        guard !messages.isEmpty else { return nil }
        return messages.joined(separator: "\n")
    }

    private static func workerPrompt(
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

    private static func message(for error: Error) -> String {
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

    private static func partialStatus(created: CCXWorkCreateResult) -> String {
        let format = String(
            localized: "ccx.tasks.workCreate.partial.created",
            defaultValue: "Created WorkExecution. Attach or prompt the Worker manually if retry does not recover: %@",
            bundle: .main,
            comment: "WorkExecution created but subsequent step failed; placeholder is the WorkExecution ID."
        )
        return String(format: format, locale: .current, created.workExecutionId)
    }

    private static func attachMessage(for error: Error) -> String {
        let format = String(
            localized: "ccx.tasks.workCreate.error.attach",
            defaultValue: "WorkExecution was created, but the Worker could not be attached. %@",
            bundle: .main,
            comment: "WorkExecution created but Worker attach failed; placeholder is recovery guidance."
        )
        return String(format: format, locale: .current, Self.recoveryMessage(for: error))
    }

    private static func promptMessage(for error: Error) -> String {
        let format = String(
            localized: "ccx.tasks.workCreate.error.prompt",
            defaultValue: "WorkExecution was created and a Worker was attached, but the prompt could not be sent. %@",
            bundle: .main,
            comment: "WorkExecution created and Worker attached but prompt failed; placeholder is recovery guidance."
        )
        return String(format: format, locale: .current, Self.recoveryMessage(for: error))
    }

    private static func recoveryMessage(for error: Error) -> String {
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
