import Foundation

/// Parses launch arguments that the ccx controller passes via
/// `open -a ccx-cmux --args --project-id <id>`.
public struct CCXLaunchArguments: Sendable {
    public let projectId: String?
    public let isCCXLaunch: Bool

    public init(projectId: String?, isCCXLaunch: Bool) {
        self.projectId = projectId
        self.isCCXLaunch = isCCXLaunch
    }

    public static func parse(_ arguments: [String] = CommandLine.arguments) -> CCXLaunchArguments {
        var projectId: String?
        var isCCXLaunch = false
        var iter = arguments.makeIterator()
        _ = iter.next() // skip executable path
        while let arg = iter.next() {
            switch arg {
            case "--ccx", "--ccx-project-picker":
                isCCXLaunch = true
            case "--project-id":
                isCCXLaunch = true
                projectId = iter.next()
            case let s where s.hasPrefix("--project-id="):
                isCCXLaunch = true
                projectId = String(s.dropFirst("--project-id=".count))
            default:
                continue
            }
        }
        return CCXLaunchArguments(projectId: projectId, isCCXLaunch: isCCXLaunch)
    }
}
