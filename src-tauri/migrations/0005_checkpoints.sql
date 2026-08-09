-- GAG-012: append-only checkpoint audit index. Git remains the content truth.
CREATE TABLE checkpoints (
    id                  TEXT PRIMARY KEY NOT NULL,
    task_id             TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    attempt_number      INTEGER NOT NULL DEFAULT 0,
    commit_sha          TEXT NOT NULL,
    tree_sha            TEXT NOT NULL,
    head_before         TEXT NOT NULL,
    selection_manifest  TEXT NOT NULL,
    selection_hash      TEXT NOT NULL,
    message             TEXT NOT NULL,
    created_at          TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_checkpoints_task_commit
    ON checkpoints(task_id, commit_sha);
CREATE INDEX idx_checkpoints_task_created
    ON checkpoints(task_id, created_at DESC);
