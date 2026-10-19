CREATE TABLE workspace (
    path TEXT PRIMARY KEY,
    registered_at INTEGER NOT NULL
);

CREATE TABLE session (
    id TEXT PRIMARY KEY,
    workspace TEXT NOT NULL REFERENCES workspace(path),
    title TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    active_leaf INTEGER
);

CREATE TABLE message (
    id INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    parent_id INTEGER REFERENCES message(id),
    role TEXT NOT NULL,
    text TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE INDEX message_session ON message (session_id, id);

CREATE TABLE event (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT,
    created_at INTEGER NOT NULL,
    event TEXT NOT NULL
);

CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);

INSERT INTO meta (key, value) VALUES ('next_message_id', 1);
