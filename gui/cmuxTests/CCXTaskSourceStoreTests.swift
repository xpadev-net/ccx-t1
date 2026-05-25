import Foundation
import XCTest

#if DEBUG
@testable import cmux_DEV
#else
@testable import cmux
#endif

@MainActor
final class CCXTaskSourceStoreTests: XCTestCase {
    func testLoadAppliesSnapshotAndDraft() async {
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, arguments, stdin in
                XCTAssertEqual(arguments, [
                    "task-source", "read", "--project-id", "p_123", "--json",
                ])
                XCTAssertNil(stdin)
                return .success(Self.result(stdout: """
                {
                  "project_id": "p_123",
                  "path": "/repo/z/tasks.md",
                  "content": "- [ ] first\\n",
                  "hash": "hash-1",
                  "mtime": "2026-05-26T00:00:00Z",
                  "warning": null
                }
                """))
            })
        }

        await store.load()

        XCTAssertEqual(store.draftContent, "- [ ] first\n")
        XCTAssertEqual(store.loadedHash, "hash-1")
        XCTAssertFalse(store.isDirty)
        XCTAssertNil(store.errorMessage)
    }

    func testSaveUsesLoadedHashAndClearsDirtyState() async {
        var invocations = 0
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, arguments, stdin in
                invocations += 1
                if invocations == 1 {
                    return .success(Self.result(stdout: """
                    {
                      "project_id": "p_123",
                      "path": "/repo/z/tasks.md",
                      "content": "old",
                      "hash": "hash-1",
                      "mtime": "2026-05-26T00:00:00Z",
                      "warning": null
                    }
                    """))
                }
                XCTAssertEqual(arguments, [
                    "task-source", "write", "--project-id", "p_123",
                    "--expected-hash", "hash-1", "--stdin", "--json",
                ])
                XCTAssertEqual(String(data: stdin ?? Data(), encoding: .utf8), "new")
                return .success(Self.result(stdout: """
                {
                  "project_id": "p_123",
                  "path": "/repo/z/tasks.md",
                  "hash": "hash-2",
                  "mtime": "2026-05-26T00:00:01Z",
                  "bytes_written": 3,
                  "warning": null
                }
                """))
            })
        }

        await store.load()
        store.draftContent = "new"
        await store.save()

        XCTAssertEqual(store.loadedHash, "hash-2")
        XCTAssertEqual(store.snapshot?.content, "new")
        XCTAssertFalse(store.isDirty)
    }

    func testSaveConflictKeepsDraft() async {
        var invocations = 0
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, _, _ in
                invocations += 1
                if invocations == 1 {
                    return .success(Self.result(stdout: """
                    {
                      "project_id": "p_123",
                      "path": "/repo/z/tasks.md",
                      "content": "old",
                      "hash": "hash-1",
                      "mtime": "2026-05-26T00:00:00Z",
                      "warning": null
                    }
                    """))
                }
                return .success(CCXControllerCLIProcessResult(
                    exitCode: 2,
                    stdout: Data(),
                    stderr: Data("error wording is intentionally not part of the contract".utf8)
                ))
            })
        }

        await store.load()
        store.draftContent = "draft"
        await store.save()

        XCTAssertEqual(store.draftContent, "draft")
        XCTAssertTrue(store.isDirty)
        XCTAssertNotNil(store.conflictMessage)
        XCTAssertNil(store.errorMessage)
    }

    func testReloadDoesNotDiscardUnsavedDraft() async {
        var invocations = 0
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, _, _ in
                invocations += 1
                return .success(Self.result(stdout: """
                {
                  "project_id": "p_123",
                  "path": "/repo/z/tasks.md",
                  "content": "loaded-\(invocations)",
                  "hash": "hash-\(invocations)",
                  "mtime": "2026-05-26T00:00:00Z",
                  "warning": null
                }
                """))
            })
        }

        await store.load()
        store.draftContent = "draft"
        await store.reload()

        XCTAssertEqual(store.draftContent, "draft")
        XCTAssertEqual(store.loadedHash, "hash-1")
        XCTAssertEqual(invocations, 1)
    }

    func testWarningMessageUsesLocalizedKnownWarningCode() async {
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, _, _ in
                .success(Self.result(stdout: """
                {
                  "project_id": "p_123",
                  "path": "/repo/z/tasks.md",
                  "content": "loaded",
                  "hash": "hash-1",
                  "mtime": "2026-05-26T00:00:00Z",
                  "warning": {
                    "code": "task_source_in_canonical_repo_dirty",
                    "message": "backend text should not be displayed"
                  }
                }
                """))
            })
        }

        await store.load()

        XCTAssertEqual(
            store.warningMessage,
            String(
                localized: "ccx.tasks.warning.canonicalRepoDirty",
                defaultValue: "Task source is inside the canonical repository and the working tree has uncommitted changes."
            )
        )
        XCTAssertFalse(store.warningMessage?.contains("backend text") ?? true)
    }

    func testOrchestratorPromptIncludesProjectStateFormatAndOriginalRequest() {
        let prompt = CCXTaskSourceStore.orchestratorPrompt(
            request: "Add export support",
            project: Self.project,
            workExecutions: [
                CCXWorkExecution(
                    workExecutionId: "we_1",
                    projectId: "p_123",
                    state: "running",
                    branchName: "codex/export",
                    worktreePath: "/repo/.ccx/we_1",
                    taskFilePath: "/ccx/we_1/task.md",
                    prNumber: nil,
                    prUrl: nil,
                    headCommit: nil,
                    displayText: "Existing task",
                    selectedAt: "2026-05-26T00:00:00Z",
                    artifactState: "pending",
                    syncStatus: "pending"
                ),
            ],
            desiredTaskFormat: "- [ ] title"
        )

        XCTAssertTrue(prompt.contains("canonical_repo: /repo"))
        XCTAssertTrue(prompt.contains("task_source_file: /repo/z/tasks.md"))
        XCTAssertTrue(prompt.contains("we_1: state=running"))
        XCTAssertTrue(prompt.contains("- [ ] title"))
        XCTAssertTrue(prompt.contains("Add export support"))
        XCTAssertTrue(prompt.contains("Inspect the repository code"))
        XCTAssertTrue(prompt.contains("ccx task-source append"))
        XCTAssertTrue(prompt.contains("Preserve the GUI original request"))
    }

    func testComposerPromptsExistingOrchestratorSession() async {
        var capturedPrompt: String?
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, arguments, stdin in
                XCTAssertEqual(arguments, [
                    "agent", "prompt", "--session-id", "sess_existing", "--stdin", "--json",
                ])
                capturedPrompt = String(data: stdin ?? Data(), encoding: .utf8)
                return .success(Self.result(stdout: """
                {
                  "session_id": "sess_existing",
                  "status": "sent"
                }
                """))
            })
        }
        store.composerInput = "Add export support"

        await store.submitNaturalLanguageTask(
            project: Self.project,
            workExecutions: [],
            orchestratorSessionId: "sess_existing"
        )

        XCTAssertEqual(store.composerInput, "")
        XCTAssertNotNil(store.composerStatusMessage)
        XCTAssertNil(store.composerErrorMessage)
        XCTAssertTrue(capturedPrompt?.contains("Add export support") ?? false)
    }

    func testComposerStartsOrchestratorWhenMissingThenPrompts() async {
        var calls: [[String]] = []
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, arguments, _ in
                calls.append(arguments)
                if arguments.contains("start-orchestrator") {
                    return .success(Self.result(stdout: """
                    {
                      "agent_session_id": "sess_started",
                      "project_id": "p_123",
                      "role": "orchestrator",
                      "status": "started"
                    }
                    """))
                }
                return .success(Self.result(stdout: """
                {
                  "session_id": "sess_started",
                  "status": "sent"
                }
                """))
            })
        }
        store.composerInput = "Add export support"

        await store.submitNaturalLanguageTask(
            project: Self.project,
            workExecutions: [],
            orchestratorSessionId: nil
        )

        XCTAssertEqual(calls, [
            ["agent", "start-orchestrator", "--project-id", "p_123", "--json"],
            ["agent", "prompt", "--session-id", "sess_started", "--stdin", "--json"],
        ])
        XCTAssertNil(store.composerErrorMessage)
    }

    func testComposerStartsThroughCLIWhenNoLiveSessionIsAvailable() async {
        var calls: [[String]] = []
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, arguments, _ in
                calls.append(arguments)
                if arguments.contains("start-orchestrator") {
                    return .success(Self.result(stdout: """
                    {
                      "agent_session_id": "sess_started",
                      "project_id": "p_123",
                      "role": "orchestrator",
                      "status": "started"
                    }
                    """))
                }
                return .success(Self.result(stdout: """
                {
                  "session_id": "sess_started",
                  "status": "sent"
                }
                """))
            })
        }

        store.composerInput = "First request"
        await store.submitNaturalLanguageTask(
            project: Self.project,
            workExecutions: [],
            orchestratorSessionId: nil
        )
        store.composerInput = "Second request"
        await store.submitNaturalLanguageTask(
            project: Self.project,
            workExecutions: [],
            orchestratorSessionId: nil
        )

        XCTAssertEqual(
            calls.filter { $0.contains("start-orchestrator") }.count,
            2
        )
        XCTAssertEqual(
            calls.filter { $0.contains("prompt") && $0.contains("sess_started") }.count,
            2
        )
    }

    func testComposerUsesLiveSessionAfterPromptFailure() async {
        var calls: [[String]] = []
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, arguments, _ in
                calls.append(arguments)
                if arguments.contains("start-orchestrator") {
                    return .success(Self.result(stdout: """
                    {
                      "agent_session_id": "sess_stale",
                      "project_id": "p_123",
                      "role": "orchestrator",
                      "status": "started"
                    }
                    """))
                }
                if arguments.contains("sess_stale") {
                    return .success(CCXControllerCLIProcessResult(
                        exitCode: 1,
                        stdout: Data(),
                        stderr: Data("session stopped".utf8)
                    ))
                }
                return .success(Self.result(stdout: """
                {
                  "session_id": "sess_live",
                  "status": "sent"
                }
                """))
            })
        }

        store.composerInput = "First request"
        await store.submitNaturalLanguageTask(
            project: Self.project,
            workExecutions: [],
            orchestratorSessionId: nil
        )
        XCTAssertNotNil(store.composerErrorMessage)

        store.composerInput = "Second request"
        await store.submitNaturalLanguageTask(
            project: Self.project,
            workExecutions: [],
            orchestratorSessionId: "sess_live"
        )

        XCTAssertEqual(
            calls,
            [
                ["agent", "start-orchestrator", "--project-id", "p_123", "--json"],
                ["agent", "prompt", "--session-id", "sess_stale", "--stdin", "--json"],
                ["agent", "prompt", "--session-id", "sess_live", "--stdin", "--json"],
            ]
        )
        XCTAssertNil(store.composerErrorMessage)
        XCTAssertNotNil(store.composerStatusMessage)
    }

    func testComposerUsesDedicatedPromptErrorMessage() async {
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, _, _ in
                .success(CCXControllerCLIProcessResult(
                    exitCode: 1,
                    stdout: Data(),
                    stderr: Data("prompt failed".utf8)
                ))
            })
        }
        store.composerInput = "Add export support"

        await store.submitNaturalLanguageTask(
            project: Self.project,
            workExecutions: [],
            orchestratorSessionId: "sess_existing"
        )

        XCTAssertEqual(
            store.composerErrorMessage,
            String(
                localized: "ccx.tasks.composer.error.generic",
                defaultValue: "Could not send the request to Orchestrator. Check the agent session, then try again."
            )
        )
    }

    func testSourceChangeAutoReloadsCleanDraft() async {
        var invocations = 0
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, _, _ in
                invocations += 1
                return .success(Self.result(stdout: """
                {
                  "project_id": "p_123",
                  "path": "/repo/z/tasks.md",
                  "content": "version-\(invocations)",
                  "hash": "hash-\(invocations)",
                  "mtime": "2026-05-26T00:00:00Z",
                  "warning": null
                }
                """))
            })
        }

        await store.load()
        await store.handleTaskSourceChanged(newHash: "hash-2")

        XCTAssertEqual(store.draftContent, "version-2")
        XCTAssertNil(store.sourceChangeMessage)
    }

    func testSourceChangePreservesDirtyDraftAndShowsReloadMessage() async {
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, _, _ in
                .success(Self.result(stdout: """
                {
                  "project_id": "p_123",
                  "path": "/repo/z/tasks.md",
                  "content": "loaded",
                  "hash": "hash-1",
                  "mtime": "2026-05-26T00:00:00Z",
                  "warning": null
                }
                """))
            })
        }

        await store.load()
        store.draftContent = "local draft"
        await store.handleTaskSourceChanged(newHash: "hash-2")

        XCTAssertEqual(store.draftContent, "local draft")
        XCTAssertNotNil(store.sourceChangeMessage)
    }

    func testDiscardShowsReloadAvailableMessage() async {
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, _, _ in
                .success(Self.result(stdout: """
                {
                  "project_id": "p_123",
                  "path": "/repo/z/tasks.md",
                  "content": "loaded",
                  "hash": "hash-1",
                  "mtime": "2026-05-26T00:00:00Z",
                  "warning": null
                }
                """))
            })
        }

        await store.load()
        store.draftContent = "local draft"
        await store.handleTaskSourceChanged(newHash: "hash-2")
        store.discardChanges()

        XCTAssertFalse(store.isDirty)
        XCTAssertEqual(
            store.sourceChangeMessage,
            String(
                localized: "ccx.tasks.source.reloadAvailable",
                defaultValue: "Task source changed on disk. Reload to show the latest content."
            )
        )
    }

    func testSourceChangeIgnoresAlreadyLoadedHash() async {
        var invocations = 0
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, _, _ in
                invocations += 1
                return .success(Self.result(stdout: """
                {
                  "project_id": "p_123",
                  "path": "/repo/z/tasks.md",
                  "content": "loaded",
                  "hash": "hash-1",
                  "mtime": "2026-05-26T00:00:00Z",
                  "warning": null
                }
                """))
            })
        }

        await store.load()
        await store.handleTaskSourceChanged(newHash: "hash-1")

        XCTAssertEqual(invocations, 1)
        XCTAssertNil(store.sourceChangeMessage)
    }

    func testSourceChangeDuringLoadRetriesAfterInitialLoadCompletes() async {
        var invocations = 0
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, _, _ in
                invocations += 1
                if invocations == 1 {
                    try await Task.sleep(nanoseconds: 50_000_000)
                }
                return .success(Self.result(stdout: """
                {
                  "project_id": "p_123",
                  "path": "/repo/z/tasks.md",
                  "content": "version-\(invocations)",
                  "hash": "hash-\(invocations)",
                  "mtime": "2026-05-26T00:00:00Z",
                  "warning": null
                }
                """))
            })
        }

        let loadTask = Task { await store.load() }
        try? await Task.sleep(nanoseconds: 10_000_000)
        await store.handleTaskSourceChanged(newHash: "hash-2")
        await loadTask.value

        XCTAssertEqual(invocations, 2)
        XCTAssertEqual(store.draftContent, "version-2")
    }

    func testSaveClearsSourceChangeMessage() async {
        var invocations = 0
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, arguments, _ in
                invocations += 1
                if arguments.contains("read") {
                    return .success(Self.result(stdout: """
                    {
                      "project_id": "p_123",
                      "path": "/repo/z/tasks.md",
                      "content": "loaded",
                      "hash": "hash-1",
                      "mtime": "2026-05-26T00:00:00Z",
                      "warning": null
                    }
                    """))
                }
                return .success(Self.result(stdout: """
                {
                  "project_id": "p_123",
                  "path": "/repo/z/tasks.md",
                  "hash": "hash-2",
                  "mtime": "2026-05-26T00:00:01Z",
                  "bytes_written": 5,
                  "warning": null
                }
                """))
            })
        }

        await store.load()
        store.draftContent = "draft"
        await store.handleTaskSourceChanged(newHash: "hash-remote")
        XCTAssertNotNil(store.sourceChangeMessage)
        await store.save()

        XCTAssertEqual(invocations, 2)
        XCTAssertNil(store.sourceChangeMessage)
    }

    func testDiscardRestoresLoadedContent() async {
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, _, _ in
                .success(Self.result(stdout: """
                {
                  "project_id": "p_123",
                  "path": "/repo/z/tasks.md",
                  "content": "loaded",
                  "hash": "hash-1",
                  "mtime": "2026-05-26T00:00:00Z",
                  "warning": null
                }
                """))
            })
        }

        await store.load()
        store.draftContent = "draft"
        store.discardChanges()

        XCTAssertEqual(store.draftContent, "loaded")
        XCTAssertFalse(store.isDirty)
    }

    func testWorkItemCandidatesIncludeHeadingsAndCheckboxes() {
        let candidates = CCXTaskSourceStore.workItemCandidates(in: """
        # Phase 1
        text
        - [ ] Build create flow
        * [x] done
        ## Phase 2
        """)

        XCTAssertEqual(candidates.map(\.selectorType), ["heading", "checkbox", "heading"])
        XCTAssertEqual(candidates[0].displayText, "Phase 1")
        XCTAssertEqual(candidates[1].displayText, "Build create flow")
        XCTAssertEqual(candidates[1].selectorValue, "L3:- [ ] Build create flow")
    }

    func testCreateWorkExecutionCreatesAttachesAndPromptsWorker() async {
        var calls: [[String]] = []
        var promptedMessage: String?
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, arguments, stdin in
                calls.append(arguments)
                if arguments.contains("read") {
                    return .success(Self.result(stdout: """
                    {
                      "project_id": "p_123",
                      "path": "/repo/z/tasks.md",
                      "content": "- [ ] Build create flow\\n",
                      "hash": "hash-1",
                      "mtime": "2026-05-26T00:00:00Z",
                      "warning": null
                    }
                    """))
                }
                if arguments.contains("create") {
                    return .success(Self.result(stdout: """
                    {
                      "work_execution_id": "we_1",
                      "branch_name": "ccx/we_1/build",
                      "worktree_path": "/worktrees/we_1",
                      "task_file_path": "/work-executions/we_1/task.md"
                    }
                    """))
                }
                if arguments.contains("attach") {
                    return .success(Self.result(stdout: """
                    {
                      "agent_session_id": "sess_worker",
                      "work_execution_id": "we_1",
                      "role": "worker",
                      "mode": "writer",
                      "status": "attached"
                    }
                    """))
                }
                promptedMessage = String(data: stdin ?? Data(), encoding: .utf8)
                return .success(Self.result(stdout: """
                {
                  "session_id": "sess_worker",
                  "status": "sent"
                }
                """))
            })
        }
        await store.load()

        await store.createWorkExecutionFromSelection(project: Self.project)

        XCTAssertEqual(calls.map { Array($0.prefix(2)) }, [
            ["task-source", "read"],
            ["work", "create"],
            ["agent", "attach"],
            ["agent", "prompt"],
        ])
        XCTAssertTrue(calls[1].contains("checkbox"))
        XCTAssertTrue(calls[1].contains("Build create flow"))
        XCTAssertTrue(promptedMessage?.contains("we_1") ?? false)
        XCTAssertNotNil(store.workCreateStatusMessage)
        XCTAssertNil(store.workCreateErrorMessage)
    }

    func testCreateWorkExecutionDoesNotUseUnsavedDraftCandidate() async {
        var calls: [[String]] = []
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, arguments, _ in
                calls.append(arguments)
                return .success(Self.result(stdout: """
                {
                  "project_id": "p_123",
                  "path": "/repo/z/tasks.md",
                  "content": "- [ ] Saved item\\n",
                  "hash": "hash-1",
                  "mtime": "2026-05-26T00:00:00Z",
                  "warning": null
                }
                """))
            })
        }

        await store.load()
        store.draftContent = "- [ ] Unsaved item\n"
        await store.createWorkExecutionFromSelection(project: Self.project)

        XCTAssertTrue(store.isDirty)
        XCTAssertFalse(store.canCreateWorkExecution)
        XCTAssertEqual(calls, [
            ["task-source", "read", "--project-id", "p_123", "--json"],
        ])
        XCTAssertNil(store.workCreateStatusMessage)
    }

    func testCreateWorkExecutionAttachFailureDoesNotRecreateOnRetry() async {
        var calls: [[String]] = []
        var attachAttempts = 0
        let store = CCXTaskSourceStore(projectId: "p_123") {
            .success(Self.cli { _, arguments, _ in
                calls.append(arguments)
                if arguments.contains("read") {
                    return .success(Self.result(stdout: """
                    {
                      "project_id": "p_123",
                      "path": "/repo/z/tasks.md",
                      "content": "- [ ] Build create flow\\n",
                      "hash": "hash-1",
                      "mtime": "2026-05-26T00:00:00Z",
                      "warning": null
                    }
                    """))
                }
                if arguments.contains("create") {
                    return .success(Self.result(stdout: """
                    {
                      "work_execution_id": "we_1",
                      "branch_name": "ccx/we_1/build",
                      "worktree_path": "/worktrees/we_1",
                      "task_file_path": "/work-executions/we_1/task.md"
                    }
                    """))
                }
                if arguments.contains("attach") {
                    attachAttempts += 1
                    if attachAttempts == 1 {
                        return .success(CCXControllerCLIProcessResult(
                            exitCode: 1,
                            stdout: Data(),
                            stderr: Data("attach failed".utf8)
                        ))
                    }
                    return .success(Self.result(stdout: """
                    {
                      "agent_session_id": "sess_worker",
                      "work_execution_id": "we_1",
                      "role": "worker",
                      "mode": "writer",
                      "status": "attached"
                    }
                    """))
                }
                return .success(Self.result(stdout: """
                {
                  "session_id": "sess_worker",
                  "status": "sent"
                }
                """))
            })
        }

        await store.load()
        await store.createWorkExecutionFromSelection(project: Self.project)
        XCTAssertEqual(store.lastCreatedWorkExecutionId, "we_1")
        XCTAssertTrue(store.workCreateStatusMessage?.contains("we_1") ?? false)
        XCTAssertNotNil(store.workCreateErrorMessage)

        await store.createWorkExecutionFromSelection(project: Self.project)

        XCTAssertEqual(
            calls.map { Array($0.prefix(2)) },
            [
                ["task-source", "read"],
                ["work", "create"],
                ["agent", "attach"],
                ["agent", "attach"],
                ["agent", "prompt"],
            ]
        )
        XCTAssertNotNil(store.workCreateStatusMessage)
        XCTAssertNil(store.workCreateErrorMessage)
    }

    private static func cli(
        _ handler: @escaping (
            URL,
            [String],
            Data?
        ) async throws -> Result<CCXControllerCLIProcessResult, Error>
    ) -> CCXControllerCLI {
        CCXControllerCLI(executableURL: URL(fileURLWithPath: "/bin/ccx")) { executable, arguments, stdin in
            try await handler(executable, arguments, stdin).get()
        }
    }

    private static func result(stdout: String) -> CCXControllerCLIProcessResult {
        CCXControllerCLIProcessResult(
            exitCode: 0,
            stdout: Data(stdout.utf8),
            stderr: Data()
        )
    }

    private static let project = CCXProjectSummary(
        projectId: "p_123",
        displaySlug: "repo",
        canonicalRepo: "/repo",
        taskSourceFile: "/repo/z/tasks.md",
        createdAt: "2026-05-26T00:00:00Z"
    )
}
