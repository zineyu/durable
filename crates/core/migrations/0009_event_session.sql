ALTER TABLE event ADD COLUMN session TEXT;

UPDATE event
SET session = json_extract(event, '$.session'),
    event = json_set(
        json_remove(event, '$.session'),
        '$.sessionId',
        json_extract(event, '$.session.id')
    )
WHERE json_extract(event, '$.session') IS NOT NULL;
