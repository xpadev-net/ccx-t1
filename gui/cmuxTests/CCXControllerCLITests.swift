import Foundation
import XCTest

#if DEBUG
@testable import cmux_DEV
#else
@testable import cmux
#endif

final class CCXControllerCLITests: XCTestCase {
    private var tempDirs: [URL] = []

    override func tearDownWithError() throws {
        for dir in tempDirs {
            try? FileManager.default.removeItem(at: dir)
        }
        tempDirs.removeAll()
        try super.tearDownWithError()
    }

    func testResolvePrefersCCXCLIEnvironment() throws {
        let envPath = try makeExecutable(named: "env-ccx")
        let homePath = try makeExecutable(named: "ccx", inSubdirectory: "home/bin")
        let pathCandidate = try makeExecutable(named: "ccx", inSubdirectory: "path")

        let result = CCXControllerCLI.resolveExecutable(environment: [
            "CCX_CLI": envPath.path,
            "CCX_HOME": homePath.deletingLastPathComponent().deletingLastPathComponent().path,
            "PATH": pathCandidate.deletingLastPathComponent().path,
        ])

        XCTAssertEqual(try result.get(), envPath)
    }

    func testResolveUsesCCXHomeBeforePATH() throws {
        let homeCandidate = try makeExecutable(named: "ccx", inSubdirectory: "home/bin")
        let pathCandidate = try makeExecutable(named: "ccx", inSubdirectory: "path")

        let result = CCXControllerCLI.resolveExecutable(environment: [
            "CCX_HOME": homeCandidate.deletingLastPathComponent().deletingLastPathComponent().path,
            "PATH": pathCandidate.deletingLastPathComponent().path,
        ])

        XCTAssertEqual(try result.get(), homeCandidate)
    }

    func testResolveUsesPATHWhenNoConfiguredCLIExists() throws {
        let pathCandidate = try makeExecutable(named: "ccx", inSubdirectory: "path")

        let result = CCXControllerCLI.resolveExecutable(environment: [
            "CCX_HOME": tempDirectory().appendingPathComponent("missing-home").path,
            "PATH": pathCandidate.deletingLastPathComponent().path,
        ])

        XCTAssertEqual(try result.get(), pathCandidate)
    }

    func testResolveReportsNonExecutableCCXCLI() throws {
        let file = try makeFile(named: "ccx")

        let result = CCXControllerCLI.resolveExecutable(environment: [
            "CCX_CLI": file.path,
            "PATH": "",
        ])

        XCTAssertEqual(result.failure, .notExecutable(file.path))
    }

    func testRegisterParsesProjectSummaryAndPassesArguments() async throws {
        let stdout = """
        {
          "project_id": "p_123",
          "display_slug": "repo",
          "canonical_repo": "/repo",
          "task_source_file": "/repo/z/tasks.md",
          "created_at": "2026-05-25T00:00:00Z"
        }
        """.data(using: .utf8)!
        var capturedExecutable: URL?
        var capturedArguments: [String] = []
        let cli = CCXControllerCLI(executableURL: URL(fileURLWithPath: "/bin/ccx")) { executable, arguments in
            capturedExecutable = executable
            capturedArguments = arguments
            return CCXControllerCLIProcessResult(exitCode: 0, stdout: stdout, stderr: Data())
        }

        let summary = try await cli.register(
            canonicalRepo: URL(fileURLWithPath: "/repo", isDirectory: true),
            taskSourceFile: URL(fileURLWithPath: "/repo/z/tasks.md")
        )

        XCTAssertEqual(capturedExecutable?.path, "/bin/ccx")
        XCTAssertEqual(capturedArguments, [
            "project",
            "register",
            "--canonical-repo",
            "/repo",
            "--task-source-file",
            "/repo/z/tasks.md",
        ])
        XCTAssertEqual(summary.projectId, "p_123")
        XCTAssertEqual(summary.displaySlug, "repo")
        XCTAssertEqual(summary.canonicalRepo, "/repo")
        XCTAssertEqual(summary.taskSourceFile, "/repo/z/tasks.md")
        XCTAssertEqual(summary.createdAt, "2026-05-25T00:00:00Z")
    }

    func testRegisterFailureKeepsStderr() async {
        let cli = CCXControllerCLI(executableURL: URL(fileURLWithPath: "/bin/ccx")) { _, _ in
            CCXControllerCLIProcessResult(
                exitCode: 2,
                stdout: Data("partial".utf8),
                stderr: Data("canonical repo is invalid".utf8)
            )
        }

        do {
            _ = try await cli.register(
                canonicalRepo: URL(fileURLWithPath: "/missing", isDirectory: true),
                taskSourceFile: URL(fileURLWithPath: "/missing/tasks.md")
            )
            XCTFail("register should throw")
        } catch let error as CCXControllerCLIError {
            XCTAssertEqual(error, .processFailed(
                exitCode: 2,
                stdout: "partial",
                stderr: "canonical repo is invalid"
            ))
            XCTAssertFalse(error.localizedDescription.contains("canonical repo is invalid"))
            XCTAssertTrue(error.localizedDescription.contains("exit code 2"))
        } catch {
            XCTFail("unexpected error: \(error)")
        }
    }

    func testRegisterTimeoutTerminatesProcess() async throws {
        let executable = try makeFile(named: "ccx", content: "#!/bin/sh\nsleep 60\n")
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: executable.path)
        let cli = CCXControllerCLI(executableURL: executable, timeoutWaiter: {})

        do {
            _ = try await cli.register(
                canonicalRepo: URL(fileURLWithPath: "/repo", isDirectory: true),
                taskSourceFile: URL(fileURLWithPath: "/repo/z/tasks.md")
            )
            XCTFail("register should time out")
        } catch let error as CCXControllerCLIError {
            XCTAssertEqual(error, .timedOut(seconds: 120))
        } catch {
            XCTFail("unexpected error: \(error)")
        }
    }

    private func makeExecutable(named name: String, inSubdirectory subdirectory: String? = nil) throws -> URL {
        let file = try makeFile(named: name, inSubdirectory: subdirectory)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: file.path)
        return file
    }

    private func makeFile(
        named name: String,
        inSubdirectory subdirectory: String? = nil,
        content: String = "#!/bin/sh\n"
    ) throws -> URL {
        let directory: URL
        if let subdirectory {
            directory = tempDirectory().appendingPathComponent(subdirectory, isDirectory: true)
        } else {
            directory = tempDirectory()
        }
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let file = directory.appendingPathComponent(name)
        try Data(content.utf8).write(to: file)
        return file
    }

    private func tempDirectory() -> URL {
        let dir = FileManager.default.temporaryDirectory
            .appendingPathComponent("CCXControllerCLITests-\(UUID().uuidString)", isDirectory: true)
        tempDirs.append(dir)
        return dir
    }
}

private extension Result {
    var failure: Failure? {
        guard case .failure(let error) = self else { return nil }
        return error
    }
}
