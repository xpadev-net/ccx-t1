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
///
/// All disk and SQLite I/O happens on a dedicated background queue; only the
/// final `@Published` writes hop back to the main actor. This keeps the main
/// thread responsive even when agents are writing into the project directory.
@MainActor
public final class CCXProjectStore: ObservableObject {
    @Published public private(set) var project: CCXProjectSummary?
    @Published public private(set) var workExecutions: [CCXWorkExecution] = []
    @Published public private(set) var agentSessions: [CCXAgentSession] = []
    @Published public private(set) var recentEvents: [CCXEventEntry] = []
    @Published public private(set) var lastRefreshError: String?

    public let projectId: String
    private let paths: Paths

    private var eventStream: FSEventStreamRef?
    private let refreshQueue = DispatchQueue(label: "ccx.project-store.refresh")
    private var isRefreshing = false
    private var refreshPending = false

    public init(projectId: String, ccxHome: URL? = nil) {
        self.projectId = projectId
        let home = ccxHome ?? Self.defaultCCXHome()
        let dir = home.appendingPathComponent("projects/\(projectId)", isDirectory: true)
        self.paths = Paths(
            projectDir: dir,
            sqlite: dir.appendingPathComponent("state.sqlite"),
            events: dir.appendingPathComponent("events.jsonl"),
            config: dir.appendingPathComponent("project.json")
        )
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

        let path = paths.projectDir.path as CFString
        let pathsArray = [path] as CFArray
        let latency: CFTimeInterval = 0.25
        guard let stream = FSEventStreamCreate(
            kCFAllocatorDefault,
            callback,
            context,
            pathsArray,
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

    /// Schedule a refresh on the background queue. If one is already running
    /// when called, we set `refreshPending` so a fresh snapshot fires exactly
    /// once after the in-flight load completes — coalescing the FSEvent burst
    /// that agents typically produce.
    public func refresh() {
        if isRefreshing {
            refreshPending = true
            return
        }
        isRefreshing = true
        let paths = self.paths
        let projectId = self.projectId
        refreshQueue.async {
            let snapshot = Snapshot.load(paths: paths, projectId: projectId)
            Task { @MainActor [weak self] in
                guard let self else { return }
                self.apply(snapshot: snapshot)
                self.isRefreshing = false
                if self.refreshPending {
                    self.refreshPending = false
                    self.refresh()
                }
            }
        }
    }

    private func apply(snapshot: Snapshot) {
        project = snapshot.project
        workExecutions = snapshot.workExecutions
        agentSessions = snapshot.agentSessions
        recentEvents = snapshot.recentEvents
        lastRefreshError = snapshot.lastRefreshError
    }
}

// MARK: - Background snapshot loader

extension CCXProjectStore {
    struct Paths: Sendable {
        let projectDir: URL
        let sqlite: URL
        let events: URL
        let config: URL
    }

    struct Snapshot: Sendable {
        var project: CCXProjectSummary?
        var workExecutions: [CCXWorkExecution]
        var agentSessions: [CCXAgentSession]
        var recentEvents: [CCXEventEntry]
        var lastRefreshError: String?

        static func load(paths: Paths, projectId: String) -> Snapshot {
            var snapshot = Snapshot(
                project: nil,
                workExecutions: [],
                agentSessions: [],
                recentEvents: [],
                lastRefreshError: nil
            )
            snapshot.project = Self.loadProjectConfig(at: paths.config, fallbackId: projectId)
            let (we, sessions, sqliteError) = Self.loadSqlite(at: paths.sqlite, projectId: projectId)
            snapshot.workExecutions = we
            snapshot.agentSessions = sessions
            snapshot.lastRefreshError = sqliteError
            snapshot.recentEvents = Self.loadRecentEvents(at: paths.events)
            return snapshot
        }

        private static func loadProjectConfig(at url: URL, fallbackId: String) -> CCXProjectSummary? {
            guard let data = try? Data(contentsOf: url),
                  let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                return nil
            }
            return CCXProjectSummary(
                projectId: (json["project_id"] as? String) ?? fallbackId,
                displaySlug: (json["display_slug"] as? String) ?? "",
                canonicalRepo: (json["canonical_repo"] as? String) ?? "",
                taskSourceFile: (json["task_source_file"] as? String) ?? "",
                createdAt: (json["created_at"] as? String) ?? ""
            )
        }

        private static func loadSqlite(
            at url: URL,
            projectId: String
        ) -> ([CCXWorkExecution], [CCXAgentSession], String?) {
            var db: OpaquePointer?
            let flags = SQLITE_OPEN_READONLY | SQLITE_OPEN_FULLMUTEX
            guard sqlite3_open_v2(url.path, &db, flags, nil) == SQLITE_OK, let db else {
                if let db { sqlite3_close(db) }
                let message = String(
                    localized: "ccx.error.sqliteOpen",
                    defaultValue: "CCX controller data is not yet available — the controller may still be starting up. Please wait a moment and try again."
                )
                return ([], [], message)
            }
            defer { sqlite3_close(db) }
            let we = queryWorkExecutions(db: db, projectId: projectId)
            let sessions = queryAgentSessions(db: db, projectId: projectId)
            return (we, sessions, nil)
        }

        private static func queryWorkExecutions(db: OpaquePointer, projectId: String) -> [CCXWorkExecution] {
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
                    workExecutionId: column(stmt, 0) ?? "",
                    projectId: column(stmt, 1) ?? "",
                    state: column(stmt, 2) ?? "",
                    branchName: column(stmt, 3),
                    worktreePath: column(stmt, 4),
                    taskFilePath: column(stmt, 5),
                    prNumber: intColumn(stmt, 6),
                    prUrl: column(stmt, 7),
                    headCommit: column(stmt, 8),
                    displayText: column(stmt, 9),
                    selectedAt: column(stmt, 10),
                    artifactState: column(stmt, 11),
                    syncStatus: column(stmt, 12)
                )
            }
        }

        private static func queryAgentSessions(db: OpaquePointer, projectId: String) -> [CCXAgentSession] {
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
                    agentSessionId: column(stmt, 0) ?? "",
                    projectId: column(stmt, 1) ?? "",
                    workExecutionId: column(stmt, 2),
                    state: column(stmt, 3) ?? "",
                    role: column(stmt, 4) ?? "",
                    attachMode: column(stmt, 5),
                    cmuxTabId: column(stmt, 6),
                    tmuxSessionId: column(stmt, 7),
                    pid: intColumn(stmt, 8),
                    cwd: column(stmt, 9),
                    startedAt: column(stmt, 10),
                    lastHeartbeatAt: column(stmt, 11),
                    exitCode: intColumn(stmt, 12)
                )
            }
        }

        private static func prepareAndStep<T>(
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

        /// Tail the JSONL log and decode the most recent N entries.
        /// `events.jsonl` is append-only and can grow to hundreds of megabytes
        /// in long-running projects; we walk backwards in 64 KiB chunks until
        /// we have enough newlines to slice out the last N lines, so memory
        /// stays bounded regardless of file size.
        private static let recentEventLineLimit = 100
        private static let tailReadChunkSize = 64 * 1024

        private static func loadRecentEvents(at url: URL) -> [CCXEventEntry] {
            guard let handle = try? FileHandle(forReadingFrom: url) else {
                return []
            }
            defer { try? handle.close() }

            var fileLength: UInt64 = 0
            do {
                fileLength = try handle.seekToEnd()
            } catch {
                return []
            }
            if fileLength == 0 { return [] }

            var buffer = Data()
            var offset = fileLength
            var newlineCount = 0
            let targetNewlines = recentEventLineLimit + 1  // need one extra to bound the first line

            while offset > 0 && newlineCount < targetNewlines {
                let chunk = UInt64(tailReadChunkSize)
                let readLen = min(chunk, offset)
                let newOffset = offset - readLen
                do {
                    try handle.seek(toOffset: newOffset)
                } catch {
                    break
                }
                let part = (try? handle.read(upToCount: Int(readLen))) ?? Data()
                buffer = part + buffer
                offset = newOffset
                newlineCount = buffer.reduce(0) { $0 + ($1 == 0x0A ? 1 : 0) }
            }

            guard let text = String(data: buffer, encoding: .utf8) else { return [] }
            let lines = text.split(separator: "\n", omittingEmptySubsequences: true).suffix(recentEventLineLimit)
            var entries: [CCXEventEntry] = []
            entries.reserveCapacity(lines.count)
            for line in lines {
                let raw = String(line)
                guard let data = raw.data(using: .utf8),
                      let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                    continue
                }
                let payload = json["payload"] as? [String: Any]
                entries.append(CCXEventEntry(
                    eventId: (json["event_id"] as? String) ?? UUID().uuidString,
                    projectId: (json["project_id"] as? String) ?? "",
                    kind: (json["event_type"] as? String) ?? (json["kind"] as? String) ?? "unknown",
                    actor: (json["actor"] as? String) ?? "",
                    timestamp: (json["occurred_at"] as? String) ?? (json["timestamp"] as? String) ?? "",
                    taskSourceFile: (payload?["task_source_file"] as? String) ?? (json["task_source_file"] as? String),
                    newHash: (payload?["new_hash"] as? String) ?? (json["new_hash"] as? String),
                    raw: raw
                ))
            }
            return entries.reversed()
        }
    }
}
