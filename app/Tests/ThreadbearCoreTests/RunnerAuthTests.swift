import Foundation
import XCTest
@testable import ThreadbearCore

/// The runner RPC's challenge auth (docs/protocol.md, "Auth"): the token
/// never goes over the socket, and nothing that can't prove it has the
/// token is trusted as the runner.
final class RunnerAuthTests: XCTestCase {
    private let token = "test-token-0123456789"

    // MARK: The proofs

    func testProofsMatchTheRunners() {
        // Worked out with runner/src/rpc-auth.ts's authProof.
        let runnerNonce = String(repeating: "a", count: 64)
        let clientNonce = String(repeating: "0123456789abcdef", count: 4)
        XCTAssertEqual(RunnerAuth.proof(token: token, role: .client, runnerNonce: runnerNonce, clientNonce: clientNonce, port: 8797),
                       "3ff1854cab19b025f45ba2df4f40da330a65cee664c32eb9c3a69e471b570a0a")
        XCTAssertEqual(RunnerAuth.proof(token: token, role: .runner, runnerNonce: runnerNonce, clientNonce: clientNonce, port: 8797),
                       "8230a5ad1a4a1ddeab7cee6ffcd1a4ab7b4926a1b8a1fe23eeccd12faef46d5a")
    }

    func testProofsAreBoundToTheDialedLoopbackPort() throws {
        XCTAssertEqual(RunnerAuth.loopbackPort(URL(string: "ws://127.0.0.1:8797/rpc")!), 8797)
        XCTAssertEqual(RunnerAuth.loopbackPort(URL(string: "ws://localhost:9000/rpc")!), 9000)
        XCTAssertNil(RunnerAuth.loopbackPort(URL(string: "wss://bruce.example.ts.net/rpc")!))
        // A challenge passed on from a runner on 9001 says 9001; a client that
        // dialed 8797 binds its proof to 8797 all the same, so the runner on
        // 9001 turns it down. Another Mac's runner is taken at its word.
        let challenge = Data(#"{"method":"auth.challenge","params":{"nonce":"\#(String(repeating: "a", count: 64))","port":9001}}"#.utf8)
        let clientNonce = String(repeating: "0123456789abcdef", count: 4)
        let local = try RunnerAuth.answer(challenge: challenge, token: token, dialedPort: 8797, clientNonce: clientNonce)
        XCTAssertTrue(local.request.contains("3ff1854cab19b025f45ba2df4f40da330a65cee664c32eb9c3a69e471b570a0a"))
        let remote = try RunnerAuth.answer(challenge: challenge, token: token, dialedPort: nil, clientNonce: clientNonce)
        XCTAssertEqual(remote.expected, RunnerAuth.proof(token: token, role: .runner, runnerNonce: String(repeating: "a", count: 64), clientNonce: clientNonce, port: 9001))
        XCTAssertNotEqual(remote.expected, local.expected)
    }

    func testAnswerNeverCarriesTheTokenAndChecksTheRunnersProof() throws {
        let runnerNonce = RunnerAuth.newNonce()
        XCTAssertTrue(RunnerAuth.isNonce(runnerNonce))
        let challenge = Data(#"{"jsonrpc":"2.0","method":"auth.challenge","params":{"version":1,"nonce":"\#(runnerNonce)","port":8797}}"#.utf8)
        let (request, expected) = try RunnerAuth.answer(challenge: challenge, token: token, dialedPort: 8797)
        XCTAssertFalse(request.contains(token))
        let sent = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(request.utf8)) as? [String: Any])
        XCTAssertEqual(sent["method"] as? String, "auth.prove")
        let params = try XCTUnwrap(sent["params"] as? [String: String])
        let clientNonce = try XCTUnwrap(params["nonce"])
        XCTAssertEqual(params["proof"], RunnerAuth.proof(token: token, role: .client, runnerNonce: runnerNonce, clientNonce: clientNonce, port: 8797))
        XCTAssertEqual(expected, RunnerAuth.proof(token: token, role: .runner, runnerNonce: runnerNonce, clientNonce: clientNonce, port: 8797))

        XCTAssertNoThrow(try RunnerAuth.check(reply: Data(#"{"jsonrpc":"2.0","id":"auth","result":{"proof":"\#(expected)"}}"#.utf8), expected: expected))
        // Its own proof played back, a wrong one, none, or another message.
        for reply in [#"{"id":"auth","result":{"proof":"\#(params["proof"]!)"}}"#, #"{"id":"auth","result":{"proof":"00"}}"#,
                      #"{"id":"auth","result":{}}"#, #"{"method":"runner.notice","params":{}}"#] {
            XCTAssertThrowsError(try RunnerAuth.check(reply: Data(reply.utf8), expected: expected), reply)
        }
    }

    func testAnythingButAChallengeIsRefused() {
        for first in [#"{"jsonrpc":"2.0","method":"runner.notice","params":{"level":"info","message":"hi"}}"#,
                      #"{"jsonrpc":"2.0","method":"auth.challenge","params":{"nonce":"short"}}"#,
                      #"{"jsonrpc":"2.0","id":1,"result":{}}"#, "not json"] {
            XCTAssertThrowsError(try RunnerAuth.answer(challenge: Data(first.utf8), token: token, dialedPort: nil), first)
            XCTAssertThrowsError(try RunnerAuth.answer(challenge: Data(first.utf8), token: token, dialedPort: 8797), first)
        }
    }

    // MARK: Over a real socket

    private var processes: [Process] = []
    private var dirs: [URL] = []

    override func tearDown() async throws {
        for process in processes where process.isRunning {
            process.terminate()
            process.waitUntilExit()
        }
        for dir in dirs { try? FileManager.default.removeItem(at: dir) }
    }

    private static var mockDir: URL {
        URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("Tools/mock-runner")
    }

    private func node() throws -> String {
        guard FileManager.default.fileExists(atPath: Self.mockDir.appendingPathComponent("node_modules/ws").path) else {
            throw try missing("Tools/mock-runner has no node_modules; run npm install there")
        }
        // check.sh's Node (THREADBEAR_NODE) first, then PATH and Homebrew's.
        let env = ProcessInfo.processInfo.environment
        let nodes = [env["THREADBEAR_NODE"]].compactMap { $0 }
            + ((env["PATH"] ?? "").split(separator: ":").map(String.init) + ["/opt/homebrew/bin", "/usr/local/bin"]).map { $0 + "/node" }
        guard let node = nodes.first(where: { FileManager.default.isExecutableFile(atPath: $0) }) else {
            throw try missing("node isn't on PATH")
        }
        return node
    }

    /// As MockRunnerTests: a failure when ./check.sh says the mock is there.
    private func missing(_ why: String) throws -> XCTSkip {
        if ProcessInfo.processInfo.environment["THREADBEAR_REQUIRE_MOCK"] == "1" {
            XCTFail("\(why) (THREADBEAR_REQUIRE_MOCK is set)")
            throw CancellationError()
        }
        return XCTSkip(why)
    }

    private func tempDir() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("tb-auth-test-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        dirs.append(dir)
        return dir
    }

    /// Something else listening where the runner should be, the way another
    /// account on the Mac could while the runner is down. It writes every
    /// upgrade request's headers and every message it gets to `log`.
    /// `silent` says nothing; `old` talks like a runner from before the
    /// challenge (a notice, and `{}` to anything); `challenge` sends a
    /// challenge and then a made-up proof.
    private func startImpostor(mode: String, port: Int, log: URL) async throws -> Int {
        let script = """
        import { WebSocketServer } from 'ws';
        import fs from 'node:fs';
        const [mode, port, log] = process.argv.slice(1);
        const wss = new WebSocketServer({ host: '127.0.0.1', port: Number(port) });
        wss.on('listening', () => fs.writeFileSync(log + '.port', String(wss.address().port)));
        wss.on('connection', (ws, req) => {
          fs.appendFileSync(log, JSON.stringify(req.headers) + '\\n');
          if (mode === 'challenge') ws.send(JSON.stringify({ jsonrpc: '2.0', method: 'auth.challenge', params: { version: 1, nonce: 'b'.repeat(64) } }));
          if (mode === 'old') ws.send(JSON.stringify({ jsonrpc: '2.0', method: 'runner.notice', params: { level: 'info', message: 'hi' } }));
          ws.on('message', (data) => {
            fs.appendFileSync(log, data.toString() + '\\n');
            let id = null;
            try { id = JSON.parse(data.toString()).id ?? null; } catch {}
            if (mode === 'challenge') ws.send(JSON.stringify({ jsonrpc: '2.0', id, result: { proof: 'c'.repeat(64) } }));
            if (mode === 'old') ws.send(JSON.stringify({ jsonrpc: '2.0', id, result: {} }));
          });
        });
        """
        let process = Process()
        process.executableURL = URL(fileURLWithPath: try node())
        process.currentDirectoryURL = Self.mockDir
        process.arguments = ["--input-type=module", "-e", script, mode, String(port), log.path]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        FileManager.default.createFile(atPath: log.path, contents: nil)
        try process.run()
        processes.append(process)
        let portFile = URL(fileURLWithPath: log.path + ".port")
        for _ in 0..<200 {
            if let text = try? String(contentsOf: portFile, encoding: .utf8), let port = Int(text) { return port }
            try await Task.sleep(for: .milliseconds(25))
        }
        throw XCTSkip("the impostor didn't start")
    }

    private func startMock(dataDir: URL, port: Int) async throws -> Process {
        try? FileManager.default.removeItem(at: dataDir.appendingPathComponent("runner.json"))
        let process = Process()
        process.executableURL = URL(fileURLWithPath: try node())
        process.arguments = [Self.mockDir.appendingPathComponent("server.mjs").path, "--data-dir", dataDir.path,
                             "--fixtures", Self.mockDir.appendingPathComponent("fixtures").path, "--quiet", "--port", String(port)]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        processes.append(process)
        for _ in 0..<200 {
            if let data = try? Data(contentsOf: dataDir.appendingPathComponent("runner.json")),
               let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any], object["port"] is Int { return process }
            try await Task.sleep(for: .milliseconds(25))
        }
        throw XCTSkip("the mock runner didn't start")
    }

    private func stop(_ process: Process) {
        process.terminate()
        process.waitUntilExit()
    }

    private func assertNothingGiven(_ log: URL, token: String, file: StaticString = #filePath, line: UInt = #line) throws {
        let seen = try String(contentsOf: log, encoding: .utf8)
        XCTAssertFalse(seen.isEmpty, "the client never reached the impostor", file: file, line: line)
        XCTAssertFalse(seen.contains(token), "the token reached the impostor: \(seen)", file: file, line: line)
        XCTAssertFalse(seen.lowercased().contains("\"authorization\""), "an Authorization header reached the impostor: \(seen)", file: file, line: line)
    }

    /// Finding: a listener under another local account must not receive
    /// the token during runner downtime, whatever it pretends to be.
    func testAListenerOnTheRunnersPortGetsNoToken() async throws {
        for mode in ["silent", "old", "challenge"] {
            let dir = try tempDir()
            let log = dir.appendingPathComponent("impostor.log")
            let port = try await startImpostor(mode: mode, port: 0, log: log)
            let transport = WebSocketTransport(url: URL(string: "ws://127.0.0.1:\(port)/rpc")!, token: token, authTimeout: .milliseconds(800))
            let client = RPCClient(transport: transport)
            do {
                try await client.start()
                XCTFail("\(mode): connected to something that never proved it's the runner")
            } catch let error as RPCError {
                guard case .connectFailed(let why) = error else { return XCTFail("\(mode): \(error)") }
                XCTAssertTrue(why.contains("didn't prove it's the runner"), "\(mode): \(why)")
            }
            client.close()
            try assertNothingGiven(log, token: token)
        }
    }

    /// Finding: reconnecting to the genuine runner after an outage doesn't
    /// hand its token to whatever held the port in between.
    func testReconnectsToTheRealRunnerAfterAnImpostorHeldThePort() async throws {
        let dataDir = try tempDir()
        let first = try await startMock(dataDir: dataDir, port: 0)
        let endpoint = try RunnerConfig.endpoint(dataDir: dataDir)
        let port = try XCTUnwrap(endpoint.url.port)
        let up = RPCClient(transport: WebSocketTransport(url: endpoint.url, token: endpoint.token))
        try await up.start()
        _ = try await up.hello(version: "test")
        up.close()

        // The runner goes down and something else takes its port.
        stop(first)
        let log = dataDir.appendingPathComponent("impostor.log")
        _ = try await startImpostor(mode: "challenge", port: port, log: log)
        let duringOutage = RPCClient(transport: WebSocketTransport(url: endpoint.url, token: endpoint.token, authTimeout: .milliseconds(800)))
        do {
            try await duringOutage.start()
            XCTFail("trusted the impostor")
        } catch {}
        duringOutage.close()
        try assertNothingGiven(log, token: endpoint.token)

        // It goes; the real runner is back on the same port.
        for process in processes where process.isRunning && process !== first { stop(process) }
        _ = try await startMock(dataDir: dataDir, port: port)
        let again = try RunnerConfig.endpoint(dataDir: dataDir)
        XCTAssertEqual(again.url.port, port)
        let back = RPCClient(transport: WebSocketTransport(url: again.url, token: again.token))
        try await back.start()
        let hello = try await back.hello(version: "test")
        XCTAssertFalse(hello.runnerVersion.isEmpty)
        back.close()
    }
}
