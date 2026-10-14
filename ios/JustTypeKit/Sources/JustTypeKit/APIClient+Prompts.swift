import Foundation

/// Daily Prompts, critiques, the inbox and the writer's settings.
///
/// The rules (who may see what, how an answer splits into scenes) are the
/// server's; this only asks.
public extension APIClient {
    private static func q(_ value: String) -> String {
        value.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed.subtracting(["&", "+", "="])) ?? value
    }

    // MARK: - Prompts

    func todayPrompt() async throws -> API.TodayResponse {
        try await send("GET", "prompts/today")
    }

    func prompt(day: String) async throws -> API.PromptView {
        try await send("GET", "prompts/\(day)")
    }

    func promptArchive(before: String? = nil, limit: Int = 30) async throws -> API.ArchivePage {
        var path = "prompts?limit=\(limit)"
        if let before { path += "&before=\(Self.q(before))" }
        return try await send("GET", path)
    }

    // MARK: - Taking part

    func enrollment() async throws -> API.EnrollmentView {
        try await send("GET", "prompts/enrollment")
    }

    func joinPrompts(timezone: String) async throws -> API.EnrollmentView {
        struct Body: Encodable { let timezone: String }
        return try await send("POST", "prompts/enrollment", body: Body(timezone: timezone))
    }

    func updateEnrollment(emailEnabled: Bool? = nil, localTime: String? = nil, timezone: String? = nil) async throws -> API.EnrollmentView {
        struct Body: Encodable {
            let emailEnabled: Bool?
            let emailLocalTime: String?
            let emailTimezone: String?
        }
        return try await send(
            "PATCH", "prompts/enrollment",
            body: Body(emailEnabled: emailEnabled, emailLocalTime: localTime, emailTimezone: timezone)
        )
    }

    func leavePrompts() async throws -> API.EnrollmentView {
        try await send("DELETE", "prompts/enrollment")
    }

    // MARK: - An answer

    /// The writer's own answer to `day`, or nil when they have not started one.
    func entry(day: String) async throws -> API.PromptEntryView? {
        do {
            return try await send("GET", "prompts/\(day)/entry") as API.PromptEntryView
        } catch APIError.http(404, _, _) {
            return nil
        }
    }

    func saveEntry(day: String, content: API.DocNode, baseHash: String?, localDay: String) async throws -> API.PromptEntryView {
        struct Body: Encodable {
            let content: API.DocNode
            let baseHash: String?
            let localDay: String
        }
        return try await send("PUT", "prompts/\(day)/entry", body: Body(content: content, baseHash: baseHash, localDay: localDay))
    }

    func submitEntry(day: String) async throws -> API.PromptEntryView {
        try await send("POST", "prompts/\(day)/entry/submit")
    }

    func withdrawEntry(day: String) async throws -> API.PromptEntryView {
        try await send("POST", "prompts/\(day)/entry/withdraw")
    }

    func deleteEntry(day: String) async throws {
        let _: EmptyResponse = try await send("DELETE", "prompts/\(day)/entry")
    }

    // MARK: - Everyone's

    func entries(day: String, fewestCritiques: Bool = false, cursor: String? = nil) async throws -> API.EntryPage {
        var path = "prompts/\(day)/entries?sort=\(fewestCritiques ? "fewest_critiques" : "newest")"
        if let cursor { path += "&cursor=\(Self.q(cursor))" }
        return try await send("GET", path)
    }

    func promptEntry(id: String) async throws -> API.PromptEntryView {
        try await send("GET", "prompt-entries/\(id)")
    }

    /// What turning the answer into a project would make. The split is the
    /// server's, so the phone and the web always agree on it.
    func previewWork(entryId: String) async throws -> API.ToWorkPreview {
        try await send("POST", "prompt-entries/\(entryId)/to-work/preview")
    }

    func makeWork(entryId: String, title: String) async throws -> API.OutlineResponse {
        struct Body: Encodable { let title: String }
        return try await send("POST", "prompt-entries/\(entryId)/to-work", body: Body(title: title))
    }

    // MARK: - Critiques

    func critiques(entryId: String) async throws -> [API.CritiqueView] {
        try await send("GET", "prompt-entries/\(entryId)/critiques")
    }

    func critique(entryId: String, body: String, parentId: String? = nil) async throws -> API.CritiqueView {
        struct Body: Encodable { let body: String; let parentId: String? }
        return try await send("POST", "prompt-entries/\(entryId)/critiques", body: Body(body: body, parentId: parentId))
    }

    func editCritique(id: String, body: String) async throws -> API.CritiqueView {
        struct Body: Encodable { let body: String }
        return try await send("PATCH", "prompt-critiques/\(id)", body: Body(body: body))
    }

    func removeCritique(id: String) async throws {
        let _: EmptyResponse = try await send("DELETE", "prompt-critiques/\(id)")
    }

    func reportCritique(id: String, reason: API.ReportReason) async throws {
        struct Body: Encodable { let reason: API.ReportReason }
        let _: EmptyResponse = try await send("POST", "prompt-critiques/\(id)/report", body: Body(reason: reason))
    }

    // MARK: - The inbox

    func inbox(cursor: String? = nil) async throws -> API.InboxPage {
        try await send("GET", cursor.map { "inbox?cursor=\(Self.q($0))" } ?? "inbox")
    }

    func unreadCount() async throws -> Int {
        let count: API.UnreadCount = try await send("GET", "inbox/unread-count")
        return count.count
    }

    @discardableResult
    func markRead(itemId: String) async throws -> Int {
        let count: API.UnreadCount = try await send("POST", "inbox/\(itemId)/read")
        return count.count
    }

    @discardableResult
    func markDigestRead(day: String) async throws -> Int {
        let count: API.UnreadCount = try await send("POST", "inbox/digests/\(day)/read")
        return count.count
    }

    @discardableResult
    func markAllRead() async throws -> Int {
        let count: API.UnreadCount = try await send("POST", "inbox/read-all")
        return count.count
    }

    // MARK: - Settings

    /// Settings every device reads from the account, so the web and the phone agree.
    func updateSettings(doubleEnterSplitsScenes: Bool) async throws -> API.User {
        struct Body: Encodable { let doubleEnterSplitsScenes: Bool }
        return try await send("PATCH", "me/settings", body: Body(doubleEnterSplitsScenes: doubleEnterSplitsScenes))
    }

    /// The theme, kept with the account so the web and the phone look alike.
    func updateSettings(theme: API.Theme) async throws -> API.User {
        struct Body: Encodable { let theme: API.Theme }
        return try await send("PATCH", "me/settings", body: Body(theme: theme))
    }

    /// How often the summary email goes: none, daily or weekly.
    func summaryEmail() async throws -> API.SummaryEmailView {
        try await send("GET", "me/summary-email")
    }

    /// Chooses how often, with the device's timezone so it arrives in the
    /// writer's morning and counts their days as they do.
    func setSummaryEmail(_ often: API.SummaryEmail, timezone: String) async throws -> API.SummaryEmailView {
        try await send(
            "PATCH", "me/summary-email",
            body: API.UpdateSummaryEmailRequest(often: often, timezone: timezone)
        )
    }
}

public extension API.PromptEntryView {
    /// The answer as the editor's document.
    var document: API.DocNode {
        guard let data = try? JSONEncoder().encode(content),
              let doc = try? JSONDecoder().decode(API.DocNode.self, from: data)
        else { return .empty }
        return doc
    }
}

public extension API.PromptView {
    /// The writer's own answer's state, or nil when not started.
    var entryState: API.EntryState? { API.EntryState(rawValue: myState) }
}
