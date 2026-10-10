import SwiftUI
import JustTypeKit

/// What happened that you should hear about, with yesterday's summary pinned
/// at the top until read. The tab's badge is the same count the web's bell
/// shows; there is no push yet, so it is refreshed on opening and on coming
/// to the front.
struct InboxView: View {
    @EnvironmentObject private var session: SessionStore
    @State private var page: API.InboxPage?
    @State private var items: [API.InboxItemView] = []
    @State private var path: [PromptRoute] = []

    var body: some View {
        NavigationStack(path: $path) {
            List {
                if let digest = page?.digest {
                    Section {
                        Text(digest.text)
                        Button("Dismiss") {
                            Task {
                                if let count = try? await session.client.markDigestRead(day: digest.day) { session.setUnread(count) }
                                await load()
                            }
                        }
                        .font(.footnote)
                    }
                }
                ForEach(items, id: \.id) { item in
                    Button {
                        Task { await open(item) }
                    } label: {
                        HStack(alignment: .top, spacing: 8) {
                            Circle().fill(item.readAt == nil ? Color.accentColor : .clear).frame(width: 8, height: 8).padding(.top, 6)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(item.text).fontWeight(item.readAt == nil ? .semibold : .regular)
                                Text(PromptDays.time(item.createdAt)).font(.caption).foregroundStyle(.secondary)
                            }
                        }
                    }
                    .buttonStyle(.plain)
                }
                if page?.nextCursor != nil {
                    Button("Older") { Task { await load(more: true) } }
                }
            }
            .overlay {
                if page != nil && items.isEmpty && page?.digest == nil {
                    ContentUnavailableView("Nothing yet", systemImage: "tray", description: Text("Critiques and replies to you show up here."))
                }
            }
            .navigationTitle("Inbox")
            .toolbar {
                Button("Mark all read") {
                    Task {
                        if let count = try? await session.client.markAllRead() { session.setUnread(count) }
                        await load()
                    }
                }
                .disabled(session.unread == 0)
            }
            .navigationDestination(for: PromptRoute.self) { route in
                switch route {
                case .day(let day): PromptDayView(day: day)
                case .write(let day): PromptWriteView(day: day)
                case .entry(let id): PromptEntryReadView(entryId: id)
                case .archive: PromptArchiveView()
                }
            }
            .task { await load() }
            .refreshable { await load() }
        }
    }

    private func load(more: Bool = false) async {
        guard let next = try? await session.client.inbox(cursor: more ? page?.nextCursor : nil) else { return }
        items = more ? items + next.items : next.items
        page = more ? API.InboxPage(digest: page?.digest, items: items, nextCursor: next.nextCursor) : next
        await session.refreshUnread()
    }

    /// The item's link is a web path; the ones about answers open here.
    private func open(_ item: API.InboxItemView) async {
        if item.readAt == nil, let count = try? await session.client.markRead(itemId: item.id) {
            session.setUnread(count)
        }
        if let route = PromptLink.route(path: item.link) { path.append(route) }
        await load()
    }
}

/// Paths in the web app that have a screen here.
public enum PromptLink {
    /// `/prompts/<day>/write`, `/prompts/<day>/entries/<id>`, `/prompts/<day>`.
    static func route(path: String) -> PromptRoute? {
        let clean = path.split(separator: "#").first.map(String.init) ?? path
        let parts = clean.split(separator: "/").map(String.init)
        guard parts.first == "prompts", parts.count >= 2 else { return nil }
        let day = parts[1]
        guard day.count == 10 else { return nil }
        if parts.count == 3, parts[2] == "write" { return .write(day) }
        if parts.count == 4, parts[2] == "entries" { return .entry(parts[3]) }
        if parts.count == 2 { return .day(day) }
        return nil
    }

    /// Whether a link opened from outside the app (the email) is for Daily Prompts.
    public static func handles(_ url: URL) -> Bool { route(path: url.path) != nil }
}
