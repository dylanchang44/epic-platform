-- Only public research is persisted. No holdings, account IDs or brokerage files.
CREATE TABLE research_snapshots (
    id INTEGER PRIMARY KEY,
    symbol TEXT NOT NULL,
    earnings_period TEXT NOT NULL,
    period_end TEXT NOT NULL,
    retrieved_at TEXT NOT NULL,
    snapshot_json TEXT NOT NULL CHECK (json_valid(snapshot_json)),
    UNIQUE (symbol, earnings_period),
    UNIQUE (symbol, period_end)
);
CREATE INDEX research_latest ON research_snapshots(symbol, period_end DESC);
