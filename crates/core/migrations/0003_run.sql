CREATE TABLE run (
    id INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    failure TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX run_session ON run (session_id, id);

ALTER TABLE message ADD COLUMN run_id INTEGER REFERENCES run(id);
ALTER TABLE message ADD COLUMN tool_calls TEXT NOT NULL DEFAULT '[]';

INSERT INTO meta (key, value) VALUES ('next_run_id', 1);
