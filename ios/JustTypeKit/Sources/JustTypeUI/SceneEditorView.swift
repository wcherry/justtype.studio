import SwiftUI
import JustTypeKit

/// The mobile writing surface.
///
/// It edits the document, not a flattening of it: marks, headings, quotes and
/// lists all survive a round trip, because the thing doing the editing is the
/// same ProseMirror the browser runs. A scene written on the web opens here
/// and goes back with every mark intact — which is what 4.1 was for.
struct SceneEditorView: View {
    let sceneId: String
    let title: String
    /// False for someone on the book to read or suggest, or handed other
    /// chapters: the text opens without a caret rather than refusing saves.
    var editable = true

    @EnvironmentObject private var session: SessionStore
    @State private var scene: API.SceneWithContent?
    @State private var document = API.DocNode.empty
    @State private var words = 0
    @State private var activeMarks: Set<String> = []
    @State private var command: WebEditorView.Command?
    @State private var issued = 0
    @State private var problem: String?
    @State private var saveState: SaveState = .idle
    @State private var saveTask: Task<Void, Never>?
    /// The hash this document was built on top of, sent with every save so the
    /// server can refuse a write whose base has moved.
    @State private var baseHash: String?
    @State private var showConflict = false

    enum SaveState {
        case idle, dirty, saving, saved, queued, conflict

        var label: String {
            switch self {
            case .idle: ""
            case .dirty: "Unsaved"
            case .saving: "Saving…"
            case .saved: "Saved"
            // Not a failure. The words are on the phone and they will go up.
            case .queued: "Kept on this phone"
            case .conflict: "Changed elsewhere"
            }
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let problem {
                Label(problem, systemImage: "exclamationmark.triangle")
                    .font(.footnote)
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.yellow.opacity(0.15))
            }

            if !editable {
                Label("Read only — this chapter is not yours to change.", systemImage: "lock")
                    .font(.footnote)
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.secondary.opacity(0.1))
            }

            WebEditorView(
                document: document,
                command: command,
                editable: editable,
                onChange: { doc, count in
                    document = doc
                    words = count
                    scheduleSave()
                },
                onSelection: { activeMarks = $0.marks },
                onProblem: { problem = $0 }
            )

            if editable {
                FormatBar(active: activeMarks) { send($0) }
            }
        }
        // The shortcuts a writer with a keyboard already has muscle memory
        // for. The web view swallows ⌘B itself when it has focus; these are
        // for the rest of the time, and for an iPad's Command key overlay.
        .background {
            VStack {
                Button("Bold") { send("bold") }.keyboardShortcut("b")
                Button("Italic") { send("italic") }.keyboardShortcut("i")
                Button("Underline") { send("underline") }.keyboardShortcut("u")
                Button("Save now") { Task { await save(createRevision: true) } }
                    .keyboardShortcut("s")
            }
            .opacity(0)
        }
        .navigationTitle(scene?.title ?? title)
        .alert("This scene changed somewhere else", isPresented: $showConflict) {
            Button("Reload it") {
                Task {
                    await session.sync.discardMine(sceneId: sceneId)
                    await load()
                }
            }
            Button("Keep mine", role: .destructive) {
                Task {
                    // Rebase onto whatever the server has now, which is how a
                    // client says "I have seen theirs and still mean this".
                    let outcome = await session.sync.keepMine(sceneId: sceneId)
                    if case .saved(let hash) = outcome {
                        baseHash = hash
                        saveState = .saved
                    }
                    await session.refreshPendingCount()
                }
            }
        } message: {
            Text("Another device saved this scene while you were editing. Your text is still here until you choose.")
        }
        .toolbar {
            ToolbarItem(placement: .automatic) {
                HStack(spacing: 10) {
                    Text("\(words) words")
                    Text(saveState.label)
                        .foregroundStyle(saveState == .conflict ? .orange : .secondary)
                }
                .font(.caption)
                .foregroundStyle(.secondary)
            }
        }
        .task { await load() }
        .onDisappear {
            saveTask?.cancel()
            if saveState == .dirty { Task { await save(createRevision: true) } }
        }
    }

    private func send(_ name: String) {
        // The shortcuts outlive the format bar; a command is an edit.
        guard editable else { return }
        issued += 1
        command = WebEditorView.Command(name: name, issue: issued)
    }

    private func load() async {
        // Through the sync engine, so an unsent edit wins over the server's
        // older copy — handing a writer their own work back, undone, is the
        // worst thing an offline mode can do.
        guard let loaded = await session.sync.scene(id: sceneId) else { return }
        scene = loaded.scene
        baseHash = loaded.scene.contentHash
        document = loaded.scene.content
        words = loaded.scene.content.wordCount
        saveState = await session.sync.pending(for: sceneId) == nil ? .idle : .queued
        await session.refreshPendingCount()
        // So "start writing" comes back here, and the widget knows whose
        // words it is counting.
        await session.publishWritingSnapshot(
            lastScene: (id: loaded.scene.id, title: loaded.scene.title, workId: loaded.scene.workId)
        )
    }

    /// Debounced autosave, with a checkpoint revision when the writer pauses.
    private func scheduleSave() {
        guard scene != nil, editable else { return }
        saveState = .dirty
        saveTask?.cancel()
        saveTask = Task {
            try? await Task.sleep(for: .milliseconds(1200))
            guard !Task.isCancelled else { return }
            await save(createRevision: false)
            try? await Task.sleep(for: .seconds(20))
            guard !Task.isCancelled else { return }
            await save(createRevision: true)
        }
    }

    private func save(createRevision: Bool) async {
        guard let scene, editable else { return }
        saveState = .saving
        let outcome = await session.sync.save(
            sceneId: scene.id,
            content: document,
            baseHash: baseHash,
            createRevision: createRevision
        )
        switch outcome {
        case .saved(let hash):
            baseHash = hash
            saveState = .saved
            await session.publishWritingSnapshot()
        case .queued:
            saveState = .queued
        case .conflict:
            // Not a failure to retry: retrying unchanged can only fail again.
            saveState = .conflict
            showConflict = true
        }
        await session.refreshPendingCount()
    }
}

/// The formatting a thumb can reach.
///
/// Deliberately short. A phone is where a writer adds a line on a train, not
/// where they lay out a book, and a row of twenty buttons would cost more
/// screen than the paragraph it is formatting.
private struct FormatBar: View {
    let active: Set<String>
    let send: (String) -> Void

    private static let buttons: [(name: String, icon: String, label: String)] = [
        ("bold", "bold", "Bold"),
        ("italic", "italic", "Italic"),
        ("underline", "underline", "Underline"),
        ("h2", "textformat.size", "Heading"),
        ("quote", "text.quote", "Quote"),
        ("bullet", "list.bullet", "List"),
        // A phone has no ⌘Z, and shaking it at a paragraph is not a design.
        ("undo", "arrow.uturn.backward", "Undo"),
        ("redo", "arrow.uturn.forward", "Redo"),
    ]

    var body: some View {
        HStack(spacing: 0) {
            ForEach(Self.buttons, id: \.name) { button in
                Button {
                    send(button.name)
                } label: {
                    Image(systemName: button.icon)
                        .frame(maxWidth: .infinity, minHeight: 44)
                        .background(active.contains(button.name) ? Color.accentColor.opacity(0.18) : .clear)
                }
                .accessibilityLabel(button.label)
                .accessibilityIdentifier("format.\(button.name)")
            }
        }
        .buttonStyle(.plain)
        .background(.thinMaterial)
        .overlay(alignment: .top) { Divider() }
    }
}
