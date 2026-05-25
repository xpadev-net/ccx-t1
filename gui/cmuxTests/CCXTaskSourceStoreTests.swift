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
}
