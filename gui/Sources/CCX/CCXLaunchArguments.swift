import Foundation

/// Parses launch arguments that the ccx controller passes via
/// `open -a ccx-cmux --args --project-id <id>`.
public struct CCXLaunchArguments: Sendable {
    public let projectId: String?

    public init(projectId: String?) {
        self.projectId = projectId
    }

    public static func parse(_ arguments: [String] = CommandLine.arguments) -> CCXLaunchArguments {
        var projectId: String?
        var iter = arguments.makeIterator()
        _ = iter.next() // skip executable path
        while let arg = iter.next() {
            switch arg {
            case "--project-id":
                projectId = iter.next()
            case let s where s.hasPrefix("--project-id="):
                projectId = String(s.dropFirst("--project-id=".count))
            default:
                continue
            }
        }
        return CCXLaunchArguments(projectId: projectId)
    }
}
