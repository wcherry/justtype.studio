import ScriptoriumKit
import SwiftUI
import WidgetKit

/// Today's words on the home screen.
///
/// It reads what the app left behind rather than fetching anything: a widget
/// cannot sign in, and giving an extension a refresh token so it can draw a
/// progress bar would be a credential in a second place for no reason.
struct WordGoalEntry: TimelineEntry {
    let date: Date
    let snapshot: WritingSnapshot?
}

struct WordGoalProvider: TimelineProvider {
    func placeholder(in context: Context) -> WordGoalEntry {
        WordGoalEntry(date: Date(), snapshot: WritingSnapshot(words: 740, goal: 1000, streak: 4))
    }

    func getSnapshot(in context: Context, completion: @escaping (WordGoalEntry) -> Void) {
        completion(WordGoalEntry(date: Date(), snapshot: SharedState.load()))
    }

    func getTimeline(in context: Context, completion: @escaping (Timeline<WordGoalEntry>) -> Void) {
        let entry = WordGoalEntry(date: Date(), snapshot: SharedState.load())
        // Refreshed at the top of the hour rather than on a timer: the number
        // only moves when the writer opens the app, and the app reloads the
        // timeline itself when it does.
        let next = Calendar.current.date(byAdding: .hour, value: 1, to: Date()) ?? Date()
        completion(Timeline(entries: [entry], policy: .after(next)))
    }
}

struct WordGoalView: View {
    var entry: WordGoalEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Today")
                .font(.caption2)
                .foregroundStyle(.secondary)

            if let snapshot = entry.snapshot {
                Text("\(snapshot.words)")
                    .font(.system(.title, design: .serif))
                    .contentTransition(.numericText())

                if let goal = snapshot.goal {
                    ProgressView(value: snapshot.fraction)
                        .tint(snapshot.fraction >= 1 ? .green : .accentColor)
                    Text("of \(goal)")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                } else {
                    Text("words")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }

                // The week, when the writer has set a target for it: a kinder
                // measure for someone who writes three evenings, not seven.
                if let weekGoal = snapshot.weekGoal {
                    Text("\((snapshot.weekWords ?? 0)) of \(weekGoal) this week")
                        .font(.caption2)
                        .foregroundStyle(snapshot.weekFraction >= 1 ? .green : .secondary)
                }

                if snapshot.streak > 1 {
                    Label("\(snapshot.streak) days", systemImage: "flame")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
            } else {
                // Not an error. Nobody has opened the app yet, and saying so
                // is better than a zero that looks like a bad day.
                Text("Open Scriptorium to start counting.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        .containerBackground(.fill.tertiary, for: .widget)
    }
}

@main
struct ScriptoriumWidgetBundle: WidgetBundle {
    var body: some Widget { WordGoalWidget() }
}

struct WordGoalWidget: Widget {
    var body: some WidgetConfiguration {
        StaticConfiguration(kind: "app.scriptorium.wordgoal", provider: WordGoalProvider()) { entry in
            WordGoalView(entry: entry)
        }
        .configurationDisplayName("Today's words")
        .description("How far you have got against your daily goal.")
        .supportedFamilies([.systemSmall])
    }
}
