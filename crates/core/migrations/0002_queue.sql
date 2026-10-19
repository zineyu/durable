ALTER TABLE session ADD COLUMN title_auto INTEGER NOT NULL DEFAULT 1;

CREATE TABLE queue_item (
    id INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX queue_item_session ON queue_item (session_id, id);

INSERT INTO meta (key, value) VALUES ('next_queue_item_id', 1);
