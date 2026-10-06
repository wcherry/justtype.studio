import SwiftUI
import JustTypeKit

/// The reading and reviewing surface.
///
/// The same page the editor uses, with the caret turned off: still a real
/// ProseMirror document, so a reader can select *words* rather than tapping a
/// paragraph, and the anchor that produces is the same one the web client
/// would produce for the same passage. Character-level anchoring on a phone is
/// what 4.3 was for, and this is the whole of how it works — one flag.
struct ReviewView: View {
    let card: API.PublicationCard

    @EnvironmentObject private var session: SessionStore
    @State private var content: API.VersionContent?
    @State private var index = 0
    @State private var selection: WebEditorView.Selection?
    @State private var comments: [API.CommentView] = []
    @State private var pending: PendingAnchor?
    @State private var showingComments = false
    @State private var showingType = false
    @State private var page = (current: 1, of: 1)

    // A reader's settings, kept: nobody wants to set the type size twice.
    @AppStorage("reader.size") private var size = 17.0
    @AppStorage("reader.family") private var family = "serif"
    @AppStorage("reader.leading") private var leading = 1.6
    @AppStorage("reader.paged") private var paged = false

    struct PendingAnchor: Identifiable {
        let sceneId: String
        let from: Int
        let to: Int
        let quote: String
        var id: String { "\(sceneId):\(from)-\(to)" }
    }

    private var scenes: [API.ContentScene] {
        (content?.chapters ?? []).flatMap(\.scenes)
    }

    private var scene: API.ContentScene? {
        scenes.indices.contains(index) ? scenes[index] : nil
    }

    var body: some View {
        VStack(spacing: 0) {
            if let prompt = card.reviewPrompt, !prompt.isEmpty, index == 0 {
                Text("The author asked: \(prompt)")
                    .font(.footnote)
                    .padding(10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.quaternary)
            }

            if let scene {
                WebEditorView(
                    document: scene.content,
                    editable: false,
                    typography: .init(size: size, family: family, leading: leading, paged: paged),
                    onSelection: { selection = $0.isEmpty ? nil : $0 },
                    onPages: { page = ($0, $1) }
                )
                .id(scene.sceneId)
            } else {
                ProgressView().frame(maxHeight: .infinity)
            }

            if let selection, let scene {
                // The bar only exists while there is a selection to act on,
                // which keeps the reading surface a reading surface.
                Button {
                    pending = PendingAnchor(
                        sceneId: scene.sceneId,
                        from: selection.from,
                        to: selection.to,
                        quote: selection.quote
                    )
                } label: {
                    Label("Comment on “\(selection.quote.prefix(28))”", systemImage: "text.bubble")
                        .font(.footnote)
                        .frame(maxWidth: .infinity, minHeight: 44)
                }
                .background(.tint.opacity(0.12))
            } else {
                SceneBar(
                    title: scene?.title ?? "",
                    position: index + 1,
                    count: scenes.count,
                    page: paged ? page : nil,
                    onPrevious: { move(by: -1) },
                    onNext: { move(by: 1) }
                )
            }
        }
        .navigationTitle(card.workTitle)
        #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .automatic) {
                Button { showingType = true } label: {
                    Label("Reading", systemImage: "textformat.size")
                }
            }
            ToolbarItem(placement: .automatic) {
                Button { showingComments = true } label: {
                    Label("\(comments.count)", systemImage: "text.bubble")
                }
            }
        }
        .task { await load() }
        .sheet(isPresented: $showingType) {
            TypographySheet(size: $size, family: $family, leading: $leading, paged: $paged)
                #if os(iOS)
                    .presentationDetents([.height(320)])
                #endif
        }
        .sheet(item: $pending) { anchor in
            NewCommentSheet(quote: anchor.quote) { body, category in
                pending = nil
                Task {
                    _ = try? await session.client.addComment(
                        publicationId: card.id,
                        body: body,
                        sceneId: anchor.sceneId,
                        anchor: (from: anchor.from, to: anchor.to),
                        anchorQuote: anchor.quote,
                        category: category
                    )
                    await loadComments()
                }
            }
        }
        .sheet(isPresented: $showingComments) {
            CommentsView(
                publicationId: card.id,
                comments: comments,
                isAuthor: session.user?.id == card.author.id
            ) {
                await loadComments()
            }
        }
    }

    private func move(by step: Int) {
        let next = index + step
        guard scenes.indices.contains(next) else { return }
        index = next
        selection = nil
        Task { await recordProgress() }
    }

    private func load() async {
        content = try? await session.client.publicationContent(id: card.id)
        await loadComments()
        await recordProgress()
    }

    private func loadComments() async {
        comments = (try? await session.client.comments(publicationId: card.id)) ?? []
    }

    /// Reading on a phone should count for as much as reading in a browser.
    private func recordProgress() async {
        guard let scene else { return }
        try? await session.client.recordProgress(publicationId: card.id, sceneId: scene.sceneId)
    }
}

/// Where you are, and how to get on.
private struct SceneBar: View {
    let title: String
    let position: Int
    let count: Int
    /// Page of pages, when reading paged. Nil when scrolling.
    let page: (current: Int, of: Int)?
    let onPrevious: () -> Void
    let onNext: () -> Void

    var body: some View {
        HStack {
            Button(action: onPrevious) { Image(systemName: "chevron.left") }
                .disabled(position <= 1)
            Spacer()
            VStack(spacing: 1) {
                Text(title).font(.caption)
                Text(page.map { "page \($0.current) of \($0.of) · \(position)/\(count)" }
                    ?? "\(position) of \(count)")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Button(action: onNext) { Image(systemName: "chevron.right") }
                .disabled(position >= count)
        }
        .padding(.horizontal, 20)
        .frame(height: 44)
        .background(.thinMaterial)
        .overlay(alignment: .top) { Divider() }
    }
}

/// How the book should read.
private struct TypographySheet: View {
    @Binding var size: Double
    @Binding var family: String
    @Binding var leading: Double
    @Binding var paged: Bool
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section("Type") {
                    Picker("Face", selection: $family) {
                        Text("Serif").tag("serif")
                        Text("Sans").tag("sans")
                        Text("Mono").tag("mono")
                    }
                    .pickerStyle(.segmented)
                    HStack {
                        Image(systemName: "textformat.size.smaller")
                        Slider(value: $size, in: 13...26, step: 1)
                        Image(systemName: "textformat.size.larger")
                    }
                    HStack {
                        Text("Leading").font(.caption).foregroundStyle(.secondary)
                        Slider(value: $leading, in: 1.2...2.2, step: 0.1)
                    }
                }
                Section {
                    Toggle("Turn pages", isOn: $paged)
                } footer: {
                    Text("A novel in portrait is a stack of pages rather than a scroll.")
                }
            }
            .navigationTitle("Reading")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}

struct NewCommentSheet: View {
    let quote: String
    let onSubmit: (String, API.CommentCategory) -> Void

    @State private var body_ = ""
    @State private var category: API.CommentCategory = .general
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section("On this passage") {
                    Text(quote).font(.system(.footnote, design: .serif)).lineLimit(4)
                }
                Section {
                    TextField("What about this passage?", text: $body_, axis: .vertical)
                        .lineLimit(3...8)
                    Picker("Kind", selection: $category) {
                        ForEach(API.CommentCategory.allCases, id: \.self) {
                            Text($0.rawValue.capitalized).tag($0)
                        }
                    }
                }
            }
            .navigationTitle("Comment")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Post") { onSubmit(body_, category) }
                        .disabled(body_.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
    }
}
