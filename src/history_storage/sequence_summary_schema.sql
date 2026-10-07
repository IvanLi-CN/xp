CREATE TABLE IF NOT EXISTS repository_history_sequence_summary_blocks (
    source_node_id TEXT NOT NULL,
    source_epoch INTEGER NOT NULL,
    stream TEXT NOT NULL,
    block_index INTEGER NOT NULL,
    first_sequence INTEGER NOT NULL,
    last_sequence INTEGER NOT NULL,
    record_count INTEGER NOT NULL,
    digest BLOB NOT NULL,
    dirty INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (source_node_id, source_epoch, stream, block_index)
);
CREATE INDEX IF NOT EXISTS repository_history_sequence_summary_blocks_dirty
    ON repository_history_sequence_summary_blocks (dirty, source_node_id, source_epoch, stream, block_index);
CREATE TRIGGER IF NOT EXISTS repository_history_sequence_summary_insert
AFTER INSERT ON repository_history_records WHEN NEW.is_tombstone = 0
BEGIN
    INSERT INTO repository_history_sequence_summary_blocks
      (source_node_id, source_epoch, stream, block_index, first_sequence, last_sequence, record_count, digest, dirty)
    VALUES (NEW.source_node_id, NEW.source_epoch, NEW.stream, NEW.sequence / 4096,
            NEW.sequence, NEW.sequence, 0, zeroblob(32), 1)
    ON CONFLICT(source_node_id, source_epoch, stream, block_index) DO UPDATE SET dirty = 1;
END;
CREATE TRIGGER IF NOT EXISTS repository_history_sequence_summary_update
AFTER UPDATE OF source_node_id, source_epoch, stream, sequence, is_tombstone, subject_node_id,
    observer_node_id, schema_id, schema_version, record_key, observed_start, observed_end,
    received_at, payload ON repository_history_records
BEGIN
    UPDATE repository_history_sequence_summary_blocks SET dirty = 1
      WHERE source_node_id = OLD.source_node_id AND source_epoch = OLD.source_epoch
        AND stream = OLD.stream AND block_index = OLD.sequence / 4096;
    INSERT INTO repository_history_sequence_summary_blocks
      (source_node_id, source_epoch, stream, block_index, first_sequence, last_sequence, record_count, digest, dirty)
    SELECT NEW.source_node_id, NEW.source_epoch, NEW.stream, NEW.sequence / 4096,
           NEW.sequence, NEW.sequence, 0, zeroblob(32), 1 WHERE NEW.is_tombstone = 0
    ON CONFLICT(source_node_id, source_epoch, stream, block_index) DO UPDATE SET dirty = 1;
END;
CREATE TRIGGER IF NOT EXISTS repository_history_sequence_summary_delete
AFTER DELETE ON repository_history_records
BEGIN
    UPDATE repository_history_sequence_summary_blocks SET dirty = 1
      WHERE source_node_id = OLD.source_node_id AND source_epoch = OLD.source_epoch
        AND stream = OLD.stream AND block_index = OLD.sequence / 4096;
END;
INSERT OR IGNORE INTO repository_history_sequence_summary_blocks
  (source_node_id, source_epoch, stream, block_index, first_sequence, last_sequence, record_count, digest, dirty)
SELECT source_node_id, source_epoch, stream, sequence / 4096,
       MIN(sequence), MAX(sequence), COUNT(*), zeroblob(32), 1
  FROM repository_history_records WHERE is_tombstone = 0
 GROUP BY source_node_id, source_epoch, stream, sequence / 4096;
