-- GAG-013 recovery hardening. Keep 0006 immutable for installed databases.
ALTER TABLE integration_attempts
    ADD COLUMN repo_identity TEXT NOT NULL DEFAULT '';

-- A durable repository lease for attempts that have a stable common-git-dir
-- identity. Legacy v6 rows may contain duplicates, so they are deliberately
-- excluded from this index and handled fail-closed by the repository query.
CREATE UNIQUE INDEX idx_integration_attempts_active_repo_identity
    ON integration_attempts(repo_identity)
    WHERE cleanup_status <> 'completed' AND repo_identity <> '';
