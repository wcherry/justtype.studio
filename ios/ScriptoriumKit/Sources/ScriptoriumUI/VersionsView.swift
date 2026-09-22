import SwiftUI
import ScriptoriumKit

struct VersionsView: View {
    let workId: String

    @EnvironmentObject private var session: SessionStore
    @State private var versions: [API.Version] = []
    @State private var offering: API.Version?
    @State private var busy = false

    var body: some View {
        List {
            Section {
                Button {
                    Task {
                        busy = true
                        _ = try? await session.client.snapshot(workId: workId, label: nil)
                        await load()
                        busy = false
                    }
                } label: {
                    Label("Snapshot the current draft", systemImage: "camera")
                }
                .disabled(busy)
            }

            ForEach(versions) { version in
                Section {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack {
                            Text(version.label).font(.headline)
                            Spacer()
                            Text(version.state.rawValue.capitalized)
                                .font(.caption)
                                .padding(.horizontal, 8).padding(.vertical, 2)
                                .background(.quaternary, in: Capsule())
                        }
                        Text("\(version.wordCount.formatted()) words")
                            .font(.caption).foregroundStyle(.secondary)

                        if version.state == .draft {
                            Button("Publish this version") {
                                Task {
                                    _ = try? await session.client.publish(versionId: version.id)
                                    await load()
                                }
                            }
                        } else if version.state == .published {
                            Button("Offer for review") { offering = version }
                            Button("Mark final") {
                                Task {
                                    _ = try? await session.client.finalize(versionId: version.id)
                                    await load()
                                }
                            }
                        }
                    }
                }
            }
        }
        .navigationTitle("Versions")
        .task { await load() }
        .refreshable { await load() }
        .sheet(item: $offering) { version in
            OfferSheet(version: version) { visibility, prompt in
                offering = nil
                Task {
                    _ = try? await session.client.offerForReview(
                        versionId: version.id,
                        visibility: visibility,
                        reviewPrompt: prompt.isEmpty ? nil : prompt
                    )
                    await load()
                }
            }
        }
    }

    private func load() async {
        versions = (try? await session.client.versions(workId: workId)) ?? []
    }
}

struct OfferSheet: View {
    let version: API.Version
    let onSubmit: (API.Visibility, String) -> Void

    @State private var visibility: API.Visibility = .platform
    @State private var prompt = ""
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Picker("Who can read it", selection: $visibility) {
                    ForEach(API.Visibility.allCases, id: \.self) { Text($0.label).tag($0) }
                }
                Section("What do you want feedback on?") {
                    TextField("Does the opening earn the next page?", text: $prompt, axis: .vertical)
                        .lineLimit(3...6)
                }
            }
            .navigationTitle("Offer \(version.label)")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Offer") { onSubmit(visibility, prompt) }
                }
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }
            }
        }
    }
}
