-- Quote sync state describes asset refreshes, not downstream API attempts.
-- Durable UTC events are necessary for restart history and for regrouping old
-- requests after the statistics timezone changes. No local-day buckets or
-- request payloads are stored. This profile-local table is intentionally not synced.
CREATE TABLE provider_request_events (
    id INTEGER PRIMARY KEY,
    provider_id TEXT NOT NULL,
    occurred_at TEXT NOT NULL
);
CREATE INDEX provider_request_events_occurred_at_provider
    ON provider_request_events(occurred_at, provider_id);
