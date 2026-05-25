import Foundation
import os
import SwiftUI

private let ccxTaskSourceLogger = Logger(
    subsystem: "com.cmuxterm.ccx",
    category: "CCXTaskSource"
)

struct CCXTaskSourceFileStatus: Equatable, Sendable {
    enum Kind: Equatable, Sendable {
        case checking
        case missingPath
        case missingFile
        case directory
        case notMarkdown
        case ready
        case unreadable
    }

    let path: String
    let kind: Kind
    let checkedAt: Date
    let modifiedAt: Date?

    private init(path: String, kind: Kind, checkedAt: Date, modifiedAt: Date?) {
        self.path = path.trimmingCharacters(in: .whitespacesAndNewlines)
        self.kind = kind
        self.checkedAt = checkedAt
        self.modifiedAt = modifiedAt
    }

    static func checking(path: String, checkedAt: Date = Date()) -> CCXTaskSourceFileStatus {
        CCXTaskSourceFileStatus(path: path, kind: .checking, checkedAt: checkedAt, modifiedAt: nil)
    }

    init(
        path: String,
        fileManager: FileManager = .default,
        checkedAt: Date = Date()
    ) {
        let trimmedPath = path.trimmingCharacters(in: .whitespacesAndNewlines)
        self.path = trimmedPath
        self.checkedAt = checkedAt

        guard !trimmedPath.isEmpty else {
            self.kind = .missingPath
            self.modifiedAt = nil
            return
        }

        var isDirectory: ObjCBool = false
        guard fileManager.fileExists(atPath: trimmedPath, isDirectory: &isDirectory) else {
            self.kind = .missingFile
            self.modifiedAt = nil
            return
        }

        guard !isDirectory.boolValue else {
            self.kind = .directory
            self.modifiedAt = nil
            return
        }

        let markdownExtensions = ["md", "markdown"]
        guard markdownExtensions.contains(URL(fileURLWithPath: trimmedPath).pathExtension.lowercased()) else {
            self.kind = .notMarkdown
            self.modifiedAt = nil
            return
        }

        do {
            guard fileManager.isReadableFile(atPath: trimmedPath) else {
                throw CocoaError(.fileReadNoPermission)
            }
            let attributes = try fileManager.attributesOfItem(atPath: trimmedPath)
            self.kind = .ready
            self.modifiedAt = attributes[.modificationDate] as? Date
        } catch {
            ccxTaskSourceLogger.warning(
                "Could not read task source file at \(trimmedPath, privacy: .private): \(error.localizedDescription, privacy: .private)"
            )
            self.kind = .unreadable
            self.modifiedAt = nil
        }
    }

    var fileURL: URL? {
        hasPath ? URL(fileURLWithPath: path) : nil
    }

    var revealURL: URL? {
        fileURL
    }

    var hasPath: Bool {
        !path.isEmpty
    }

    var canOpen: Bool {
        kind == .ready
    }

    var existsOnDisk: Bool {
        switch kind {
        case .ready, .directory, .notMarkdown, .unreadable:
            return true
        case .checking, .missingPath, .missingFile:
            return false
        }
    }

    var isReady: Bool {
        kind == .ready
    }

    var displayPath: String {
        hasPath ? path : String(localized: "ccx.tasks.source.path.empty", defaultValue: "Not configured")
    }

    var badgeLabel: String {
        switch kind {
        case .checking:
            return String(localized: "ccx.tasks.status.checking", defaultValue: "Checking")
        case .ready:
            return String(localized: "ccx.tasks.status.ready", defaultValue: "Ready")
        case .missingPath:
            return String(localized: "ccx.tasks.status.notConfigured", defaultValue: "Not configured")
        default:
            return String(localized: "ccx.tasks.status.needsAttention", defaultValue: "Needs attention")
        }
    }

    var badgeTint: Color {
        switch kind {
        case .ready:
            return .green
        case .checking, .missingPath:
            return .secondary
        default:
            return .orange
        }
    }

    var message: String? {
        switch kind {
        case .checking:
            return String(localized: "ccx.tasks.message.checking",
                          defaultValue: "Checking the registered task source file.")
        case .ready:
            return String(localized: "ccx.tasks.message.ready",
                          defaultValue: "The registered Markdown task source is available.")
        case .missingPath:
            return String(localized: "ccx.tasks.message.missingPath",
                          defaultValue: "This project does not have a task source file configured.")
        case .missingFile:
            return String(localized: "ccx.tasks.message.missingFile",
                          defaultValue: "The configured task source file does not exist.")
        case .directory:
            return String(localized: "ccx.tasks.message.directory",
                          defaultValue: "The configured task source path points to a directory.")
        case .notMarkdown:
            return String(localized: "ccx.tasks.message.notMarkdown",
                          defaultValue: "The configured task source file is not a Markdown file.")
        case .unreadable:
            return String(localized: "ccx.tasks.message.unreadable",
                          defaultValue: "The task source file could not be read. Check file permissions and try again.")
        }
    }
}
