-- Rebuild the closed job schema without changing existing identities or failures.
-- Both parent and child are copied before dropping the old child, then parent.
CREATE TABLE jobs_v3 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK(kind IN ('portfolio_review','watchlist_briefing')),
    input_json TEXT NOT NULL CHECK(json_valid(input_json)),
    status TEXT NOT NULL CHECK(status IN ('queued','running','succeeded','failed','interrupted')),
    created_at TEXT NOT NULL, queued_at TEXT NOT NULL,
    started_at TEXT, completed_at TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count >= 0),
    step TEXT NOT NULL, error TEXT, result_id INTEGER,
    idempotency_key TEXT,
    UNIQUE(kind,input_json,idempotency_key),
    CHECK((status='succeeded')=(result_id IS NOT NULL))
);
INSERT INTO jobs_v3 SELECT id,kind,'{"type":"portfolio_review"}',status,created_at,queued_at,started_at,completed_at,attempt_count,step,error,result_id,idempotency_key FROM jobs;
CREATE TABLE job_failures_v3 (
    job_id INTEGER NOT NULL REFERENCES jobs_v3(id),
    attempt INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('failed','interrupted')),
    step TEXT NOT NULL, started_at TEXT NOT NULL, completed_at TEXT NOT NULL, error TEXT NOT NULL,
    PRIMARY KEY(job_id,attempt)
);
INSERT INTO job_failures_v3 SELECT * FROM job_failures;
DROP TABLE job_failures;
DROP TABLE jobs;
ALTER TABLE jobs_v3 RENAME TO jobs;
ALTER TABLE job_failures_v3 RENAME TO job_failures;
CREATE INDEX jobs_queue ON jobs(status,queued_at,id);

CREATE TABLE watchlist_briefings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL,
    symbols_json TEXT NOT NULL CHECK(json_valid(symbols_json)),
    document_json TEXT NOT NULL CHECK(json_valid(document_json)),
    origin_job_id INTEGER NOT NULL UNIQUE
);
CREATE TRIGGER briefings_no_update BEFORE UPDATE ON watchlist_briefings BEGIN SELECT RAISE(ABORT,'saved briefings are immutable'); END;
CREATE TRIGGER briefings_no_delete BEFORE DELETE ON watchlist_briefings BEGIN SELECT RAISE(ABORT,'saved briefings are immutable'); END;
