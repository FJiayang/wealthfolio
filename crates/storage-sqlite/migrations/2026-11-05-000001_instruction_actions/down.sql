-- Copying fails atomically if new operation types cannot fit the old schema.
CREATE TABLE trade_instructions_old (
    id TEXT NOT NULL PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE,
    request_hash TEXT NOT NULL,
    account_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    account_name TEXT NOT NULL,
    asset_snapshot TEXT NOT NULL,
    side TEXT NOT NULL,
    quantity TEXT,
    amount TEXT,
    amount_currency TEXT,
    limit_price TEXT,
    expected_completion_at TEXT,
    status TEXT NOT NULL,
    resolution_note TEXT,
    source TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    started_at TEXT,
    closed_at TEXT,
    completed_at TEXT,
    version INTEGER NOT NULL,
    goal_id TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    notes TEXT,
    CHECK (status IN ('PENDING', 'IN_PROGRESS', 'COMPLETED', 'CANCELLED')),
    CHECK (side IN ('BUY', 'SELL')),
    CHECK (source IN ('UI', 'MCP')),
    CHECK ((quantity IS NULL) != (amount IS NULL)),
    CHECK ((status = 'COMPLETED') = (completed_at IS NOT NULL))
);
INSERT INTO trade_instructions_old (id, request_id, request_hash, account_id, asset_id, account_name, asset_snapshot, side, quantity, amount, amount_currency, limit_price, expected_completion_at, status, resolution_note, source, created_at, updated_at, started_at, closed_at, completed_at, version, goal_id, notes) SELECT id, request_id, request_hash, account_id, asset_id, account_name, asset_snapshot, side, quantity, amount, amount_currency, limit_price, expected_completion_at, status, resolution_note, source, created_at, updated_at, started_at, closed_at, completed_at, version, goal_id, notes FROM trade_instructions;
DROP TABLE trade_instructions;
ALTER TABLE trade_instructions_old RENAME TO trade_instructions;
CREATE INDEX trade_instructions_status_created ON trade_instructions(status, created_at DESC, id DESC);
CREATE INDEX trade_instructions_account_created ON trade_instructions(account_id, created_at DESC, id DESC);
CREATE INDEX trade_instructions_goal_created ON trade_instructions(goal_id, created_at DESC, id DESC);
