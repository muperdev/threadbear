import Foundation

/// A thread's messages on the phone (backend API 2), as far back as you've
/// scrolled: the newest ones live from phone:messages, and older chunks asked
/// for once each with beforeSeq.
///
/// The live page stays the size it opened with. It used to grow by raising
/// its limit as you scrolled back, up to 2,000 rows, and a live query reads
/// its whole page again for every new message: a scrolled-back phone on the
/// launch thread cost 37.6 MB of Convex I/O in a day (8 Nov), more than a
/// free plan's daily share. Older rows come in chunks from where the shown
/// ones begin, read once, since past messages don't change.
public struct MessageHistory: Sendable, Equatable {
    /// The most rows one phone:messages query returns (PAGE_MAX in convex/phone.ts).
    public static let liveMax = 2000

    /// How much more than you've loaded with "Load earlier" `older` keeps,
    /// in rows, pictures and bytes (about: its text and the pictures'
    /// addresses, see `bytes(of:)`), the same as one page of phone:messages
    /// can be at most (PAGE_MAX, PAGE_BYTES). Rows sliding off the live
    /// page used to be kept for as long as the thread was open, so another
    /// runner writing to its own thread a few rows at a time grew the
    /// phone's memory without end, all of it drawn (Daybreak, 10 Nov). Now
    /// what slides off by itself fits in this, past which the oldest rows
    /// go (and can be loaded again). Each chunk you load raises the
    /// allowance by the size of what it brings that's older than anything
    /// this view has held, up to `loadedMax` times the caps, so going back
    /// is never cut off short of that, and memory only grows as far back
    /// as you go. Rows let go and read again earn nothing: the phone loads
    /// a chunk by itself when Load earlier scrolls into view, and a runner
    /// keeping that row in view had it fetch the rows the cap let go, over
    /// and over, each time adding room (Codex's review).
    public static let olderMaxRows = 2000
    public static let olderMaxPictures = 2000
    public static let olderMaxBytes = 8 * 1024 * 1024
    public static let loadedMax = 10

    /// The newest messages, live.
    public private(set) var live: MessagePage?
    /// Older messages from chunks, oldest first, all before `live`'s.
    public private(set) var older: [PhoneMessage] = []
    public private(set) var olderImages: [MessageImage] = []
    /// Whether there's more before `older`; nil until a chunk has come.
    public private(set) var olderHasMore: Bool?
    /// Pictures from phone:images (API 37) from `older`'s first row on: one
    /// that finishes uploading after its row was read (into a chunk, or off
    /// the live page's top) comes only from there.
    public private(set) var lateImages: [MessageImage] = []
    /// What chunks you've loaded added, on top of the caps above.
    private var loadedRows = 0, loadedPictures = 0, loadedBytes = 0
    /// The lowest seq this view has held: only rows below it earn room.
    private var lowest: Int?
    /// A chunk brought nothing that stayed (it was all let go at once, or
    /// empty), so no more are read: the phone loads by itself as Load
    /// earlier comes into view, and would fetch the same rows forever.
    public private(set) var stuck = false

    public init() {}

    public var messages: [PhoneMessage] { older + (live?.messages ?? []) }

    /// The pictures of every row shown, each once.
    public var images: [MessageImage] {
        var seen = Set<String>()
        let shown = Set(messages.map(\.seq))
        return (olderImages + (live?.images ?? []) + lateImages.filter { shown.contains($0.seq) }).filter { seen.insert($0.url).inserted }
    }

    public var hasMore: Bool { !stuck && (olderHasMore ?? (live?.hasMore ?? false)) }

    public var firstSeq: Int? { older.first?.seq ?? live?.messages.first?.seq }

    /// A new value of the live query. Rows that slide off its top as new ones
    /// come in move to `older`, or there'd be a hole above the live ones.
    public mutating func takeLive(_ page: MessagePage) {
        if let old = live, let first = page.messages.first?.seq {
            if let last = old.messages.last?.seq, last < first {
                // Nothing in common with the page before (the phone was away
                // while a whole page of new messages came): what's between
                // them is unknown, so start again from the newest rather
                // than show a hole.
                self = MessageHistory()
            } else {
                // Its top can also move back up (big rows above the byte cap
                // that got smaller): those rows are the live page's again.
                older.removeAll { $0.seq >= first }
                olderImages.removeAll { $0.seq >= first }
                let after = older.last?.seq ?? Int.min
                older += old.messages.filter { $0.seq < first && $0.seq > after }
                olderImages += Self.unseen(old.images.filter { $0.seq < first && $0.seq > after }, in: olderImages)
                capOlder()
            }
        }
        live = page
        if let first = page.messages.first?.seq { lowest = min(lowest ?? first, first) }
    }

    /// About how much a row holds: its text and the other words on it,
    /// plus a little for the rest. Counted from lengths Swift already
    /// knows, so it costs nothing per update.
    static func bytes(of m: PhoneMessage) -> Int {
        var n = 64 + m.text.utf8.count + (m.origin?.utf8.count ?? 0) + (m.toolKind?.utf8.count ?? 0) + (m.turn?.utf8.count ?? 0)
        n += (m.buttons?.count ?? 0) * 256 + (m.wokeBy?.count ?? 0) * 256
        return n
    }

    static func bytes(of p: MessageImage) -> Int { 64 + p.url.utf8.count + (p.src?.utf8.count ?? 0) }

    /// Keeps one of each picture, then lets the oldest of `older` go until
    /// it's within its allowance, with their pictures. Walks from the
    /// newest, so the rows nearest the live page are the ones kept. A
    /// picture counts with the row it goes after (the nearest at or before
    /// its seq), and one before every row (or with no rows kept at all)
    /// counts on its own, so no picture goes uncounted. Pictures came again
    /// with every live page before (Codex's review): one at a seq below
    /// the first row, with no older rows, was added once per update.
    private mutating func capOlder() {
        let maxRows = Self.olderMaxRows + loadedRows, maxPictures = Self.olderMaxPictures + loadedPictures
        let maxBytes = Self.olderMaxBytes + loadedBytes
        // Indexes into olderImages, newest first, so what's kept is marked
        // without hashing any picture's address.
        let order = olderImages.indices.sorted { olderImages[$0].seq > olderImages[$1].seq }
        let pictures = order.map { olderImages[$0] }
        var kept = 0, pictureCount = 0, total = 0
        var keepFrom = older.count
        while keepFrom > 0 {
            let m = older[keepFrom - 1]
            var size = Self.bytes(of: m), n = 0
            while kept + n < pictures.count, pictures[kept + n].seq >= m.seq {
                size += Self.bytes(of: pictures[kept + n])
                n += 1
            }
            if older.count - keepFrom + 1 > maxRows || pictureCount + n > maxPictures || total + size > maxBytes { break }
            total += size
            pictureCount += n
            kept += n
            keepFrom -= 1
        }
        if keepFrom == 0 {
            while kept < pictures.count, pictureCount < maxPictures, total + Self.bytes(of: pictures[kept]) <= maxBytes {
                total += Self.bytes(of: pictures[kept])
                pictureCount += 1
                kept += 1
            }
        }
        guard keepFrom > 0 || kept < pictures.count else { return }
        if keepFrom > 0 {
            older.removeFirst(keepFrom)
            let first = older.first?.seq ?? Int.max
            lateImages.removeAll { $0.seq < first }
            olderHasMore = true
        }
        var keep = [Bool](repeating: false, count: olderImages.count)
        for i in order[..<kept] { keep[i] = true }
        olderImages = olderImages.indices.filter { keep[$0] }.map { olderImages[$0] }
    }

    /// The pictures of `new` that `existing` doesn't have already. Pictures
    /// came again with every live page (Codex's review): one at a seq below
    /// the first row, with no older rows, was added once per update.
    /// Addresses are only compared between pictures at the same seq.
    static func unseen(_ new: [MessageImage], in existing: [MessageImage]) -> [MessageImage] {
        guard !new.isEmpty else { return [] }
        var bySeq: [Int: [String]] = [:]
        let seqs = Set(new.map(\.seq))
        for p in existing where seqs.contains(p.seq) { bySeq[p.seq, default: []].append(p.url) }
        var out: [MessageImage] = []
        for p in new where !(bySeq[p.seq]?.contains(p.url) ?? false) {
            out.append(p)
            bySeq[p.seq, default: []].append(p.url)
        }
        return out
    }

    /// Where phone:images should watch from: the first row read once, or
    /// nil while the live page is all there is.
    public var lateImagesFrom: Int? { older.first?.seq }

    public mutating func takeLateImages(_ images: [MessageImage]) { lateImages = images }

    /// The seq to ask for an older chunk before, or nil when there's no more.
    public var nextBefore: Int? { hasMore ? firstSeq : nil }

    /// A chunk of rows before `seq`. False, and nothing changes, if the
    /// shown rows no longer start there (it came too late).
    @discardableResult
    public mutating func takeOlder(_ chunk: MessagePage, before seq: Int) -> Bool {
        guard seq == firstSeq else { return false }
        let rows = chunk.messages.filter { $0.seq < seq }, pictures = chunk.images.filter { $0.seq < seq }
        // What's older than anything held before is room on top of the
        // caps, up to loadedMax times them; rows read again earn nothing.
        let floor = lowest ?? seq
        let fresh = rows.filter { $0.seq < floor }, freshPictures = pictures.filter { $0.seq < floor }
        loadedRows = min(loadedRows + fresh.count, Self.olderMaxRows * Self.loadedMax)
        loadedPictures = min(loadedPictures + freshPictures.count, Self.olderMaxPictures * Self.loadedMax)
        let freshBytes = fresh.reduce(0) { $0 + Self.bytes(of: $1) } + freshPictures.reduce(0) { $0 + Self.bytes(of: $1) }
        loadedBytes = min(loadedBytes + freshBytes, Self.olderMaxBytes * Self.loadedMax)
        if let low = rows.first?.seq { lowest = min(floor, low) }
        older = rows + older
        olderImages = Self.unseen(pictures, in: olderImages) + olderImages
        olderHasMore = chunk.hasMore
        capOlder()
        // Nothing it brought stayed: stop, rather than read it again.
        if (firstSeq ?? seq) >= seq { stuck = true }
        return true
    }
}
