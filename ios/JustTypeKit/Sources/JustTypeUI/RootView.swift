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

    public init(baseURL: URL, startWriting: Binding<OpenScene?> = .constant(nil)) {
        _session = StateObject(wrappedValue: SessionStore(baseURL: baseURL))
        _startWriting = startWriting
    }

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
                MainTabs()
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

struct MainTabs: View {
    var body: some View {
        TabView {
            LibraryView()
                .tabItem { Label("Desk", systemImage: "books.vertical") }
            FeedView()
                .tabItem { Label("Review", systemImage: "text.bubble") }
            AccountView()
                .tabItem { Label("You", systemImage: "person") }
        }
    }
}

struct AccountView: View {
    @EnvironmentObject private var session: SessionStore

    var body: some View {
        NavigationStack {
            List {
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
                }
            }
            .navigationTitle("You")
        }
    }
}
