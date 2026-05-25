import Foundation
import XCTest

#if DEBUG
@testable import cmux_DEV
#else
@testable import cmux
#endif

final class CCXProjectsStoreTests: XCTestCase {
    private var tempDirs: [URL] = []

    override func tearDownWithError() throws {
        for dir in tempDirs {
            do {
                try FileManager.default.removeItem(at: dir)
            } catch {
                let nsError = error as NSError
                if nsError.domain == NSCocoaErrorDomain,
                   nsError.code == NSFileNoSuchFileError {
                    continue
                }
                throw error
            }
        }
        tempDirs.removeAll()
        try super.tearDownWithError()
    }

    func testSnapshotLoadsProjectSummariesFromProjectConfigs() throws {
        let home = tempDirectory()
        try writeIndex(
            """
            [
              {
                "project_id": "p_1",
                "display_slug": "fallback",
                "canonical_repo": "/fallback"
              }
            ]
            """,
            to: home
        )
        try writeProjectConfig(
            """
            {
              "project_id": "p_1",
              "display_slug": "repo",
              "canonical_repo": "/repo",
              "task_source_file": "/repo/z/tasks.md",
              "created_at": "2026-05-25T00:00:00Z"
            }
            """,
            projectId: "p_1",
            home: home
        )

        let snapshot = CCXProjectsStore.Snapshot.load(paths: .init(
            ccxHome: home,
            index: home.appendingPathComponent("projects.json")
        ))

        XCTAssertNil(snapshot.lastRefreshError)
        XCTAssertEqual(snapshot.projects.count, 1)
        XCTAssertEqual(snapshot.projects[0].projectId, "p_1")
        XCTAssertEqual(snapshot.projects[0].displaySlug, "repo")
        XCTAssertEqual(snapshot.projects[0].canonicalRepo, "/repo")
        XCTAssertEqual(snapshot.projects[0].taskSourceFile, "/repo/z/tasks.md")
        XCTAssertEqual(snapshot.projects[0].createdAt, "2026-05-25T00:00:00Z")
    }

    func testSnapshotFallsBackToIndexEntryWhenProjectConfigIsMissing() throws {
        let home = tempDirectory()
        try writeIndex(
            """
            [
              {
                "project_id": "p_missing",
                "display_slug": "repo",
                "canonical_repo": "/repo"
              }
            ]
            """,
            to: home
        )

        let snapshot = CCXProjectsStore.Snapshot.load(paths: .init(
            ccxHome: home,
            index: home.appendingPathComponent("projects.json")
        ))

        XCTAssertNil(snapshot.lastRefreshError)
        XCTAssertEqual(snapshot.projects, [
            CCXProjectSummary(
                projectId: "p_missing",
                displaySlug: "repo",
                canonicalRepo: "/repo",
                taskSourceFile: "",
                createdAt: ""
            ),
        ])
    }

    func testSnapshotTreatsMissingIndexAsEmpty() {
        let home = tempDirectory()

        let snapshot = CCXProjectsStore.Snapshot.load(paths: .init(
            ccxHome: home,
            index: home.appendingPathComponent("projects.json")
        ))

        XCTAssertNil(snapshot.lastRefreshError)
        XCTAssertEqual(snapshot.projects, [])
    }

    func testSnapshotReportsInvalidIndex() throws {
        let home = tempDirectory()
        try writeIndex("{ invalid json", to: home)

        let snapshot = CCXProjectsStore.Snapshot.load(paths: .init(
            ccxHome: home,
            index: home.appendingPathComponent("projects.json")
        ))

        XCTAssertEqual(snapshot.projects, [])
        XCTAssertNotNil(snapshot.lastRefreshError)
    }

    func testProjectSnapshotDecodesRustEventSchema() throws {
        let home = tempDirectory()
        let projectDir = home
            .appendingPathComponent("projects", isDirectory: true)
            .appendingPathComponent("p_1", isDirectory: true)
        try FileManager.default.createDirectory(at: projectDir, withIntermediateDirectories: true)
        try Data("""
        {"event_id":"e_1","project_id":"p_1","occurred_at":"2026-05-26T00:00:00Z","actor":"controller","event_type":"task_source_file_changed","payload":{"task_source_file":"/repo/z/tasks.md","new_hash":"hash-2"}}
        """.utf8).write(to: projectDir.appendingPathComponent("events.jsonl"))

        let snapshot = CCXProjectStore.Snapshot.load(paths: .init(
            projectDir: projectDir,
            sqlite: projectDir.appendingPathComponent("state.sqlite"),
            events: projectDir.appendingPathComponent("events.jsonl"),
            config: projectDir.appendingPathComponent("project.json")
        ), projectId: "p_1")

        XCTAssertEqual(snapshot.recentEvents.count, 1)
        XCTAssertEqual(snapshot.recentEvents[0].kind, "task_source_file_changed")
        XCTAssertEqual(snapshot.recentEvents[0].timestamp, "2026-05-26T00:00:00Z")
        XCTAssertEqual(snapshot.recentEvents[0].taskSourceFile, "/repo/z/tasks.md")
        XCTAssertEqual(snapshot.recentEvents[0].newHash, "hash-2")
    }

    private func writeIndex(_ json: String, to home: URL) throws {
        try FileManager.default.createDirectory(at: home, withIntermediateDirectories: true)
        try Data(json.utf8).write(to: home.appendingPathComponent("projects.json"))
    }

    private func writeProjectConfig(_ json: String, projectId: String, home: URL) throws {
        let dir = home
            .appendingPathComponent("projects", isDirectory: true)
            .appendingPathComponent(projectId, isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try Data(json.utf8).write(to: dir.appendingPathComponent("project.json"))
    }

    private func tempDirectory() -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("CCXProjectsStoreTests-\(UUID().uuidString)", isDirectory: true)
        tempDirs.append(dir)
        return dir
    }
}
