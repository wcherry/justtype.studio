import SwiftUI
import JustTypeKit

/// One answer, whole, with its critiques below.
struct PromptEntryReadView: View {
    let entryId: String
    @EnvironmentObject private var session: SessionStore
    @State private var entry: API.PromptEntryView?
    @State private var problem: String?
    @State private var converting = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                if let problem {
                    ContentUnavailableView("Not here", systemImage: "doc.questionmark", description: Text(problem))
                }
                if let entry {
                    Text(entry.prompt)
                        .font(.system(.callout, design: .serif))
                        .foregroundStyle(.secondary)
                    HStack(spacing: 6) {
                        Text(entry.author.displayName).font(.subheadline.weight(.semibold))
                        Text("· \(entry.wordCount.formatted()) words").font(.caption).foregroundStyle(.secondary)
                        if entry.late { Tag("late") }
                        if entry.editedAt != nil { Tag("edited") }
                        if entry.mine && entry.state != .submitted { Tag(entry.state.rawValue) }
                    }
                    if entry.mine {
                        HStack(spacing: 16) {
                            NavigationLink("Edit", value: PromptRoute.write(entry.day))
                            Button("Turn into a project") { converting = true }
                        }
                        .font(.footnote)
                    }
                    EntryReadingText(document: entry.document)
                    Divider()
                    CritiquesView(entryId: entry.id, open: entry.state == .submitted, isAuthor: entry.mine)
                }
            }
            .padding()
        }
        .navigationTitle(entry.map { PromptDays.name($0.day) } ?? "Answer")
        .sheet(isPresented: $converting) { ToWorkSheet(entryId: entryId) }
        .task {
            do {
                entry = try await session.client.promptEntry(id: entryId)
            } catch APIError.http(403, _, _) {
                problem = "Join Daily Prompts to read the answers."
            } catch {
                problem = "It may have been withdrawn or deleted."
            }
        }
    }
}

/// The answer's text, as plain paragraphs: read here, not edited, and short
/// enough that the web view's reader would be more weight than it is worth.
struct EntryReadingText: View {
    let document: API.DocNode
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(Array(document.plainParagraphs.enumerated()), id: \.offset) { _, paragraph in
                Text(paragraph)
                    .font(.system(.body, design: .serif))
                    .lineSpacing(5)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .textSelection(.enabled)
    }
}

extension API.DocNode {
    /// Each block's text, in order, blank ones dropped.
    var plainParagraphs: [String] {
        func text(_ node: API.DocNode) -> String {
            if node.type == "hardBreak" { return "\n" }
            if let t = node.text { return t }
            return (node.content ?? []).map(text).joined()
        }
        return (content ?? []).map { block -> String in
            if block.type == "horizontalRule" { return "⁂" }
            return text(block)
        }
        .filter { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    }
}

/// Critiques and replies, to any depth. On a phone they indent three levels;
/// deeper replies carry on at that depth and say whom they answer, so a long
/// thread stays readable on the smallest screen.
struct CritiquesView: View {
    let entryId: String
    let open: Bool
    let isAuthor: Bool

    static let maxIndent = 3

    @EnvironmentObject private var session: SessionStore
    @State private var critiques: [API.CritiqueView] = []
    @State private var draft = ""
    @State private var replyingTo: API.CritiqueView?
    @State private var editing: API.CritiqueView?
    @State private var reporting: API.CritiqueView?
    @State private var problem: String?
    @State private var notice: String?

    struct Row: Identifiable {
        let critique: API.CritiqueView
        let depth: Int
        let replyingTo: String?
        var id: String { critique.id }
    }

    /// The tree flattened in reading order, each with its shown depth.
    static func arrange(_ list: [API.CritiqueView], maxIndent: Int = maxIndent) -> [Row] {
        let children = Dictionary(grouping: list, by: { $0.parentId ?? "" })
        let byId = Dictionary(uniqueKeysWithValues: list.map { ($0.id, $0) })
        var rows: [Row] = []
        func walk(_ parent: String, _ depth: Int) {
            for c in children[parent] ?? [] {
                let name = c.parentId.flatMap { byId[$0]?.author?.displayName } ?? "a removed critique"
                rows.append(Row(critique: c, depth: min(depth, maxIndent), replyingTo: depth > maxIndent ? name : nil))
                walk(c.id, depth + 1)
            }
        }
        walk("", 0)
        return rows
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            let live = critiques.filter { !$0.removed }.count
            Text(live == 1 ? "1 critique" : "\(live) critiques").font(.headline)
            if !open && isAuthor && !critiques.isEmpty {
                Text("Withdrawn: only you, and each critic their own, can see these.").font(.caption).foregroundStyle(.secondary)
            }
            if let problem { Text(problem).font(.footnote).foregroundStyle(.red) }
            if let notice { Text(notice).font(.footnote).foregroundStyle(.secondary) }
            ForEach(Self.arrange(critiques)) { row in
                CritiqueRow(
                    row: row,
                    open: open,
                    isAuthor: isAuthor,
                    reply: { replyingTo = row.critique },
                    edit: { editing = row.critique },
                    remove: { Task { await act { try await session.client.removeCritique(id: row.critique.id) } } },
                    report: { reporting = row.critique }
                )
            }
            if open && !isAuthor {
                TextField("Your critique", text: $draft, axis: .vertical)
                    .lineLimit(3...8)
                    .textFieldStyle(.roundedBorder)
                Button("Post") {
                    let text = draft
                    Task { await act { _ = try await session.client.critique(entryId: entryId, body: text) }; draft = "" }
                }
                .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                Text("Your name is shown with it. Everyone in Daily Prompts can read it.").font(.caption).foregroundStyle(.secondary)
            }
        }
        .task { await load() }
        .sheet(item: $replyingTo) { parent in
            CritiqueComposer(title: "Reply to \(parent.author?.displayName ?? "this")", initial: "") { text in
                await act { _ = try await session.client.critique(entryId: entryId, body: text, parentId: parent.id) }
            }
        }
        .sheet(item: $editing) { critique in
            CritiqueComposer(title: "Edit your critique", initial: critique.body) { text in
                await act { _ = try await session.client.editCritique(id: critique.id, body: text) }
            }
        }
        .confirmationDialog("Report this critique to the writer of the answer?", isPresented: Binding(get: { reporting != nil }, set: { if !$0 { reporting = nil } }), titleVisibility: .visible) {
            ForEach([(API.ReportReason.abuse, "Abusive"), (.spam, "Spam"), (.off_topic, "Not about the writing"), (.other, "Something else")], id: \.0) { reason, label in
                Button(label) {
                    guard let target = reporting else { return }
                    Task {
                        await act { try await session.client.reportCritique(id: target.id, reason: reason) }
                        notice = "Reported. The writer of this answer has been told."
                    }
                }
            }
        }
    }

    private func load() async {
        do {
            critiques = try await session.client.critiques(entryId: entryId)
            problem = nil
        } catch {
            problem = error.localizedDescription
        }
    }

    private func act(_ work: () async throws -> Void) async {
        do {
            try await work()
            await load()
        } catch {
            problem = error.localizedDescription
        }
    }
}

extension API.CritiqueView: Identifiable {}

private struct CritiqueRow: View {
    let row: CritiquesView.Row
    let open: Bool
    let isAuthor: Bool
    let reply: () -> Void
    let edit: () -> Void
    let remove: () -> Void
    let report: () -> Void
    @State private var confirmRemove = false

    var body: some View {
        let c = row.critique
        VStack(alignment: .leading, spacing: 4) {
            if c.removed {
                Text("Removed").italic().font(.footnote).foregroundStyle(.secondary)
            } else {
                HStack(spacing: 4) {
                    Text(c.author?.displayName ?? "").font(.footnote.weight(.semibold))
                    if let name = row.replyingTo {
                        Text("replying to \(name)").font(.caption2).foregroundStyle(.secondary)
                    }
                    if c.editedAt != nil { Text("· edited").font(.caption2).foregroundStyle(.secondary) }
                }
                Text(c.body).font(.callout).fixedSize(horizontal: false, vertical: true)
                HStack(spacing: 14) {
                    if open { Button("Reply", action: reply) }
                    if c.mine { Button("Edit", action: edit) }
                    if c.canRemove { Button(c.mine ? "Delete" : "Remove", role: .destructive) { confirmRemove = true } }
                    if !c.mine && !isAuthor { Button("Report", action: report) }
                }
                .font(.caption)
                .buttonStyle(.borderless)
            }
        }
        .padding(.leading, CGFloat(row.depth) * 14 + 8)
        .overlay(alignment: .leading) {
            Rectangle().fill(.quaternary).frame(width: 2).padding(.leading, CGFloat(row.depth) * 14)
        }
        .confirmationDialog(c.mine ? "Delete your critique?" : "Remove this critique from your answer?", isPresented: $confirmRemove, titleVisibility: .visible) {
            Button(c.mine ? "Delete" : "Remove", role: .destructive, action: remove)
        }
    }
}

private struct CritiqueComposer: View {
    let title: String
    let initial: String
    let send: (String) async -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""

    var body: some View {
        NavigationStack {
            Form {
                TextField("Write it here", text: $text, axis: .vertical).lineLimit(4...12)
            }
            .navigationTitle(title)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Send") {
                        Task {
                            await send(text)
                            dismiss()
                        }
                    }
                    .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
            .onAppear { text = initial }
        }
    }
}
