-- GAG-011: durable managed-worktree identity and verification metadata.
-- Existing rows remain readable and are reconciled before destructive use.

ALTER TABLE worktrees ADD COLUMN repo_identity TEXT NOT NULL DEFAULT '';
ALTER TABLE worktrees ADD COLUMN common_git_dir TEXT NOT NULL DEFAULT '';
ALTER TABLE worktrees ADD COLUMN relative_path TEXT NOT NULL DEFAULT '';
ALTER TABLE worktrees ADD COLUMN created_at TEXT NOT NULL DEFAULT '';
ALTER TABLE worktrees ADD COLUMN last_verified_at TEXT NOT NULL DEFAULT '';
ALTER TABLE worktrees ADD COLUMN recovery_bundle_id TEXT;
ALTER TABLE worktrees ADD COLUMN disk_usage_bytes INTEGER NOT NULL DEFAULT 0;
ALTER TABLE worktrees ADD COLUMN locked INTEGER NOT NULL DEFAULT 0;
ALTER TABLE worktrees ADD COLUMN merged INTEGER NOT NULL DEFAULT 0;

CREATE INDEX idx_worktrees_repo_identity ON worktrees(repo_identity);
CREATE INDEX idx_worktrees_state ON worktrees(state);
