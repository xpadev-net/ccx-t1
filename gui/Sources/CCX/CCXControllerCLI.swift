import Foundation

nonisolated public struct CCXControllerCLI {
    public typealias Runner = (
        _ executableURL: URL,
        _ arguments: [String]
    ) async throws -> CCXControllerCLIProcessResult
    typealias TimeoutWaiter = () async throws -> Void

    public let executableURL: URL

    private let runner: Runner
    private static let processTimeoutSeconds = 120

    public init(executableURL: URL) {
        self.executableURL = executableURL
        self.runner = { executableURL, arguments in
            try await CCXControllerCLI.runProcess(
                executableURL: executableURL,
                arguments: arguments
            )
        }
    }

    init(
        executableURL: URL,
        runner: @escaping Runner
    ) {
        self.executableURL = executableURL
        self.runner = runner
    }

    init(
        executableURL: URL,
        timeoutWaiter: @escaping TimeoutWaiter
    ) {
        self.executableURL = executableURL
        self.runner = { executableURL, arguments in
            try await CCXControllerCLI.runProcess(
                executableURL: executableURL,
                arguments: arguments,
                timeoutWaiter: timeoutWaiter
            )
        }
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
        arguments: [String],
        timeoutWaiter: @escaping TimeoutWaiter = defaultTimeoutWaiter
    ) async throws -> CCXControllerCLIProcessResult {
        let cancellation = ProcessCancellation()
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                let process = Process()
                process.executableURL = executableURL
                process.arguments = arguments

                let stdoutPipe = Pipe()
                let stderrPipe = Pipe()
                let output = ProtectedProcessOutput()
                let completion = ProcessCompletion(
                    continuation: continuation,
                    process: process,
                    stdoutPipe: stdoutPipe,
                    stderrPipe: stderrPipe,
                    output: output
                )
                cancellation.set(completion)
                process.standardOutput = stdoutPipe
                process.standardError = stderrPipe

                stdoutPipe.fileHandleForReading.readabilityHandler = { handle in
                    output.appendStdout(handle.availableData)
                }
                stderrPipe.fileHandleForReading.readabilityHandler = { handle in
                    output.appendStderr(handle.availableData)
                }

                process.terminationHandler = { process in
                    completion.finish(
                        .success(CCXControllerCLIProcessResult(
                            exitCode: process.terminationStatus,
                            stdout: output.stdout,
                            stderr: output.stderr
                        )),
                        collectRemainingOutput: true,
                        terminateProcess: false
                    )
                }

                let timeoutTask = Task {
                    do {
                        try await timeoutWaiter()
                    } catch {
                        return
                    }
                    completion.finish(
                        .failure(CCXControllerCLIError.timedOut(seconds: processTimeoutSeconds)),
                        collectRemainingOutput: false,
                        terminateProcess: true
                    )
                }
                completion.setTimeoutTask(timeoutTask)

                do {
                    guard completion.prepareToLaunch() else {
                        completion.finish(
                            .failure(CCXControllerCLIError.cancelled),
                            collectRemainingOutput: false,
                            terminateProcess: true
                        )
                        return
                    }
                    try process.run()
                    _ = completion.finishIfCancellationRequestedAfterLaunch()
                } catch {
                    if completion.finishIfCancellationRequestedAfterLaunch() {
                        return
                    }
                    completion.finish(
                        .failure(CCXControllerCLIError.launchFailed(error.localizedDescription)),
                        collectRemainingOutput: false,
                        terminateProcess: false
                    )
                }
            }
        } onCancel: {
            cancellation.cancel()
        }
    }

    private static func defaultTimeoutWaiter() async throws {
        try await ContinuousClock().sleep(for: .seconds(processTimeoutSeconds))
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
    case timedOut(seconds: Int)
    case cancelled

    public var errorDescription: String? {
        switch self {
        case .executableNotFound:
            return String(
                localized: "ccx.controller.executableNotFound",
                defaultValue: "CCX controller CLI was not found. Set CCX_CLI, install it under $CCX_HOME/bin/ccx, or add ccx to PATH."
            )
        case .notExecutable(let path):
            return String(
                localized: "ccx.controller.notExecutable",
                defaultValue: "CCX controller CLI is not executable: \(path)"
            )
        case .launchFailed:
            return String(
                localized: "ccx.controller.launchFailed",
                defaultValue: "CCX controller CLI could not be started. Check file permissions or reinstall it."
            )
        case .processFailed(let exitCode, _, _):
            return String(
                localized: "ccx.controller.processFailed",
                defaultValue: "CCX controller CLI failed (exit code \(exitCode))."
            )
        case .invalidJSON:
            return String(
                localized: "ccx.controller.invalidJSON",
                defaultValue: "CCX controller CLI returned an unexpected response format."
            )
        case .timedOut(let seconds):
            return String(
                localized: "ccx.controller.timedOut",
                defaultValue: "CCX controller CLI timed out after \(seconds) seconds."
            )
        case .cancelled:
            return String(
                localized: "ccx.controller.cancelled",
                defaultValue: "CCX controller CLI request was cancelled."
            )
        }
    }
}

// `withTaskCancellationHandler`'s `onCancel` closure can be invoked from any
// concurrency context, and `set()` is called from the synchronous body of
// `withCheckedThrowingContinuation`, so actor isolation cannot own this state.
// `NSLock` serialises the two-step set/cancel handoff safely.
private final class ProcessCancellation: @unchecked Sendable {
    private let lock = NSLock()
    private var completion: ProcessCompletion?

    func set(_ completion: ProcessCompletion) {
        lock.lock()
        self.completion = completion
        lock.unlock()
    }

    func cancel() {
        lock.lock()
        let completion = self.completion
        lock.unlock()
        completion?.cancel()
    }
}

// `Process.terminationHandler` and `FileHandle.readabilityHandler` are
// non-async callbacks, so actor isolation cannot own this state. The lock
// protects the single-resume guard and all handler cleanup.
private final class ProcessCompletion: @unchecked Sendable {
    private let lock = NSLock()
    private var didFinish = false
    private var launchStarted = false
    private var cancelRequestedBeforeLaunchReturned = false
    private var timeoutTask: Task<Void, Never>?

    private let continuation: CheckedContinuation<CCXControllerCLIProcessResult, Error>
    private let process: Process
    private let stdoutPipe: Pipe
    private let stderrPipe: Pipe
    private let output: ProtectedProcessOutput

    init(
        continuation: CheckedContinuation<CCXControllerCLIProcessResult, Error>,
        process: Process,
        stdoutPipe: Pipe,
        stderrPipe: Pipe,
        output: ProtectedProcessOutput
    ) {
        self.continuation = continuation
        self.process = process
        self.stdoutPipe = stdoutPipe
        self.stderrPipe = stderrPipe
        self.output = output
    }

    func setTimeoutTask(_ task: Task<Void, Never>) {
        lock.lock()
        if didFinish {
            lock.unlock()
            task.cancel()
            return
        }
        timeoutTask = task
        lock.unlock()
    }

    func prepareToLaunch() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard !didFinish else { return false }
        launchStarted = true
        return true
    }

    func finishIfCancellationRequestedAfterLaunch() -> Bool {
        lock.lock()
        let shouldCancel = cancelRequestedBeforeLaunchReturned && !didFinish
        lock.unlock()
        guard shouldCancel else { return false }
        finish(
            .failure(CCXControllerCLIError.cancelled),
            collectRemainingOutput: false,
            terminateProcess: true
        )
        return true
    }

    func cancel() {
        lock.lock()
        guard !didFinish else {
            lock.unlock()
            return
        }
        if launchStarted, !process.isRunning {
            cancelRequestedBeforeLaunchReturned = true
            lock.unlock()
            return
        }
        lock.unlock()
        finish(
            .failure(CCXControllerCLIError.cancelled),
            collectRemainingOutput: false,
            terminateProcess: true
        )
    }

    func finish(
        _ result: Result<CCXControllerCLIProcessResult, Error>,
        collectRemainingOutput: Bool,
        terminateProcess: Bool
    ) {
        lock.lock()
        guard !didFinish else {
            lock.unlock()
            return
        }
        didFinish = true
        let task = timeoutTask
        timeoutTask = nil
        stdoutPipe.fileHandleForReading.readabilityHandler = nil
        stderrPipe.fileHandleForReading.readabilityHandler = nil
        if terminateProcess, process.isRunning {
            process.terminate()
        }
        lock.unlock()

        task?.cancel()
        if collectRemainingOutput {
            output.appendStdout(stdoutPipe.fileHandleForReading.readDataToEndOfFile())
            output.appendStderr(stderrPipe.fileHandleForReading.readDataToEndOfFile())
        }

        switch result {
        case .success(let processResult):
            continuation.resume(returning: CCXControllerCLIProcessResult(
                exitCode: processResult.exitCode,
                stdout: output.stdout,
                stderr: output.stderr
            ))
        case .failure(let error):
            continuation.resume(throwing: error)
        }
    }
}

// `readabilityHandler` is a non-async `(FileHandle) -> Void` closure, so actor
// isolation cannot be used here. `NSLock` serialises all appends, making
// `@unchecked Sendable` safe for this process bridge.
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
