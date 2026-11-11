import XCTest
@testable import ThreadbearCore

/// Shared projects (docs/multiplayer-design.md): the team as team.get sends it.
final class TeamTests: XCTestCase {
    private func decode<T: Decodable>(_ type: T.Type, _ json: String) throws -> T {
        try JSONDecoder().decode(type, from: Data(json.utf8))
    }

    func testATeamDecodesAsTheRunnerSendsIt() throws {
        let team = try decode(TeamState.self, """
        {"on":true,"joined":true,"connected":true,"url":"https://h.convex.cloud","name":"Collabute",
         "me":{"memberId":"m_1","name":"Behrooz","host":true,"me":true,"online":true},
         "members":[{"memberId":"m_1","name":"Behrooz","host":true,"me":true,"online":true},{"memberId":"m_2","name":"Sam","host":false,"me":false,"online":false,"seenAt":"2026-11-11T05:00:00.000Z"}],
         "projects":[{"id":"tp_1","title":"Collabute","sharedBy":"Behrooz","repos":["github.com/collabute/gateway"],"updatedAt":"2026-11-11T05:00:00.000Z","localProjectId":"p_1","mine":true}],
         "cards":[{"id":"team:sam:abcd1234","projectId":"tp_1","memberId":"m_2","memberName":"Sam","threadId":"abcd1234ef","title":"Schema","status":"needsThem","headline":"Waiting on a review","branch":"feat/schema","updatedAt":1762837200000,"mine":false},
                  {"id":"team:behrooz:11112222","projectId":"tp_1","memberId":"m_1","memberName":"Behrooz","threadId":"11112222ab","title":"Mine","status":"working","updatedAt":1762837100000,"mine":true},
                  {"id":"team:sam:99990000","projectId":"tp_1","memberId":"m_2","memberName":"Sam","threadId":"99990000aa","title":"Odd","status":"flying","updatedAt":1}],
         "waiting":[{"id":"tm_1","kind":"handover","projectId":"tp_1","localProjectId":"p_1","fromName":"Sam","fromTitle":"On-call","text":"Two alarms","repo":"github.com/collabute/infra","branch":"fix/alarms","links":[{"title":"PR","url":"https://github.com/collabute/infra/pull/9"}],"createdAt":"2026-11-11T05:00:00.000Z"}],
         "log":{"tp_1":[{"memberId":"m_2","memberName":"Sam","text":"Schema behind a flag","at":1762837200000}]},
         "somethingNew":42}
        """)
        XCTAssertEqual(team.name, "Collabute")
        XCTAssertEqual(team.members.map(\.name), ["Behrooz", "Sam"])
        XCTAssertEqual(team.me?.host, true)
        let project = Project(id: "p_1", title: "Collabute", team: ProjectTeam(id: "tp_1"))
        XCTAssertEqual(team.shared(project)?.localProjectId, "p_1")
        // The board leaves out Mike's own threads, newest first, and an unknown status reads as idle.
        XCTAssertEqual(team.board("tp_1").map(\.title), ["Schema", "Odd"])
        XCTAssertEqual(team.board("tp_1").last?.status, .idle)
        XCTAssertEqual(team.waiting(for: "tp_1").first?.kind, .handover)
        XCTAssertEqual(team.waiting(for: "tp_1").first?.links.first?.title, "PR")
        XCTAssertEqual(team.log["tp_1"]?.first?.text, "Schema behind a flag")
    }

    func testAProjectCarriesItsTeamLink() throws {
        let p = try decode(Project.self, #"{"id":"p_1","title":"T","team":{"id":"tp_1","delivery":"deliver","syncedAt":5}}"#)
        XCTAssertEqual(p.team, ProjectTeam(id: "tp_1", deliver: true))
        XCTAssertNil(try decode(Project.self, #"{"id":"p_2","title":"T"}"#).team)
    }

    func testAnOlderRunnerSaysNothingOfTeams() throws {
        let team = try decode(TeamState.self, "{}")
        XCTAssertFalse(team.on)
        XCTAssertFalse(team.joined)
        XCTAssertTrue(team.members.isEmpty)
    }
}
