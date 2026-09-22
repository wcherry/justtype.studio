import SwiftUI
import ScriptoriumKit

struct FeedView: View {
    @EnvironmentObject private var session: SessionStore
    @State private var cards: [API.PublicationCard] = []
    @State private var mine = false

    var body: some View {
        NavigationStack {
            List {
                if cards.isEmpty {
                    ContentUnavailableView(
                        "Nothing on offer",
                        systemImage: "text.bubble",
                        description: Text("When writers publish a version for review, it shows up here.")
                    )
                }
                ForEach(cards) { card in
                    NavigationLink(value: card) {
                        VStack(alignment: .leading, spacing: 5) {
                            Text(card.workTitle).font(.system(.headline, design: .serif))
                            Text("\(card.author.displayName) · \(card.wordCount.formatted()) words · \(card.commentCount) comments")
                                .font(.caption).foregroundStyle(.secondary)
                            if let prompt = card.reviewPrompt, !prompt.isEmpty {
                                Text("“\(prompt)”").font(.footnote).italic()
                            }
                            if !card.excerpt.isEmpty {
                                Text(card.excerpt)
                                    .font(.footnote)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(3)
                            }
                        }
                        .padding(.vertical, 2)
                    }
                }
            }
            .navigationTitle(mine ? "Your publications" : "Looking for readers")
            .navigationDestination(for: API.PublicationCard.self) { card in
                ReviewView(card: card)
            }
            .toolbar {
                Button(mine ? "Everyone’s" : "Only mine") {
                    mine.toggle()
                    Task { await load() }
                }
                .font(.footnote)
            }
            .task { await load() }
            .refreshable { await load() }
        }
    }

    private func load() async {
        cards = (try? await session.client.feed(mine: mine)) ?? []
    }
}
