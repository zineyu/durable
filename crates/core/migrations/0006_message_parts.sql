CREATE TABLE message_part (
    message_id INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    body TEXT NOT NULL,
    PRIMARY KEY (message_id, seq)
);

INSERT INTO message_part (message_id, seq, kind, state, body)
SELECT id, 0, 'text', 'complete',
       json_object('kind', 'text', 'text', text, 'providerMeta', NULL)
FROM message
WHERE text <> '';

INSERT INTO message_part (message_id, seq, kind, state, body)
SELECT message.id,
       CAST(call.key AS INTEGER) + (CASE WHEN message.text <> '' THEN 1 ELSE 0 END),
       'tool_call',
       'complete',
       json_object(
           'kind', 'tool_call',
           'callId', json_extract(call.value, '$.id'),
           'name', json_extract(call.value, '$.name'),
           'arguments', json_extract(call.value, '$.arguments'),
           'providerMeta', NULL
       )
FROM message
JOIN json_each(message.tool_calls) AS call
WHERE json_array_length(message.tool_calls) > 0;

CREATE TEMP TABLE legacy_tool_call AS
SELECT message.id AS assistant_id,
       message.session_id AS session_id,
       message.run_id AS run_id,
       CAST(call.key AS INTEGER) AS seq,
       json_extract(call.value, '$.id') AS call_id,
       json_extract(call.value, '$.outcome') AS outcome,
       (SELECT value FROM meta WHERE key = 'next_message_id')
           + ROW_NUMBER() OVER (ORDER BY message.id ASC, CAST(call.key AS INTEGER) ASC) - 1 AS tool_id
FROM message
JOIN json_each(message.tool_calls) AS call;

INSERT INTO message (id, session_id, parent_id, role, text, run_id, created_at)
SELECT tool_id,
       session_id,
       COALESCE(LAG(tool_id) OVER (PARTITION BY assistant_id ORDER BY seq ASC), assistant_id),
       'tool',
       '',
       run_id,
       (SELECT created_at FROM message WHERE id = assistant_id)
FROM legacy_tool_call
ORDER BY assistant_id ASC, seq ASC;

INSERT INTO message_part (message_id, seq, kind, state, body)
SELECT tool_id,
       0,
       'tool_result',
       'complete',
       json_object(
           'kind', 'tool_result',
           'callId', call_id,
           'state', json_object('status', 'finished', 'outcome', json(outcome)),
           'providerMeta', NULL
       )
FROM legacy_tool_call
ORDER BY assistant_id ASC, seq ASC;

UPDATE meta
SET value = value + (SELECT COUNT(*) FROM legacy_tool_call)
WHERE key = 'next_message_id';

UPDATE message
SET parent_id = (
    SELECT MAX(tool_id) FROM legacy_tool_call
    WHERE legacy_tool_call.assistant_id = message.parent_id
)
WHERE role <> 'tool' AND parent_id IN (SELECT assistant_id FROM legacy_tool_call);

UPDATE session
SET active_leaf = (
    SELECT MAX(tool_id) FROM legacy_tool_call
    WHERE legacy_tool_call.assistant_id = session.active_leaf
)
WHERE active_leaf IN (SELECT assistant_id FROM legacy_tool_call);

DROP TABLE legacy_tool_call;

ALTER TABLE message DROP COLUMN text;
ALTER TABLE message DROP COLUMN tool_calls;
