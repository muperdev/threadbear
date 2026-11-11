import Foundation

/// Links and pictures that point at a file on this Mac by its path.
public enum PathLinks {
    /// The markdown with the spaces in `/…` and `~/…` destinations escaped.
    /// Agents write `![Before](/Users/me/Library/Application Support/x.png)`
    /// as it is, and a space ends a destination for the parser, so the
    /// picture showed as raw `![Before](…)` text. Every thread's media is
    /// under Application Support, so most saved shots hit it. A destination
    /// with a title (`"…"`, `'…'` or `(…)` after a space), web addresses,
    /// code spans and fenced code (in a list item or a quote too) stay as
    /// written.
    ///
    /// It reads markdown only as far as finding those, so a few rare forms
    /// still get their spaces escaped, as `work:` links' do (WorkLinks): a
    /// path in indented code, a `)` inside a quoted title, and `](/a b)`
    /// that isn't a link (`\[x](/a b)`). The cross-lab review of 10 Nov 2026
    /// found these, and they were left as not worth a markdown parser here.
    public static func escapeDestinations(_ markdown: String) -> String {
        guard markdown.contains("](/") || markdown.contains("](~/") else { return markdown }
        let ns = markdown as NSString
        let whole = NSRange(location: 0, length: ns.length)
        let units = Array(markdown.utf16)
        // Sorted by where they start, and walked once alongside the links, so
        // a block of many code spans and links stays linear (cross-lab
        // review, 10 Nov 2026: checking every span for every link was
        // 400 million checks for 20,000 of each, before the renderer's
        // limits see the block). Everything here is a plain scan rather than
        // a pattern that could retry from every space in a long run.
        let code = (fences(units) + codeSpans(units)).sorted { $0.location < $1.location }
        var next = 0
        var out = ""
        var last = 0
        for match in destination.matches(in: markdown, range: whole) {
            // No `)` to end it: not a link, and the rest of its line is used up.
            guard match.range(at: 2).location != NSNotFound else { continue }
            // Code that ends before this link can't reach a later one either.
            while next < code.count, NSMaxRange(code[next]) <= match.range.location { next += 1 }
            if next < code.count, code[next].location < NSMaxRange(match.range) { continue }
            let body = match.range(at: 1)
            // Spaces before the `)` are allowed and aren't part of the path.
            var end = NSMaxRange(body)
            while end > body.location, units[end - 1] == space || units[end - 1] == tab { end -= 1 }
            let path = Array(units[body.location..<end])
            guard path.contains(space), !path.contains(doubleQuote), !hasTitle(path) else { continue }
            out += ns.substring(with: NSRange(location: last, length: body.location - last))
            out += String(utf16CodeUnits: path, count: path.count).replacingOccurrences(of: " ", with: "%20")
            last = end
        }
        return out + ns.substring(from: last)
    }

    private static let space = UInt16(UInt8(ascii: " ")), tab = UInt16(UInt8(ascii: "\t"))
    private static let doubleQuote = UInt16(UInt8(ascii: "\"")), quote = UInt16(UInt8(ascii: "'"))
    private static let backslash = UInt16(UInt8(ascii: "\\")), newline = UInt16(UInt8(ascii: "\n"))
    private static let openBracket = UInt16(UInt8(ascii: "(")), closeBracket = UInt16(UInt8(ascii: ")"))
    private static let backtick = UInt16(UInt8(ascii: "`")), tilde = UInt16(UInt8(ascii: "~"))
    private static let greaterThan = UInt16(UInt8(ascii: ">")), dash = UInt16(UInt8(ascii: "-"))
    private static let star = UInt16(UInt8(ascii: "*")), plus = UInt16(UInt8(ascii: "+")), dot = UInt16(UInt8(ascii: "."))
    private static let zero = UInt16(UInt8(ascii: "0")), nine = UInt16(UInt8(ascii: "9"))

    /// Whether a destination (trailing spaces gone) ends with a link title,
    /// which a space sets off: `/a.md 'Bob\'s plan'` or `/a.md (Read this)`
    /// (a `"` title is never escaped anyway). A path ending `(2)` with no
    /// extension reads as one too, and is left as written, as it was before
    /// spaces were escaped at all.
    static func hasTitle(_ d: [UInt16]) -> Bool {
        func escaped(_ i: Int) -> Bool {
            var n = 0
            while i - n - 1 >= 0, d[i - n - 1] == backslash { n += 1 }
            return n % 2 == 1
        }
        guard let last = d.last, last == quote || last == closeBracket, !escaped(d.count - 1) else { return false }
        let open = last == quote ? quote : openBracket
        var i = d.count - 2
        while i >= 0 {
            if d[i] == open, !escaped(i) { break }
            // A bracketed title can't hold an unescaped bracket of its own.
            if open == openBracket, d[i] == closeBracket, !escaped(i) { return false }
            i -= 1
        }
        guard i >= 1 else { return false }
        return d[i - 1] == space || d[i - 1] == tab
    }

    /// Where fenced code is: from a line starting (after any indent) with
    /// three or more backticks or tildes to a line of at least as many of
    /// the same and nothing else, or to the end.
    static func fences(_ u: [UInt16]) -> [NSRange] {
        var ranges: [NSRange] = []
        var open: (start: Int, mark: UInt16, count: Int)?
        var lineStart = 0
        while lineStart < u.count {
            var lineEnd = lineStart
            while lineEnd < u.count, u[lineEnd] != newline { lineEnd += 1 }
            var i = lineStart
            // Past the indent, and any quote marks and list markers before
            // the fence (`> ~~~`, `- ~~~`, `1. ~~~`).
            while true {
                while i < lineEnd, u[i] == space || u[i] == tab { i += 1 }
                if i < lineEnd, u[i] == greaterThan { i += 1; continue }
                if i + 1 < lineEnd, u[i] == dash || u[i] == star || u[i] == plus, u[i + 1] == space { i += 2; continue }
                var d = i
                while d < lineEnd, u[d] >= zero, u[d] <= nine { d += 1 }
                if d > i, d + 1 < lineEnd, u[d] == dot || u[d] == closeBracket, u[d + 1] == space { i = d + 2; continue }
                break
            }
            if i < lineEnd, u[i] == backtick || u[i] == tilde {
                let mark = u[i]
                var run = i
                while run < lineEnd, u[run] == mark { run += 1 }
                let count = run - i
                if count >= 3 {
                    if let o = open {
                        var rest = run
                        while rest < lineEnd, u[rest] == space || u[rest] == tab { rest += 1 }
                        if mark == o.mark, count >= o.count, rest == lineEnd {
                            ranges.append(NSRange(location: o.start, length: min(lineEnd + 1, u.count) - o.start))
                            open = nil
                        }
                    } else {
                        open = (lineStart, mark, count)
                    }
                }
            }
            lineStart = lineEnd + 1
        }
        if let o = open { ranges.append(NSRange(location: o.start, length: u.count - o.start)) }
        return ranges
    }

    /// Inline code: a run of backticks to the next run of exactly as many.
    /// A run with no partner is plain text. Each length keeps a pointer that
    /// only moves forward, so the scan stays linear.
    static func codeSpans(_ u: [UInt16]) -> [NSRange] {
        var runs: [(start: Int, count: Int)] = []
        var i = 0
        while i < u.count {
            guard u[i] == backtick else { i += 1; continue }
            var j = i
            while j < u.count, u[j] == backtick { j += 1 }
            // A backslash before the run makes its first backtick plain text.
            var slashes = 0
            while i - slashes - 1 >= 0, u[i - slashes - 1] == backslash { slashes += 1 }
            let start = slashes % 2 == 1 ? i + 1 : i
            if j > start { runs.append((start, j - start)) }
            i = j
        }
        var byLength: [Int: [Int]] = [:]
        for (k, r) in runs.enumerated() { byLength[r.count, default: []].append(k) }
        var cursor: [Int: Int] = [:]
        var spans: [NSRange] = []
        var k = 0
        while k < runs.count {
            let n = runs[k].count
            let list = byLength[n]!
            var c = cursor[n] ?? 0
            while c < list.count, list[c] <= k { c += 1 }
            cursor[n] = c
            guard c < list.count else { k += 1; continue }
            let close = runs[list[c]]
            spans.append(NSRange(location: runs[k].start, length: close.start + close.count - runs[k].start))
            k = list[c] + 1
        }
        return spans
    }

    // One level of brackets inside, for `Screenshot (2).png`. The `)` is
    // optional so that one with none uses up the rest of its line, rather
    // than each `](/` after it scanning to the end again (WorkLinks).
    nonisolated(unsafe) private static let destination = try! NSRegularExpression(pattern: #"\]\((~?/(?:[^()\n]|\([^()\n]*\))*)(\))?"#)
}
