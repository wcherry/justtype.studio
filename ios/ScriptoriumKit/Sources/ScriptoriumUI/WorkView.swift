import SwiftUI
import ScriptoriumKit

struct WorkView: View {
    let workId: String
    let title: String

    @EnvironmentObject private var session: SessionStore
    @State private var outline: API.OutlineResponse?
    @State private var addingScene: String?
    @State private var newSceneTitle = ""

    var body: some View {
        List {
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
        .navigationDestination(for: API.Scene.self) { scene in
            SceneEditorView(sceneId: scene.id, title: scene.title)
        }
        .toolbar {
            NavigationLink {
                VersionsView(workId: workId)
            } label: {
                Label("Versions", systemImage: "clock.arrow.circlepath")
            }
        }
        .task { await load() }
        .refreshable { await load() }
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

    private func load() async {
        outline = try? await session.client.outline(workId: workId)
    }
}
