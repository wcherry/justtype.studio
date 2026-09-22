import SwiftUI
import ScriptoriumKit

/// The mobile writing surface.
///
/// It edits the scene as plain paragraphs. A scene that already carries marks or
/// block types this editor cannot represent opens read-only, because flattening
/// an author's formatting on save would be worse than not editing at all.
/// Full rich-text parity is a roadmap item, not a silent compromise.
struct SceneEditorView: View {
    let sceneId: String
    let title: String

    @EnvironmentObject private var session: SessionStore
    @State private var scene: SceneWithContent?
    @State private var text = ""
    @State private var readOnly = false
    @State private var saveState: SaveState = .idle
    @State private var saveTask: Task<Void, Never>?
    /// The hash this text was built on top of, sent with every save so the
    /// server can refuse a write whose base has moved.
    @State private var baseHash: String?
    @State private var showConflict = false

    enum SaveState {
        case idle, dirty, saving, saved, failed, conflict

        var label: String {
            switch self {
            case .idle: ""
            case .dirty: "Unsaved"
            case .saving: "Saving…"
            case .saved: "Saved"
            case .failed: "Save failed"
            case .conflict: "Changed elsewhere"
            }
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if readOnly {
                Label(
                    "This scene uses formatting the mobile editor cannot edit yet. Open it on the web to change it.",
                    systemImage: "exclamationmark.triangle"
                )
                .font(.footnote)
                .padding(12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(.yellow.opacity(0.15))
            }

            TextEditor(text: $text)
                .font(.system(.body, design: .serif))
                .lineSpacing(5)
                .padding(.horizontal, 12)
                .disabled(readOnly)
                .onChange(of: text) { _, _ in scheduleSave() }
        }
        .navigationTitle(scene?.title ?? title)
        .alert("This scene changed somewhere else", isPresented: $showConflict) {
            Button("Reload it") { Task { await load() } }
            Button("Keep mine", role: .destructive) {
                Task {
                    // Adopt the server's current hash, which is how a client
                    // says "I have seen theirs and still mean this".
                    if let latest = try? await session.client.scene(id: sceneId) {
                        baseHash = latest.contentHash
                        await save(createRevision: true)
                    }
                }
            }
        } message: {
            Text("Another device saved this scene while you were editing. Your text is still here until you choose.")
        }
        .toolbar {
            ToolbarItem(placement: .automatic) {
                HStack(spacing: 10) {
                    Text("\(wordCount) words")
                    Text(saveState.label)
                        .foregroundStyle(saveState == .failed ? .red : .secondary)
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

    private var wordCount: Int {
        text.split(whereSeparator: { $0.isWhitespace }).count
    }

    private func load() async {
        guard let loaded = try? await session.client.scene(id: sceneId) else { return }
        scene = loaded
        baseHash = loaded.contentHash
        readOnly = loaded.content.hasRichFormatting
        text = loaded.content.plainText
        saveState = .idle
    }

    /// Debounced autosave, with a checkpoint revision when the writer pauses.
    private func scheduleSave() {
        guard !readOnly, scene != nil else { return }
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
        guard let scene else { return }
        saveState = .saving
        do {
            let result = try await session.client.saveScene(
                id: scene.id,
                content: DocNode.fromPlainText(text),
                createRevision: createRevision,
                baseHash: baseHash
            )
            baseHash = result.scene.contentHash
            saveState = .saved
        } catch APIError.conflict {
            // Not a failure to retry: retrying unchanged can only fail again.
            saveState = .conflict
            showConflict = true
        } catch {
            saveState = .failed
        }
    }
}
