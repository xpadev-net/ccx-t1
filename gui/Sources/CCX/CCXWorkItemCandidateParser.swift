import Foundation

enum CCXWorkItemCandidateParser {
    static func parse(_ markdown: String) -> [CCXTaskSourceWorkItemCandidate] {
        var occurrenceCounts: [String: Int] = [:]
        var candidates: [CCXTaskSourceWorkItemCandidate] = []
        for (index, rawLine) in markdown.split(separator: "\n", omittingEmptySubsequences: false).enumerated() {
            let lineNumber = index + 1
            let line = String(rawLine)
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed.hasPrefix("#") {
                let title = trimmed.drop { $0 == "#" }.trimmingCharacters(in: .whitespaces)
                guard !title.isEmpty else { continue }
                let occurrence = nextOccurrence(
                    selectorType: "heading",
                    displayText: title,
                    occurrenceCounts: &occurrenceCounts
                )
                candidates.append(CCXTaskSourceWorkItemCandidate(
                    id: stableId(selectorType: "heading", displayText: title, occurrence: occurrence),
                    selectorType: "heading",
                    selectorValue: "L\(lineNumber):\(title)",
                    displayText: title
                ))
                continue
            }
            if trimmed.hasPrefix("- [ ]") || trimmed.hasPrefix("* [ ]") {
                let title = trimmed.dropFirst(5).trimmingCharacters(in: .whitespaces)
                guard !title.isEmpty else { continue }
                let occurrence = nextOccurrence(
                    selectorType: "checkbox",
                    displayText: title,
                    occurrenceCounts: &occurrenceCounts
                )
                candidates.append(CCXTaskSourceWorkItemCandidate(
                    id: stableId(selectorType: "checkbox", displayText: title, occurrence: occurrence),
                    selectorType: "checkbox",
                    selectorValue: "L\(lineNumber):\(trimmed)",
                    displayText: title
                ))
            }
        }
        return candidates
    }

    private static func nextOccurrence(
        selectorType: String,
        displayText: String,
        occurrenceCounts: inout [String: Int]
    ) -> Int {
        let key = "\(selectorType):\(normalizedText(displayText))"
        let next = (occurrenceCounts[key] ?? 0) + 1
        occurrenceCounts[key] = next
        return next
    }

    private static func stableId(selectorType: String, displayText: String, occurrence: Int) -> String {
        "\(selectorType)-\(stableHash(normalizedText(displayText)))-\(occurrence)"
    }

    private static func normalizedText(_ text: String) -> String {
        text.lowercased().split(whereSeparator: { $0.isWhitespace }).joined(separator: " ")
    }

    private static func stableHash(_ text: String) -> String {
        var hash: UInt64 = 14_695_981_039_346_656_037
        for byte in text.utf8 {
            hash ^= UInt64(byte)
            hash &*= 1_099_511_628_211
        }
        return String(hash, radix: 16)
    }
}
