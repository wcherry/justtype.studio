import SwiftUI
import JustTypeKit

/// A work: its outline, and whatever scene is open.
///
/// A `NavigationSplitView` rather than a stack, which costs nothing on a phone
/// — it collapses to exactly the stack it was — and gives an iPad the thing an
/// iPad is for: the outline beside the page, so a writer can see where they
/// are in the book while they work on a paragraph of it.
struct WorkView: View {
    let workId: String
    let title: String

    @EnvironmentObject private var session: SessionStore
    @State private var outline: API.OutlineResponse?
    @State private var addingScene: String?
    @State private var newSceneTitle = ""
    @State private var selected: API.Scene?

    var body: some View {
        NavigationSplitView {
            outlineList
        } detail: {
            if let selected {
                SceneEditorView(
                    sceneId: selected.id,
                    title: selected.title,
                    editable: outline?.canWrite(chapterId: selected.chapterId) ?? true
                )
                .id(selected.id)
            } else {
                ContentUnavailableView(
                    "Pick a scene",
                    systemImage: "text.alignleft",
                    description: Text("Or add one. A chapter with nothing in it is still a plan.")
                )
            }
        }
        .task { await load() }
        .alert("New scene", isPresented: .constant(addingScene != nil)) {
            TextField("Title", text: $newSceneTitle)
            Button("Add") {
                let chapterId = addingScene
                addingScene = nil
                Task {
                    if let chapterId, !newSceneTitle.isEmpty {
                        _ = try? await session.client.createScene(chapterId: chapterId, title: newSceneTitle)
                        newSceneTitle = ""
                        await load()
                    }
                }
            }
            Button("Cancel", role: .cancel) { addingScene = nil }
        }
    }

    private var outlineList: some View {
        List(selection: $selected) {
            if let outline {
                ForEach(outline.chapters) { chapter in
                    Section {
                        ForEach(chapter.scenes) { scene in
                            NavigationLink(value: scene) {
                                HStack {
                                    Text(scene.title)
                                    Spacer()
                                    Text("\(scene.wordCount)")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                        Button {
                            addingScene = chapter.id
                        } label: {
                            Label("Add scene", systemImage: "plus")
                                .font(.footnote)
                        }
                    } header: {
                        Text(chapter.title)
                    }
                }
            } else {
                ProgressView()
            }
        }
        .navigationTitle(outline?.title ?? title)
        .toolbar {
            NavigationLink {
                VersionsView(workId: workId)
            } label: {
                Label("Versions", systemImage: "clock.arrow.circlepath")
            }
        }
        .refreshable { await load() }
        // Keyboard shortcuts, for an iPad with a keyboard attached. Moving
        // between scenes without reaching for the screen is most of what makes
        // one usable for a day's work.
        .background {
            VStack {
                Button("Next scene") { step(1) }
                    .keyboardShortcut(.downArrow, modifiers: [.command, .option])
                Button("Previous scene") { step(-1) }
                    .keyboardShortcut(.upArrow, modifiers: [.command, .option])
            }
            .opacity(0)
        }
    }

    /// Scenes in reading order, which is the order the keyboard walks them in.
    private var scenes: [API.Scene] {
        (outline?.chapters ?? []).flatMap(\.scenes)
    }

    private func step(_ by: Int) {
        guard !scenes.isEmpty else { return }
        guard let current = selected.flatMap({ chosen in scenes.firstIndex(where: { $0.id == chosen.id }) })
        else {
            selected = scenes.first
            return
        }
        let next = current + by
        guard scenes.indices.contains(next) else { return }
        selected = scenes[next]
    }

    private func load() async {
        outline = await session.sync.outline(workId: workId)?.outline
    }
}
