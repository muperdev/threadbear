import Foundation

/// Links and pictures that point at a file on this Mac by its path.
public enum PathLinks {
    /// The markdown with the spaces in `/…` and `~/…` destinations escaped.
    /// Agents write `![Before](/Users/me/Library/Application Support/x.png)`
    /// as it is, and a space ends a destination for the parser, so the
    /// picture showed as raw `![Before](…)` text. Every thread's media is
    /// under Application Support, so most saved shots hit it. A destination
    /// with a title (`"…"`), web addresses, code spans and fenced code stay
    /// as written.
    public static func escapeDestinations(_ markdown: String) -> String {
        guard markdown.contains("](/") || markdown.contains("](~/") else { return markdown }
        let start = markdown.drop { $0 == " " }
        if start.hasPrefix("```") || start.hasPrefix("~~~") { return markdown }
        let ns = markdown as NSString
        let whole = NSRange(location: 0, length: ns.length)
        let codeSpans = codeSpan.matches(in: markdown, range: whole).map(\.range)
        var out = ""
        var last = 0
        for match in destination.matches(in: markdown, range: whole) {
            // No `)` to end it: not a link, and the rest of its line is used up.
            guard match.range(at: 2).location != NSNotFound else { continue }
            let body = match.range(at: 1)
            let raw = ns.substring(with: body)
            guard raw.contains(" "), !raw.contains("\""),
                  !codeSpans.contains(where: { NSIntersectionRange($0, match.range).length > 0 }) else { continue }
            out += ns.substring(with: NSRange(location: last, length: body.location - last))
            out += raw.replacingOccurrences(of: " ", with: "%20")
            last = body.location + body.length
        }
        return out + ns.substring(from: last)
    }

    // One level of brackets inside, for `Screenshot (2).png`. The `)` is
    // optional so that one with none uses up the rest of its line, rather
    // than each `](/` after it scanning to the end again (WorkLinks).
    nonisolated(unsafe) private static let destination = try! NSRegularExpression(pattern: #"\]\((~?/(?:[^()\n]|\([^()\n]*\))*)(\))?"#)
    nonisolated(unsafe) private static let codeSpan = try! NSRegularExpression(pattern: #"`+[^`\n]*`+"#)
}
