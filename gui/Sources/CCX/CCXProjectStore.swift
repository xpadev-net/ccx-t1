import Foundation
import Combine
import SQLite3

/// SQLITE_TRANSIENT tells SQLite to copy the bound buffer (it's safe to free
/// after the bind call returns). The constant isn't exposed through the
/// Swift overlay, so we synthesize the sentinel pointer once at file scope.
private let SQLITE_TRANSIENT_DESTRUCTOR = unsafeBitCast(
    -1, to: sqlite3_destructor_type.self
)

/// Reads CCX state for a single project from the on-disk artifacts the Rust
/// controller writes to `$CCX_HOME/projects/<projectId>/`:
///   - `state.sqlite`  — denormalized read model (`work_executions`, `agent_sessions`, ...)
///   - `events.jsonl`  — append-only audit log (source of truth)
///   - `project.json`  — project configuration
///
/// The store is an `ObservableObject` that publishes refreshed snapshots
/// whenever an FSEvent fires on the project directory. It is intentionally
/// read-only: mutations must go through the controller CLI to preserve
/// event-sourcing invariants.
@MainActor
public final class CCXProjectStore: ObservableObject {
    @Published public private(set) var project: CCXProjectSummary?
    @Published public private(set) var workExecutions: [CCXWorkExecution] = []
    @Published public private(set) var agentSessions: [CCXAgentSession] = []
    @Published public private(set) var recentEvents: [CCXEventEntry] = []
    @Published public private(set) var lastRefreshError: String?

    public let projectId: String
    private let projectDir: URL
    private let sqlitePath: URL
    private let eventsPath: URL
    private let configPath: URL

    private var eventStream: FSEventStreamRef?
    private let refreshQueue = DispatchQueue(label: "ccx.project-store.refresh")

    public init(projectId: String, ccxHome: URL? = nil) {
        self.projectId = projectId
        let home = ccxHome ?? Self.defaultCCXHome()
        self.projectDir = home.appendingPathComponent("projects/\(projectId)", isDirectory: true)
        self.sqlitePath = projectDir.appendingPathComponent("state.sqlite")
        self.eventsPath = projectDir.appendingPathComponent("events.jsonl")
        self.configPath = projectDir.appendingPathComponent("project.json")
    }

    // `nonisolated` lets the deinit run off the main actor; FSEventStream APIs
    // are thread-safe so cleanup does not need to hop back.
    nonisolated deinit {
        if let stream = eventStream {
            FSEventStreamStop(stream)
            FSEventStreamInvalidate(stream)
            FSEventStreamRelease(stream)
        }
    }

    public static func defaultCCXHome() -> URL {
        if let env = ProcessInfo.processInfo.environment["CCX_HOME"], !env.isEmpty {
            return URL(fileURLWithPath: env, isDirectory: true)
        }
        let home = FileManager.default.homeDirectoryForCurrentUser
        return home.appendingPathComponent(".ccx", isDirectory: true)
    }

    public func start() {
        refresh()
        startWatching()
    }

    private func startWatching() {
        guard eventStream == nil else { return }
        let context = UnsafeMutablePointer<FSEventStreamContext>.allocate(capacity: 1)
        context.pointee = FSEventStreamContext(
            version: 0,
            info: Unmanaged.passUnretained(self).toOpaque(),
            retain: nil,
            release: nil,
            copyDescription: nil
        )
        defer { context.deallocate() }

        let callback: FSEventStreamCallback = { _, info, _, _, _, _ in
            guard let info = info else { return }
            let store = Unmanaged<CCXProjectStore>.fromOpaque(info).takeUnretainedValue()
            Task { @MainActor in store.refresh() }
        }

        let path = projectDir.path as CFString
        let paths = [path] as CFArray
        let latency: CFTimeInterval = 0.25
        guard let stream = FSEventStreamCreate(
            kCFAllocatorDefault,
            callback,
            context,
            paths,
            FSEventStreamEventId(kFSEventStreamEventIdSinceNow),
            latency,
            FSEventStreamCreateFlags(kFSEventStreamCreateFlagFileEvents)
        ) else {
            return
        }
        FSEventStreamSetDispatchQueue(stream, refreshQueue)
        FSEventStreamStart(stream)
        self.eventStream = stream
    }

    public func refresh() {
        loadProjectConfig()
        loadWorkExecutionsAndSessions()
        loadRecentEvents()
    }

    private func loadProjectConfig() {
        guard let data = try? Data(contentsOf: configPath),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            project = nil
            return
        }
        project = CCXProjectSummary(
            projectId: (json["project_id"] as? String) ?? projectId,
            displaySlug: (json["display_slug"] as? String) ?? "",
            canonicalRepo: (json["canonical_repo"] as? String) ?? "",
            taskSourceFile: (json["task_source_file"] as? String) ?? "",
            createdAt: (json["created_at"] as? String) ?? ""
        )
    }

    private func loadWorkExecutionsAndSessions() {
        var db: OpaquePointer?
        let flags = SQLITE_OPEN_READONLY | SQLITE_OPEN_FULLMUTEX
        guard sqlite3_open_v2(sqlitePath.path, &db, flags, nil) == SQLITE_OK, let db else {
            lastRefreshError = String(
                localized: "ccx.error.sqliteOpen",
                defaultValue: "Cannot open state.sqlite (controller may not have written it yet)."
            )
            workExecutions = []
            agentSessions = []
            if let db { sqlite3_close(db) }
            return
        }
        defer { sqlite3_close(db) }
        lastRefreshError = nil
        workExecutions = queryWorkExecutions(db: db)
        agentSessions = queryAgentSessions(db: db)
    }

    private func queryWorkExecutions(db: OpaquePointer) -> [CCXWorkExecution] {
        let sql = """
        SELECT work_execution_id, project_id, state, branch_name, worktree_path,
               task_file_path, pr_number, pr_url, head_commit, display_text,
               selected_at, artifact_state, sync_status
        FROM work_executions
        WHERE project_id = ?
        ORDER BY COALESCE(selected_at, '') DESC
        LIMIT 200
        """
        return prepareAndStep(db: db, sql: sql, bind: projectId) { stmt in
            CCXWorkExecution(
                workExecutionId: Self.column(stmt, 0) ?? "",
                projectId: Self.column(stmt, 1) ?? "",
                state: Self.column(stmt, 2) ?? "",
                branchName: Self.column(stmt, 3),
                worktreePath: Self.column(stmt, 4),
                taskFilePath: Self.column(stmt, 5),
                prNumber: Self.intColumn(stmt, 6),
                prUrl: Self.column(stmt, 7),
                headCommit: Self.column(stmt, 8),
                displayText: Self.column(stmt, 9),
                selectedAt: Self.column(stmt, 10),
                artifactState: Self.column(stmt, 11),
                syncStatus: Self.column(stmt, 12)
            )
        }
    }

    private func queryAgentSessions(db: OpaquePointer) -> [CCXAgentSession] {
        let sql = """
        SELECT agent_session_id, project_id, work_execution_id, state, role,
               attach_mode, cmux_tab_id, tmux_session_id, pid, cwd,
               started_at, last_heartbeat_at, exit_code
        FROM agent_sessions
        WHERE project_id = ?
        ORDER BY COALESCE(started_at, '') DESC
        LIMIT 200
        """
        return prepareAndStep(db: db, sql: sql, bind: projectId) { stmt in
            CCXAgentSession(
                agentSessionId: Self.column(stmt, 0) ?? "",
                projectId: Self.column(stmt, 1) ?? "",
                workExecutionId: Self.column(stmt, 2),
                state: Self.column(stmt, 3) ?? "",
                role: Self.column(stmt, 4) ?? "",
                attachMode: Self.column(stmt, 5),
                cmuxTabId: Self.column(stmt, 6),
                tmuxSessionId: Self.column(stmt, 7),
                pid: Self.intColumn(stmt, 8),
                cwd: Self.column(stmt, 9),
                startedAt: Self.column(stmt, 10),
                lastHeartbeatAt: Self.column(stmt, 11),
                exitCode: Self.intColumn(stmt, 12)
            )
        }
    }

    private func prepareAndStep<T>(
        db: OpaquePointer,
        sql: String,
        bind: String,
        rowMap: (OpaquePointer) -> T
    ) -> [T] {
        var stmt: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK, let stmt else {
            if let stmt { sqlite3_finalize(stmt) }
            return []
        }
        defer { sqlite3_finalize(stmt) }
        sqlite3_bind_text(stmt, 1, bind, -1, SQLITE_TRANSIENT_DESTRUCTOR)
        var rows: [T] = []
        while sqlite3_step(stmt) == SQLITE_ROW {
            rows.append(rowMap(stmt))
        }
        return rows
    }

    private static func column(_ stmt: OpaquePointer, _ idx: Int32) -> String? {
        guard let cstr = sqlite3_column_text(stmt, idx) else { return nil }
        return String(cString: cstr)
    }

    private static func intColumn(_ stmt: OpaquePointer, _ idx: Int32) -> Int64? {
        if sqlite3_column_type(stmt, idx) == SQLITE_NULL { return nil }
        return sqlite3_column_int64(stmt, idx)
    }

    /// Tail the JSONL log and decode the most recent N entries. The audit log
    /// can grow large, so we cap by line count rather than memory size.
    private func loadRecentEvents() {
        guard let handle = try? FileHandle(forReadingFrom: eventsPath) else {
            recentEvents = []
            return
        }
        defer { try? handle.close() }
        guard let data = try? handle.readToEnd(), let text = String(data: data, encoding: .utf8) else {
            recentEvents = []
            return
        }
        let lines = text.split(separator: "\n").suffix(100)
        var entries: [CCXEventEntry] = []
        entries.reserveCapacity(lines.count)
        for line in lines {
            let raw = String(line)
            guard let data = raw.data(using: .utf8),
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                continue
            }
            entries.append(CCXEventEntry(
                eventId: (json["event_id"] as? String) ?? UUID().uuidString,
                projectId: (json["project_id"] as? String) ?? "",
                kind: (json["kind"] as? String) ?? "unknown",
                actor: (json["actor"] as? String) ?? "",
                timestamp: (json["timestamp"] as? String) ?? "",
                raw: raw
            ))
        }
        recentEvents = entries.reversed()
    }
}
