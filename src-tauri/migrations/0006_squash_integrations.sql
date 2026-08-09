-- GAG-013: durable squash-integration attempts and append-only audit events.
CREATE TABLE integration_attempts (
    id                          TEXT PRIMARY KEY NOT NULL,
    task_id                     TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    repo_root                   TEXT NOT NULL,
    source_ref                  TEXT NOT NULL,
    source_tip_sha              TEXT NOT NULL,
    source_range                TEXT NOT NULL,
    source_dirty                INTEGER NOT NULL,
    source_worktree_digest      TEXT NOT NULL,
    target_ref                  TEXT NOT NULL,
    expected_target_sha         TEXT NOT NULL,
    commit_message              TEXT NOT NULL,
    validation_commands_json    TEXT NOT NULL,
    validation_digest           TEXT NOT NULL,
    approval_digest             TEXT NOT NULL,
    state                       TEXT NOT NULL,
    temporary_worktree_id       TEXT,
    temporary_worktree_path     TEXT,
    temporary_branch            TEXT,
    conflict_summary_json       TEXT,
    validation_result_json      TEXT,
    result_commit_sha           TEXT,
    recovery_bundle_path        TEXT,
    cleanup_status              TEXT NOT NULL DEFAULT 'not_started',
    created_at                  TEXT NOT NULL,
    updated_at                  TEXT NOT NULL
);

CREATE INDEX idx_integration_attempts_task_created
    ON integration_attempts(task_id, created_at DESC);

CREATE TABLE integration_audit_events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id      TEXT NOT NULL REFERENCES integration_attempts(id) ON DELETE CASCADE,
    state           TEXT NOT NULL,
    detail_json     TEXT NOT NULL,
    occurred_at     TEXT NOT NULL
);

CREATE INDEX idx_integration_audit_attempt
    ON integration_audit_events(attempt_id, id);
