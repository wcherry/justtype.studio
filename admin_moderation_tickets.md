# Users and moderation in the admin dashboard — ticket drafts

Draft for review before filing on GitHub. One epic plus child issues, in the
same shape as the Daily Prompts epic. Placeholders `A1`…`A19` are swapped for
issue numbers when filed.

| # | Type | Title |
|---|---|---|
| Epic | Epic | Site administration: managing accounts, restrictions and moderation |
| A1 | Maint | Staff roles: moderators beside admins, and one permission check |
| A2 | Feature | The admin dashboard shell, and the list of accounts |
| A3 | Feature | An account's page: who they are, what they've done, what's been done to them |
| A4 | Feature | Create an account for someone |
| A5 | Feature | Edit an account: name, address, verification, second factor |
| A6 | Feature | Password reset and sign-out, sent by staff |
| A7 | Feature | Deactivate or delete an account |
| A8 | Maint | Restrictions: the model, and enforcing it in one place |
| A9 | Feature | Restricting an account: ban, suspend or read-only, site-wide or in discussions |
| A10 | Feature | Telling the writer, and an appeal |
| A11 | Feature | Keeping a banned person out: the permanent ban list |
| A12 | Feature | Reporting everywhere, and reports that reach staff |
| A13 | Feature | The moderation queue: flagged content and flagged accounts |
| A14 | Feature | The word filter |
| A15 | Feature | External moderation: sending public content to a moderation platform |
| A16 | Feature | The moderation log |
| A17 | Feature | iOS: restricted accounts, removed content and the new reports |
| A18 | Maint | Closing an account: the 30-day window, then deletion |
| A19 | Feature | External moderation: decisions made on the platform come back |

---

## Epic — `[Epic]: Site administration: managing accounts, restrictions and moderation`

**Labels:** epic

### Roadmap section

10 — Site administration and moderation

### Goal

Give the people who run Just Type Studio the tools to look after the site
without opening a database shell: find an account and fix what's wrong with
it, restrict someone who's causing harm, and deal with what readers report or
the filters catch. All of this happens in the existing **Admin** area of the
web app.

### Background

Until now the product has had no moderator. The moderation migration
(`2026-09-23-000010_moderation`) says so: a report on a comment goes to the
author whose work it's on, and `blocked_readers` is one writer's door, not a
platform ban. Daily Prompts kept that rule and listed site moderation under
its *Future work*.

That doesn't hold any more. Daily Prompts lets writers read and critique
strangers without an invitation, invites (`routes/invites.rs`) bring in
people nobody on staff knows, and a report about an author's own behaviour
can only go to that author. The pieces this builds on already exist:

- `users.is_admin`, the `AdminUser` extractor and the `/admin/prompts` page
  (Daily Prompts T2/T3).
- `AuthUser` checks the session on every request (`auth/mod.rs`), so a
  restriction can take effect at once, not when a token expires.
- `email_tokens` with a `reset` purpose, and `/api/auth/password/forgot`.
- `sessions` with `revoked_at`, and *sign out everywhere else*.
- `comment_reports`, `prompt_critique_reports` and `ReportReason`.
- `audit_events`, which outlives what it records.
- The job runner (`jobs.rs`), the mail queue, and the inbox (`domain/inbox.rs`).
- Account deletion with hand-over to a former member (`privacy.rs`).
- The in-process rate limiter (`limit.rs`).

### Principles

These hold for every ticket below.

1. **Private writing is never moderated.** Manuscripts, drafts, notebooks,
   annotations and unsubmitted Daily Prompts answers aren't scanned, filtered,
   or shown to staff. Moderation applies only to what other people can see:
   comments, reader questions and answers, Daily Prompts submissions and
   critiques, display names, bios, *about the author*, and publication
   titles and blurbs.
2. **Staff never see or set a password**, and never see a reset or setup
   token. They can only cause the normal email to be sent.
3. **Deletion only follows the two closing restrictions, and never at
   once.** A site-wide *read-only* restriction and a *permanent ban* both
   close the account: 30 days later the account, all of its writing and all
   of its comments are deleted (A18). Read-only leaves those 30 days for the
   writer to export their work. A permanent ban hides everything at once and
   leaves those 30 days for an appeal. No other restriction deletes
   anything.
4. **Everything staff do is recorded** in `audit_events`, with a reason, and
   shows in the moderation log (A16).
5. **Authors keep their own tools.** An author can still delete comments on
   their own publication and block readers. Site moderation is added on top
   of that and doesn't replace it.
6. **Admins are still made on the command line.** `is_admin` is never set
   through the API (Daily Prompts T2). Admins can make and unmake
   moderators from the dashboard.

### Scope

- A dashboard at `/admin` with sections for accounts, the moderation queue,
  the word filter, external moderation settings, the permanent ban list,
  the log, and the existing prompts page.
- Two staff roles: **admin** (everything) and **moderator** (content and
  discussion restrictions, no account details). Moderators can be in-house
  staff or outside contractors, so they don't see email addresses or other
  personal details (A1).
- Accounts: list, search, view, create, edit, send a password reset, sign
  out every device, deactivate, delete.
- Restrictions:
  - **Kind:** *ban* (can't sign in), *suspend* (can sign in and read, but
    can't post or join discussions) or *read-only* (can read and export, but
    can't write, edit, publish or post anything).
  - **Duration:** 3 days, 7 days, 30 days, or permanent.
  - **Scope:** the whole site, or **discussions only** (comments, reader
    questions and answers, Daily Prompts submissions and critiques).
  - **Closing restrictions:** *read-only, site-wide* and *permanent ban,
    site-wide* both end in the account being deleted 30 days later (A18).
    Read-only, site-wide always runs those 30 days and has no other
    duration.
- Telling the writer what happened and why, by email and in the inbox, with
  one appeal per restriction.
- A permanent ban list that keeps the address and the reason for every
  permanent ban, forever, so that address can never sign up again (A11).
- Reporting entries, questions, answers and accounts, as well as comments
  and critiques. Reports go to the author as they do today, and also to
  staff.
- A moderation queue of flagged content and flagged accounts, from four
  sources: reports, the word filter, the external platform, and automatic
  thresholds.
- The word filter: a list of terms, each with an action (block, hold for
  review, flag, or mask).
- **Internal and external moderation.** *Internal* means staff working our
  own queue (A13). *External* means a hosted content-moderation platform in
  the style of Stream's moderation product. It classifies public text (and
  images later) with AI and its own blocklists, recommends an action, and
  has its own review dashboard where outside moderators can work (A15).
  Decisions made there come back to us by webhook and are applied through
  the same code as our own actions (A19). The platform sits behind a
  provider interface, so the vendor can be changed.
- A moderation log of every staff action.
- iOS follow-up after the web version ships.

### Out of scope

- Billing, plans and refunds (*6.4*, deferred).
- Granting `is_admin` from the dashboard.
- Impersonating a user ("log in as"). It's useful for support but needs its
  own threat model.
- Scanning private writing for any reason.
- IP or device bans. Addresses are shared and change, and the cost of a wrong
  ban is high. A11 blocks email addresses only. See *Future work*.
- Running our own AI classifier. Classification is the external platform's
  job (A15).
- Shadow bans. Every restriction is visible to the person it applies to.
- Bulk actions across many accounts at once (beyond the queue's bulk
  dismiss).
- A public transparency report.

### Future work

- **IP and device signals** for ban evasion, if A11 turns out not to be
  enough.
- **Trusted flaggers**: weighting reports from long-standing readers more
  heavily.
- **Support impersonation** with consent and a visible banner.
- **Moderating images** through the external platform, if public profiles
  ever get avatars that people upload.

### Proposed architecture

**Data model (new migration, `…_moderation_staff`):**

- `users.is_moderator INTEGER NOT NULL DEFAULT 0` (A1).
- `users.deactivated_at TEXT NULL` (A7).
- `users.closing_at TEXT NULL` and `users.purge_at TEXT NULL`: set by a
  closing restriction, cleared if it's lifted or overturned (A18).
- `staff_notes (id, user_id, author_id, body, created_at)`: internal notes on
  an account, which only staff can see (A3).
- `restrictions (id, user_id, kind 'warning'|'ban'|'suspend'|'read_only', scope 'site'|'discussion', starts_at, ends_at NULL, reason_code, reason_text, internal_note NULL, created_by, created_at, lifted_at NULL, lifted_by NULL, lift_reason NULL)`.
  A permanent restriction has `ends_at = NULL`. Temporary ones last 3, 7 or
  30 days. A restriction is in force
  when `starts_at <= now AND (ends_at IS NULL OR ends_at > now) AND lifted_at IS NULL` (A8).
- `appeals (id, restriction_id UNIQUE, body, state 'open'|'upheld'|'overturned', decided_by NULL, decision_text NULL, created_at, decided_at NULL)` (A10).
- `permanent_bans (id, email, email_canonical UNIQUE, former_user_id, display_name, reason_code, reason_text, restriction_id NULL, banned_by, created_at, removed_at NULL, removed_by NULL, removed_reason NULL)`.
  No foreign keys, like `audit_events`: the row has to outlive the account it
  records (A11).
- `blocked_domains (domain PRIMARY KEY, note, created_by, created_at)` for
  throwaway-mail domains (A11).
- `reports (id, subject_type, subject_id, subject_author_id, reporter_id, reason, detail NULL, created_at, UNIQUE(subject_type, subject_id, reporter_id))`.
  This is the general table for new kinds of report. `comment_reports` and
  `prompt_critique_reports` stay as they are, so authors' views don't
  change (A12).
- `moderation_items (id, subject_type, subject_id, subject_author_id, state 'open'|'held'|'actioned'|'dismissed', sources JSON, report_count, top_reason, first_flagged_at, last_flagged_at, decided_by NULL, decided_at NULL, decision NULL, UNIQUE(subject_type, subject_id))`.
  There's one row per flagged thing, whatever flagged it, so twenty reports
  are one item in the queue (A13).
- A `moderation_state` column on each moderated table (`comments`,
  `questions`, `answers`, `prompt_entries`, `prompt_critiques`):
  `'visible'|'held'|'removed'`. It's separate from the author's own
  `deleted_at`, so restoring a removed item doesn't undo an author's
  deletion (A13).
- `word_filter_terms (id, term, normalized, match 'word'|'substring', action 'block'|'hold'|'flag'|'mask', applies_to JSON, note, created_by, created_at, updated_at)` (A14).
- `moderation_settings (key PRIMARY KEY, value JSON, updated_by, updated_at)`
  for thresholds, the escalation ladder and the external platform's
  configuration (A13, A15).
- `external_checks (id, subject_type, subject_id, provider, external_id NULL, state 'pending'|'done'|'failed', attempts, recommended_action NULL, labels JSON NULL, created_at, checked_at NULL)` (A15).
- `external_events (id, provider, event_id UNIQUE, kind, payload JSON, received_at, applied_at NULL, error NULL)`.
  Webhooks are stored before they're applied, and a repeat `event_id` is
  ignored (A19).
- New `AuditAction`s: `AccountCreatedByStaff`, `AccountEdited`,
  `PasswordResetSent`, `SessionsRevokedByStaff`, `AccountDeactivated`,
  `AccountReactivated`, `AccountDeletedByStaff`, `ModeratorGranted`,
  `ModeratorRevoked`, `RestrictionAdded`, `RestrictionChanged`,
  `RestrictionLifted`, `AppealDecided`, `PermanentBanListed`,
  `PermanentBanRemoved`, `AccountPurged`, `ContentRemoved`,
  `ContentRestored`, `ContentHeld`, `ReportDismissed`, `WordFilterChanged`,
  `ExternalModerationChanged`.

**Backend:**

- `auth/mod.rs`: `StaffUser` and `Permission` (A1). `AuthUser` also loads the
  restrictions in force on the account (A8).
- `domain/restrictions.rs`: one function, `can(user, Capability) -> Result<(), ApiError>`,
  which every write route calls (A8).
- `domain/filter.rs`: normalizing text and matching it against the word list
  (A14).
- `domain/external_moderation.rs`: the `ModerationProvider` trait, with a
  `StreamProvider` (or whichever vendor is chosen) and a `NoProvider`
  (A15, A19).
- New route modules: `routes/admin_users.rs`, `routes/admin_moderation.rs`,
  `routes/appeals.rs`, and `routes/moderation_webhooks.rs` (A19).
  `routes/admin.rs` keeps the prompts and jobs.
- New jobs in `jobs.rs`: `external_checks` and `apply_external_events`
  every tick, `expire_restrictions` (sends the *your restriction has ended*
  notice), `auto_flag_accounts`, `closing_reminders`, and `purge_accounts`
  daily (A18).

**Web:**

- `/admin` becomes a shell with a side menu (A2), and `/admin/prompts` moves
  inside it unchanged.
- New pages: `AdminUsersPage`, `AdminUserPage`, `AdminQueuePage`,
  `AdminWordsPage`, `AdminExternalPage`, `AdminBansPage`, `AdminLogPage`.
- The *Admin* header link shows for moderators too and opens on the queue.
- Writer-facing: a restriction banner, the appeal form, a *removed by a
  moderator* placeholder, and **Report** on every public thing.

**iOS:** follow-up ticket A17.

### Milestones

- [ ] Phase 1 — Groundwork: staff roles (A1), the dashboard and account list (A2), the account page (A3)
- [ ] Phase 2 — Managing accounts: create (A4), edit (A5), reset and sign-out (A6), deactivate and delete (A7)
- [ ] Phase 3 — Restrictions: the model and enforcement (A8), the UI (A9), notices and appeals (A10), the permanent ban list (A11), closing and deletion (A18)
- [ ] Phase 4 — Moderation: reporting (A12), the queue (A13), the word filter (A14), the log (A16)
- [ ] Phase 5 — External moderation: sending content (A15), decisions coming back (A19)
- [ ] Phase 6 — iOS (A17)

### Child issues

- [ ] A1 … A19

### Dependencies

- A sending domain and SMTP credentials before the staff-sent mail in A4, A6
  and A10 goes beyond the outbox. Until then it can be tested against the
  outbox, as Daily Prompts was.
- An account with the external moderation platform, its API key and webhook
  secret, and a signed data processing agreement before A15 is turned on in
  production. Until then, A15 and A19 are built and tested against a stub
  provider.
- Community guidelines written in plain language, before A9 ships. Every
  restriction cites a section of them.

### Acceptance criteria

- [ ] An admin can find any account by name, email or id, open it, and see
      its status, sessions, works, reports, restrictions and notes.
- [ ] An admin can create an account, and the person sets their own password
      from an email. No staff member ever sees a password or a token.
- [ ] An admin can send a password reset and sign out every device for an
      account.
- [ ] Staff can ban, suspend or make an account read-only, for 3, 7 or 30
      days or permanently, across the site or only in discussions. Every
      write route enforces it, and a test proves each route does.
- [ ] A restricted writer is told what happened, why and until when, and can
      appeal once.
- [ ] A site-wide read-only account can export its work for 30 days, and is
      then deleted with all of its writing and comments.
- [ ] A permanently banned account's public things are hidden at once, and
      the account, its writing and its comments are deleted 30 days later.
      Until then, the person can get their export by email.
- [ ] A co-written book owned by a closed account passes to its earliest
      collaborator instead of being deleted.
- [ ] Every permanent ban leaves a row with the address and the reason that
      outlives the account, and that address can never register again.
- [ ] Anything public can be reported. Reports reach the author as they do
      today and also reach the staff queue.
- [ ] The queue shows flagged content and flagged accounts from reports, the
      word filter, the external platform and thresholds, and staff can
      dismiss, remove, restore or restrict from it.
- [ ] A decision made on the external platform's dashboard is applied here
      and appears in our log.
- [ ] Private writing is never sent to the word filter, the external
      platform or the queue, and a test proves it.
- [ ] Moderators can't see email addresses or other personal details, or
      change accounts.
- [ ] Every staff action is in the moderation log with who, what, when and
      why.
- [ ] The privacy page and terms describe staff moderation, the external
      platform, the 30-day deletion, and keeping a banned address forever,
      and `terms_version` is bumped.
- [ ] ROADMAP.md Phase 10 is updated as each piece lands.

---

## A1 — `[Maint]: Staff roles: moderators beside admins, and one permission check`

**Labels:** maintenance · **Type:** Security hardening · **Component:** Backend (Rust), Web (React)

### Why is this needed?

Today there is one site-wide role, `is_admin`, and one extractor,
`AdminUser`. Moderation needs a second role with less power, for people who
review content but shouldn't see addresses or change accounts. Those people
may be outside contractors. Checking roles separately in each route would
drift, so this ticket adds one place where permissions are decided.

### Proposed approach

- Add `users.is_moderator`. Admins can grant and revoke it from the account
  page (A3), and it's recorded as `ModeratorGranted` / `ModeratorRevoked`. An
  admin is implicitly a moderator.
- A `Permission` enum and a single table mapping each role to its
  permissions:

  | Permission | Moderator | Admin |
  |---|---|---|
  | `ViewQueue`, `ActOnContent` | ✓ | ✓ |
  | `RestrictDiscussion` (3, 7, 30 days or permanent) | ✓ | ✓ |
  | `RestrictSiteTemporary` (ban or suspend, 3, 7 or 30 days) | ✓ | ✓ |
  | `CloseAccount` (read-only site-wide, or permanent site ban) | | ✓ |
  | `ManagePermanentBans` (the list in A11) | | ✓ |
  | `DecideAppeal` (not on their own restriction) | ✓ | ✓ |
  | `ViewAccountPii` (email, legal name, address, phone, IPs) | | ✓ |
  | `ManageAccounts` (create, edit, reset, delete) | | ✓ |
  | `ManageModerators`, `ManageWordFilter`, `ManageExternalModeration` | | ✓ |
  | `ViewLog` | own actions | all |

- A `StaffUser` extractor that carries the role, and
  `StaffUser::require(Permission)`. `AdminUser` stays and is the same check
  as `require` with an admin-only permission.
- A staff member can't act on their own account or on another staff
  member's account, except an admin acting on a moderator. Admins act on
  other admins only from the command line.
- `/api/auth/me` returns `staffRole: null | 'moderator' | 'admin'`, and the
  header link shows for both roles.

### Acceptance criteria

- [ ] A table-driven test calls every `/api/admin/*` route as a writer, a
      moderator and an admin, and checks the status code against the table
      above.
- [ ] `is_moderator` and `is_admin` are never accepted in any request body
      except the grant/revoke moderator route.
- [ ] Moderator responses never include email, legal name, mailing address,
      phone or IPs. A test serializes each admin response as a moderator and
      checks for those fields.
- [ ] A staff member can't restrict, edit or delete themselves.

---

## A2 — `[Feature]: The admin dashboard shell, and the list of accounts`

**Labels:** feature · **Affected area:** API, Web UI

### What problem would this solve?

The only admin page is `/admin/prompts`. Staff need one place to start, and
the most common job is finding an account.

### Proposed solution

- `/admin` gets a side menu: **Queue** (with an open-item count),
  **Accounts**, **Appeals**, **Permanent bans**, **Word filter**,
  **External moderation**, **Log**,
  **Prompts**, **Jobs**. Each item shows only if the role allows it. The
  existing prompts page moves in unchanged.
- The landing page shows counts: open queue items, held items, open
  appeals, restrictions ending in the next 24 hours, sign-ups today, and
  reports today.
- `GET /api/admin/users?q=&status=&role=&verified=&sort=&cursor=`:
  - `q` matches display name, or email or id for admins. A moderator's
    search matches display name only.
  - `status`: `active`, `unverified`, `restricted`, `banned`, `deactivated`.
  - Sorting by joined, last seen or report count.
  - Cursor pagination, 50 per page.
- The list shows the name, email (admins only), joined date, last seen, a
  verified tick, two-factor on/off, a role badge, a status badge (with the
  end date of any restriction), and the number of open reports.

### Acceptance criteria

- [ ] Searching by part of a name, a whole email, or an id finds the account.
- [ ] A moderator searching for an email address finds nothing and isn't
      told whether the address exists.
- [ ] Each filter and sort works, and paging doesn't repeat or skip rows when
      accounts are added in between.
- [ ] The page works at phone width.

### Affected area

API · Web UI

Depends on: A1.

---

## A3 — `[Feature]: An account's page: who they are, what they've done, what's been done to them`

**Labels:** feature · **Affected area:** API, Web UI

### What problem would this solve?

Before acting on an account, staff need to see it whole: whether it's new,
whether it's been reported before, and what other staff have already done or
noted.

### Proposed solution

`/admin/users/:id`, from `GET /api/admin/users/{id}`, with these sections:

- **Summary:** name, email and verified state (admins only), joined, last
  seen, two-factor, terms version, role, current restrictions.
- **Activity (counts only):** works, publications, comments, critiques,
  Daily Prompts submissions, and invites sent. Links go to their *public*
  pages. Private writing isn't shown (Principle 1).
- **Sessions (admins):** device, IP, created, last seen, and a revoke button
  for each (A6).
- **Reports:** reports *by* and *about* this account, linking to queue items.
- **Restrictions:** current and past, with who set them, why, and any appeal.
- **Notes:** internal staff notes (`staff_notes`), newest first. Any staff
  member can add one. Notes can't be edited or deleted, only added to.
- **Audit:** this account's `audit_events`, filtered to what the viewer's
  role allows.
- **Actions:** buttons for A5, A6, A7 and A9, each shown only if the role
  allows it.

### Acceptance criteria

- [ ] Nothing on the page reveals the contents of a private work, draft or
      notebook.
- [ ] A moderator sees the page without personal details or sessions.
- [ ] A note can be added and appears in the log; there is no route to edit
      or delete one.

### Affected area

API · Web UI

Depends on: A1, A2.

---

## A4 — `[Feature]: Create an account for someone`

**Labels:** feature · **Affected area:** API, Web UI, Email

### What problem would this solve?

Support sometimes needs to set up an account: for a writer who couldn't get
through sign-up, or for staff. Today the only way in is `/api/auth/register`.

### Proposed solution

- `POST /api/admin/users { email, displayName, moderator?: bool }`, for
  admins only.
- It creates the user with an unusable password hash and
  `terms_accepted_at = NULL`, then sends a **set your password** email using
  `email_tokens` with a new `setup` purpose. The token lasts 7 days; resets
  last 60 minutes.
- Following the link, the person sets a password, agrees to the terms, and
  is signed in. Setting the password also verifies the address, because they
  proved they can read it.
- If the address is already registered, the request gets a 409 (it's an
  admin, so there's nothing to hide), unless the address is in
  `blocked_emails`, which gets a 409 that says so.
- **Resend setup email** on the account page while it's still pending.
- Recorded as `AccountCreatedByStaff`.

### Acceptance criteria

- [ ] The new account can't sign in until the password is set.
- [ ] The setup link works once and expires after 7 days. An expired link
      says to ask for a new one.
- [ ] No API response contains the token.
- [ ] The person has to agree to the terms before using the account.

### Affected area

API · Web UI · Email

Depends on: A1, A2.

---

## A5 — `[Feature]: Edit an account: name, address, verification, second factor`

**Labels:** feature · **Affected area:** API, Web UI, Email

### What problem would this solve?

The usual support requests: *I can't get into my old email*, *I lost the
phone with my authenticator and my recovery codes*, *my display name is my
legal name and I want it gone*, and the moderation one, *this display name or
bio breaks the guidelines*.

### Proposed solution

`PATCH /api/admin/users/{id}` (admins), with each field optional:

- `displayName`, `bio`, `aboutAuthor`: also allowed for moderators, for
  guideline breaches only, and a reason is required. The old value is kept in
  the audit detail.
- `email`: the new address must not be registered or blocked. The old
  address gets a notice, the new one gets a verification link, and
  `email_verified_at` is cleared until it's used.
- `markVerified: true`: for when support has proved the address another
  way. A reason is required.
- `resetTwoFactor: true`: clears `totp_secret`, `totp_enabled_at` and
  `recovery_codes`, and signs out every session. A reason is required, and
  the account's email gets a notice. The account page warns that this is the
  one action that lets someone into an account without the second factor,
  and asks for the reason before going ahead.

Each change is a separate `AccountEdited` audit event naming the field,
except the 2FA reset, which records `TwoFactorDisabled` with
`by staff: <reason>`.

### Acceptance criteria

- [ ] Changing the email sends both mails and leaves the account unverified
      until the new link is used.
- [ ] A two-factor reset signs out every session and emails the account.
- [ ] A moderator can change a name or bio with a reason, and can't change
      anything else.
- [ ] The audit log shows the old and new values for name and bio changes.

### Affected area

API · Web UI · Email

Depends on: A1, A3.

---

## A6 — `[Feature]: Password reset and sign-out, sent by staff`

**Labels:** feature · **Affected area:** API, Web UI, Email

### What problem would this solve?

A writer who says they're locked out, or whose account looks compromised,
needs a reset and needs every device signed out, without staff ever handling
a password.

### Proposed solution

- `POST /api/admin/users/{id}/password-reset`: sends the same mail as
  `/api/auth/password/forgot`, using the same `mint` helper and the `reset`
  purpose. It doesn't count against the requester's mail limit, but it is
  limited to 3 per account per hour. With `signOut: true`, it also revokes
  every session at once.
- `POST /api/admin/users/{id}/sessions/revoke-all` and
  `DELETE /api/admin/users/{id}/sessions/{session_id}`: these reuse the
  account code that already handles *sign out everywhere else*.
- Recorded as `PasswordResetSent` and `SessionsRevokedByStaff`.
- The mail says that support sent it, and what to do if the person didn't
  ask.

### Acceptance criteria

- [ ] The reset mail works exactly like a self-service one: it lasts 60
      minutes, works once, and signs out every device when used.
- [ ] After *revoke all*, the account's next request with an old token gets
      a 401.
- [ ] No response contains the reset token.

### Affected area

API · Web UI · Email

Depends on: A1, A3.

---

## A7 — `[Feature]: Deactivate or delete an account`

**Labels:** feature · **Affected area:** API, Web UI, Privacy

### What problem would this solve?

Some writers ask support to close their account. Sometimes an account has to
be removed (spam, a court order). The self-service delete in
`privacy.rs` needs the person's password, so staff can't use it.

### Proposed solution

- **Deactivate** (`POST /api/admin/users/{id}/deactivate`): sets
  `deactivated_at` and revokes every session. The account's public things
  (profile, publications, comments) are hidden as if it didn't exist, but
  nothing is deleted. **Reactivate** reverses it. This is the reversible
  option, and it's the one to use while unsure.
- **Delete** (`DELETE /api/admin/users/{id}`, admins only): runs the same
  code path as self-service deletion (`hand_over`, former member, audit), with
  the same rule about co-written works: a 409 unless
  `deleteSharedWorks: true`. Before deleting it also:
  - offers to email the person their data export first, and does so unless
    the admin opts out with a reason (for example, the account is spam);
  - asks the admin to type the account's display name to confirm.
- **Delete content only:** removes every public discussion item by the
  account (comments, critiques, submissions, questions and answers), marking
  them `removed`, and leaves the account. This is for spam accounts that are
  restricted rather than deleted.
- Recorded as `AccountDeactivated`, `AccountReactivated` and
  `AccountDeletedByStaff`. The account-deleted line outlives the account, as
  it does today.
- This is deletion at a writer's request or for a legal reason. Deletion
  that follows a closing restriction (read-only site-wide, or a permanent
  ban) is A18, and runs on its own after 30 days.

### Acceptance criteria

- [ ] A deactivated account can't sign in and its public things disappear;
      reactivating restores all of them.
- [ ] Deleting by staff leaves the database in the same state as
      self-service deletion. One test runs both and compares.
- [ ] Co-written works aren't deleted without the explicit flag.
- [ ] The confirmation needs the display name typed.

### Affected area

API · Web UI · Privacy

Depends on: A1, A3.

---

## A8 — `[Maint]: Restrictions: the model, and enforcing it in one place`

**Labels:** maintenance · **Type:** Security hardening · **Component:** Backend (Rust)

### Why is this needed?

A restriction is only as good as its weakest route. If each handler checks
for itself, one will be missed: a new reaction endpoint, an edit route that
isn't a create route. Enforcement needs one chokepoint and a test that
proves every write route goes through it.

### Proposed approach

- The `restrictions` table (see the epic). An account can have more than one
  restriction at a time. The strictest one in force wins.
- What each restriction blocks:

  | | Durations | Sign in | Read | Export own data | Write and edit own work | Publish | Post in discussions¹ | Their public things | Afterwards |
  |---|---|---|---|---|---|---|---|---|---|
  | **Ban, site, temporary** | 3, 7, 30 days | ✗ | ✗ | ✗ | ✗ | ✗ | ✗ | visible | restored to normal |
  | **Ban, site, permanent** | permanent | ✗ | ✗ | by email² | ✗ | ✗ | ✗ | **hidden at once** | **deleted after 30 days** (A18) |
  | **Suspend, site** | 3, 7, 30 days, permanent | ✓ | ✓ | ✓ | ✓ | ✗ | ✗ | visible | restored to normal |
  | **Read-only, site** | always 30 days | ✓ | ✓ | ✓ | ✗ | ✗ | ✗ | visible | **deleted after 30 days** (A18) |
  | **Ban, discussion** | 3, 7, 30 days, permanent | ✓ | ✓, but discussions are hidden | ✓ | ✓ | ✓ | ✗ | visible | restored to normal |
  | **Suspend / read-only, discussion** | 3, 7, 30 days, permanent | ✓ | ✓ | ✓ | ✓ | ✓ | ✗ | visible | restored to normal |

  ¹ Comments, replies, votes, reactions, reader questions and answers, Daily
  Prompts submissions and critiques, reports, follows, and invites.
  ² A permanently banned person can ask for their data export during the 30
  days, from a link in the ban email or from the sign-in page. It's built by
  the existing export and sent to the account's address, and the download
  link works without signing in. Requests are limited by the existing `Mail`
  limiter, and stop when the account is purged.

  The two rows marked *deleted after 30 days* are the **closing
  restrictions**. Setting one sets `users.closing_at` and `users.purge_at`
  (A18). A permanent restriction limited to discussions never deletes
  anything.

  Within discussions, suspend and read-only behave the same. Both names are
  kept so the reason shown to the writer fits what happened.
- `AuthUser` gains `restrictions: Vec<ActiveRestriction>`, loaded in the
  same query that checks the session, so there's no extra round trip.
- **Ban, site**: the session check fails with
  `403 { code: "account_banned", until, reason }`. Login and refresh refuse
  with the same body. Setting a site ban also revokes every session.
- **Permanent ban, site**: every public read (profiles, publications,
  discussion lists, search, feeds, digests) leaves out anything by the
  account from the moment it's set, as `deactivated_at` does in A7. Nothing
  is deleted until A18 runs, so an overturned appeal brings it all back.
- **Read-only, site**: every response carries `closing: { purgeAt }`, so
  the web and iOS can show the countdown and the export button.
- A `Capability` enum (`Write`, `Publish`, `Discuss`, `Report`, `Invite`,
  `Follow`) and `restrictions::can(&user, Capability)`. Every mutating
  handler calls it first. Refusals are
  `403 { code: "account_restricted", capability, until, reason }`.
- **Enforcement test:** walk the OpenAPI document (`routes/openapi.rs`), call
  every non-GET route as a restricted user of each kind, and check it's
  refused with the right code, except an explicit allowlist (sign out,
  export, appeal, settings, own-account deletion). A new write route that
  forgets the check fails this test.
- Time is compared with `clock::now()`, so the test clock can expire a
  restriction. Nothing needs to run for a restriction to end. The
  `expire_restrictions` job only sends the *it has ended* notice.

### Risks

- **Threads lose history.** When a closed account's comments are deleted,
  replies to them stay, under a *deleted* placeholder, the rule *6.2* set
  for comments. Temporary restrictions and discussion-only restrictions
  leave past content visible unless staff remove it (A7, *delete content
  only*).

### Acceptance criteria

- [ ] The OpenAPI-driven test passes for every row of the table, and fails
      when a new write route is added without the check.
- [ ] A site ban takes effect on the account's very next request.
- [ ] A temporary restriction stops applying at `ends_at` without any job
      running.
- [ ] A permanent site ban hides every public thing by the account at once,
      and lifting it brings every one back.
- [ ] A site-wide read-only account can still request and download its
      export.
- [ ] A permanently banned account can get its export by email until it's
      purged, and not after.

---

## A9 — `[Feature]: Restricting an account: ban, suspend or read-only, site-wide or in discussions`

**Labels:** feature · **Affected area:** API, Web UI

### What problem would this solve?

Staff need to set, change and lift restrictions quickly, with a reason that
the writer and other staff can understand later.

### Proposed solution

- **Restrict** on the account page and on queue items opens a form:
  - Kind: Ban · Suspend · Read-only.
  - Scope: Whole site · Discussions only.
  - Duration: 3 days · 7 days · 30 days · permanent. The options are
    limited by role (A1). *Read-only, whole site* has no duration choice:
    it's always 30 days and then deletion.
  - For the two closing restrictions, the form says in red what will
    happen: *This account, all of its writing and all of its comments will
    be deleted on 10 November*, and the admin types the display name to
    confirm.
  - Reason: a community guidelines section (required) plus a message to the
    writer (required). The message is shown to them.
  - Internal note (optional, staff only).
  - Link to queue items (optional): the content that prompted this.
  - A permanent site ban always adds the address to the permanent ban list
    (A11), with this reason. It can't be unticked.
- `POST /api/admin/users/{id}/restrictions`,
  `PATCH /api/admin/restrictions/{id}` (change the duration to another of
  3, 7, 30 days or permanent, or change the message), and
  `POST /api/admin/restrictions/{id}/lift { reason }`. Lifting a closing
  restriction before `purge_at` cancels the deletion (A18).
- **Escalation hint:** the form shows the account's past restrictions and
  suggests the next step from a ladder in `moderation_settings`, for example
  *warning → 3-day discussion suspension → 30-day site suspension →
  permanent ban*. It's a hint only, and staff choose.
- A *warning* is a restriction with no effect (`kind = 'warning'`,
  zero length), so warnings count in the history and the ladder.

### Acceptance criteria

- [ ] Each combination of kind, scope and duration can be set, changed and
      lifted, and each is in the log.
- [ ] Only 3, 7, 30 days and permanent can be chosen, and the API refuses
      anything else.
- [ ] A moderator can't set either closing restriction.
- [ ] Setting a closing restriction needs the display name typed, and
      lifting it before the 30 days are up cancels the deletion.
- [ ] The writer's message is required and is what the writer sees (A10).

### Affected area

API · Web UI

Depends on: A1, A3, A8.

---

## A10 — `[Feature]: Telling the writer, and an appeal`

**Labels:** feature · **Affected area:** API, Web UI, Email

### What problem would this solve?

A restriction the writer doesn't understand gets read as a bug or as
malice. People need to be told what happened, why, until when, and how to
disagree.

### Proposed solution

- When a restriction is set, changed or lifted, the writer gets an email
  (through the mail queue) and an inbox item (a new kind in
  `domain/inbox.rs`). Both carry the guideline section, the staff message,
  the end date, and a link to appeal. Restrictions set as warnings say so.
- **Banner:** a signed-in, restricted writer sees a banner on every page:
  *You can't post in discussions until 14 October — why?*. A banned
  writer sees the same information on the sign-in page instead of a generic
  error.
- **Disabled controls:** the comment box, **Submit** and **Publish** are
  disabled with the same explanation. They aren't hidden, so the writer
  isn't left wondering where they went.
- **Appeal:** `POST /api/me/restrictions/{id}/appeal { body }` allows one per
  restriction, up to 2,000 characters. A banned account appeals through a
  link in the email, which carries a single-use token.
- **Appeals** in the dashboard: staff decide `upheld` or `overturned`, with
  a message to the writer. Whoever set the restriction can't decide its
  appeal. Overturning lifts the restriction and restores any content that was
  removed alongside it, if staff tick the box.
- A notice is sent when a temporary restriction ends.
- **Closing restrictions** say plainly, in the email, the inbox and the
  banner, that the account and everything in it will be deleted on a given
  date:
  - *Read-only, site-wide:* the banner has a countdown and an **Export my
    work** button that starts the existing data export. Reminders go out 7
    days and 1 day before deletion (the `closing_reminders` job).
  - *Permanent ban:* the email gives the deletion date, the appeal link,
    and a link to have their data export emailed to them (A8).
- An appeal on a closing restriction has to be decided before `purge_at`.
  An open appeal shows at the top of the Appeals page with the days left,
  and the purge waits for it (A18).

### Acceptance criteria

- [ ] Every restriction sends an email and an inbox item with the reason and
      end date.
- [ ] A banned writer can read why at sign-in, request their export by
      email, and appeal through the
      emailed link.
- [ ] Only one appeal per restriction. The person who set it can't decide it.
- [ ] Overturning lifts the restriction at once, and for a closing
      restriction cancels the deletion and brings back everything hidden.
- [ ] A site-wide read-only writer sees the deletion date and an export
      button on every page, and gets the 7-day and 1-day reminders.

### Affected area

API · Web UI · Email

Depends on: A8, A9.

---

## A11 — `[Feature]: Keeping a banned person out: the permanent ban list`

**Labels:** feature · **Affected area:** API, Web UI, Privacy

### What problem would this solve?

A permanent ban means little if the same person can sign up again with the
same address in a minute, and the account itself is deleted 30 days after
the ban (A18). Something has to remember who was banned and why, after the
account is gone.

### Proposed solution

- `permanent_bans` (see the epic) keeps the address, a canonical form of
  it, the display name and former user id at the time, the guideline
  section, the reason text, and who banned them. Like `audit_events`, it has
  no foreign keys and is kept forever. A18 deliberately leaves it in place.
- The canonical form folds case, and for Gmail it also drops dots and
  `+tags`, so `j.ane+x@gmail.com` matches `jane@gmail.com`. The address as
  typed is kept too, for the record.
- A permanent site ban writes the row in the same transaction as the
  restriction (A9). An admin can also add a row by hand, for an address
  that never had an account here.
- Registration, a staff-created account (A4), an email change (A5) and
  invites (`routes/invites.rs`) all check the list. Registration and the
  invite form give the same generic refusal they give for any address they
  won't use, so the list can't be probed. A4 and A5 are admin-only, so they
  say plainly that the address is banned and why.
- **Permanent bans** page (admins): search by address or name, see the
  reason, and remove a row with a reason. Removing one is recorded as
  `PermanentBanRemoved`. Overturning a permanent ban on appeal (A10) removes
  its row too.
- `blocked_domains` for throwaway-mail domains (`@spam.example`), managed
  on the same page.
- The privacy page says that the address and the reason for a permanent ban
  are kept after the account is deleted, and why.

### Acceptance criteria

- [ ] A banned address, including dot and `+tag` variants on Gmail, can't
      register, be invited, or be set as an account's email, before or after
      the account is deleted.
- [ ] The registration and invite responses for a banned address are the
      same as for any other refused address.
- [ ] Deleting the account in A18 leaves the `permanent_bans` row intact,
      with its address and reason.
- [ ] Removing a row, by hand or by appeal, is in the log and lets the
      address register again.

### Affected area

API · Web UI · Privacy

Depends on: A8, A9.

---

## A12 — `[Feature]: Reporting everywhere, and reports that reach staff`

**Labels:** feature · **Affected area:** API, Web UI

### What problem would this solve?

Only comments and critiques can be reported, and those reports go only to
the author. A Daily Prompts entry, a reader question, a profile, or an
author's own behaviour can't be reported at all, and nothing reaches
anybody who can act on the whole site.

### Proposed solution

- **Report** on: Daily Prompts submissions, reader questions and answers,
  profiles (name, bio, *about the author*), and publications (title, blurb).
  These use `POST /api/reports { subjectType, subjectId, reason, detail }`
  and the new `reports` table.
- New `ReportReason`s: `harassment`, `hate`, `sexual_content`,
  `self_harm`, `impersonation`, `illegal`, alongside the existing `abuse`,
  `spam`, `spoiler`, `off_topic` and `other`. `spoiler` and `off_topic`
  still go only to the author, since they're about craft. Every other reason
  also creates or bumps a `moderation_items` row (A13).
- The existing comment and critique report routes keep writing their own
  tables and the author's view is unchanged. They also bump the moderation
  item, so staff see them too.
- `self_harm` reports go to the top of the queue, and the reporter sees a
  link to support resources.
- Reporting is limited to 20 per account per day (a new `limit.rs`
  action), and reporting the same thing twice is the same report, as it is
  today.

### Acceptance criteria

- [ ] Each new surface has a **Report** control and the report reaches the
      queue.
- [ ] Comment and critique reports still reach the author exactly as before,
      and also reach the queue unless the reason is `spoiler` or
      `off_topic`.
- [ ] Reporters aren't shown to the reported account, only to staff.

### Affected area

API · Web UI

Depends on: A1.

---

## A13 — `[Feature]: The moderation queue: flagged content and flagged accounts`

**Labels:** feature · **Affected area:** API, Web UI

### What problem would this solve?

Reports, the word filter, the external platform and thresholds all produce things
that need a person to look at them. Staff need one list, ordered by what
matters most, with the actions next to it.

### Proposed solution

- `moderation_items` (see the epic). Every source creates or bumps the row
  for its subject. There's never more than one row per subject, and a
  dismissed item that's flagged again reopens.
- **Queue page** with tabs for *Content*, *Accounts*, *Held* and *Done*:
  - Ordered by severity (self-harm and illegal first), then report count,
    then age.
  - Each row shows the content (or the profile text), the author (by name,
    plus email for admins), the sources (*3 reports · word filter: hold ·
    external: remove, harassment*), the reasons, the reporters' details, and
    surrounding context (the parent comment, the publication's title).
  - Filters: source, reason, surface, and *assigned to me*.
  - Claiming: opening an item claims it for 10 minutes, so two staff don't
    work on the same one.
- **Actions on content:** *Dismiss* (with a reason), *Remove* (sets
  `moderation_state = 'removed'`, shows *removed by a moderator* in the
  thread, and keeps replies), *Hold/Release*, *Restore*, and *Restrict the
  author*, which opens A9 with the item linked. The author of removed
  content gets an inbox notice naming the guideline section.
- **Flagged accounts:** the `auto_flag_accounts` job flags an account when
  it reaches a threshold in `moderation_settings`. The defaults are 3 or more
  distinct reporters in 7 days, 2 or more removals in 30 days, or 5 or more
  comments in its first hour. Account items open the account page.
- Bulk *dismiss* for items from the same source. There's no bulk remove.
- Removed or held content is excluded from every public read: lists,
  search, feeds, the inbox digest and exports of *other* people's threads.
  It stays in its author's own data export.

### Acceptance criteria

- [ ] Ten reports on one comment are one queue item with a count of ten.
- [ ] Removing a comment hides it everywhere public, keeps its replies, and
      tells its author. Restoring it brings it back.
- [ ] Held content is visible to its author (marked *awaiting review*) and
      to staff, and to no one else.
- [ ] An account that crosses a threshold appears under *Accounts* within a
      minute.
- [ ] Two staff opening the same item: the second is told who has it.

### Affected area

API · Web UI

Depends on: A1, A12.

---

## A14 — `[Feature]: The word filter`

**Labels:** feature · **Affected area:** API, Web UI

### What problem would this solve?

Some words and phrases (slurs, known spam strings, links to known scams) are
never acceptable in a public discussion, and a person shouldn't have to see
each one to remove it.

### Proposed solution

- `word_filter_terms`, managed on `/admin/words` by admins. Each term has:
  - **Match:** `word` (whole word, the default) or `substring`. There are no
    regexes, so an admin can't write one that is slow or matches everything.
  - **Action:**
    - `block`: the post is refused with *That contains a word that isn't
      allowed here*. The word isn't named, so the list can't be probed one
      word at a time.
    - `hold`: the post is saved as `held` and a queue item is created.
    - `flag`: the post is published and a queue item is created.
    - `mask`: the post is published with the term replaced by `•••`. The
      original is kept for staff.
  - **Applies to:** any of comments, questions and answers, submissions,
    critiques, display names, bios and *about the author*, and publication
    titles and blurbs. **Never** private writing (Principle 1).
- **Normalization** in `domain/filter.rs` before matching: Unicode NFKC,
  lowercase, accents removed, common character swaps (`0→o`, `1→i/l`,
  `3→e`, `4→a`, `5→s`, `@→a`, `$→s`), repeated letters collapsed, and
  separators between single letters removed (`s.p.a.m`). Matching uses one
  Aho–Corasick automaton, rebuilt when the list changes and cached in
  `AppState`.
- **Allowlist:** terms that would otherwise match (the Scunthorpe problem),
  kept on the same page.
- **Test box:** paste text and see which terms match and what would happen.
- **Import and export** as CSV. A starter list ships as a commented file in
  `data/`, empty by default, for the admin to choose from.
- Editing an existing post is checked again. Changing the list doesn't
  re-scan old posts automatically, but a button queues a re-scan of the
  last 30 days.

### Acceptance criteria

- [ ] Each action behaves as described, on each surface it's set to apply
      to, and on edits as well as new posts.
- [ ] Common evasions (`sp4m`, `s p a m`, `ｓｐａｍ`, `spaaaam`) match.
- [ ] An allowlisted word containing a filtered term isn't caught.
- [ ] Text in manuscripts, drafts, notebooks and unsubmitted entries is
      never passed to the filter, and a test proves it.
- [ ] Checking a 10,000-character post against 5,000 terms takes under
      5 ms.

### Affected area

API · Web UI

Depends on: A13.

---

## A15 — `[Feature]: External moderation: sending public content to a moderation platform`

**Labels:** feature · **Affected area:** API, Web UI, Privacy

### What problem would this solve?

The word filter catches words, not meaning. Harassment and threats are often
made of ordinary words, and staff can't read every public post as the site
grows. A hosted moderation platform, in the style of Stream's moderation
product, classifies text with AI and its own blocklists, recommends an
action, and gives outside moderators a dashboard to work in, so we don't
have to build or staff all of that ourselves.

### Proposed solution

- A `ModerationProvider` trait in `domain/external_moderation.rs`:
  - `check(item) -> Verdict { recommended: Keep | Flag | Remove, labels, external_id }`,
    where `item` is the surface, our id for the thing, an opaque id for its
    author, and its text.
  - `report_decision(external_id, decision)`, so the platform's review queue
    is closed when our staff decide first (used by A19).
  - `StreamProvider` is the first implementation. The vendor is a setting, so
    another one is a new implementation of the trait, not a rewrite. Field
    and endpoint names follow the vendor's API docs once the account is set
    up. `NoProvider` is the default when nothing is configured.
- **When content is checked:** every new or edited public post (the surfaces
  in Principle 1, and profile and publication text) is checked when it's
  posted, with a 1-second timeout.
  - *Keep*: published as usual.
  - *Flag*: published, and a queue item is created (A13) with the labels.
  - *Remove*: what happens is a setting. The default is **hold**: the post
    is saved as `held`, the author sees *awaiting review*, and a queue item
    is created. The alternative is **block**: the post is refused, as the
    word filter's *block* does.
  - **Timeout or error:** the post is published, an `external_checks` row is
    left `pending`, and the `external_checks` job retries it with back-off.
    A platform that's down never stops anyone posting, and the backlog
    shows on the Jobs page.
- The word filter (A14) runs first, in process. If it blocks a post, nothing
  is sent to the platform.
- **What is sent:** the text, the surface, our id for the thing, and an
  opaque author id (an HMAC of the user id with a secret kept only on our
  server), so the platform can spot repeat offenders without knowing who
  they are. No email, name or other personal detail is sent, except a
  display name or bio when that is the thing being checked. Private writing
  is never sent (Principle 1).
- `/admin/external` (admins): choose the provider, set the API key and the
  webhook secret (stored encrypted, never shown again), choose which
  surfaces are checked, map *Remove* to hold or block, and run a test check
  on pasted text. Changes are recorded as `ExternalModerationChanged`.
- The privacy page names the platform and what is sent to it. Turning it on
  in production needs the terms version bumped and the data processing
  agreement signed (see *Dependencies*).

### Acceptance criteria

- [ ] Each verdict (keep, flag, remove as hold, remove as block) behaves as
      described on each surface, for new posts and edits.
- [ ] With the platform down or slow, posting still works within the
      timeout, and the backlog is checked when it comes back.
- [ ] Nothing private, and no email or name (except the name or bio being
      checked), is ever sent. A test asserts the outgoing request bodies
      against a stub provider.
- [ ] A post the word filter blocks is never sent.
- [ ] Changing the provider or its settings is in the log.

### Affected area

API · Web UI · Privacy

Depends on: A13, A14.

---

## A16 — `[Feature]: The moderation log`

**Labels:** feature · **Affected area:** API, Web UI

### What problem would this solve?

Staff need to be able to see what each other did and why, both to stay
consistent and so the people running the site can check the moderators.

### Proposed solution

- `/admin/log`, read from `audit_events` with the new actions. It's filtered
  by actor, subject account, action and date. Admins see everything.
  Moderators see their own actions and the actions on any account they're
  looking at.
- Each line shows who did what, to whom, when and why, with a link to the
  item or account. Actions that came from the external platform (A19) show
  the platform and its moderator's id as the actor, and purges (A18) show
  *scheduled deletion*.
- Export to CSV for admins.
- Nothing in the log can be edited or deleted, from the API or the UI.

### Acceptance criteria

- [ ] Every action in A4–A15, A18 and A19 writes exactly one log line with a reason where
      a reason was required.
- [ ] There's no route that changes or removes a log line.
- [ ] A moderator can't see personal details in the log.

### Affected area

API · Web UI

Depends on: A1.

---

## A17 — `[Feature]: iOS: restricted accounts, removed content and the new reports`

**Labels:** feature · **Affected area:** iOS

### What problem would this solve?

The iOS app has to handle the new refusals instead of showing a generic
error, and has to offer the new reporting.

### Proposed solution

- Handle `account_banned` at sign-in and on refresh: show the reason, the end
  date and the appeal link, and stop retrying.
- Handle `account_restricted`: show the same banner as the web, and disable
  the comment, submit and publish controls with the explanation.
- Show *removed by a moderator* placeholders, and *awaiting review* on the
  writer's own held posts.
- **Report** on submissions, profiles and publications, with the new
  reasons.
- The restriction and appeal-decision inbox kinds.
- For a site-wide read-only account: the deletion date, the countdown and
  the **Export my work** button, as on the web.

No staff tools on iOS. The dashboard stays web-only.

### Acceptance criteria

- [ ] A banned account sees the reason at sign-in, not a network error.
- [ ] A read-only account can read and export but every write control is
      disabled with an explanation.
- [ ] Each new report works from the app and appears in the web queue.

### Affected area

iOS

Depends on: A8, A10, A12, A13, A18.

---

## A18 — `[Maint]: Closing an account: the 30-day window, then deletion`

**Labels:** maintenance · **Type:** Data lifecycle · **Component:** Backend (Rust)

### Why is this needed?

Two restrictions end with the account being deleted 30 days after they're
set: *read-only, site-wide*, which leaves the writer time to export their
work, and *permanent ban, site-wide*, which hides everything at once and
leaves time for an appeal. Something has to carry out the deletion on time,
completely and only once, and not run when it shouldn't.

### Proposed approach

- Setting a closing restriction (A9) sets `users.closing_at = now` and
  `users.purge_at = now + 30 days`. Lifting it, or overturning it on appeal,
  clears both. A second closing restriction doesn't move `purge_at` later.
- A daily `purge_accounts` job in `jobs.rs`, keyed by day in `job_runs`,
  deletes every account whose `purge_at` has passed, **unless**:
  - the closing restriction has been lifted (checked again, inside the
    transaction), or
  - an appeal on it is still open. The purge waits, and the account shows
    on the dashboard landing page as *waiting on an appeal*.
- **What is deleted:** the account and everything that belongs to it: its
  works, scenes, notebooks, versions, publications, files in storage, Daily
  Prompts entries, comments, critiques, reader questions and answers,
  reactions, votes, follows and sessions. Replies to its comments stay,
  under a *deleted* placeholder. It runs through the same code as
  self-service deletion (`privacy.rs`), extended to delete the account's
  comments and critiques rather than hand them to a former member.
- **Co-written works are handed over, not deleted.** A work the closed
  account owns that has other collaborators passes to the collaborator who
  joined it first (the earliest `work_collaborators` row), who becomes its
  owner. The closed account's own contributions inside it (scenes and
  revisions it wrote) stay with the book, credited to a former member, as
  `hand_over` already does for books the departing account didn't start.
  The new owner gets an email and an inbox item saying the book is now
  theirs, without saying why the previous owner left. Works with no other
  collaborators are deleted. Each hand-over is an audit line on the work.
- **What survives:** the `permanent_bans` row (A11), the `audit_events`
  lines (`AccountPurged` with counts, nothing else), and moderation items,
  which keep their decision but lose the content.
- Before it runs, `closing_reminders` sends the 7-day and 1-day reminders
  for read-only accounts (A10).
- The external platform (A15) is asked to delete what it holds for the
  account's opaque id, if its API supports that.

### Risks

- **Deleting the wrong account.** The job only acts on accounts with a
  closing restriction still in force and `purge_at` in the past, checked in
  the same transaction as the delete. A test lifts the restriction one
  second before the job runs and checks nothing is deleted.
- **A restore after a purge.** Litestream keeps backups, so a purged
  account can still be in a backup for as long as backups are kept. The
  privacy page should say so, as it already does for self-service deletion.

### Acceptance criteria

- [ ] On day 30, a site-wide read-only account and a permanently banned
      account are deleted with all of their writing and comments, and the
      test clock proves it happens on that day, not before.
- [ ] Lifting or overturning the restriction on day 29 cancels the deletion
      and nothing is lost.
- [ ] An open appeal holds the purge until it's decided.
- [ ] After the purge, the `permanent_bans` row and the audit line remain,
      and nothing else of the account does.
- [ ] Running the job twice in a day deletes nothing extra and doesn't fail.
- [ ] A co-written work owned by a closed account survives the purge, owned
      by its earliest collaborator, who is told it's now theirs.

Depends on: A8, A9, A10, A11.

---

## A19 — `[Feature]: External moderation: decisions made on the platform come back`

**Labels:** feature · **Affected area:** API, Web UI

### What problem would this solve?

External moderators work in the platform's own dashboard. Their decisions
have to take effect here, and our staff's decisions have to close the same
items there, or the two queues drift apart and the same post is reviewed
twice.

### Proposed solution

- `POST /api/webhooks/moderation/{provider}` receives the platform's events.
  It checks the signature with the webhook secret, stores the event in
  `external_events` (a repeated `event_id` is ignored), and returns 200 at
  once. The `apply_external_events` job applies them.
- **Our queue is the source of truth.** Each event is applied through the
  same code as the matching staff action in A13 or A9, with the actor
  recorded as `external:<provider>:<their moderator id>`:
  - *Content marked safe* → dismiss the item and release it if it was held.
  - *Content removed* → remove it, and notify the author, as in A13.
  - *Content restored* → restore it.
  - *User banned or suspended for 3, 7 or 30 days* → a temporary
    restriction, with the scope chosen by a mapping on `/admin/external`
    (discussions only by default).
  - *Any permanent ban, or anything that would close an account* → never
    applied automatically. It becomes a queue item, *External moderator
    recommends a permanent ban*, for an admin to decide, because closing an
    account deletes it (A18).
  - *Anything we don't recognize* → stored, not applied, and shown on the
    Jobs page.
- **The other direction:** when our staff decide an item that came from
  the platform, `report_decision` tells the platform, so its review queue
  closes too. Failures are retried by the same job.
- The webhook route takes no `AuthUser`, is limited by the existing
  `Requests` limiter, and is the only unauthenticated route that changes
  anything, so it is listed separately in the enforcement test (A8).

### Acceptance criteria

- [ ] An event with a bad signature is refused and stored nowhere.
- [ ] Each mapped event is applied once, even if delivered twice, and shows
      in our log with the external moderator as the actor.
- [ ] A permanent ban from the platform never takes effect without an
      admin, and appears in the queue.
- [ ] Deciding an item here closes it on the platform, checked against the
      stub provider.

### Affected area

API · Web UI

Depends on: A9, A13, A15.

---

## Decisions from review

1. **External moderation** is a hosted platform in the style of Stream's
   moderation product, with its own AI checks, blocklists and review
   dashboard (A15, A19).
2. **Read-only, site-wide** stops a writer editing their own work too. It
   gives them 30 days to export, after which the account, all of its
   writing and all of its comments are deleted (A8, A18).
3. **Durations** are 3, 7 or 30 days, or permanent (A9).
4. **A permanent ban** hides everything at once. After 30 days the account
   and everything in it is deleted. The address and the reason are kept
   forever in `permanent_bans`, so that address can never sign up again
   (A11, A18).
5. **A permanently banned person can get their data export by email**
   during the 30 days, until the account is purged (A8, A10).
6. **Co-written works are handed to a collaborator** when an account is
   closed: the one who joined the book first becomes its owner (A18).
7. **Moderator powers are confirmed:** any 3-, 7- or 30-day restriction and
   permanent discussion-only ones. Both closing restrictions are for admins
   only. Moderators can decide appeals on restrictions they didn't set (A1).
