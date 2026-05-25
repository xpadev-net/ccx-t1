import Foundation
import Observation

public struct CCXProjectRegistrationFormState: Equatable {
    public var repositoryPath: String
    public var taskSourceFilePath: String

    public init(repositoryPath: String = "", taskSourceFilePath: String = "") {
        self.repositoryPath = repositoryPath
        self.taskSourceFilePath = taskSourceFilePath
    }

    public var trimmedRepositoryPath: String {
        repositoryPath.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    public var trimmedTaskSourceFilePath: String {
        taskSourceFilePath.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    public func validationError(fileManager: FileManager = .default) -> CCXProjectRegistrationValidationError? {
        let repository = trimmedRepositoryPath
        guard !repository.isEmpty else { return .emptyRepository }
        var isDirectory = ObjCBool(false)
        guard fileManager.fileExists(atPath: repository, isDirectory: &isDirectory), isDirectory.boolValue else {
            return .repositoryNotDirectory
        }
        let gitDirectory = URL(fileURLWithPath: repository, isDirectory: true).appendingPathComponent(".git", isDirectory: true)
        isDirectory = ObjCBool(false)
        guard fileManager.fileExists(atPath: gitDirectory.path, isDirectory: &isDirectory), isDirectory.boolValue else {
            return .repositoryMissingGitDirectory
        }

        let taskSource = trimmedTaskSourceFilePath
        guard !taskSource.isEmpty else { return .emptyTaskSourceFile }
        isDirectory = ObjCBool(false)
        guard fileManager.fileExists(atPath: taskSource, isDirectory: &isDirectory) else {
            return .taskSourceFileMissing
        }
        guard !isDirectory.boolValue else { return .taskSourceFileIsDirectory }
        guard URL(fileURLWithPath: taskSource).pathExtension.lowercased() == "md" else {
            return .taskSourceFileNotMarkdown
        }

        return nil
    }
}

public enum CCXProjectRegistrationValidationError: Error, Equatable, LocalizedError {
    case emptyRepository
    case repositoryNotDirectory
    case repositoryMissingGitDirectory
    case emptyTaskSourceFile
    case taskSourceFileMissing
    case taskSourceFileIsDirectory
    case taskSourceFileNotMarkdown

    public var errorDescription: String? {
        switch self {
        case .emptyRepository:
            return String(localized: "ccx.projectRegistration.error.emptyRepository",
                          defaultValue: "Choose a repository folder.")
        case .repositoryNotDirectory:
            return String(localized: "ccx.projectRegistration.error.repositoryNotDirectory",
                          defaultValue: "Repository path must be an existing folder.")
        case .repositoryMissingGitDirectory:
            return String(localized: "ccx.projectRegistration.error.repositoryMissingGitDirectory",
                          defaultValue: "Repository folder must contain a .git directory.")
        case .emptyTaskSourceFile:
            return String(localized: "ccx.projectRegistration.error.emptyTaskSourceFile",
                          defaultValue: "Choose a task source Markdown file.")
        case .taskSourceFileMissing:
            return String(localized: "ccx.projectRegistration.error.taskSourceFileMissing",
                          defaultValue: "Task source file does not exist.")
        case .taskSourceFileIsDirectory:
            return String(localized: "ccx.projectRegistration.error.taskSourceFileIsDirectory",
                          defaultValue: "Task source must be a file.")
        case .taskSourceFileNotMarkdown:
            return String(localized: "ccx.projectRegistration.error.taskSourceFileNotMarkdown",
                          defaultValue: "Task source file must use the .md extension.")
        }
    }
}

@MainActor
@Observable
final class CCXProjectRegistrationViewModel {
    typealias CLIProvider = () -> Result<CCXControllerCLI, CCXControllerCLIError>

    var form: CCXProjectRegistrationFormState
    private(set) var isSubmitting = false
    private(set) var errorMessage: String?

    private let fileManager: FileManager
    private let cliProvider: CLIProvider

    init(
        form: CCXProjectRegistrationFormState = CCXProjectRegistrationFormState(),
        fileManager: FileManager = .default,
        cliProvider: @escaping CLIProvider = { CCXControllerCLI.make() }
    ) {
        self.form = form
        self.fileManager = fileManager
        self.cliProvider = cliProvider
    }

    var validationError: CCXProjectRegistrationValidationError? {
        form.validationError(fileManager: fileManager)
    }

    var canSubmit: Bool {
        validationError == nil && !isSubmitting
    }

    func clearSubmitError() {
        errorMessage = nil
    }

    func submit() async -> CCXProjectSummary? {
        guard !isSubmitting else { return nil }
        if validationError != nil {
            return nil
        }

        isSubmitting = true
        errorMessage = nil
        defer { isSubmitting = false }

        let cli: CCXControllerCLI
        switch cliProvider() {
        case .success(let resolvedCLI):
            cli = resolvedCLI
        case .failure(let error):
            errorMessage = error.localizedDescription
            return nil
        }

        do {
            return try await cli.register(
                canonicalRepo: URL(fileURLWithPath: form.trimmedRepositoryPath, isDirectory: true),
                taskSourceFile: URL(fileURLWithPath: form.trimmedTaskSourceFilePath)
            )
        } catch {
            errorMessage = Self.message(for: error)
            return nil
        }
    }

    private static func message(for error: Error) -> String {
        if case CCXControllerCLIError.processFailed = error {
            return String(localized: "ccx.projectRegistration.error.registerFailed",
                          defaultValue: "Could not register the project. Check the selected repository and task source, then try again.")
        }
        return error.localizedDescription
    }
}
