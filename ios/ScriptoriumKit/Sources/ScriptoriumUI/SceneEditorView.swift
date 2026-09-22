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

    enum SaveState {
        case idle, dirty, saving, saved, failed

        var label: String {
            switch self {
            case .idle: ""
            case .dirty: "Unsaved"
            case .saving: "Saving…"
            case .saved: "Saved"
            case .failed: "Save failed"
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
            _ = try await session.client.saveScene(
                id: scene.id,
                content: DocNode.fromPlainText(text),
                createRevision: createRevision
            )
            saveState = .saved
        } catch {
            saveState = .failed
        }
    }
}
