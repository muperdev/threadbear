import Foundation
import Testing
@testable import ThreadbearKit

/// Stop on the phone (backend API 69, phone:interrupt). docs/parity.md showed
/// Stop working on every Mac but not on the phone. The sample data's
/// running thread stops as a runner would stop it: idle, its working line gone.
struct StopTests {
    @Test func theFakeStopsARunningThread() async throws {
        let backend = FakeBackend()
        let running = try #require(await first(backend.thread("a5")))
        #expect(running.thread.isBusy)
        let id = try await backend.interrupt(threadId: "a5")
        for try await status in backend.command(id) {
            #expect(status?.state == .done)
            break
        }
        let after = try #require(await first(backend.thread("a5")))
        #expect(!after.thread.isBusy)
        #expect(after.thread.working == nil)
    }

    @Test func stopNeedsTheBackendThatHasIt() {
        #expect(BackendCompatibility.stopAPIVersion == 69)
    }

    private func first(_ stream: AsyncThrowingStream<ThreadDetail?, Error>) async throws -> ThreadDetail? {
        for try await detail in stream { return detail }
        return nil
    }
}
