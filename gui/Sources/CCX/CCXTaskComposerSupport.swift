import Foundation

enum CCXTaskComposerSupport {
    static func prompt(
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
            - Keep the exact user request text in the task source entry so traceability is preserved.
            - Append only, never rewrite the existing task source content (unless a single section edit is strictly required).
            - If you can only add content, keep your output in minimal patch/diff style and describe why each added block is needed.
            - Prefer `ccx task-source append` or `ccx task-source write` so the controller records the reflection event.
            - When a write/write-mode call conflicts, stop, re-read the latest task source, and retry with minimal additive changes against fresh content.
            - If conflict persists, report precise retry guidance and return the request context so the GUI can trigger a single retry.
            - Do not overwrite unrelated task source content.
        """
    }

    static func message(for error: Error) -> String {
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
