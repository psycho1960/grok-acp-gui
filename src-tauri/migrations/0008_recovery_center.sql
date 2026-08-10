-- GAG-014: append-only Recovery Center history.
-- Issue state changes are new revisions; plans, bundles and step results are immutable.

CREATE TABLE recovery_scans (
    id              TEXT NOT NULL PRIMARY KEY,
    trigger_kind    TEXT NOT NULL,
    started_at      TEXT NOT NULL,
    completed_at    TEXT NOT NULL,
    issue_count     INTEGER NOT NULL
);

CREATE TABLE recovery_issues (
    issue_id            TEXT NOT NULL,
    revision            INTEGER NOT NULL,
    scan_id             TEXT,
    stable_key          TEXT NOT NULL,
    kind                TEXT NOT NULL,
    severity            TEXT NOT NULL,
    status              TEXT NOT NULL,
    task_id             TEXT,
    resource_id         TEXT NOT NULL,
    canonical_path      TEXT,
    evidence_json       TEXT NOT NULL,
    impact              TEXT NOT NULL,
    recommended_action  TEXT NOT NULL,
    safe_actions_json   TEXT NOT NULL,
    detected_at         TEXT NOT NULL,
    PRIMARY KEY (issue_id, revision),
    FOREIGN KEY (scan_id) REFERENCES recovery_scans(id) ON DELETE RESTRICT
);

CREATE INDEX idx_recovery_issues_stable_revision
    ON recovery_issues(stable_key, revision DESC);
CREATE INDEX idx_recovery_issues_scan ON recovery_issues(scan_id);

CREATE TABLE recovery_action_plans (
    id                  TEXT NOT NULL PRIMARY KEY,
    issue_id            TEXT NOT NULL,
    issue_revision      INTEGER NOT NULL,
    action_kind         TEXT NOT NULL,
    resource_identity   TEXT NOT NULL,
    canonical_path      TEXT,
    expected_state_json TEXT NOT NULL,
    steps_json          TEXT NOT NULL,
    internal_context_json TEXT NOT NULL,
    destructive_level   TEXT NOT NULL,
    approval_digest     TEXT NOT NULL,
    expires_at_epoch    INTEGER NOT NULL,
    created_at          TEXT NOT NULL,
    FOREIGN KEY (issue_id, issue_revision)
        REFERENCES recovery_issues(issue_id, revision) ON DELETE RESTRICT
);

CREATE TABLE recovery_bundles (
    id                  TEXT NOT NULL PRIMARY KEY,
    issue_id            TEXT NOT NULL,
    issue_revision      INTEGER NOT NULL,
    recovery_item_id    TEXT NOT NULL,
    manifest_path       TEXT NOT NULL,
    manifest_sha256     TEXT NOT NULL,
    verified            INTEGER NOT NULL,
    created_at          TEXT NOT NULL,
    FOREIGN KEY (issue_id, issue_revision)
        REFERENCES recovery_issues(issue_id, revision) ON DELETE RESTRICT,
    FOREIGN KEY (recovery_item_id) REFERENCES recovery_items(id) ON DELETE RESTRICT
);

CREATE TABLE recovery_step_results (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    plan_id         TEXT NOT NULL,
    step_index      INTEGER NOT NULL,
    step_name       TEXT NOT NULL,
    status          TEXT NOT NULL,
    detail_redacted TEXT NOT NULL,
    occurred_at     TEXT NOT NULL,
    FOREIGN KEY (plan_id) REFERENCES recovery_action_plans(id) ON DELETE RESTRICT
);

CREATE INDEX idx_recovery_steps_plan ON recovery_step_results(plan_id, id);
