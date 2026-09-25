CREATE TABLE reviews (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TEXT NOT NULL,
    summary_json TEXT NOT NULL CHECK(json_valid(summary_json)),
    document_json TEXT NOT NULL CHECK(json_valid(document_json))
);
-- A complete document contains portfolio metadata, positions, copied research,
-- snapshot IDs, coverage, ages and calculation version. No raw CSV is stored.
CREATE TRIGGER reviews_no_update BEFORE UPDATE ON reviews
BEGIN SELECT RAISE(ABORT, 'Saved reviews are immutable'); END;
CREATE TRIGGER reviews_no_delete BEFORE DELETE ON reviews
BEGIN SELECT RAISE(ABORT, 'Saved reviews are immutable'); END;
