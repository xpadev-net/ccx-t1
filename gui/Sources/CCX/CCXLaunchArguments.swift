import Foundation

/// Parses CCX launch policy from app arguments and environment.
///
/// `--project-id <id>` opens a dashboard directly, `--ccx` opens CCX with an
/// optional default project, `--ccx-project-picker` opens the picker, and
/// `CCX_DEFAULT_PROJECT_ID` supplies a default dashboard project when no
/// explicit project id or picker request was provided.
public struct CCXLaunchArguments: Sendable {
    public let projectId: String?
    public let isCCXLaunch: Bool

    public init(projectId: String?, isCCXLaunch: Bool) {
        self.projectId = projectId
        self.isCCXLaunch = isCCXLaunch
    }

    public static func parse(
        _ arguments: [String] = CommandLine.arguments,
        environment: [String: String] = ProcessInfo.processInfo.environment
    ) -> CCXLaunchArguments {
        var projectId: String?
        var isCCXLaunch = false
        var hasProjectIdArgument = false
        var requestsProjectPicker = false
        var iter = arguments.makeIterator()
        _ = iter.next() // skip executable path
        while let arg = iter.next() {
            switch arg {
            case "--ccx":
                isCCXLaunch = true
            case "--ccx-project-picker":
                isCCXLaunch = true
                requestsProjectPicker = true
            case "--project-id":
                isCCXLaunch = true
                hasProjectIdArgument = true
                projectId = iter.next()
            case let s where s.hasPrefix("--project-id="):
                isCCXLaunch = true
                hasProjectIdArgument = true
                projectId = String(s.dropFirst("--project-id=".count))
            default:
                continue
            }
        }
        var normalizedProjectId = projectId?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        if normalizedProjectId?.isEmpty != false {
            normalizedProjectId = nil
        }
        if normalizedProjectId == nil, !hasProjectIdArgument, !requestsProjectPicker {
            let defaultProjectId = environment["CCX_DEFAULT_PROJECT_ID"]?
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if defaultProjectId?.isEmpty == false {
                normalizedProjectId = defaultProjectId
                isCCXLaunch = true
            }
        }
        return CCXLaunchArguments(projectId: normalizedProjectId, isCCXLaunch: isCCXLaunch)
    }
}
