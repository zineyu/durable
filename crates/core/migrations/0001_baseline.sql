-- 单一基线：新库一次建成全部表。
-- 旧库不迁移、不导入；打开时发现旧结构以显式错误拒绝，见 `storage/sqlite/mod.rs`。

CREATE TABLE host (
    name TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    target TEXT,
    enabled INTEGER NOT NULL,
    installed_build TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE workspace (
    host TEXT NOT NULL REFERENCES host(name),
    path TEXT NOT NULL,
    registered_at INTEGER NOT NULL,
    PRIMARY KEY (host, path)
);

CREATE TABLE session (
    id TEXT PRIMARY KEY,
    workspace_host TEXT NOT NULL,
    workspace TEXT NOT NULL,
    title TEXT NOT NULL,
    title_auto INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    active_leaf INTEGER,
    FOREIGN KEY (workspace_host, workspace) REFERENCES workspace(host, path)
);

CREATE INDEX session_workspace ON session (workspace_host, workspace);

CREATE TABLE run (
    id INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    failure TEXT,
    progress TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE INDEX run_session ON run (session_id, id);

CREATE TABLE message (
    id INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    parent_id INTEGER REFERENCES message(id),
    role TEXT NOT NULL,
    interrupted INTEGER NOT NULL DEFAULT 0,
    run_id INTEGER REFERENCES run(id),
    created_at INTEGER NOT NULL
);

CREATE INDEX message_session ON message (session_id, id);

CREATE TABLE message_part (
    message_id INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY (message_id, seq)
);

CREATE TABLE queue_item (
    id INTEGER PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    anchor INTEGER REFERENCES message(id),
    created_at INTEGER NOT NULL,
    pending_steer INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX queue_item_session ON queue_item (session_id, id);

CREATE TABLE event (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT,
    session TEXT,
    created_at INTEGER NOT NULL,
    event TEXT NOT NULL
);

CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);

INSERT INTO meta (key, value) VALUES ('next_message_id', 1);
INSERT INTO meta (key, value) VALUES ('next_queue_item_id', 1);
INSERT INTO meta (key, value) VALUES ('next_run_id', 1);
