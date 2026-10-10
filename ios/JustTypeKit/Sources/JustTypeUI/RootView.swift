import SwiftUI
import JustTypeKit

/// A scene somebody asked for from outside the app.
public struct OpenScene: Identifiable, Equatable, Sendable {
    public let id: String
    public let title: String

    public init(id: String, title: String) {
        self.id = id
        self.title = title
    }
}

public struct RootView: View {
    /// Coming back to the foreground is the moment a phone usually has signal
    /// again, so it is the moment to send what has been waiting.
    @Environment(\.scenePhase) private var scenePhase
    @StateObject private var session: SessionStore

    public init(baseURL: URL, startWriting: Binding<OpenScene?> = .constant(nil), link: Binding<URL?> = .constant(nil)) {
        _session = StateObject(wrappedValue: SessionStore(baseURL: baseURL))
        _startWriting = startWriting
        _link = link
    }

    /// A universal link the app was opened with: the daily email's "Write
    /// your answer" goes to `/prompts/<day>/write`. Held until signed in.
    @Binding var link: URL?

    /// Set by a Shortcut before the interface exists — an intent cannot
    /// navigate, so it leaves a note and this picks it up.
    @Binding var startWriting: OpenScene?

    public var body: some View {
        Group {
            if session.isLoading {
                ProgressView("Opening…")
            } else if session.user == nil {
                SignInView()
            } else {
                MainTabs(link: $link)
            }
        }
        .environmentObject(session)
        // A cover rather than a push: "start writing" should put the page in
        // front of the writer, not somewhere three taps into a hierarchy.
        .sheet(item: $startWriting) { scene in
            NavigationStack {
                SceneEditorView(sceneId: scene.id, title: scene.title)
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Done") { startWriting = nil }
                        }
                    }
            }
            #if os(iOS)
                .presentationDragIndicator(.hidden)
            #endif
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active { Task { await session.catchUp() } }
        }
        .task { await session.restore() }
    }
}

enum MainTab: Hashable { case desk, review, prompts, inbox, you }

struct MainTabs: View {
    @EnvironmentObject private var session: SessionStore
    /// A Daily Prompts link from outside (the email), waiting to be opened.
    @Binding var link: URL?
    @State private var tab: MainTab = .desk
    @State private var promptRoute: PromptRoute?

    var body: some View {
        TabView(selection: $tab) {
            LibraryView()
                .tabItem { Label("Desk", systemImage: "books.vertical") }
                .tag(MainTab.desk)
            FeedView()
                .tabItem { Label("Review", systemImage: "text.bubble") }
                .tag(MainTab.review)
            PromptsView(open: $promptRoute)
                .tabItem { Label("Prompts", systemImage: "sun.max") }
                .tag(MainTab.prompts)
            InboxView()
                .tabItem { Label("Inbox", systemImage: "tray") }
                .badge(session.unread)
                .tag(MainTab.inbox)
            AccountView()
                .tabItem { Label("You", systemImage: "person") }
                .tag(MainTab.you)
        }
        .task { await session.refreshUnread() }
        .onChange(of: link) { _, url in follow(url) }
        .onAppear { follow(link) }
    }

    private func follow(_ url: URL?) {
        guard let url, let route = PromptLink.route(path: url.path) else { return }
        tab = .prompts
        promptRoute = route
        link = nil
    }
}

/// The account and the settings, split as they are on the web: the account
/// is who you are; the settings are how the app behaves, read from the
/// account so every device agrees.
struct AccountView: View {
    @EnvironmentObject private var session: SessionStore
    @State private var section = 0
    @State private var problem: String?

    var body: some View {
        NavigationStack {
            List {
                Picker("Show", selection: $section) {
                    Text("Account").tag(0)
                    Text("Settings").tag(1)
                }
                .pickerStyle(.segmented)
                .listRowBackground(Color.clear)

                if section == 0 {
                    if let user = session.user {
                        Section("Signed in as") {
                            Text(user.displayName).font(.headline)
                            Text(user.email).foregroundStyle(.secondary)
                        }
                    }
                    Section {
                        Button("Sign out", role: .destructive) {
                            Task { await session.signOut() }
                        }
                    } footer: {
                        Text("Your password, two-factor sign-in, sessions and your data are on the web, under Account.")
                    }
                } else {
                    Section {
                        Toggle("Treat a blank line (double Enter) as a scene break", isOn: Binding(
                            get: { session.user?.doubleEnterSplitsScenes ?? false },
                            set: { on in Task { await setDoubleEnter(on) } }
                        ))
                        if let problem { Text(problem).foregroundStyle(.red).font(.footnote) }
                    } header: {
                        Text("Writing")
                    } footer: {
                        Text("When turning an answer into a project, a line that is only -- always starts a new scene; this adds a blank line too. Kept with your account: on the web, a double Enter while writing a book also moves on to the next scene.")
                    }
                    Section("Daily Prompts") {
                        Text("Taking part and the daily email are set on the Prompts tab.")
                            .font(.footnote).foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle(section == 0 ? "Account" : "Settings")
            .task { if let me = try? await session.client.me() { session.replaceUser(me) } }
        }
    }

    private func setDoubleEnter(_ on: Bool) async {
        do {
            session.replaceUser(try await session.client.updateSettings(doubleEnterSplitsScenes: on))
            problem = nil
        } catch {
            problem = error.localizedDescription
        }
    }
}
