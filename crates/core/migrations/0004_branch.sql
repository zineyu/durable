ALTER TABLE queue_item ADD COLUMN anchor INTEGER REFERENCES message(id);
