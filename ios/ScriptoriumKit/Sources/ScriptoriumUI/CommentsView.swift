import SwiftUI
import ScriptoriumKit

struct CommentsView: View {
    let publicationId: String
    let comments: [API.CommentView]
    let isAuthor: Bool
    let onChange: () async -> Void

    @EnvironmentObject private var session: SessionStore
    @Environment(\.dismiss) private var dismiss
    @State private var replyingTo: API.CommentView?

    var body: some View {
        NavigationStack {
            List {
                if comments.isEmpty {
                    ContentUnavailableView(
                        "No feedback yet",
                        systemImage: "text.bubble",
                        description: Text("Tap any paragraph in the manuscript to comment on it.")
                    )
                }
                ForEach(comments) { comment in
                    CommentRow(comment: comment, isAuthor: isAuthor, depth: 0, onReply: { replyingTo = $0 }, onChange: onChange)
                }
            }
            .navigationTitle("Feedback")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .sheet(item: $replyingTo) { parent in
                NewCommentSheet(quote: parent.body) { text, _ in
                    replyingTo = nil
                    Task {
                        _ = try? await session.client.addComment(
                            publicationId: publicationId,
                            body: text,
                            parentId: parent.id
                        )
                        await onChange()
                    }
                }
            }
        }
    }
}

struct CommentRow: View {
    let comment: API.CommentView
    let isAuthor: Bool
    let depth: Int
    let onReply: (API.CommentView) -> Void
    let onChange: () async -> Void

    @EnvironmentObject private var session: SessionStore

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                VStack(spacing: 0) {
                    Button {
                        Task {
                            _ = try? await session.client.vote(
                                commentId: comment.id,
                                value: comment.viewerVote == 1 ? 0 : 1
                            )
                            await onChange()
                        }
                    } label: {
                        Image(systemName: comment.viewerVote == 1 ? "arrowtriangle.up.fill" : "arrowtriangle.up")
                    }
                    .buttonStyle(.plain)
                    Text("\(comment.score)").font(.caption2)
                }
                .foregroundStyle(comment.viewerVote == 1 ? AnyShapeStyle(.tint) : AnyShapeStyle(.secondary))

                VStack(alignment: .leading, spacing: 4) {
                    HStack {
                        Text(comment.author.displayName).font(.subheadline.bold())
                        Text(comment.category.rawValue)
                            .font(.caption2)
                            .padding(.horizontal, 6).padding(.vertical, 1)
                            .background(.quaternary, in: Capsule())
                        if comment.state == .resolved {
                            Text("resolved").font(.caption2).foregroundStyle(.secondary)
                        }
                    }
                    if let quote = comment.anchorQuote, !quote.isEmpty {
                        Text(quote)
                            .font(.system(.caption, design: .serif))
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                            .padding(.leading, 6)
                            .overlay(alignment: .leading) {
                                Rectangle().frame(width: 2).foregroundStyle(.tint)
                            }
                    }
                    Text(comment.body).font(.callout)

                    HStack(spacing: 14) {
                        Button("Reply") { onReply(comment) }
                        if isAuthor, comment.state == .open {
                            Button("Resolve") {
                                Task {
                                    _ = try? await session.client.resolve(commentId: comment.id)
                                    await onChange()
                                }
                            }
                        }
                    }
                    .font(.caption)
                    .buttonStyle(.plain)
                    .foregroundStyle(.tint)
                }
            }
            .padding(.leading, CGFloat(depth) * 16)

            ForEach(comment.replies) { reply in
                CommentRow(comment: reply, isAuthor: isAuthor, depth: depth + 1, onReply: onReply, onChange: onChange)
            }
        }
    }
}
