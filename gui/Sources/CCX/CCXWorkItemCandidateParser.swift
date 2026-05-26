import Foundation

struct CCXTaskSourceWorkItemCandidate: Identifiable, Hashable, Sendable {
    let id: String
    let selectorType: String
    let selectorValue: String
    let displayText: String
}

enum CCXWorkItemCandidateParser {
    static func parse(_ markdown: String) -> [CCXTaskSourceWorkItemCandidate] {
        var occurrenceCounts: [String: Int] = [:]
        var candidates: [CCXTaskSourceWorkItemCandidate] = []
        var lineNumber = 0
        markdown.enumerateLines { line, _ in
            lineNumber += 1
            let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
            if trimmed.hasPrefix("#") {
                let title = trimmed.drop { $0 == "#" }.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !title.isEmpty else { return }
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
                return
            }
            if trimmed.hasPrefix("- [ ]") || trimmed.hasPrefix("* [ ]") {
                let title = trimmed.dropFirst(5).trimmingCharacters(in: .whitespacesAndNewlines)
                guard !title.isEmpty else { return }
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
        "\(selectorType)-\(normalizedText(displayText))-\(occurrence)"
    }

    private static func normalizedText(_ text: String) -> String {
        text.lowercased().split(whereSeparator: { $0.isWhitespace }).joined(separator: " ")
    }
}
