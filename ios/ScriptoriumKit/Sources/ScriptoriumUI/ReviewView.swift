import SwiftUI
import ScriptoriumKit

/// The reading and reviewing surface.
///
/// Each paragraph knows the ProseMirror positions it occupies, so tapping one
/// produces an anchor identical to the one the web client would produce for the
/// same passage. Sub-paragraph selection needs a text-view bridge and is on the
/// roadmap; paragraph-level anchoring is exact today.
struct ReviewView: View {
    let card: PublicationCard

    @EnvironmentObject private var session: SessionStore
    @State private var content: VersionContent?
    @State private var comments: [CommentView] = []
    @State private var pending: PendingAnchor?
    @State private var showingComments = false

    struct PendingAnchor: Identifiable {
        let sceneId: String
        let from: Int
        let to: Int
        let quote: String
        var id: String { "\(sceneId):\(from)-\(to)" }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                if let prompt = card.reviewPrompt, !prompt.isEmpty {
                    Text("The author asked: \(prompt)")
                        .font(.footnote)
                        .padding(12)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
                }

                ForEach(content?.chapters ?? []) { chapter in
                    Text(chapter.title)
                        .font(.system(.title3, design: .serif))
                        .padding(.top, 8)

                    ForEach(chapter.scenes) { scene in
                        ForEach(scene.content.blocks()) { block in
                            if !block.isEmpty {
                                ParagraphView(
                                    block: block,
                                    commentCount: commentCount(sceneId: scene.sceneId, block: block)
                                )
                                .onTapGesture {
                                    pending = PendingAnchor(
                                        sceneId: scene.sceneId,
                                        from: block.from,
                                        to: block.to,
                                        quote: block.text
                                    )
                                }
                            }
                        }
                    }
                }
            }
            .padding(18)
        }
        .navigationTitle(card.workTitle)
        .toolbar {
            Button { showingComments = true } label: {
                Label("\(comments.count)", systemImage: "text.bubble")
            }
        }
        .task { await load() }
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
            CommentsView(publicationId: card.id, comments: comments, isAuthor: session.user?.id == card.author.id) {
                await loadComments()
            }
        }
    }

    private func commentCount(sceneId: String, block: DocBlock) -> Int {
        comments.filter { comment in
            guard comment.sceneId == sceneId, let range = comment.anchorRange else { return false }
            return range.from >= block.from && range.to <= block.to
        }.count
    }

    private func load() async {
        content = try? await session.client.publicationContent(id: card.id)
        await loadComments()
    }

    private func loadComments() async {
        comments = (try? await session.client.comments(publicationId: card.id)) ?? []
    }
}

struct ParagraphView: View {
    let block: DocBlock
    let commentCount: Int

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Text(block.text)
                .font(.system(block.type == "heading" ? .title3 : .body, design: .serif))
                .lineSpacing(5)
                .frame(maxWidth: .infinity, alignment: .leading)
            if commentCount > 0 {
                Label("\(commentCount)", systemImage: "bubble.left")
                    .font(.caption2)
                    .foregroundStyle(.tint)
            }
        }
        .contentShape(Rectangle())
        .padding(.vertical, 2)
        .background(commentCount > 0 ? Color.yellow.opacity(0.12) : .clear)
    }
}

struct NewCommentSheet: View {
    let quote: String
    let onSubmit: (String, CommentCategory) -> Void

    @State private var body_ = ""
    @State private var category: CommentCategory = .general
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
                        ForEach(CommentCategory.allCases, id: \.self) {
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
