-- Where each public sign-up came from, for the sign-up limits and for
-- cleaning up after a scripted run of fake accounts.
--
-- A row is written before the account, as a reservation (`user_id` NULL), and
-- the limits are counted with it in place, so two requests at once both see
-- each other. It is removed if the account is not made (a taken address, a
-- refused name), so a mistake costs nothing. `ip` is the address the request
-- came from, an IPv6 address cut to its /64. `human_verified` is true when
-- the sign-up passed the CAPTCHA or was for an address a writer invited.
--
-- The row outlives a deleted account (`user_id` goes NULL): a script that
-- deletes what it made does not get its allowance back.
CREATE TABLE signup_events (
    id             TEXT PRIMARY KEY NOT NULL,
    user_id        TEXT REFERENCES users(id) ON DELETE SET NULL,
    ip             TEXT NOT NULL,
    email_domain   TEXT NOT NULL,
    human_verified BOOLEAN NOT NULL,
    created_at     TEXT NOT NULL
);

CREATE INDEX signup_events_ip_created ON signup_events (ip, created_at);
CREATE INDEX signup_events_created ON signup_events (created_at);
CREATE INDEX signup_events_user ON signup_events (user_id);
