import SwiftUI
import JustTypeKit

/// What the book says about itself in an export: language, description, who
/// else worked on it, the cover. Read-only on iOS for now; they are edited
/// under Front matter on the web.
struct BookDetailsView: View {
    let workId: String

    @EnvironmentObject private var session: SessionStore
    @State private var details: API.MetadataResponse?
    @State private var failed = false

    var body: some View {
        List {
            if let details {
                Section {
                    row("Language", details.language.map(Self.languageName) ?? "Not set (exports say English)")
                    row("Reads", Self.direction(details.textDirection))
                } footer: {
                    Text("The language sets hyphenation and the reading voice.")
                }
                Section("Description") {
                    Text(details.description?.nilIfBlank ?? "None")
                        .foregroundStyle(details.description?.nilIfBlank == nil ? .secondary : .primary)
                }
                Section("Publishing") {
                    row("Publisher", details.publisher)
                    row("Published", details.published)
                    row("Identifier", details.identifier)
                    row("Keywords", details.keywords.isEmpty ? nil : details.keywords.joined(separator: ", "))
                }
                if !details.contributors.isEmpty {
                    Section("Others who worked on it") {
                        ForEach(Array(details.contributors.enumerated()), id: \.offset) { _, person in
                            row(person.name, person.role.capitalized)
                        }
                    }
                }
                Section {
                    if details.hasCover, let w = details.coverWidth, let h = details.coverHeight {
                        row("Cover", "\(w) × \(h)")
                    } else {
                        row("Cover", details.hasCover ? "Added" : "None")
                    }
                } footer: {
                    if let warning = details.coverWarning { Text(warning) }
                }
                Section {
                } footer: {
                    Text("Edit these on the web, under Front matter.")
                }
            } else if failed {
                ContentUnavailableView(
                    "Could not load the details",
                    systemImage: "wifi.slash",
                    description: Text("Pull to try again.")
                )
            } else {
                ProgressView()
            }
        }
        .navigationTitle("Publishing details")
        .refreshable { await load() }
        .task { await load() }
    }

    private func row(_ label: String, _ value: String?) -> some View {
        LabeledContent(label) {
            Text(value?.nilIfBlank ?? "—")
                .foregroundStyle(value?.nilIfBlank == nil ? .tertiary : .secondary)
                .multilineTextAlignment(.trailing)
        }
    }

    private func load() async {
        do {
            details = try await session.client.metadata(workId: workId)
            failed = false
        } catch {
            failed = details == nil
        }
    }

    /// "fr-CA" as "French (Canada) · fr-CA", in the reader's own language.
    static func languageName(_ tag: String) -> String {
        guard let name = Locale.current.localizedString(forIdentifier: tag) else { return tag }
        return "\(name) · \(tag)"
    }

    static func direction(_ value: String?) -> String {
        switch value {
        case "rtl": return "Right to left"
        case "ltr": return "Left to right"
        default: return "As the language does"
        }
    }
}

private extension String {
    var nilIfBlank: String? {
        trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : self
    }
}
