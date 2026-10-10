# Daily Prompts — ticket drafts

Draft for review before filing on GitHub. One epic plus child issues, in the
same shape as the export epic (#28, children #17–27). Placeholders `T1`…`T15`
are swapped for issue numbers when filed.

| # | Type | Title |
|---|---|---|
| Epic | Epic | Daily Prompts — a prompt a day, written, shared and critiqued |
| T1 | Maint | Background job runner and the Pacific "prompt day" |
| T2 | Maint | Site administrators |
| T3 | Feature | Prompts: the queue, the admin page, the AI fallback and the archive |
| T4 | Feature | Daily Prompts page, enrollment, and the header link |
| T5 | Feature | Writing an answer: private draft, submit, edit, withdraw |
| T6 | Feature | The shared feed of submissions |
| T7 | Feature | Critiques: threaded comments at any depth, with reporting |
| T8 | Improvement | Split the Account page into *Account* and *Settings* tabs |
| T9 | Feature | Turn an entry into a project, with scene separators |
| T10 | Feature | The daily prompt email |
| T11 | Feature | In-app inbox with a daily summary (the prompts part) |
| T12 | Feature | iOS: Daily Prompts — read, write, submit, turn into a project |
| T13 | Feature | iOS: critiques |
| T14 | Feature | iOS: inbox |
| T15 | Feature | iOS: the double-Enter setting and the split settings screen |

---

## Epic — `[Epic]: Daily Prompts — a prompt a day, written, shared and critiqued`

**Labels:** epic

### Roadmap section

9 — Daily Prompts

### Goal

Give writers a reason to write every day. Each day has one prompt. Enrolled
writers get it by email (or find it under **Daily Prompts** in the header),
write an answer, and choose whether to keep it private, submit it so other
enrolled writers can read and critique it, or turn it into a project.

### Background

Just Type Studio is built for long work, and long work stalls. A small daily
exercise keeps people writing between projects, and some answers grow into
projects of their own. It is also the first part of the product where writers
read each other without an invitation, so it brings in community without
opening up anyone's manuscripts.

The mail plumbing already exists (`backend/src/mail.rs`, outbox and SMTP).
What's missing is anything that runs on a schedule, anyone who can curate
content for the whole site, and any notification surface. This epic builds
the minimum of each.

### Scope

- One prompt per **prompt day**. A prompt day runs midnight to midnight in the
  server's timezone, **Pacific (America/Los_Angeles)**, and follows daylight
  saving.
- Prompts come from a curated queue that site admins manage, and admins can
  pin a prompt to a date. When nothing is queued, Claude writes one the day
  before, so an admin can see it before it opens.
- An archive of past prompts that can be browsed and still written on.
- Opt-in enrollment from the Daily Prompts page. Writing an answer needs
  enrollment. Getting the email is a separate toggle and needs a verified
  address.
- Writing an answer in the existing editor in a single-scene mode, with no
  word limit. The draft is private and autosaved.
- Submitting, editing (shown as *edited*) and withdrawing a submission.
- A feed of each day's submissions, visible to enrolled writers only. A
  writer doesn't need to submit before reading others.
- Critiques: public to enrolled writers, never anonymous, with replies to any
  depth. Reporting works the way it does on comments today: a report goes to
  the author of the entry.
- Turning an answer into a project: a new work whose scenes are split on
  separators, with the prompt saved in the notebook.
- Splitting the Account page into *Account* and *Settings* tabs, with a new
  setting for whether double Enter separates scenes.
- The daily email, sent at a time each writer picks in their own timezone.
- An in-app inbox with a daily summary covering prompt activity. It's
  designed so project comments can be added later.
- iOS follow-ups after the web version ships.

### Out of scope

- Emailing writers about critiques. That goes through the inbox only, for now.
- Project-comment notifications in the inbox. Those get a follow-up ticket.
- Writers proposing prompts, voting on prompts, or prompts tied to genres or
  groups.
- Leaderboards, streak rewards and badges. The existing streak in goals isn't
  touched.
- Ranking or scoring critiques. They're plain comments, not the reviewer
  standing of *3.2*.
- Anonymous submissions or critiques.
- Site moderation, live notices for the inbox, and blocking. See *Future
  work* below.

### Future work

These were raised in review and left out of this epic on purpose. Each needs
its own ticket before Daily Prompts grows beyond a small group of writers.

- **Moderation.** The moderation migration (`2026-09-23-000010_moderation`)
  says there is no moderator: a report goes to the author whose work it's
  about. Daily Prompts keeps that rule for now. A follow-up would decide
  whether site admins (T2) become moderators, and then add:
  - reporting an entry, not just a critique
  - an admin report queue (`/admin/reports`), and whether it also takes
    reports on book comments
  - removing an entry or a critique as an admin
  - suspending a writer from Daily Prompts site-wide
  - limits on who can post: a verified address, and a rate limit on
    submissions (critiques are already rate-limited in T7)
- **Live notices for the inbox.** `live.rs` only has `publication:` and
  `work:` rooms (`may_join`), so nothing can be pushed to one writer. Live
  unread counts need a `user:{id}` room that only that user may join. Until
  then, T11 refreshes the count on navigation and when the window regains
  focus.
- **Blocking.** `blocked_readers` is one author's list of people who can't
  read their work, and it only works in one direction. Daily Prompts needs to
  decide whether blocking there works both ways, and whether it should be the
  same list as for books (blocking a critic would also shut them out of the
  author's published books) or a separate one.

### Proposed architecture

**Data model (new migration):**

- `daily_prompts (id, day DATE UNIQUE NULL, position INTEGER NULL, body, source 'curated'|'ai', created_by NULL, created_at)`.
  A row with `day = NULL` is in the queue, and `position` orders the queue.
  A row with a `day` is assigned to that day, either pinned by an admin or
  taken from the queue the day before (T3). It opens when that day starts.
- `prompt_enrollments (user_id PK, enrolled_at, email_enabled, email_local_time 'HH:MM', email_timezone IANA, unsubscribe_token)`.
- `prompt_entries (id, prompt_id, author_id, body, state 'draft'|'submitted'|'withdrawn', submitted_at NULL, edited_at NULL, created_at, updated_at, UNIQUE(prompt_id, author_id))`.
  The body is stored the way scene bodies are, so the editor and the converter
  can share code.
- `prompt_critiques (id, entry_id, parent_id NULL → prompt_critiques, author_id, body, deleted_at NULL, created_at, updated_at)`.
  This is a separate table from `comments`, which is tied to publications and
  versions. Soft deletion keeps a removed critique as a placeholder when it has
  replies, the same rule *6.2* set for comments.
- `prompt_critique_reports` follows the shape of `comment_reports`.
- `inbox_items (id, user_id, kind, subject_type, subject_id, actor_id NULL, created_at, read_at NULL)`
  and `inbox_digests (user_id, day, summary_json, created_at)`. These are
  generic on purpose (see T11).
- `prompt_email_sends (user_id, day, sent_at)` (T10).
- `users.is_admin` (T2) and `users.double_enter_splits_scenes` (T8/T9).

**Backend:**

- New routes in `routes/prompts.rs`, `routes/critiques.rs` and `routes/inbox.rs`.
- A small job runner (T1) that drives four jobs: preparing tomorrow's prompt,
  opening the day's prompt, sending the emails, and building the inbox
  digests.
- `ai.rs` writes the fallback prompts.
- `mail.rs` gains templates and a one-click unsubscribe.

**Web:**

- A **Daily Prompts** link in `web/src/App.tsx`, after Review.
- New pages: `DailyPromptsPage` (today, the archive and the feed), entry and
  editor routes under `/prompts/...`, and `/inbox`.
- `SettingsPage` is split into tabs.

**iOS:** follow-up tickets T12–T15.

### Milestones

- [ ] Phase 1 — Groundwork: job runner and prompt day (T1), admins (T2), prompts and archive (T3)
- [ ] Phase 2 — Writing: the page and enrollment (T4), writing an answer (T5), settings tabs (T8), turning an answer into a project (T9)
- [ ] Phase 3 — Sharing: the feed (T6), critiques (T7)
- [ ] Phase 4 — Reaching people: the email (T10), the inbox (T11)
- [ ] Phase 5 — iOS (T12–T15)

### Child issues

- [ ] T1 … T15

### Dependencies

- A sending domain and SMTP credentials before T10 can go live. Until then,
  T10 can be built and tested against the outbox.
- A working Anthropic key for the AI fallback in T3. As with *8.4*, it can be
  verified against a stub until then.

### Acceptance criteria

- [ ] An enrolled writer can find today's prompt under **Daily Prompts**, write
      an answer, submit it, and see it in the day's feed.
- [ ] Another enrolled writer can critique it, and anyone enrolled can reply
      to that critique, to any depth.
- [ ] A writer who isn't enrolled can't see the feed or the critiques.
- [ ] Any answer can be turned into a project, with scenes split on `--`, and
      on a double Enter when that setting is on.
- [ ] With email on, a writer gets the prompt at the time they chose, with a
      working link and a one-click unsubscribe.
- [ ] Critiques and replies show up in the inbox and its daily summary.
- [ ] Everything a writer creates here is in their data export and is handled
      on account deletion.
- [ ] The privacy page describes the new public content and the email. If the
      terms change, `terms_version` is bumped.
- [ ] ROADMAP.md Phase 9 is updated as each piece lands.

---

## T1 — `[Maint]: Background job runner and the Pacific "prompt day"`

**Labels:** maintenance · **Type:** Infrastructure · **Component:** Backend (Rust)

### Why is this needed?

Nothing in the server runs on a schedule. Backups are run by cron or launchd
from outside the server (`docs/OPERATIONS.md`). Daily Prompts needs four
things to happen on time: preparing tomorrow's prompt, opening the day's
prompt at midnight Pacific, sending each writer's email at their chosen time,
and building the inbox digests. The product also has no notion of "the day" yet. Goals use the
writer's own day.

### Proposed approach

- A `jobs` module. On startup, the server spawns one Tokio task that ticks
  every minute and runs any job that is due.
- Each job records its runs in `job_runs (job, key, started_at, finished_at, error)`
  with `UNIQUE(job, key)`. For example, `open_prompt` for `2026-10-12`. This
  means a restart, a second replica or a missed tick can't run a job twice or
  skip one. A job that's overdue runs on the next tick.
- `prompt_day(now) -> NaiveDate` uses `chrono-tz` and America/Los_Angeles.
  It's configurable with `JUSTTYPE_PROMPT_TZ` but defaults to Pacific, so
  development and production agree. Daylight saving changes are handled: on
  those dates the day is 23 or 25 hours long.
- Jobs that act per user, like the emails in T10, keep their own per-user
  record instead of one `job_runs` row per tick.
- `/api/status` reports `degraded` when a job hasn't run when it should have,
  and nothing more. The details (each job's last run and error) are on an
  admin-only route, `GET /api/admin/jobs` (T2).

### Risks

- There is one server process; Litestream replicates the database for backup
  and doesn't run a second writer. The risks are a slow job overlapping the
  next tick, and a restart in the middle of a job. So the claim is made in the
  same transaction as the work, or the work is idempotent. Mail can't share a
  transaction with an SMTP send, so jobs that send mail write to the outbox in
  `mail.rs` inside the transaction, and the outbox does the sending.

### Acceptance criteria

- [ ] Unit tests for `prompt_day` across both DST transitions and the
      23:59 / 00:00 Pacific boundary.
- [ ] Restarting the server mid-day doesn't re-run a finished job, and a job
      missed while the server was down runs once when it comes back.
- [ ] A job still running when the next tick comes isn't started a second
      time.
- [ ] `/api/status` reports a missed job as degraded, and only admins can see
      the job details.

---

## T2 — `[Maint]: Site administrators`

**Labels:** maintenance · **Type:** Security hardening · **Component:** Backend (Rust), Web (React)

### Why is this needed?

Up to now, every role has been scoped to a single work: owner, collaborator,
reader. Curating the prompt queue is the first job that belongs to the whole
site. Admins don't moderate in this epic; whether they should is listed under
the epic's *Future work*.

### Proposed approach

- Add `users.is_admin INTEGER NOT NULL DEFAULT 0`. It's set from the command
  line (`cargo xtask admin grant <email>` / `revoke`), never through the API.
- An `AdminUser` extractor that returns 403 for anyone else.
- Granting and revoking are written to the audit log (`audit_events`, with
  `work_id` left empty).
- The web app shows an *Admin* link in the header, next to *Account*, only to
  admins.

### Acceptance criteria

- [ ] Non-admins get 403 on every `/api/admin/*` route, and a test proves it.
- [ ] Grant and revoke work and appear in the audit log.
- [ ] `is_admin` is never accepted in any request body.

---

## T3 — `[Feature]: Prompts: the queue, the admin page, the AI fallback and the archive`

**Labels:** feature · **Affected area:** API, Web UI, AI summaries

### What problem would this solve?

Every prompt day needs exactly one prompt, chosen ahead of time by a person
where possible, and never missing.

### Proposed solution

- `daily_prompts` table (see the epic). The admin page at `/admin/prompts`
  lets admins add, edit, reorder and delete prompts in the queue, **pin** a
  prompt to a future date, and lists past and upcoming days with the prompt
  each one got and its source (curated or AI).
- The `prepare_prompt` job (T1), run at noon Pacific, assigns **tomorrow's**
  prompt, so admins have half a day to see it and change it:
  1. If a prompt is already pinned to tomorrow, keep it.
  2. Otherwise, take the first prompt in the queue.
  3. If the queue is empty, have Claude write one through `ai.rs`. The request
     includes the last 30 prompts so it doesn't repeat itself. Store the
     result with `source = 'ai'`. No writer's text is sent, so this doesn't
     need the writing assistant's consent (`assistant_allowed_at`).
- The `open_prompt` job (T1), run at the start of each prompt day, checks that
  the day has a prompt. If it doesn't (the prepare job failed or the server
  was down), it runs the same steps; if the AI call fails, it uses a small
  built-in list of evergreen prompts, so a day always has a prompt.
- Writers aren't told whether a prompt was curated or written by AI.
- Admins are warned on the admin page when fewer than 7 prompts are queued.
- Admins can edit a prompt after its day has opened, but the change is logged
  in the audit log. They can't delete it once anyone has written to it.
- The API:
  - `GET /api/prompts/today`
  - `GET /api/prompts?before=<day>&limit=`, for the archive
  - `GET /api/prompts/{day}`
  - Future days are never returned.

### Acceptance criteria

- [ ] With nothing queued, a day still opens with a prompt (AI, or the
      built-in list if the AI call fails). Tested with a stubbed AI.
- [ ] A prompt pinned to a date opens on that date, and tomorrow's prompt is
      visible and editable on the admin page from noon Pacific.
- [ ] A prompt can't be seen before its day opens, not even through the
      archive API.
- [ ] Only admins can see or change the queue.
- [ ] The archive pages back through every past day.

### Affected area

API · Web UI · AI summaries

Depends on: T1, T2.

---

## T4 — `[Feature]: Daily Prompts page, enrollment, and the header link`

**Labels:** feature · **Affected area:** Web UI, Notifications

### What problem would this solve?

Daily Prompts needs a home, and writers choose whether to take part.

### Proposed solution

- A **Daily Prompts** link in the header, after Desk and Review
  (`web/src/App.tsx`), going to `/prompts`.
- `/prompts` for someone who isn't enrolled: today's prompt, a short
  explanation, and **Join Daily Prompts**. They can't see the feed or write
  an answer. Joining doesn't turn on email.
- `/prompts` for someone who's enrolled has three parts:
  - **Today**: the prompt, the writer's own status (*Not started* /
    *Draft* / *Submitted*), and **Write** or **Continue**.
  - **Today's submissions**: the feed (T6).
  - **Archive**: past prompts, each showing whether the writer answered it.
- **Preferences** on the page:
  - leave Daily Prompts
  - email on or off
  - the time to send the email, in the writer's own timezone. The timezone is
    detected from the browser and can be changed.
  - Email can't be turned on until the address is verified, and the page links
    to the existing verification banner.
- `POST/DELETE /api/prompts/enrollment`, `PATCH /api/prompts/enrollment`.
- Leaving turns off the email and hides the feed from the writer. Their
  submitted entries and critiques stay visible to others, and they can still
  open their own entries. Rejoining brings the feed back.
- On account deletion, the enrollment and the record of emails sent are
  deleted.

### Acceptance criteria

- [ ] The header link shows for signed-in writers and highlights on
      `/prompts/*`.
- [ ] Join, leave and the email preferences round-trip and are in the data
      export (*6.2*), listed in the export's readme (`routes/privacy.rs`).
- [ ] After leaving, the writer's submitted entries still show in the feed
      for others.
- [ ] The feed isn't returned, by the UI or the API, to someone who isn't
      enrolled.

### Affected area

Web UI · Notifications · API

Depends on: T3.

---

## T5 — `[Feature]: Writing an answer: private draft, submit, edit, withdraw`

**Labels:** feature · **Affected area:** Editor, API

### What problem would this solve?

The writing part of the feature: somewhere to answer the prompt that works the
way the rest of the editor does.

### Proposed solution

- `/prompts/{day}/write` opens the existing editor in a **single-scene mode**:
  the prompt shown above the page, no outline, no chapters, no versions. It
  keeps the goals bar, the writing room (⌘⇧F), focus mode and type settings.
  There is no word limit.
- Writing needs enrollment (T4). One entry per writer per prompt, autosaved
  as a private `draft`. Drafts are never shown to anyone else.
- Words written here count toward the writer's daily and weekly goals
  (*8.2*). Each entry save reports its net word change to `writing_days`, with
  the day supplied by the client, the same way scene saves do.
- **Submit** makes the entry public to enrolled writers. A writer can submit
  on any day. `submitted_at` is set by the **first** submission and never
  changes after that, not by edits and not by withdrawing and submitting
  again. An entry is *late* when its first submission came after the
  prompt's Pacific day ended. Each prompt's list of submissions (T6) shows
  late entries too, marked *late*.
- **Edit** after submitting changes the published text in place and shows
  *edited* with the time. No history is kept.
- **Withdraw** takes the entry back to a private draft and hides it from the
  feed. Its critiques are kept. While it's withdrawn, the author sees all of
  them, and each critic sees and can edit or delete their own; nobody else
  sees any. Submitting again shows them all again.
- **Delete** removes the entry and **all** of its critiques, after asking
  first. Inbox items about the entry or its critiques are deleted in the same
  transaction.
- On account deletion, the writer's entries are deleted, as their works are.
  Critiques on them go with them.
- The API:
  - `GET/PUT /api/prompts/{day}/entry`
  - `POST …/entry/submit`
  - `POST …/entry/withdraw`
  - `DELETE …/entry`
- Offline: entries go through the same offline save queue the editor uses.

### Acceptance criteria

- [ ] Drafts autosave and survive a reload and offline/online.
- [ ] Submit, edit (with the *edited* mark), withdraw and resubmit each behave
      as described. API tests cover who can see what at each step.
- [ ] Words count toward goals.
- [ ] Withdrawing and submitting again, or editing, doesn't change
      `submitted_at` or the *late* mark.
- [ ] Entries are in the data export and are deleted with the account.
- [ ] The writing room works on an entry.

### Affected area

Editor · Writing room (distraction-free) · Word-count goals · API

Depends on: T4.

---

## T6 — `[Feature]: The shared feed of submissions`

**Labels:** feature · **Affected area:** Web UI, API

### What problem would this solve?

Writers enrolled in Daily Prompts want to read what others made of the same
prompt.

### Proposed solution

- **Today's submissions** on `/prompts`, and the same list on each archive day
  (`/prompts/{day}`). An archive day's list includes entries submitted after
  the day ended, marked *late* (T5).
- Each card shows the author's display name (linking to their profile), the
  first few lines, the word count, the critique count, and *edited* / *late*
  marks.
- Order: newest first, with a toggle for *fewest critiques first*, so new
  writers aren't buried.
- `/prompts/{day}/entries/{id}` shows the whole entry using the reader's
  typography, with the critiques below (T7).
- Entries stay in the feed after their author leaves Daily Prompts (T4).
- Blocking is out of scope for this epic (see *Future work*).
- The API: `GET /api/prompts/{day}/entries?sort=&cursor=` and
  `GET /api/prompt-entries/{id}`. Both require enrollment.

### Acceptance criteria

- [ ] Only submitted entries show, paged by cursor. Drafts and withdrawn
      entries never do.
- [ ] Late entries show on the archive day, marked.
- [ ] Non-enrolled requests get 403.

### Affected area

Web UI · API · Profiles, following & discovery

Depends on: T5.

---

## T7 — `[Feature]: Critiques: threaded comments at any depth, with reporting`

**Labels:** feature · **Affected area:** Feedback & comments, Moderation & reporting

### What problem would this solve?

Writers want responses to what they submitted. Critiques are a simple
comment-and-reply system, not anchored notes or reactions.

### Proposed solution

- `prompt_critiques` with `parent_id`, so a reply is just a critique on a
  critique. There is **no depth limit**.
- The UI indents replies to a capped visual depth, around 6 levels. Below
  that, the thread continues flat with "replying to *name*", so deep threads
  stay readable on a phone.
- Every enrolled writer can read every critique on a submitted entry. Names
  are always shown.
- Writers can edit their own critiques (marked *edited*) and delete them. A
  deleted critique that has replies stays as a "removed" placeholder, the same
  rule as *6.2*.
- **Report**: uses the existing reasons (`comment_reports` shape). A report
  goes to the entry's author, who can remove the critique, the same way
  reports on book comments go to the book's author. Site moderation and an
  admin report queue are future work (see the epic).
- Blocking is out of scope for this epic (see *Future work*).
- Rate limit on posting critiques (`limit.rs`).
- Inbox items for new critiques and replies are added by T11, not here.
- The API:
  - `GET /api/prompt-entries/{id}/critiques`, which returns the whole tree
    flat, with `parent_id`, for the client to nest
  - `POST` (with an optional `parent_id`)
  - `PATCH /api/prompt-critiques/{id}`
  - `DELETE`
  - `POST …/report`

### Acceptance criteria

- [ ] Replies nest to any depth through the API. The UI caps indentation and
      stays readable at depth 20.
- [ ] Deleting a critique with replies leaves a placeholder; without replies,
      it's removed.
- [ ] Reports reach the entry's author, who can remove the critique.
- [ ] On a withdrawn entry, the author sees every critique, each critic sees
      their own, and nobody else sees any.
- [ ] Critiques are in the data export, and are handled on account deletion
      as comments are (blanked and credited to a "former member").

### Affected area

Feedback & comments · Moderation & reporting · API

Depends on: T6.

---

## T8 — `[Improvement]: Split the Account page into Account and Settings tabs`

**Labels:** enhancement · **Affected area:** Web UI, Accounts / sign-in

### Current behavior

`/settings` (`web/src/pages/SettingsPage.tsx`) is one long page: Address,
Password, Two-factor sign-in, Signed in, Summaries, Your data, and Delete the
account. T9 adds a writing setting, and Daily Prompts adds more.

### Proposed improvement

- Two tabs, using `components/Tabs.tsx`:
  - **Account**: Address, Password, Two-factor sign-in, Signed in, Your data,
    Delete the account.
  - **Settings**: Summaries, a new **Writing** section, and a link to the Daily
    Prompts preferences.
- The tab is in the URL (`/settings/account`, `/settings/general`), and
  `/settings` redirects to Account, so existing links still work.
- The new **Writing** section has one checkbox: **"Treat a blank line (double
  Enter) as a scene break when turning text into a project"**. It's off by
  default. It's stored as `users.double_enter_splits_scenes` and returned
  with the rest of the writer's settings, so iOS reads the same value.
- The header link stays *Account* and goes to `/settings`.

### Why it matters

The page is already long, and each new feature makes it longer.

### Related issues

T9, T15.

---

## T9 — `[Feature]: Turn an entry into a project, with scene separators`

**Labels:** feature · **Affected area:** Chapters, scenes & outline, Notebook

### What problem would this solve?

Some answers deserve to keep going. A writer should be able to make one into a
real project in one step, without copying and pasting.

### Proposed solution

- A **Turn into a project** button on the writer's own entry, whether it's a
  draft, submitted or withdrawn.
- It creates a new work owned by the writer:
  - **Title**: the first line of the prompt, shortened and editable in a
    confirm dialog. The writer can rename it after.
  - **One chapter**, holding the entry split into scenes.
  - The **prompt** is saved as a notebook entry of kind *note*, titled
    "Daily prompt — {date}".
- **Scene separators**: the text is split into scenes at:
  - **A line that is only `--`** (whitespace around it is ignored). This
    always applies. The separator line itself is dropped.
  - **A blank paragraph, i.e. double Enter**, but **only when the writer has
    turned on** the setting from T8. Runs of several blank paragraphs count as
    one break.
  - Empty scenes produced by the split are dropped. Text with no separators
    becomes one scene.
- A preview in the confirm dialog shows how many scenes will be created, and
  their first lines.
- The project is a **copy**. The entry stays as it was, and later edits to
  either one don't affect the other. The entry shows "Turned into *{work}*"
  with a link, and can be turned into a project again (making another copy).
- Words copied into the new work don't count toward goals a second time:
  creating the work isn't a save, so it reports nothing to `writing_days`.
- The API:
  - `POST /api/prompt-entries/{id}/to-work/preview` returns the scenes the
    split would make (count and first lines) without creating anything. Both
    the web dialog and iOS (T12) use it, so neither client implements the
    rules itself.
  - `POST /api/prompt-entries/{id}/to-work { title }` returns the new work.
  - The splitting lives in `domain/` as a pure function, shared by both
    routes and the tests.

### Acceptance criteria

- [ ] Unit tests for the splitter:
  - `--` on its own line splits
  - `--` inside a sentence, an em dash, or `---` does not
  - blank paragraphs split only with the setting on
  - runs of blank paragraphs collapse into one break
  - leading and trailing separators don't create empty scenes
- [ ] The new work opens straight away, with its scenes and the notebook note.
- [ ] The preview matches the work that's created.
- [ ] Converting doesn't change the entry or its critiques, and doesn't add
      to the writer's word count for the day.

### Affected area

Chapters, scenes & outline · Notebook · API

Depends on: T5, T8.

---

## T10 — `[Feature]: The daily prompt email`

**Labels:** feature · **Affected area:** Notifications

### What problem would this solve?

Writers who have to remember to visit a page don't keep a daily habit. A
writer who opts in should get the prompt in their inbox, with a link straight
to the page where they write their answer.

### Proposed solution

- The `send_prompt_emails` job (T1), checked every minute, finds every writer
  who:
  - has email on and a verified address
  - hasn't been sent today's prompt yet
  - has reached the time they chose (`email_local_time` in their
    `email_timezone`)
- **Daylight saving in the writer's timezone:** a chosen time that doesn't
  exist that day (02:30 when clocks go forward) is sent at the first minute
  after the gap. A time that happens twice (when clocks go back) is sent the
  first time.
- **Which prompt is sent:** whichever prompt day is open in Pacific time at
  the moment of sending. Because Pacific midnight falls in the morning or
  afternoon in many places (08:00 in London, 17:00 in Tokyo), the time picker
  shows a note when the chosen time falls in the last 6 hours of a Pacific
  day: *"Today's prompt closes at {local time}. You'll have {n} hours."* The
  default send time is the first hour after the prompt opens in the writer's
  timezone, rounded to the hour, with a minimum of 07:00 local time.
- **Content:**
  - the prompt text
  - a **Write your answer** link to `/prompts/{day}/write`. A writer who is
    signed out goes to sign-in and comes back to the same link. There is no
    magic-link sign-in.
  - a link to today's submissions
  - a footer with a **one-click unsubscribe** that turns email off, plus
    `List-Unsubscribe` and `List-Unsubscribe-Post` headers.
  - Both plain text and HTML.
- Each send is recorded in `prompt_email_sends (user_id, day, sent_at)` with
  `UNIQUE(user_id, day)`, written in the same transaction as the message is
  put in the outbox (T1), so a writer never gets two emails for one day. Mail
  failures are retried with backoff for the rest of the day, then given up.
- Works end to end against the outbox today. Turning on SMTP is a deployment
  setting.

### Acceptance criteria

- [ ] Smoke test: enroll, turn email on, advance the clock, read the email
      from the outbox, follow the link, and land on the writing page for that
      day.
- [ ] Unsubscribe works with one click without being signed in, and is
      covered by a test.
- [ ] No emails to unverified addresses. Tests cover DST days in both Pacific
      and the writer's timezone, including a skipped and a repeated local
      time, and writers in at least three timezones.

### Affected area

Notifications · Accounts / sign-in

Depends on: T1, T4, T5. Going live depends on a sending domain (epic dependency).

---

## T11 — `[Feature]: In-app inbox with a daily summary (the prompts part)`

**Labels:** feature · **Affected area:** Notifications, Web UI

### What problem would this solve?

There's no way to find out that someone responded to you without going to
look. *2.4* left notifications undone for exactly this reason. Daily Prompts
needs an inbox now, and project comments will need it next. This ticket
builds the inbox and what Daily Prompts needs. The project-comment events will
be added in a separate ticket.

### Proposed solution

**Generic model, so project comments can be added later:**
- `inbox_items (id, user_id, kind, subject_type, subject_id, actor_id, created_at, read_at)`.
  `kind` is an open set, and a registry in `domain/` maps each kind to how it
  is described and where it links.
- Items are created in the same transaction as the event that caused them.
  Each writer only gets items about things they're allowed to see.
- Nobody gets an item about their own action: replying to your own critique,
  or critiquing your own entry, creates nothing for you.
- When the thing an item is about is deleted (an entry, or a critique with no
  replies), its items are deleted in the same transaction.

**Prompt events in this ticket** (moved here from T7):
- `prompt.critique`: someone critiqued your entry.
- `prompt.reply`: someone replied to your critique. The entry's author also
  gets this for replies anywhere in a thread on their entry, but only once per
  event, not once as author and again as the parent's author.
- "Today's prompt is open" isn't stored as an item. The digest adds it when
  it's shown.

**Daily summary:**
- Built by a job (T1) at the start of each Pacific prompt day, for each writer
  with any activity in the previous day.
- It reads like *"Yesterday: 4 critiques and 2 replies on your answer to
  '{prompt}'. Today's prompt is open."*
- It's pinned at the top of the inbox until read.
- The summary groups its counts by `kind`, so project kinds can be added
  without changing the summary code.

**UI:**
- A bell in the header with an unread count. It's fetched on navigation and
  when the window regains focus. Live updates need a per-user live room,
  which is future work (see the epic).
- `/inbox` lists everything with read/unread state, has *Mark all read*, and
  each item links to the thing it's about.

**API:** `GET /api/inbox?cursor=`, `POST /api/inbox/{id}/read`,
`POST /api/inbox/read-all`, `GET /api/inbox/unread-count`.

### Acceptance criteria

- [ ] Critiques and replies create the right items for the right people, with
      no duplicates and no items about a writer's own actions. Covered by
      tests.
- [ ] Deleting an entry or critique removes the items about it.
- [ ] The daily summary is built once per day per writer with activity, and
      survives a restart.
- [ ] The unread count is up to date after navigating or returning to the
      window.
- [ ] Adding a new `kind` needs only a registry entry. Proven by a test kind
      in the tests.
- [ ] Inbox items are in the data export and deleted with the account.

### Affected area

Notifications · Web UI · API

Depends on: T1, T7. Followed by the project-comment notifications ticket (to
be written by the maintainer).

---

## T12 — `[Feature]: iOS: Daily Prompts — read, write, submit, turn into a project`

**Labels:** feature · **Affected area:** iOS app

### What problem would this solve?

Writers using the app on their phone should be able to answer the prompt
there, and the email link should open the app.

### Proposed solution

- A **Daily Prompts** tab or section in the iOS app, with today's prompt, the
  archive, and the feed.
- Join and leave Daily Prompts, and set the email preferences.
- Writing uses the iOS rich-text editor (*4.1*) in a single-scene mode, with
  offline-first saving (*4.2*).
- Submit, edit, withdraw and delete, with the same rules as T5.
- **Turn into a project**, using the same separator rules as T9. The confirm
  sheet shows the scenes from T9's preview endpoint, and the conversion
  endpoint does the splitting, so the rules stay the same on both platforms.
- Universal links: `/prompts/{day}/write` in the email opens the app when it's
  installed. This needs an `apple-app-site-association` file served by the
  web server (there isn't one yet) and the Associated Domains entitlement in
  the app.

### Acceptance criteria

- [ ] Everything in T4–T6 and T9 works on iPhone and iPad.
- [ ] The email link opens the writing screen in the app.
- [ ] An entry written offline syncs and can be submitted once the device is
      back online.

### Affected area

iOS app · Offline sync

Depends on: T4, T5, T6, T9.

---

## T13 — `[Feature]: iOS: critiques`

**Labels:** feature · **Affected area:** iOS app, Feedback & comments

### Proposed solution

- Read and write critiques and replies on iOS. The visual depth cap is lower
  on a phone, around 3 levels, after which the thread continues flat with
  "replying to *name*".
- Edit, delete and report, with the same rules as T7.

### Acceptance criteria

- [ ] Parity with T7. A 20-deep thread is readable on the smallest supported
      iPhone.

Depends on: T7, T12.

---

## T14 — `[Feature]: iOS: inbox`

**Labels:** feature · **Affected area:** iOS app, Notifications

### Proposed solution

- The inbox and the pinned daily summary on iOS, with an unread badge on its
  tab.
- No badge on the app icon: without push, it would only update while the app
  is open, and would show a stale count the rest of the time.
- Push notifications are **not** included: they need APNs keys and a decision
  about how often to notify. That gets its own ticket when wanted, along with
  the app-icon badge.

### Acceptance criteria

- [ ] Parity with T11's list, read state and summary. The tab badge matches
      the web's unread count.

Depends on: T11, T12.

---

## T15 — `[Feature]: iOS: the double-Enter setting and the split settings screen`

**Labels:** feature · **Affected area:** iOS app, Accounts / sign-in

### Proposed solution

- Mirror T8's Account / Settings split in the iOS settings screen.
- Add the **"Treat a blank line (double Enter) as a scene break"** toggle,
  synced with the web through the same user setting.

### Acceptance criteria

- [ ] Changing the toggle on either platform is reflected on the other, and
      T12's *Turn into a project* respects it.

Depends on: T8, T12.

---

## ROADMAP.md addition

Insert before *Sequencing note*:

```markdown
## Phase 9 — Daily Prompts

A prompt a day, for enrolled writers: by email, or under **Daily Prompts** in
the header. Write an answer; keep it, submit it for other writers to read and
critique, or turn it into a project. The day is Pacific time, the server's.

### 9.1 Groundwork · **Later**
- A job runner that survives restarts and runs each job once, and the Pacific
  prompt day. Site administrators, the first role that is not one work's.

### 9.2 Prompts · **Later**
- A curated queue with an admin page, and prompts pinned to a date. Claude
  writes one the day before when the queue is empty, and a built-in list
  covers a failed call. An archive of every past
  day, still open to write on.

### 9.3 Writing and sharing · **Later**
- Opt-in enrollment. A private, autosaved answer in the editor's single-scene
  mode; submit, edit (marked), withdraw. A feed of the day's submissions,
  visible to enrolled writers only.
- Critiques: plain comments, public, named, with replies to any depth;
  reporting reaches the entry's author. Site moderation, blocking and live
  inbox notices come later.

### 9.4 Into a project · **Later**
- Any answer becomes a new work, split into scenes at a line of `--`, and at
  a blank line when the writer has asked for that (a new *Writing* setting, on
  an Account page now split into *Account* and *Settings*). The prompt goes
  into the notebook.

### 9.5 Reaching people · **Later**
- The prompt by email at each writer's chosen local time, with one-click
  unsubscribe. An in-app inbox — critiques, replies, a daily summary —
  built to take project comments next.

### 9.6 iOS · **Later**
- Writing, critiques, the inbox and the new setting on the phone; the email's
  link opens the app.
```
