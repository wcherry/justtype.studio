import SwiftUI
import JustTypeKit

/// Answering a prompt: the scene editor's page, for one answer. It saves to
/// the phone first and goes up when it can, so an answer written on a plane
/// is there to submit on landing.
struct PromptWriteView: View {
    let day: String

    @EnvironmentObject private var session: SessionStore
    @Environment(\.dismiss) private var dismiss
    @State private var prompt: API.PromptView?
    @State private var entry: API.PromptEntryView?
    @State private var enrolled = true
    @State private var document = API.DocNode.empty
    @State private var loaded = false
    @State private var words = 0
    @State private var baseHash: String?
    @State private var saveState: SceneEditorView.SaveState = .idle
    @State private var saveTask: Task<Void, Never>?
    @State private var activeMarks: Set<String> = []
    @State private var command: WebEditorView.Command?
    @State private var issued = 0
    @State private var problem: String?
    @State private var showConflict = false
    @State private var converting = false
    @State private var confirmDelete = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let prompt {
                Text(prompt.body)
                    .font(.system(.callout, design: .serif))
                    .padding(.horizontal, 16).padding(.vertical, 10)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.secondary.opacity(0.08))
            }
            if let problem {
                Label(problem, systemImage: "exclamationmark.triangle")
                    .font(.footnote).padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(.yellow.opacity(0.15))
            }
            if !enrolled {
                Label("Join Daily Prompts to write an answer.", systemImage: "lock")
                    .font(.footnote).padding(12)
            }
            if let note = stateNote {
                Text(note).font(.caption).foregroundStyle(.secondary).padding(.horizontal, 16).padding(.top, 6)
            }
            if loaded {
                WebEditorView(
                    document: document,
                    command: command,
                    editable: enrolled,
                    onChange: { doc, count in
                        document = doc
                        words = count
                        scheduleSave()
                    },
                    onSelection: { activeMarks = $0.marks },
                    onProblem: { problem = $0 }
                )
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .navigationTitle(PromptDays.name(day))
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .automatic) {
                HStack(spacing: 8) {
                    Text("\(words) words")
                    Text(saveState.label).foregroundStyle(saveState == .conflict ? .orange : .secondary)
                }
                .font(.caption).foregroundStyle(.secondary)
            }
            ToolbarItem(placement: .primaryAction) {
                Menu {
                    if enrolled && entry?.state != .submitted {
                        Button(entry?.state == .withdrawn ? "Submit again" : "Submit") { Task { await act { try await session.client.submitEntry(day: day) } } }
                            .disabled(words == 0)
                    }
                    if enrolled && entry?.state == .submitted {
                        Button("Withdraw") { Task { await act { try await session.client.withdrawEntry(day: day) } } }
                    }
                    if entry != nil {
                        Button("Turn into a project") { Task { await save(); converting = true } }
                        Button("Delete", role: .destructive) { confirmDelete = true }
                    }
                } label: {
                    Label("Answer", systemImage: "ellipsis.circle")
                }
            }
        }
        .alert("This answer changed somewhere else", isPresented: $showConflict) {
            Button("Reload it") {
                Task {
                    await session.sync.discardMyEntry(day: day)
                    await load()
                }
            }
            Button("Keep mine", role: .destructive) {
                Task {
                    if case .saved(let saved) = await session.sync.keepMyEntry(day: day) {
                        entry = saved
                        baseHash = saved.contentHash
                        saveState = .saved
                    }
                    await session.refreshPendingCount()
                }
            }
        } message: {
            Text("Another device saved this answer while you were writing. Your text is still here until you choose.")
        }
        .confirmationDialog("Delete this answer and every critique on it?", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("Delete", role: .destructive) {
                Task {
                    do {
                        try await session.client.deleteEntry(day: day)
                        await session.sync.discardMyEntry(day: day)
                        dismiss()
                    } catch {
                        problem = error.localizedDescription
                    }
                }
            }
        }
        .sheet(isPresented: $converting) {
            if let entry { ToWorkSheet(entryId: entry.id) }
        }
        .safeAreaInset(edge: .bottom) {
            if enrolled && loaded { PromptFormatBar(active: activeMarks) { send($0) } }
        }
        .task { await load() }
        .onDisappear {
            saveTask?.cancel()
            if saveState == .dirty { Task { await save() } }
        }
    }

    private var stateNote: String? {
        guard let entry else { return nil }
        switch entry.state {
        case .draft: return "A private draft: only you can see it."
        case .submitted: return "Submitted: everyone in Daily Prompts can read it. Changes show as edited."
        case .withdrawn: return "Withdrawn: private again, its critiques kept."
        }
    }

    private func send(_ name: String) {
        guard enrolled else { return }
        issued += 1
        command = WebEditorView.Command(name: name, issue: issued)
    }

    private func load() async {
        prompt = try? await session.client.prompt(day: day)
        enrolled = (try? await session.client.enrollment().enrolled) ?? enrolled
        let found = await session.sync.entry(day: day)
        entry = found.entry
        baseHash = found.entry?.contentHash
        document = found.document ?? .empty
        words = document.wordCount
        saveState = await session.sync.pending(for: PendingSave.entryKey(day: day)) == nil ? .idle : .queued
        loaded = true
    }

    private func scheduleSave() {
        guard enrolled else { return }
        saveState = .dirty
        saveTask?.cancel()
        saveTask = Task {
            try? await Task.sleep(for: .milliseconds(1200))
            guard !Task.isCancelled else { return }
            await save()
        }
    }

    private func save() async {
        guard enrolled, saveState != .idle || entry == nil else { return }
        saveState = .saving
        let localDay = ISO8601DateFormatter.localDay.string(from: Date())
        switch await session.sync.saveEntry(day: day, content: document, baseHash: baseHash, localDay: localDay) {
        case .saved(let saved):
            entry = saved
            baseHash = saved.contentHash
            saveState = .saved
            await session.publishWritingSnapshot()
        case .queued:
            saveState = .queued
        case .conflict:
            saveState = .conflict
            showConflict = true
        case .refused(let message):
            saveState = .idle
            problem = message
        }
        await session.refreshPendingCount()
    }

    private func act(_ work: () async throws -> API.PromptEntryView) async {
        await save()
        do {
            entry = try await work()
            problem = nil
        } catch {
            problem = error.localizedDescription
        }
    }
}

/// The formatting a thumb can reach, as on a scene.
private struct PromptFormatBar: View {
    let active: Set<String>
    let send: (String) -> Void
    private static let buttons: [(name: String, icon: String, label: String)] = [
        ("bold", "bold", "Bold"),
        ("italic", "italic", "Italic"),
        ("underline", "underline", "Underline"),
        ("quote", "text.quote", "Quote"),
        ("undo", "arrow.uturn.backward", "Undo"),
        ("redo", "arrow.uturn.forward", "Redo"),
    ]
    var body: some View {
        HStack(spacing: 0) {
            ForEach(Self.buttons, id: \.name) { button in
                Button { send(button.name) } label: {
                    Image(systemName: button.icon)
                        .frame(maxWidth: .infinity, minHeight: 44)
                        .background(active.contains(button.name) ? Color.accentColor.opacity(0.18) : .clear)
                }
                .accessibilityLabel(button.label)
            }
        }
        .buttonStyle(.plain)
        .background(.thinMaterial)
        .overlay(alignment: .top) { Divider() }
    }
}

/// Turning an answer into a project. The scenes shown are the server's
/// split, the same rules the web uses, so this is exactly what will be made.
struct ToWorkSheet: View {
    let entryId: String
    @EnvironmentObject private var session: SessionStore
    @Environment(\.dismiss) private var dismiss
    @State private var preview: API.ToWorkPreview?
    @State private var title = ""
    @State private var made: API.OutlineResponse?
    @State private var problem: String?
    @State private var busy = false

    var body: some View {
        NavigationStack {
            Form {
                if let made {
                    Section {
                        Label("Made “\(made.title)”. It is on your desk.", systemImage: "checkmark.circle")
                    }
                } else {
                    Section {
                        TextField("Title", text: $title)
                    } footer: {
                        Text("A new book of yours, with this answer copied into one chapter and the prompt in its notebook. The answer stays as it is.")
                    }
                    if let preview {
                        Section {
                            ForEach(Array(preview.scenes.enumerated()), id: \.offset) { index, scene in
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(scene.firstLine.isEmpty ? "No text" : scene.firstLine).font(.system(.callout, design: .serif))
                                    Text("Scene \(index + 1) · \(scene.wordCount) words").font(.caption).foregroundStyle(.secondary)
                                }
                            }
                        } header: {
                            Text(preview.scenes.count == 1 ? "1 scene" : "\(preview.scenes.count) scenes")
                        } footer: {
                            Text(preview.blankParagraphSplits
                                 ? "Split at each line that is only -- and at each blank line."
                                 : "Split at each line that is only --. A blank line can separate scenes too: turn that on in You → Settings.")
                        }
                    }
                }
                if let problem { Text(problem).foregroundStyle(.red) }
            }
            .navigationTitle("Turn into a project")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button(made == nil ? "Cancel" : "Done") { dismiss() } }
                if made == nil {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Make it") { Task { await make() } }
                            .disabled(busy || preview?.scenes.isEmpty != false || title.trimmingCharacters(in: .whitespaces).isEmpty)
                    }
                }
            }
            .task {
                do {
                    let p = try await session.client.previewWork(entryId: entryId)
                    preview = p
                    title = p.title
                } catch {
                    problem = error.localizedDescription
                }
            }
        }
    }

    private func make() async {
        busy = true
        defer { busy = false }
        do {
            made = try await session.client.makeWork(entryId: entryId, title: title)
        } catch {
            problem = error.localizedDescription
        }
    }
}
