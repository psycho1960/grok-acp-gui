-- 0001_initial.sql — Baseline schema for Grok ACP GUI.
--
-- Applied inside a transaction by the Migration runner.
-- Once merged, this file MUST NOT be modified.
-- All subsequent schema changes must go into higher-numbered migrations.

-- Schema version tracking
CREATE TABLE IF NOT EXISTS _schema_version (
    version     INTEGER NOT NULL PRIMARY KEY,
    applied_at  TEXT    NOT NULL,
    checksum    TEXT    NOT NULL
);

-- Core entity tables (GAG-004: seven tables + indexes per Tech Design §9)

-- 1. Projects
CREATE TABLE projects (
    id              TEXT NOT NULL PRIMARY KEY,
    path            TEXT NOT NULL UNIQUE,
    display_path    TEXT NOT NULL,
    repo_root       TEXT,
    trusted_at      TEXT,
    last_opened_at  TEXT NOT NULL
);

-- 2. Tasks
CREATE TABLE tasks (
    id                  TEXT NOT NULL PRIMARY KEY,
    project_id          TEXT NOT NULL,
    title               TEXT NOT NULL,
    status              TEXT NOT NULL DEFAULT 'preparing',
    workspace_kind      TEXT NOT NULL DEFAULT 'worktree',
    mode                TEXT,
    model               TEXT,
    reasoning           TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    interrupt_reason    TEXT,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE RESTRICT
);

-- 3. Session Bindings
CREATE TABLE session_bindings (
    task_id     TEXT NOT NULL,
    session_id  TEXT NOT NULL UNIQUE,
    cwd         TEXT,
    last_seq    INTEGER NOT NULL DEFAULT 0,
    state       TEXT NOT NULL DEFAULT 'idle',
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE RESTRICT
);

-- 4. Worktrees
CREATE TABLE worktrees (
    id              TEXT NOT NULL PRIMARY KEY,
    task_id         TEXT NOT NULL,
    repo_root       TEXT NOT NULL,
    path            TEXT NOT NULL UNIQUE,
    display_path    TEXT NOT NULL,
    branch          TEXT NOT NULL,
    base_branch     TEXT NOT NULL,
    base_commit     TEXT NOT NULL,
    ownership       TEXT NOT NULL DEFAULT 'managed',
    state           TEXT NOT NULL DEFAULT 'ready',
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE RESTRICT
);

-- 5. Attachments
CREATE TABLE attachments (
    id          TEXT NOT NULL PRIMARY KEY,
    task_id     TEXT NOT NULL,
    sha256      TEXT NOT NULL UNIQUE,
    mime        TEXT NOT NULL,
    bytes       INTEGER NOT NULL DEFAULT 0,
    cache_path  TEXT NOT NULL,
    source_name TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE RESTRICT
);

-- 6. Recovery Items
CREATE TABLE recovery_items (
    id              TEXT NOT NULL PRIMARY KEY,
    task_id         TEXT NOT NULL,
    directory       TEXT NOT NULL,
    manifest_path   TEXT NOT NULL,
    expires_at      TEXT NOT NULL,
    state           TEXT NOT NULL DEFAULT 'available',
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE RESTRICT
);

-- 7. Settings
CREATE TABLE settings (
    key         TEXT NOT NULL PRIMARY KEY,
    json_value  TEXT NOT NULL
);

-- Indexes for common query patterns
CREATE INDEX idx_tasks_project_id     ON tasks(project_id);
CREATE INDEX idx_tasks_status         ON tasks(status);
CREATE INDEX idx_session_bindings_task ON session_bindings(task_id);
CREATE INDEX idx_worktrees_task_id    ON worktrees(task_id);
CREATE INDEX idx_attachments_task_id  ON attachments(task_id);
CREATE INDEX idx_recovery_items_task  ON recovery_items(task_id);
