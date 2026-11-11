import Foundation
import Testing
@testable import ThreadbearKit

@Suite struct MessageHistoryTests {
    let t0 = Date(timeIntervalSince1970: 1_800_000_000)
    func page(_ seqs: ClosedRange<Int>, hasMore: Bool, images: [Int] = []) -> MessagePage {
        MessagePage(messages: seqs.map { PhoneMessage(seq: $0, role: $0 % 2 == 0 ? .assistant : .user, text: "m\($0)", at: t0) },
                    hasMore: hasMore,
                    images: images.map { MessageImage(seq: $0, url: "https://x/\($0).png") })
    }

    /// The live page never grows: "Load earlier" reads a chunk before it,
    /// once, rather than asking the live query for more rows to read again
    /// on every new message (37.6 MB of Convex I/O on 8 Nov).
    @Test func loadEarlierAsksForAChunkNotABiggerLivePage() {
        var h = MessageHistory()
        #expect(h.nextBefore == nil)
        h.takeLive(page(921...1000, hasMore: true))
        #expect(h.nextBefore == 921)
        var all = MessageHistory()
        all.takeLive(page(1...80, hasMore: false))
        #expect(all.nextBefore == nil)
    }

    /// Going back is never cut off by the cap: what you load is room on
    /// top of it (Codex's review: a cap that stopped Load earlier left a
    /// long thread's start out of reach for good).
    @Test func chunksGoAboveTheLivePageUntilTheStart() {
        var h = MessageHistory()
        h.takeLive(page(7421...7500, hasMore: true))
        let took1 = h.takeOlder(page(7221...7420, hasMore: true, images: [7310]), before: 7421)
        #expect(took1)
        #expect(h.messages.map(\.seq) == Array(7221...7500))
        #expect(h.nextBefore == 7221)
        let took2 = h.takeOlder(page(1...7220, hasMore: false), before: 7221)
        #expect(took2)
        #expect(h.messages.count == 7500)
        #expect(!h.hasMore)
        #expect(h.nextBefore == nil)
        #expect(h.images.map(\.seq) == [7310])
    }

    /// After loading, rows sliding off the live page still only get the
    /// cap on top of what was loaded; the oldest go, and can be loaded again.
    @Test func aStreamAfterLoadingEarlierKeepsWithinWhatWasLoadedPlusTheCap() {
        var h = MessageHistory()
        h.takeLive(page(3001...3080, hasMore: true))
        _ = h.takeOlder(page(2001...3000, hasMore: true), before: 3001)
        for i in 1...400 { h.takeLive(page((3001 + i * 10)...(3080 + i * 10), hasMore: true)) }
        #expect(h.older.count == 1000 + MessageHistory.olderMaxRows)
        #expect(h.older.last?.seq == (h.live?.messages.first?.seq ?? 0) - 1)
        #expect(h.hasMore)
        #expect(h.nextBefore == h.older.first?.seq)
    }

    /// Codex's second review: the phone loads a chunk by itself when Load
    /// earlier comes into view, so rows the cap let go were read again and
    /// earned more room each time, without anyone asking. They earn
    /// nothing now, and a chunk that brings nothing that stays ends loading.
    @Test func rowsLetGoAndReadAgainEarnNoRoomAndLoadingStops() {
        var h = MessageHistory()
        h.takeLive(page(1...80, hasMore: true))
        for i in 1...300 { h.takeLive(page((1 + i * 10)...(80 + i * 10), hasMore: true)) }
        #expect(h.older.count == MessageHistory.olderMaxRows)
        let before = h.nextBefore ?? 0
        _ = h.takeOlder(page((before - 22)...(before - 1), hasMore: true), before: before)
        #expect(h.older.count == MessageHistory.olderMaxRows)
        #expect(!h.hasMore)
        #expect(h.nextBefore == nil)
    }

    /// However far back you go, what loading adds stops at loadedMax times
    /// the caps.
    @Test func loadingEarlierAddsRoomOnlyUpToItsCeiling() {
        let top = 100_000
        var h = MessageHistory()
        h.takeLive(page((top - 79)...top, hasMore: true))
        for _ in 0..<60 {
            guard let before = h.nextBefore else { break }
            _ = h.takeOlder(page((before - 1000)...(before - 1), hasMore: true), before: before)
        }
        #expect(h.older.count <= MessageHistory.olderMaxRows * (MessageHistory.loadedMax + 1))
        #expect(h.older.count >= MessageHistory.olderMaxRows * MessageHistory.loadedMax)
        #expect(!h.hasMore)
    }

    /// Daybreak, 10 Nov: another runner writing its own thread a few rows
    /// at a time, fewer than the live page holds so the pages always
    /// overlap, with nobody pressing Load earlier. Every row that slid off
    /// the page was kept for as long as the thread was open. Now `older`
    /// stays within its caps, nearest the live page, and each update is cheap.
    @Test func aPacedStreamOfBigRowsKeepsOlderWithinItsByteCap() {
        let text = String(repeating: "x", count: 8192)
        func big(_ seqs: ClosedRange<Int>) -> MessagePage {
            MessagePage(messages: seqs.map { PhoneMessage(seq: $0, role: .assistant, text: text, at: t0) }, hasMore: true, images: [])
        }
        var h = MessageHistory()
        let pages = (0...1250).map { big((1 + $0 * 8)...(80 + $0 * 8)) }
        let clock = ContinuousClock()
        let took = clock.measure { for p in pages { h.takeLive(p) } }
        let bytes = h.older.reduce(0) { $0 + MessageHistory.bytes(of: $1) }
        #expect(bytes <= MessageHistory.olderMaxBytes)
        #expect(h.older.count < 1100)
        #expect(h.older.last?.seq == (h.live?.messages.first?.seq ?? 0) - 1)
        // What went can be loaded again.
        #expect(h.nextBefore == h.older.first?.seq)
        // 1,251 updates; well under a tenth of a millisecond each would do.
        #expect(took < .seconds(2), "took \(took)")
    }

    @Test func aLongStreamOfSmallRowsKeepsOlderWithinItsRowCap() {
        var h = MessageHistory()
        for i in 0..<400 { h.takeLive(page((1 + i * 10)...(80 + i * 10), hasMore: true)) }
        #expect(h.older.count == MessageHistory.olderMaxRows)
        #expect(h.older.last?.seq == (h.live?.messages.first?.seq ?? 0) - 1)
    }

    /// Pictures between rows (at seqs no row has) count with the row
    /// before them, so a stream of pictures can't grow `older` past its
    /// bytes either.
    @Test func picturesBetweenRowsCountAgainstTheCap() {
        let url = "https://x/" + String(repeating: "p", count: 100_000)
        var h = MessageHistory()
        for i in 0..<300 {
            let a = 1 + i * 1000
            h.takeLive(MessagePage(messages: [PhoneMessage(seq: a, role: .user, text: "a", at: t0), PhoneMessage(seq: a + 1000, role: .user, text: "b", at: t0)],
                                   hasMore: true, images: [MessageImage(seq: a + 500, url: "\(url)?\(i)")]))
        }
        let bytes = h.older.reduce(0) { $0 + MessageHistory.bytes(of: $1) } + h.olderImages.reduce(0) { $0 + MessageHistory.bytes(of: $1) }
        #expect(bytes <= MessageHistory.olderMaxBytes)
        #expect(h.olderImages.count < 100)
        #expect(h.olderImages.allSatisfy { $0.seq >= (h.older.first?.seq ?? .max) })
    }

    /// Codex's review: a picture at a seq below the only row, with no
    /// older rows, was added to `older` again on every live update (a
    /// button's title changing is enough to send one), N copies after N.
    @Test func aPictureBeforeEveryRowIsKeptOnceHoweverManyUpdatesCome() {
        let one = MessagePage(messages: [PhoneMessage(seq: 10, role: .user, text: "a", at: t0)], hasMore: false,
                              images: [MessageImage(seq: 1, url: "https://x/1.png")])
        var h = MessageHistory()
        for _ in 0..<1000 { h.takeLive(one) }
        #expect(h.olderImages.count == 1)
        #expect(h.images.map(\.seq) == [1])
    }

    @Test func picturesWithNoOlderRowsAreCappedToo() {
        var h = MessageHistory()
        for i in 0..<3000 {
            h.takeLive(MessagePage(messages: [PhoneMessage(seq: 1_000_000, role: .user, text: "a", at: t0)], hasMore: false,
                                   images: [MessageImage(seq: i, url: "https://x/\(i).png")]))
        }
        #expect(h.older.isEmpty)
        #expect(h.olderImages.count <= MessageHistory.olderMaxPictures)
        #expect(h.olderImages.map(\.seq).max() == 2998)
    }

    /// New messages slide the oldest off the live page. Those rows stay, or
    /// there'd be a hole above the live ones, before any chunk too: you may be
    /// reading them.
    @Test func rowsThatSlideOffTheLivePageStay() {
        var h = MessageHistory()
        h.takeLive(page(921...1000, hasMore: true, images: [921]))
        h.takeLive(page(922...1001, hasMore: true))
        #expect(h.messages.map(\.seq) == Array(921...1001))
        // Two more while a chunk was on its way.
        h.takeLive(page(924...1003, hasMore: true))
        #expect(h.firstSeq == 921)
        let took = h.takeOlder(page(721...920, hasMore: true), before: 921)
        #expect(took)
        #expect(h.messages.map(\.seq) == Array(721...1003))
        #expect(h.images.map(\.seq) == [921])
        // And again, after the chunk.
        h.takeLive(page(925...1004, hasMore: true))
        #expect(h.messages.map(\.seq) == Array(721...1004))
    }

    /// A chunk asked for before rows that are no longer the first is dropped.
    @Test func aLateChunkIsDropped() {
        var h = MessageHistory()
        h.takeLive(page(921...1000, hasMore: true))
        let took4 = h.takeOlder(page(721...920, hasMore: true), before: 921)
        #expect(took4)
        let took5 = h.takeOlder(page(721...920, hasMore: true), before: 921)
        #expect(!took5)
        #expect(h.messages.count == 280)
    }

    /// Away long enough for a whole page of new messages: what came between
    /// is unknown, so it starts again from the newest rather than show a hole.
    @Test func noOverlapWithThePageBeforeStartsAgain() {
        var h = MessageHistory()
        h.takeLive(page(921...1000, hasMore: true))
        let took = h.takeOlder(page(721...920, hasMore: true), before: 921)
        #expect(took)
        h.takeLive(page(1101...1180, hasMore: true))
        #expect(h.messages.map(\.seq) == Array(1101...1180))
        #expect(h.nextBefore == 1101)
    }

    /// The live page's top moving back up (big rows that got smaller) takes
    /// its rows back from the chunks: nothing shows twice.
    @Test func aLivePageReachingFurtherBackTakesItsRowsBack() {
        var h = MessageHistory()
        h.takeLive(page(921...1000, hasMore: true, images: [925]))
        h.takeLive(page(931...1010, hasMore: true))
        h.takeLive(page(910...1010, hasMore: true, images: [925]))
        #expect(h.messages.map(\.seq) == Array(910...1010))
        #expect(h.images.map(\.seq) == [925])
    }

    /// A picture that finishes uploading after its row slid off the live
    /// page (or came in a chunk) arrives only from phone:images: it shows, and
    /// pictures for rows not shown don't.
    @Test func latePicturesOfRowsReadOnceShow() {
        var h = MessageHistory()
        #expect(h.lateImagesFrom == nil)
        h.takeLive(page(921...1000, hasMore: true))
        h.takeLive(page(925...1004, hasMore: true))
        #expect(h.lateImagesFrom == 921)
        h.takeLateImages([MessageImage(seq: 922, url: "https://x/922.png"), MessageImage(seq: 900, url: "https://x/900.png"), MessageImage(seq: 1001, url: "https://x/1001.png")])
        #expect(h.images.map(\.seq).sorted() == [922, 1001])
    }
}
