import Foundation

public struct CCXControllerCLI {
    public typealias Runner = (
        _ executableURL: URL,
        _ arguments: [String]
    ) async throws -> CCXControllerCLIProcessResult

    public let executableURL: URL

    private let runner: Runner

    public init(executableURL: URL) {
        self.executableURL = executableURL
        self.runner = CCXControllerCLI.runProcess
    }

    init(
        executableURL: URL,
        runner: @escaping Runner
    ) {
        self.executableURL = executableURL
        self.runner = runner
    }

    public static func resolveExecutable(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        fileManager: FileManager = .default
    ) -> Result<URL, CCXControllerCLIError> {
        if let value = trimmedNonEmpty(environment["CCX_CLI"]) {
            return executableResult(for: URL(fileURLWithPath: value), fileManager: fileManager)
        }

        if let home = ccxHome(from: environment) {
            let candidate = home.appendingPathComponent("bin/ccx")
            if fileManager.fileExists(atPath: candidate.path) {
                return executableResult(for: candidate, fileManager: fileManager)
            }
        }

        for directory in pathDirectories(from: environment["PATH"]) {
            let candidate = directory.appendingPathComponent("ccx")
            if fileManager.fileExists(atPath: candidate.path) {
                return executableResult(for: candidate, fileManager: fileManager)
            }
        }

        return .failure(.executableNotFound)
    }

    public static func make(
        environment: [String: String] = ProcessInfo.processInfo.environment,
        fileManager: FileManager = .default
    ) -> Result<CCXControllerCLI, CCXControllerCLIError> {
        resolveExecutable(environment: environment, fileManager: fileManager)
            .map { CCXControllerCLI(executableURL: $0) }
    }

    public func register(
        canonicalRepo: URL,
        taskSourceFile: URL
    ) async throws -> CCXProjectSummary {
        let result = try await runner(executableURL, [
            "project",
            "register",
            "--canonical-repo",
            canonicalRepo.path,
            "--task-source-file",
            taskSourceFile.path,
        ])
        let stdout = string(from: result.stdout)
        let stderr = string(from: result.stderr)
        guard result.exitCode == 0 else {
            throw CCXControllerCLIError.processFailed(
                exitCode: result.exitCode,
                stdout: stdout,
                stderr: stderr
            )
        }

        do {
            return try JSONDecoder().decode(CCXProjectSummary.self, from: result.stdout)
        } catch {
            throw CCXControllerCLIError.invalidJSON(stdout)
        }
    }

    private static func runProcess(
        executableURL: URL,
        arguments: [String]
    ) async throws -> CCXControllerCLIProcessResult {
        try await withCheckedThrowingContinuation { continuation in
            let process = Process()
            process.executableURL = executableURL
            process.arguments = arguments

            let stdoutPipe = Pipe()
            let stderrPipe = Pipe()
            let output = ProtectedProcessOutput()
            process.standardOutput = stdoutPipe
            process.standardError = stderrPipe

            stdoutPipe.fileHandleForReading.readabilityHandler = { handle in
                output.appendStdout(handle.availableData)
            }
            stderrPipe.fileHandleForReading.readabilityHandler = { handle in
                output.appendStderr(handle.availableData)
            }

            process.terminationHandler = { process in
                stdoutPipe.fileHandleForReading.readabilityHandler = nil
                stderrPipe.fileHandleForReading.readabilityHandler = nil
                output.appendStdout(stdoutPipe.fileHandleForReading.readDataToEndOfFile())
                output.appendStderr(stderrPipe.fileHandleForReading.readDataToEndOfFile())
                continuation.resume(returning: CCXControllerCLIProcessResult(
                    exitCode: process.terminationStatus,
                    stdout: output.stdout,
                    stderr: output.stderr
                ))
            }

            do {
                try process.run()
            } catch {
                stdoutPipe.fileHandleForReading.readabilityHandler = nil
                stderrPipe.fileHandleForReading.readabilityHandler = nil
                continuation.resume(throwing: CCXControllerCLIError.launchFailed(error.localizedDescription))
            }
        }
    }

    private static func executableResult(
        for url: URL,
        fileManager: FileManager
    ) -> Result<URL, CCXControllerCLIError> {
        var isDirectory = ObjCBool(false)
        guard fileManager.fileExists(atPath: url.path, isDirectory: &isDirectory), !isDirectory.boolValue else {
            return .failure(.executableNotFound)
        }
        guard fileManager.isExecutableFile(atPath: url.path) else {
            return .failure(.notExecutable(url.path))
        }
        return .success(url)
    }

    private static func ccxHome(from environment: [String: String]) -> URL? {
        if let value = trimmedNonEmpty(environment["CCX_HOME"]) {
            return URL(fileURLWithPath: value, isDirectory: true)
        }
        return FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".ccx", isDirectory: true)
    }

    private static func pathDirectories(from value: String?) -> [URL] {
        guard let value, !value.isEmpty else { return [] }
        return value.split(separator: ":", omittingEmptySubsequences: true)
            .map { URL(fileURLWithPath: String($0), isDirectory: true) }
    }

    private static func trimmedNonEmpty(_ value: String?) -> String? {
        guard let trimmed = value?.trimmingCharacters(in: .whitespacesAndNewlines), !trimmed.isEmpty else {
            return nil
        }
        return trimmed
    }
}

public struct CCXControllerCLIProcessResult: Sendable {
    public let exitCode: Int32
    public let stdout: Data
    public let stderr: Data

    public init(exitCode: Int32, stdout: Data, stderr: Data) {
        self.exitCode = exitCode
        self.stdout = stdout
        self.stderr = stderr
    }
}

public enum CCXControllerCLIError: Error, Equatable, LocalizedError {
    case executableNotFound
    case notExecutable(String)
    case launchFailed(String)
    case processFailed(exitCode: Int32, stdout: String, stderr: String)
    case invalidJSON(String)

    public var errorDescription: String? {
        switch self {
        case .executableNotFound:
            return "CCX controller CLI was not found. Set CCX_CLI, install it under $CCX_HOME/bin/ccx, or add ccx to PATH."
        case .notExecutable(let path):
            return "CCX controller CLI is not executable: \(path)"
        case .launchFailed(let message):
            return "Failed to launch CCX controller CLI: \(message)"
        case .processFailed(let exitCode, _, let stderr):
            let detail = stderr.trimmingCharacters(in: .whitespacesAndNewlines)
            if detail.isEmpty {
                return "CCX controller CLI failed with exit code \(exitCode)."
            }
            return "CCX controller CLI failed with exit code \(exitCode): \(detail)"
        case .invalidJSON(let stdout):
            let detail = stdout.trimmingCharacters(in: .whitespacesAndNewlines)
            if detail.isEmpty {
                return "CCX controller CLI returned empty JSON."
            }
            return "CCX controller CLI returned invalid JSON: \(detail)"
        }
    }
}

private final class ProtectedProcessOutput: @unchecked Sendable {
    private let lock = NSLock()
    private var stdoutData = Data()
    private var stderrData = Data()

    var stdout: Data {
        lock.lock()
        defer { lock.unlock() }
        return stdoutData
    }

    var stderr: Data {
        lock.lock()
        defer { lock.unlock() }
        return stderrData
    }

    func appendStdout(_ data: Data) {
        guard !data.isEmpty else { return }
        lock.lock()
        stdoutData.append(data)
        lock.unlock()
    }

    func appendStderr(_ data: Data) {
        guard !data.isEmpty else { return }
        lock.lock()
        stderrData.append(data)
        lock.unlock()
    }
}

private func string(from data: Data) -> String {
    String(data: data, encoding: .utf8) ?? String(decoding: data, as: UTF8.self)
}
