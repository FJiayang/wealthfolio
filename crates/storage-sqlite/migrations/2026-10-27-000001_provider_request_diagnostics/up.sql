ALTER TABLE provider_request_events ADD COLUMN context TEXT;
ALTER TABLE provider_request_events ADD COLUMN result TEXT;
CREATE INDEX provider_request_events_provider_time
    ON provider_request_events(provider_id, occurred_at DESC, id DESC);
