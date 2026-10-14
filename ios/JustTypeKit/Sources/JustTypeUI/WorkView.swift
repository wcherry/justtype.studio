import SwiftUI
import JustTypeKit

/// A work: its outline, and whatever scene is open.
///
/// It is pushed onto the desk's `NavigationStack`, so it cannot be a
/// `NavigationSplitView`: SwiftUI does not support one inside a stack, and on
/// a phone it drew an empty page. A phone pushes the scene onto the same stack;
/// an iPad (regular width) keeps the outline beside the page, so a writer can
/// see where they are in the book while they work on a paragraph of it.
struct WorkView: View {
    let workId: String
    let title: String

    @EnvironmentObject private var session: SessionStore
    @State private var outline: API.OutlineResponse?
    @State private var selected: API.Scene?
    @Environment(\.horizontalSizeClass) private var sizeClass

    var body: some View {
        Group {
            if sizeClass == .regular {
                HStack(spacing: 0) {
                    outlineList
                        .frame(width: 320)
                    Divider()
                    detail
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            } else {
                outlineList
                    .navigationDestination(item: $selected) { scene in
                        editor(for: scene)
                    }
            }
        }
        .task { await load() }
    }

    @ViewBuilder
    private var detail: some View {
        if let selected {
            editor(for: selected)
        } else {
            ContentUnavailableView(
                "Pick a scene",
                systemImage: "text.alignleft",
                description: Text("Or add one. A chapter with nothing in it is still a plan.")
            )
        }
    }

    private func editor(for scene: API.Scene) -> some View {
        SceneEditorView(
            sceneId: scene.id,
            title: name(of: scene),
            editable: outline?.canWrite(chapterId: scene.chapterId) ?? true
        )
        .id(scene.id)
    }

    /// A new scene is untitled — shown as "Scene 3" for where it falls — rather
    /// than asked for a name, so adding one never stops the writing; it opens
    /// straight away.
    private func addScene(to chapter: API.OutlineChapter) async {
        guard let created = try? await session.client.createScene(chapterId: chapter.id, title: "")
        else { return }
        await load()
        selected = created
    }

    /// A scene's title, or "Scene 2" for where it sits in its chapter.
    private func name(of scene: API.Scene) -> String {
        guard Naming.isUntitled(scene.title) else { return scene.title }
        let chapter = outline?.chapters.first(where: { $0.id == scene.chapterId })
        let siblings = chapter?.scenes ?? []
        let number = (siblings.firstIndex(where: { $0.id == scene.id }) ?? siblings.count) + 1
        return Naming.scene(scene.title, number: number, role: chapter?.role ?? "chapter")
    }

    private var outlineList: some View {
        List {
            if let outline {
                let names = Naming.chapters(outline.chapters.map { ($0.title, $0.role) })
                ForEach(Array(outline.chapters.enumerated()), id: \.element.id) { chapterIndex, chapter in
                    Section {
                        ForEach(Array(chapter.scenes.enumerated()), id: \.element.id) { sceneIndex, scene in
                            Button {
                                selected = scene
                            } label: {
                                HStack {
                                    Text(Naming.scene(scene.title, number: sceneIndex + 1, role: chapter.role))
                                        .foregroundStyle(Naming.isUntitled(scene.title) ? .secondary : .primary)
                                        .italic(Naming.isUntitled(scene.title))
                                    Spacer()
                                    Text("\(scene.wordCount)")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                    if sizeClass != .regular {
                                        Image(systemName: "chevron.right")
                                            .font(.caption.weight(.semibold))
                                            .foregroundStyle(.tertiary)
                                    }
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .listRowBackground(
                                sizeClass == .regular && selected?.id == scene.id
                                    ? Color.accentColor.opacity(0.15) : nil
                            )
                        }
                        Button {
                            Task { await addScene(to: chapter) }
                        } label: {
                            Label("Add scene", systemImage: "plus")
                                .font(.footnote)
                        }
                    } header: {
                        HStack(spacing: 6) {
                            Text(names[chapterIndex])
                            if let badge = Naming.roleBadge(chapter.role) {
                                Text(badge)
                                    .font(.caption2)
                                    .textCase(nil)
                                    .padding(.horizontal, 6).padding(.vertical, 1)
                                    .background(.quaternary, in: Capsule())
                                    .accessibilityLabel("Role: \(badge)")
                            }
                        }
                    }
                }
            } else {
                ProgressView()
            }
        }
        .navigationTitle(outline?.title ?? title)
        .toolbar {
            NavigationLink {
                BookDetailsView(workId: workId)
            } label: {
                Label("Publishing details", systemImage: "info.circle")
            }
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
