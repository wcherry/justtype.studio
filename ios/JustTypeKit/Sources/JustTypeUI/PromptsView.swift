import SwiftUI
import JustTypeKit

/// Where a Daily Prompts screen goes next.
enum PromptRoute: Hashable {
    case day(String)
    case write(String)
    case entry(String)
    case archive
}

/// Days for showing: "Friday 9 October", from `YYYY-MM-DD`.
enum PromptDays {
    static func name(_ day: String, year: Bool = false) -> String {
        let parse = DateFormatter()
        parse.locale = Locale(identifier: "en_US_POSIX")
        parse.timeZone = TimeZone(identifier: "UTC")
        parse.dateFormat = "yyyy-MM-dd"
        guard let date = parse.date(from: day) else { return day }
        var style = Date.FormatStyle.dateTime.weekday(.wide).day().month(.wide)
        if year { style = style.year() }
        style.timeZone = TimeZone(identifier: "UTC")!
        return date.formatted(style)
    }

    static func time(_ iso: String) -> String {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        guard let date = formatter.date(from: iso) else { return "" }
        return date.formatted(date: .omitted, time: .shortened)
    }

    static func stateLabel(_ state: String) -> String {
        switch state {
        case "draft": "Draft"
        case "submitted": "Submitted"
        case "withdrawn": "Withdrawn"
        default: "Not started"
        }
    }
}

/// Daily Prompts: today's prompt, taking part, everyone's answers, the
/// archive, and the email.
struct PromptsView: View {
    @EnvironmentObject private var session: SessionStore
    @State private var today: API.PromptView?
    @State private var enrollment: API.EnrollmentView?
    @State private var problem: String?
    @State private var path: [PromptRoute] = []
    /// A day the app was opened for, from the email's link.
    @Binding var open: PromptRoute?

    var body: some View {
        NavigationStack(path: $path) {
            List {
                if let problem {
                    Label(problem, systemImage: "wifi.slash").font(.footnote)
                }
                if let today {
                    Section {
                        PromptCard(prompt: today, enrolled: enrollment?.enrolled == true, isToday: true)
                    } header: {
                        Text("Today")
                    }
                }
                if let enrollment, !enrollment.enrolled {
                    Section {
                        Text("A prompt every day. Write an answer, keep it to yourself or share it with everyone else taking part, and read and critique theirs. An answer that wants to keep going can become a project.")
                            .font(.callout)
                        Button("Join Daily Prompts") { Task { await join() } }
                    } footer: {
                        Text("Joining does not turn on the email; you can do that afterwards.")
                    }
                }
                if enrollment?.enrolled == true, let today {
                    EntryFeedSection(day: today.day, heading: "Today's submissions")
                    Section {
                        NavigationLink("Archive", value: PromptRoute.archive)
                    }
                    if let enrollment {
                        PromptPreferencesSection(enrollment: enrollment) { self.enrollment = $0 }
                    }
                }
            }
            .navigationTitle("Daily Prompts")
            .navigationDestination(for: PromptRoute.self) { route in
                switch route {
                case .day(let day): PromptDayView(day: day)
                case .write(let day): PromptWriteView(day: day)
                case .entry(let id): PromptEntryReadView(entryId: id)
                case .archive: PromptArchiveView()
                }
            }
            .task { await load() }
            .refreshable { await load() }
            .onChange(of: open) { _, route in
                guard let route else { return }
                path = [route]
                open = nil
            }
            .onAppear {
                if let route = open {
                    path = [route]
                    open = nil
                }
            }
        }
    }

    private func load() async {
        do {
            async let t = session.client.todayPrompt()
            async let e = session.client.enrollment()
            today = try await t.prompt
            enrollment = try await e
            problem = nil
        } catch {
            problem = error.localizedDescription
        }
    }

    private func join() async {
        do {
            enrollment = try await session.client.joinPrompts(timezone: TimeZone.current.identifier)
            if let day = today?.day { today = try await session.client.prompt(day: day) }
        } catch {
            problem = error.localizedDescription
        }
    }
}

struct PromptCard: View {
    let prompt: API.PromptView
    let enrolled: Bool
    let isToday: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(prompt.body)
                .font(.system(.title3, design: .serif))
                .fixedSize(horizontal: false, vertical: true)
            if enrolled {
                HStack {
                    Text(PromptDays.stateLabel(prompt.myState))
                        .font(.caption)
                        .padding(.horizontal, 8).padding(.vertical, 2)
                        .background(.quaternary, in: Capsule())
                    Spacer()
                    NavigationLink(value: PromptRoute.write(prompt.day)) {
                        Text(prompt.myState == "none" ? "Write" : "Continue").bold()
                    }
                    .fixedSize()
                }
                Text(isToday
                     ? "Open until \(PromptDays.time(prompt.closesAt)) · \(prompt.submissions) submitted"
                     : "You can still write on it · \(prompt.submissions) submitted")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 4)
    }
}

/// Everyone's submitted answers to one prompt, late ones too.
struct EntryFeedSection: View {
    let day: String
    let heading: String
    @EnvironmentObject private var session: SessionStore
    @State private var cards: [API.EntryCard] = []
    @State private var cursor: String?
    @State private var fewest = false
    @State private var loaded = false

    var body: some View {
        Section {
            Picker("Order", selection: $fewest) {
                Text("Newest").tag(false)
                Text("Fewest critiques").tag(true)
            }
            .pickerStyle(.segmented)
            if loaded && cards.isEmpty {
                Text("No one has submitted an answer yet.").foregroundStyle(.secondary)
            }
            ForEach(cards, id: \.id) { card in
                NavigationLink(value: PromptRoute.entry(card.id)) {
                    VStack(alignment: .leading, spacing: 4) {
                        HStack(spacing: 6) {
                            Text(card.author.displayName).font(.subheadline.weight(.semibold))
                            if card.late { Tag("late") }
                            if card.editedAt != nil { Tag("edited") }
                            if card.mine { Tag("yours") }
                        }
                        Text(card.excerpt)
                            .font(.system(.footnote, design: .serif))
                            .lineLimit(3)
                        Text("\(card.wordCount.formatted()) words · \(card.critiqueCount == 1 ? "1 critique" : "\(card.critiqueCount) critiques")")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
            if cursor != nil {
                Button("More") { Task { await load(more: true) } }
            }
        } header: {
            Text(heading)
        }
        .task(id: fewest) { await load(more: false) }
    }

    private func load(more: Bool) async {
        guard let page = try? await session.client.entries(day: day, fewestCritiques: fewest, cursor: more ? cursor : nil)
        else { loaded = true; return }
        cards = more ? cards + page.entries : page.entries
        cursor = page.nextCursor
        loaded = true
    }
}

struct Tag: View {
    let text: String
    init(_ text: String) { self.text = text }
    var body: some View {
        Text(text)
            .font(.caption2)
            .padding(.horizontal, 6).padding(.vertical, 1)
            .background(.quaternary, in: Capsule())
    }
}

/// The email, when it comes, and leaving.
struct PromptPreferencesSection: View {
    let enrollment: API.EnrollmentView
    let onChange: (API.EnrollmentView) -> Void
    @EnvironmentObject private var session: SessionStore
    @State private var problem: String?

    private var time: Binding<Date> {
        Binding(
            get: {
                let parts = enrollment.emailLocalTime.split(separator: ":").compactMap { Int($0) }
                return Calendar.current.date(bySettingHour: parts.first ?? 7, minute: parts.last ?? 0, second: 0, of: Date()) ?? Date()
            },
            set: { date in
                let c = Calendar.current.dateComponents([.hour, .minute], from: date)
                let value = String(format: "%02d:%02d", c.hour ?? 7, c.minute ?? 0)
                Task { await save { try await session.client.updateEnrollment(localTime: value) } }
            }
        )
    }

    var body: some View {
        Section {
            Toggle("Email me the prompt each day", isOn: Binding(
                get: { enrollment.emailEnabled },
                set: { on in Task { await save { try await session.client.updateEnrollment(emailEnabled: on) } } }
            ))
            .disabled(!enrollment.emailVerified && !enrollment.emailEnabled)
            DatePicker("At", selection: time, displayedComponents: .hourAndMinute)
            Picker("Timezone", selection: Binding(
                get: { enrollment.emailTimezone },
                set: { tz in Task { await save { try await session.client.updateEnrollment(timezone: tz) } } }
            )) {
                ForEach(Self.zones(including: enrollment.emailTimezone), id: \.self) { Text($0.replacingOccurrences(of: "_", with: " ")).tag($0) }
            }
            if let problem { Text(problem).foregroundStyle(.red).font(.footnote) }
            Button("Leave Daily Prompts", role: .destructive) {
                Task { await save { try await session.client.leavePrompts() } }
            }
        } header: {
            Text("Preferences")
        } footer: {
            VStack(alignment: .leading, spacing: 4) {
                if !enrollment.emailVerified {
                    Text("Confirm your address to turn the email on; the link is in the message we sent when you signed up.")
                }
                Text("Each prompt opens at \(enrollment.opensAtLocal) your time and stays open for a day.")
                if enrollment.emailEnabled, let note = PromptPreferencesSection.closingNote(opensAt: enrollment.opensAtLocal, chosen: enrollment.emailLocalTime) {
                    Text(note)
                }
            }
        }
    }

    /// The note when the email would come in the prompt's last six hours.
    static func closingNote(opensAt: String, chosen: String) -> String? {
        func minutes(_ s: String) -> Int {
            let p = s.split(separator: ":").compactMap { Int($0) }
            return (p.first ?? 0) * 60 + (p.last ?? 0)
        }
        let left = (minutes(opensAt) - minutes(chosen) + 24 * 60) % (24 * 60)
        guard left > 0, left <= 6 * 60 else { return nil }
        let hours = (Double(left) / 60 * 10).rounded() / 10
        let said = hours == hours.rounded() ? String(Int(hours)) : String(format: "%.1f", hours)
        return "Today's prompt closes at \(opensAt). You'll have \(said) hour\(hours == 1 ? "" : "s")."
    }

    static func zones(including current: String) -> [String] {
        var all = TimeZone.knownTimeZoneIdentifiers.sorted()
        if !all.contains(current) { all.insert(current, at: 0) }
        return all
    }

    private func save(_ work: @escaping () async throws -> API.EnrollmentView) async {
        do {
            onChange(try await work())
            problem = nil
        } catch {
            problem = error.localizedDescription
        }
    }
}

/// Every past day, newest first.
struct PromptArchiveView: View {
    @EnvironmentObject private var session: SessionStore
    @State private var days: [API.PromptView] = []
    @State private var next: String?

    var body: some View {
        List {
            ForEach(days, id: \.day) { prompt in
                NavigationLink(value: PromptRoute.day(prompt.day)) {
                    VStack(alignment: .leading, spacing: 3) {
                        Text(PromptDays.name(prompt.day, year: true)).font(.caption).foregroundStyle(.secondary)
                        Text(prompt.body).font(.system(.callout, design: .serif)).lineLimit(2)
                        Text(prompt.myState == "none" ? "Not answered" : PromptDays.stateLabel(prompt.myState))
                            .font(.caption2).foregroundStyle(.secondary)
                    }
                }
            }
            if next != nil {
                Button("Earlier") { Task { await load(more: true) } }
            }
        }
        .navigationTitle("Archive")
        .overlay {
            if days.isEmpty { ContentUnavailableView("No earlier prompts yet", systemImage: "calendar") }
        }
        .task { await load(more: false) }
    }

    private func load(more: Bool) async {
        guard let page = try? await session.client.promptArchive(before: more ? next : nil, limit: 20) else { return }
        days = more ? days + page.prompts : page.prompts
        next = page.nextBefore
    }
}

/// One past day: its prompt and everyone's answers to it.
struct PromptDayView: View {
    let day: String
    @EnvironmentObject private var session: SessionStore
    @State private var prompt: API.PromptView?
    @State private var enrolled = false
    @State private var problem: String?

    var body: some View {
        List {
            if let problem { Text(problem).foregroundStyle(.secondary) }
            if let prompt {
                Section(PromptDays.name(prompt.day, year: true)) {
                    PromptCard(prompt: prompt, enrolled: enrolled, isToday: false)
                }
                if enrolled { EntryFeedSection(day: prompt.day, heading: "Submissions") }
            }
        }
        .navigationTitle(PromptDays.name(day))
        .task {
            do {
                prompt = try await session.client.prompt(day: day)
                enrolled = (try? await session.client.enrollment().enrolled) ?? false
            } catch {
                problem = error.localizedDescription
            }
        }
    }
}
