import Foundation
import XCTest

#if DEBUG
@testable import cmux_DEV
#else
@testable import cmux
#endif

@MainActor
final class CCXProjectPickerTests: XCTestCase {
    private var tempDirs: [URL] = []

    override func tearDownWithError() throws {
        for dir in tempDirs {
            try? FileManager.default.removeItem(at: dir)
        }
        tempDirs.removeAll()
        try super.tearDownWithError()
    }

    func testLaunchArgumentsDoNotRequestCCXForOrdinaryLaunch() {
        let args = CCXLaunchArguments.parse(["cmux"])

        XCTAssertFalse(args.isCCXLaunch)
        XCTAssertNil(args.projectId)
    }

    func testLaunchArgumentsRequestPickerWithCCXFlag() {
        let args = CCXLaunchArguments.parse(["cmux", "--ccx"])

        XCTAssertTrue(args.isCCXLaunch)
        XCTAssertNil(args.projectId)
    }

    func testLaunchArgumentsRequestPickerWithCCXProjectPickerFlag() {
        let args = CCXLaunchArguments.parse(["cmux", "--ccx-project-picker"])

        XCTAssertTrue(args.isCCXLaunch)
        XCTAssertNil(args.projectId)
    }

    func testLaunchArgumentsRequestDashboardWithProjectId() {
        let args = CCXLaunchArguments.parse(["cmux", "--project-id", "p_1"])

        XCTAssertTrue(args.isCCXLaunch)
        XCTAssertEqual(args.projectId, "p_1")
    }

    func testPanelWithoutProjectUsesPickerMode() {
        let home = temporaryHome()
        let panel = CCXDashboardPanel(projectId: nil, ccxHome: home, projectsStore: CCXProjectsStore(ccxHome: home))

        XCTAssertNil(panel.projectStore)
        XCTAssertEqual(panel.displayTitle, "CCX Projects")
    }

    func testPanelWithProjectUsesDashboardMode() {
        let home = temporaryHome()
        let panel = CCXDashboardPanel(projectId: "p_1", ccxHome: home, projectsStore: CCXProjectsStore(ccxHome: home))

        XCTAssertNotNil(panel.projectStore)
        XCTAssertEqual(panel.displayTitle, "CCX")
    }

    func testPanelUsesInjectedProjectsStore() {
        let store = CCXProjectsStore(ccxHome: temporaryHome())
        let panel = CCXDashboardPanel(projectId: nil, projectsStore: store)

        XCTAssertTrue(panel.projectsStore === store)
    }

    func testPickerRowModelUsesSummaryFields() {
        let summary = CCXProjectSummary(
            projectId: "p_1",
            displaySlug: "repo",
            canonicalRepo: "/repo",
            taskSourceFile: "/repo/z/tasks.md",
            createdAt: "2026-05-25T00:00:00Z"
        )

        let model = CCXProjectPickerRowModel(summary: summary)

        XCTAssertEqual(model.id, "p_1")
        XCTAssertEqual(model.title, "repo")
        XCTAssertEqual(model.subtitle, "/repo")
        XCTAssertEqual(model.taskSourceFile, "/repo/z/tasks.md")
    }

    func testPickerRowModelFallsBackToProjectIdForEmptySlug() {
        let summary = CCXProjectSummary(
            projectId: "p_fallback",
            displaySlug: "",
            canonicalRepo: "/repo",
            taskSourceFile: "",
            createdAt: ""
        )

        XCTAssertEqual(CCXProjectPickerRowModel(summary: summary).title, "p_fallback")
    }

    func testRegistrationFormRequiresGitDirectory() throws {
        let repo = try temporaryDirectory()
        let taskSource = try makeFile(named: "tasks.md", in: repo)
        let form = CCXProjectRegistrationFormState(
            repositoryPath: repo.path,
            taskSourceFilePath: taskSource.path
        )

        XCTAssertEqual(form.validationError(), .repositoryMissingGitDirectory)
    }

    func testRegistrationFormRequiresMarkdownTaskSourceFile() throws {
        let repo = try temporaryDirectory()
        try FileManager.default.createDirectory(
            at: repo.appendingPathComponent(".git", isDirectory: true),
            withIntermediateDirectories: true
        )
        let taskSource = try makeFile(named: "tasks.txt", in: repo)
        let form = CCXProjectRegistrationFormState(
            repositoryPath: repo.path,
            taskSourceFilePath: taskSource.path
        )

        XCTAssertEqual(form.validationError(), .taskSourceFileNotMarkdown)
    }

    func testRegistrationFormAcceptsGitRepositoryAndMarkdownTaskSource() throws {
        let repo = try temporaryDirectory()
        try FileManager.default.createDirectory(
            at: repo.appendingPathComponent(".git", isDirectory: true),
            withIntermediateDirectories: true
        )
        let taskSource = try makeFile(named: "tasks.md", in: repo)
        let form = CCXProjectRegistrationFormState(
            repositoryPath: " \(repo.path) ",
            taskSourceFilePath: " \(taskSource.path) "
        )

        XCTAssertNil(form.validationError())
        XCTAssertEqual(form.trimmedRepositoryPath, repo.path)
        XCTAssertEqual(form.trimmedTaskSourceFilePath, taskSource.path)
    }

    func testRegistrationViewModelSubmitsThroughControllerCLI() async throws {
        let repo = try temporaryDirectory()
        try FileManager.default.createDirectory(
            at: repo.appendingPathComponent(".git", isDirectory: true),
            withIntermediateDirectories: true
        )
        let taskSource = try makeFile(named: "tasks.md", in: repo)
        let expectedSummary = CCXProjectSummary(
            projectId: "p_registered",
            displaySlug: "repo",
            canonicalRepo: repo.path,
            taskSourceFile: taskSource.path,
            createdAt: "2026-05-25T00:00:00Z"
        )
        let stdout = """
        {
          "project_id": "\(expectedSummary.projectId)",
          "display_slug": "\(expectedSummary.displaySlug)",
          "canonical_repo": "\(expectedSummary.canonicalRepo)",
          "task_source_file": "\(expectedSummary.taskSourceFile)",
          "created_at": "\(expectedSummary.createdAt)"
        }
        """.data(using: .utf8)!
        var capturedArguments: [String] = []
        let cli = CCXControllerCLI(executableURL: URL(fileURLWithPath: "/bin/ccx")) { _, arguments in
            capturedArguments = arguments
            return CCXControllerCLIProcessResult(exitCode: 0, stdout: stdout, stderr: Data())
        }
        let viewModel = CCXProjectRegistrationViewModel(
            form: CCXProjectRegistrationFormState(
                repositoryPath: repo.path,
                taskSourceFilePath: taskSource.path
            ),
            cliProvider: { .success(cli) }
        )

        let registered = await viewModel.submit()

        XCTAssertEqual(registered, expectedSummary)
        XCTAssertEqual(capturedArguments, [
            "project",
            "register",
            "--canonical-repo",
            repo.path,
            "--task-source-file",
            taskSource.path,
        ])
        XCTAssertFalse(viewModel.isSubmitting)
        XCTAssertNil(viewModel.errorMessage)
    }

    func testRegistrationViewModelShowsSafeMessageOnCLIFailure() async throws {
        let repo = try temporaryDirectory()
        try FileManager.default.createDirectory(
            at: repo.appendingPathComponent(".git", isDirectory: true),
            withIntermediateDirectories: true
        )
        let taskSource = try makeFile(named: "tasks.md", in: repo)
        let cli = CCXControllerCLI(executableURL: URL(fileURLWithPath: "/bin/ccx")) { _, _ in
            CCXControllerCLIProcessResult(
                exitCode: 2,
                stdout: Data(),
                stderr: Data("task source is outside repository".utf8)
            )
        }
        let viewModel = CCXProjectRegistrationViewModel(
            form: CCXProjectRegistrationFormState(
                repositoryPath: repo.path,
                taskSourceFilePath: taskSource.path
            ),
            cliProvider: { .success(cli) }
        )

        let registered = await viewModel.submit()

        XCTAssertNil(registered)
        XCTAssertEqual(
            viewModel.errorMessage,
            "Could not register the project. Check the selected repository and task source, then try again."
        )
        XCTAssertFalse(viewModel.isSubmitting)
    }

    func testRegistrationViewModelIgnoresDuplicateSubmitWhileRegistering() async throws {
        let repo = try temporaryDirectory()
        try FileManager.default.createDirectory(
            at: repo.appendingPathComponent(".git", isDirectory: true),
            withIntermediateDirectories: true
        )
        let taskSource = try makeFile(named: "tasks.md", in: repo)
        let stdout = """
        {
          "project_id": "p_registered",
          "display_slug": "repo",
          "canonical_repo": "\(repo.path)",
          "task_source_file": "\(taskSource.path)",
          "created_at": "2026-05-25T00:00:00Z"
        }
        """.data(using: .utf8)!
        var continuation: CheckedContinuation<CCXControllerCLIProcessResult, Never>?
        var runCount = 0
        let cli = CCXControllerCLI(executableURL: URL(fileURLWithPath: "/bin/ccx")) { _, _ in
            runCount += 1
            return await withCheckedContinuation { pending in
                continuation = pending
            }
        }
        let viewModel = CCXProjectRegistrationViewModel(
            form: CCXProjectRegistrationFormState(
                repositoryPath: repo.path,
                taskSourceFilePath: taskSource.path
            ),
            cliProvider: { .success(cli) }
        )

        let firstSubmit = Task { await viewModel.submit() }
        while continuation == nil {
            await Task.yield()
        }
        let duplicate = await viewModel.submit()
        continuation?.resume(returning: CCXControllerCLIProcessResult(exitCode: 0, stdout: stdout, stderr: Data()))
        let registered = await firstSubmit.value

        XCTAssertNil(duplicate)
        XCTAssertEqual(registered?.projectId, "p_registered")
        XCTAssertEqual(runCount, 1)
        XCTAssertFalse(viewModel.isSubmitting)
    }

    private func temporaryHome() -> URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("CCXProjectPickerTests-\(UUID().uuidString)", isDirectory: true)
    }

    private func temporaryDirectory() throws -> URL {
        let dir = temporaryHome()
        tempDirs.append(dir)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    private func makeFile(named name: String, in directory: URL) throws -> URL {
        let file = directory.appendingPathComponent(name)
        try Data("# Task\n".utf8).write(to: file)
        return file
    }
}
