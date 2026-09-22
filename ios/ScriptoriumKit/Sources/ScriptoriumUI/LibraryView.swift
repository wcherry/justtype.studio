import SwiftUI
import ScriptoriumKit

struct LibraryView: View {
    @EnvironmentObject private var session: SessionStore
    @State private var works: [API.Work] = []
    @State private var templates: [API.TemplateView] = []
    @State private var creating = false
    @State private var error: String?

    var body: some View {
        NavigationStack {
            List {
                if works.isEmpty {
                    ContentUnavailableView(
                        "Nothing here yet",
                        systemImage: "book.closed",
                        description: Text("Start something — a template gives you a spine to hang it on.")
                    )
                }
                ForEach(works) { work in
                    NavigationLink(value: work) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(work.title).font(.system(.headline, design: .serif))
                            Text("\(work.kind.label) · \(work.wordCount.formatted()) words · \(work.status.rawValue)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        .padding(.vertical, 2)
                    }
                }
            }
            .navigationTitle("Your desk")
            .navigationDestination(for: API.Work.self) { work in
                WorkView(workId: work.id, title: work.title)
            }
            .toolbar {
                Button { creating = true } label: { Label("New", systemImage: "plus") }
            }
            .refreshable { await load() }
            .task { await load() }
            .sheet(isPresented: $creating) {
                NewWorkSheet(templates: templates) { title, kind, templateId in
                    creating = false
                    Task {
                        _ = try? await session.client.createWork(title: title, kind: kind, templateId: templateId)
                        await load()
                    }
                }
            }
            .alert("Could not load your desk", isPresented: .constant(error != nil)) {
                Button("OK") { error = nil }
            } message: {
                Text(error ?? "")
            }
        }
    }

    private func load() async {
        do {
            works = try await session.client.works()
            templates = (try? await session.client.templates()) ?? []
        } catch {
            self.error = error.localizedDescription
        }
    }
}

struct NewWorkSheet: View {
    let templates: [API.TemplateView]
    let onCreate: (String, API.WorkKind, String?) -> Void

    @State private var title = ""
    @State private var kind: API.WorkKind = .novel
    @State private var templateId: String = "tpl-blank"
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                TextField("Title", text: $title)
                Picker("Form", selection: $kind) {
                    ForEach(API.WorkKind.allCases, id: \.self) { Text($0.label).tag($0) }
                }
                Picker("API.TemplateView", selection: $templateId) {
                    ForEach(templates) { Text($0.name).tag($0.id) }
                }
                if let chosen = templates.first(where: { $0.id == templateId }) {
                    Text(chosen.description).font(.footnote).foregroundStyle(.secondary)
                }
            }
            .navigationTitle("New work")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Create") { onCreate(title, kind, templateId) }
                        .disabled(title.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
    }
}
