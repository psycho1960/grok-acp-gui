-- 0002_session_events.sql — GAG-006: Session event log, attempt tracking, recovery metadata.
--
-- Applied inside a transaction by the Migration runner.
-- Once merged, this file MUST NOT be modified.

-- Session event log: durable, append-only record of all session-scoped events.
-- dedup_key is UNIQUE to enforce idempotent event persistence.
CREATE TABLE session_events (
    dedup_key       TEXT NOT NULL PRIMARY KEY,
    session_id      TEXT NOT NULL,
    task_id         TEXT NOT NULL,
    sequence        INTEGER NOT NULL,
    event_type      TEXT NOT NULL,
    payload         TEXT NOT NULL,  -- JSON blob
    correlation_id  TEXT,
    persisted_at    TEXT NOT NULL,
    has_side_effects INTEGER NOT NULL DEFAULT 0, -- 0/1 boolean
    attempt_number  INTEGER NOT NULL DEFAULT 1,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE RESTRICT
);

-- Indexes for event query patterns
CREATE INDEX idx_session_events_session ON session_events(session_id, sequence);
CREATE INDEX idx_session_events_task ON session_events(task_id);
CREATE INDEX idx_session_events_attempt ON session_events(session_id, attempt_number);

-- Add attempt_number to session_bindings for recovery tracking.
-- SQLite does not support ADD COLUMN IF NOT EXISTS, but the migration runner
-- only applies this once. We use a safe ALTER TABLE.
ALTER TABLE session_bindings ADD COLUMN attempt_number INTEGER NOT NULL DEFAULT 1;

-- Add interruption metadata columns to tasks for richer recovery context.
ALTER TABLE tasks ADD COLUMN interrupted_at TEXT;
ALTER TABLE tasks ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 1;
