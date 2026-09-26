-- One migration ledger for the existing Review execution database. Jobs owns
-- these two tables; only Review's repository reads/writes review documents.
CREATE TABLE jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK(kind = 'portfolio_review'),
    status TEXT NOT NULL CHECK(status IN ('queued','running','succeeded','failed','interrupted')),
    created_at TEXT NOT NULL,
    queued_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count >= 0),
    step TEXT NOT NULL,
    error TEXT,
    result_id INTEGER,
    idempotency_key TEXT UNIQUE,
    CHECK((status = 'succeeded') = (result_id IS NOT NULL))
);
CREATE INDEX jobs_queue ON jobs(status, queued_at, id);
CREATE TABLE job_failures (
    job_id INTEGER NOT NULL REFERENCES jobs(id),
    attempt INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('failed','interrupted')),
    step TEXT NOT NULL,
    started_at TEXT NOT NULL,
    completed_at TEXT NOT NULL,
    error TEXT NOT NULL,
    PRIMARY KEY(job_id, attempt)
);
ALTER TABLE reviews ADD COLUMN origin_job_id INTEGER;
CREATE UNIQUE INDEX reviews_origin_job ON reviews(origin_job_id);
