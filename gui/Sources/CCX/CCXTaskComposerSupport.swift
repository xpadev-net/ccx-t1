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
            - Prefer `ccx task-source append` or `ccx task-source write` so the controller records the reflection event.
            - Preserve the GUI original request in the task source entry or nearby context.
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
