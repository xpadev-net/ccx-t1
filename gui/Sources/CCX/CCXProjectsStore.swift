import Foundation
import Observation

private final class CCXProjectsStoreEventCallbackBox: @unchecked Sendable {
    let handleEvent: @Sendable () -> Void

    init(handleEvent: @escaping @Sendable () -> Void) {
        self.handleEvent = handleEvent
    }
}

private let ccxProjectsStoreEventRetain: CFAllocatorRetainCallBack = { info in
    guard let info else { return nil }
    _ = Unmanaged<CCXProjectsStoreEventCallbackBox>.fromOpaque(info).retain()
    return info
}

private let ccxProjectsStoreEventRelease: CFAllocatorReleaseCallBack = { info in
    guard let info else { return }
    Unmanaged<CCXProjectsStoreEventCallbackBox>.fromOpaque(info).release()
}

/// Reads the global CCX project index from `$CCX_HOME/projects.json`.
///
/// This store is read-only. Project mutations must go through the controller
/// CLI so the Rust side remains the event-sourcing owner.
@MainActor
@Observable
public final class CCXProjectsStore {
    public private(set) var projects: [CCXProjectSummary] = []
    public private(set) var lastRefreshError: String?

    @ObservationIgnored
    private let paths: Paths

    @ObservationIgnored
    private var eventStream: FSEventStreamRef?
    @ObservationIgnored
    private let eventQueue = DispatchQueue(label: "ccx.projects-store.events")
    @ObservationIgnored
    private var refreshTask: Task<Void, Never>?
    @ObservationIgnored
    private var refreshPending = false

    public init(ccxHome: URL? = nil) {
        let home = ccxHome ?? CCXProjectStore.defaultCCXHome()
        self.paths = Paths(
            ccxHome: home,
            index: home.appendingPathComponent("projects.json")
        )
    }

    nonisolated deinit {
        refreshTask?.cancel()
        if let stream = eventStream {
            FSEventStreamStop(stream)
            FSEventStreamInvalidate(stream)
            FSEventStreamRelease(stream)
        }
    }

    public func start() {
        refresh()
        startWatching()
    }

    public func refresh() {
        if refreshTask != nil {
            refreshPending = true
            return
        }
        let paths = self.paths
        let loadTask = Task.detached(priority: .utility) {
            Snapshot.load(paths: paths)
        }
        refreshTask = Task { [weak self] in
            let snapshot = await withTaskCancellationHandler {
                await loadTask.value
            } onCancel: {
                loadTask.cancel()
            }
            guard let self else { return }
            guard !Task.isCancelled else {
                self.refreshTask = nil
                return
            }
            self.apply(snapshot: snapshot)
            self.refreshTask = nil
            if self.refreshPending {
                self.refreshPending = false
                self.refresh()
            }
        }
    }

    private func startWatching() {
        guard eventStream == nil else { return }
        let callbackBox = CCXProjectsStoreEventCallbackBox { [weak self] in
            Task { @MainActor in self?.refresh() }
        }
        let context = UnsafeMutablePointer<FSEventStreamContext>.allocate(capacity: 1)
        context.pointee = FSEventStreamContext(
            version: 0,
            info: Unmanaged.passUnretained(callbackBox).toOpaque(),
            retain: ccxProjectsStoreEventRetain,
            release: ccxProjectsStoreEventRelease,
            copyDescription: nil
        )
        defer { context.deallocate() }

        let callback: FSEventStreamCallback = { _, info, _, _, _, _ in
            guard let info = info else { return }
            let box = Unmanaged<CCXProjectsStoreEventCallbackBox>.fromOpaque(info).takeUnretainedValue()
            box.handleEvent()
        }

        let pathsArray = [paths.ccxHome.path as CFString] as CFArray
        guard let stream = FSEventStreamCreate(
            kCFAllocatorDefault,
            callback,
            context,
            pathsArray,
            FSEventStreamEventId(kFSEventStreamEventIdSinceNow),
            0.25,
            FSEventStreamCreateFlags(kFSEventStreamCreateFlagFileEvents)
        ) else {
            return
        }
        FSEventStreamSetDispatchQueue(stream, eventQueue)
        guard FSEventStreamStart(stream) else {
            FSEventStreamInvalidate(stream)
            FSEventStreamRelease(stream)
            return
        }
        eventStream = stream
    }

    private func apply(snapshot: Snapshot) {
        projects = snapshot.projects
        lastRefreshError = snapshot.lastRefreshError
    }
}

extension CCXProjectsStore {
    struct Paths: Sendable {
        let ccxHome: URL
        let index: URL
    }

    struct Snapshot: Sendable {
        var projects: [CCXProjectSummary]
        var lastRefreshError: String?

        static func load(paths: Paths) -> Snapshot {
            do {
                let entries = try loadIndex(at: paths.index)
                return Snapshot(
                    projects: entries.map { entry in summary(for: entry, ccxHome: paths.ccxHome) },
                    lastRefreshError: nil
                )
            } catch CocoaError.fileReadNoSuchFile {
                return Snapshot(projects: [], lastRefreshError: nil)
            } catch {
                return Snapshot(
                    projects: [],
                    lastRefreshError: String(
                        localized: "ccx.projects.error.loadIndex",
                        defaultValue: "Could not load CCX projects."
                    )
                )
            }
        }

        private static func loadIndex(at url: URL) throws -> [ProjectIndexEntry] {
            let data = try Data(contentsOf: url)
            return try JSONDecoder().decode([ProjectIndexEntry].self, from: data)
        }

        private static func summary(for entry: ProjectIndexEntry, ccxHome: URL) -> CCXProjectSummary {
            let config = ccxHome
                .appendingPathComponent("projects", isDirectory: true)
                .appendingPathComponent(entry.projectId, isDirectory: true)
                .appendingPathComponent("project.json")
            if let data = try? Data(contentsOf: config),
               let summary = try? JSONDecoder().decode(CCXProjectSummary.self, from: data) {
                return summary
            }
            return CCXProjectSummary(
                projectId: entry.projectId,
                displaySlug: entry.displaySlug,
                canonicalRepo: entry.canonicalRepo,
                taskSourceFile: "",
                createdAt: ""
            )
        }
    }

    private struct ProjectIndexEntry: Decodable, Sendable {
        let projectId: String
        let displaySlug: String
        let canonicalRepo: String

        private enum CodingKeys: String, CodingKey {
            case projectId = "project_id"
            case displaySlug = "display_slug"
            case canonicalRepo = "canonical_repo"
        }
    }
}
