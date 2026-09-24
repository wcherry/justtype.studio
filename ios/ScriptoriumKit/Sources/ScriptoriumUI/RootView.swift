import SwiftUI
import ScriptoriumKit

public struct RootView: View {
    /// Coming back to the foreground is the moment a phone usually has signal
    /// again, so it is the moment to send what has been waiting.
    @Environment(\.scenePhase) private var scenePhase
    @StateObject private var session: SessionStore

    public init(baseURL: URL) {
        _session = StateObject(wrappedValue: SessionStore(baseURL: baseURL))
    }

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
