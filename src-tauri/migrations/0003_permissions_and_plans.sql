-- 0003_permissions_and_plans.sql — GAG-009 approval evidence, Plan versions, and audit.
-- Approval tokens never cross the Renderer boundary. Sensitive raw command
-- arguments are not stored; only SHA-256 operation/plan digests and redacted summaries.

CREATE TABLE plans (
    request_id          TEXT NOT NULL PRIMARY KEY,
    task_id             TEXT NOT NULL,
    session_id          TEXT NOT NULL,
    correlation_id      TEXT NOT NULL,
    workspace           TEXT NOT NULL,
    version             INTEGER NOT NULL,
    plan_hash           TEXT NOT NULL,
    state               TEXT NOT NULL,
    summary_redacted    TEXT NOT NULL,
    options_json        TEXT NOT NULL,
    decided_option_id   TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    UNIQUE(task_id, version),
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE RESTRICT
);

CREATE INDEX idx_plans_session_version ON plans(session_id, version);
CREATE INDEX idx_plans_task_state ON plans(task_id, state);

CREATE TABLE permission_decisions (
    request_id          TEXT NOT NULL PRIMARY KEY,
    task_id             TEXT NOT NULL,
    session_id          TEXT NOT NULL,
    correlation_id      TEXT NOT NULL,
    workspace           TEXT NOT NULL,
    plan_version        INTEGER,
    operation_digest    TEXT NOT NULL,
    category            TEXT NOT NULL,
    summary_redacted    TEXT NOT NULL,
    options_json        TEXT NOT NULL,
    state               TEXT NOT NULL,
    scope_json          TEXT,
    expires_at_epoch    INTEGER NOT NULL,
    decided_option_id   TEXT,
    consumed_at         TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE RESTRICT
);

CREATE INDEX idx_permission_context
    ON permission_decisions(session_id, workspace, operation_digest, plan_version, state);
CREATE INDEX idx_permission_task_state ON permission_decisions(task_id, state);

CREATE TABLE approval_audit_events (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id             TEXT NOT NULL,
    session_id          TEXT NOT NULL,
    request_id          TEXT NOT NULL,
    event_kind          TEXT NOT NULL,
    decision            TEXT NOT NULL,
    operation_digest    TEXT,
    plan_version        INTEGER,
    correlation_id      TEXT NOT NULL,
    occurred_at         TEXT NOT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE RESTRICT
);

CREATE INDEX idx_approval_audit_task ON approval_audit_events(task_id, occurred_at);
