import Foundation

/// Persistence data shapes mirrored from the Rust controller's SQLite read model
/// (see `src/persistence/sqlite.rs`). These are read-only snapshots used by the
/// CCX dashboard UI; mutations belong to the controller.

public struct CCXProjectSummary: Identifiable, Hashable, Sendable {
    public let projectId: String
    public let displaySlug: String
    public let canonicalRepo: String
    public let taskSourceFile: String
    public let createdAt: String

    public var id: String { projectId }
}

public struct CCXWorkExecution: Identifiable, Hashable, Sendable {
    public let workExecutionId: String
    public let projectId: String
    public let state: String
    public let branchName: String?
    public let worktreePath: String?
    public let taskFilePath: String?
    public let prNumber: Int64?
    public let prUrl: String?
    public let headCommit: String?
    public let displayText: String?
    public let selectedAt: String?
    public let artifactState: String?
    public let syncStatus: String?

    public var id: String { workExecutionId }
}

public struct CCXAgentSession: Identifiable, Hashable, Sendable {
    public let agentSessionId: String
    public let projectId: String
    public let workExecutionId: String?
    public let state: String
    public let role: String
    public let attachMode: String?
    public let cmuxTabId: String?
    public let tmuxSessionId: String?
    public let pid: Int64?
    public let cwd: String?
    public let startedAt: String?
    public let lastHeartbeatAt: String?
    public let exitCode: Int64?

    public var id: String { agentSessionId }
}

public struct CCXEventEntry: Identifiable, Hashable, Sendable {
    public let eventId: String
    public let projectId: String
    public let kind: String
    public let actor: String
    public let timestamp: String
    public let raw: String

    public var id: String { eventId }
}

/// Lifecycle states the controller writes to `work_executions.state`.
/// Mirrors `crate::domain::work_execution::WorkExecutionState`.
public enum CCXWorkExecutionState: String, CaseIterable, Sendable {
    case created
    case taskFileCreated = "task_file_created"
    case dispatched
    case running
    case prOpen = "pr_open"
    case gateCheck = "gate_check"
    case reviewFixing = "review_fixing"
    case mergeReady = "merge_ready"
    case merging
    case merged
    case followupRequired = "followup_required"
    case returned
    case blocked
    case failed
    case hold
    case canceled
    case superseded

    public var localizedLabel: String {
        switch self {
        case .created:
            return String(localized: "ccx.work.state.created", defaultValue: "Created")
        case .taskFileCreated:
            return String(localized: "ccx.work.state.taskFileCreated", defaultValue: "Task file created")
        case .dispatched:
            return String(localized: "ccx.work.state.dispatched", defaultValue: "Dispatched")
        case .running:
            return String(localized: "ccx.work.state.running", defaultValue: "Running")
        case .prOpen:
            return String(localized: "ccx.work.state.prOpen", defaultValue: "PR open")
        case .gateCheck:
            return String(localized: "ccx.work.state.gateCheck", defaultValue: "Gate check")
        case .reviewFixing:
            return String(localized: "ccx.work.state.reviewFixing", defaultValue: "Review fixing")
        case .mergeReady:
            return String(localized: "ccx.work.state.mergeReady", defaultValue: "Merge ready")
        case .merging:
            return String(localized: "ccx.work.state.merging", defaultValue: "Merging")
        case .merged:
            return String(localized: "ccx.work.state.merged", defaultValue: "Merged")
        case .followupRequired:
            return String(localized: "ccx.work.state.followupRequired", defaultValue: "Follow-up required")
        case .returned:
            return String(localized: "ccx.work.state.returned", defaultValue: "Returned")
        case .blocked:
            return String(localized: "ccx.work.state.blocked", defaultValue: "Blocked")
        case .failed:
            return String(localized: "ccx.work.state.failed", defaultValue: "Failed")
        case .hold:
            return String(localized: "ccx.work.state.hold", defaultValue: "On hold")
        case .canceled:
            return String(localized: "ccx.work.state.canceled", defaultValue: "Canceled")
        case .superseded:
            return String(localized: "ccx.work.state.superseded", defaultValue: "Superseded")
        }
    }
}
