//! Auditable Git CLI adapter for repository and worktree lifecycle operations.
//!
//! Every invocation uses an executable plus an argument array, an explicit
//! working directory, a timeout, and bounded captured output. No shell is
//! involved at any point.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_OUTPUT_LIMIT: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitError {
    pub code: &'static str,
    pub message: &'static str,
    pub exit_code: Option<i32>,
}

impl std::fmt::Display for GitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for GitError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryInspection {
    pub canonical_root: PathBuf,
    pub common_git_dir: PathBuf,
    pub head: String,
    pub branch: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktree {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    pub prunable: bool,
}

#[derive(Debug, Clone)]
pub struct GitCli {
    executable: PathBuf,
    timeout: Duration,
    output_limit: usize,
}

impl Default for GitCli {
    fn default() -> Self {
        Self {
            executable: PathBuf::from("git"),
            timeout: DEFAULT_TIMEOUT,
            output_limit: DEFAULT_OUTPUT_LIMIT,
        }
    }
}

impl GitCli {
    pub fn inspect_repository(&self, candidate: &Path) -> Result<RepositoryInspection, GitError> {
        let canonical_candidate = canonical_directory(candidate)?;
        let root_output = self.run(&canonical_candidate, &["rev-parse", "--show-toplevel"])?;
        let canonical_root = canonical_directory(Path::new(required_line(&root_output.stdout)?))?;

        let common_output = self.run(
            &canonical_root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let common_git_dir = canonical_directory(Path::new(required_line(&common_output.stdout)?))?;
        let head = required_line(
            &self
                .run(&canonical_root, &["rev-parse", "--verify", "HEAD"])?
                .stdout,
        )?
        .to_owned();
        let branch_output = self.run_allow_status(
            &canonical_root,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
            &[0, 1],
        )?;
        let branch = (branch_output.status == 0)
            .then(|| required_line(&branch_output.stdout).map(str::to_owned))
            .transpose()?;
        let status = self.run(
            &canonical_root,
            &["status", "--porcelain=v2", "--untracked-files=all"],
        )?;

        Ok(RepositoryInspection {
            canonical_root,
            common_git_dir,
            head,
            branch,
            dirty: !status.stdout.is_empty(),
        })
    }

    pub fn list_worktrees(&self, repo_root: &Path) -> Result<Vec<GitWorktree>, GitError> {
        let repository = self.inspect_repository(repo_root)?;
        let output = self.run(
            &repository.canonical_root,
            &["worktree", "list", "--porcelain", "-z"],
        )?;
        parse_worktree_porcelain(&output.stdout)
    }

    pub fn run_checked(&self, cwd: &Path, args: &[&str]) -> Result<(), GitError> {
        self.run(cwd, args).map(|_| ())
    }

    pub fn capture(&self, cwd: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
        self.run(cwd, args).map(|output| output.stdout)
    }

    pub fn is_ancestor(
        &self,
        cwd: &Path,
        ancestor: &str,
        descendant: &str,
    ) -> Result<bool, GitError> {
        self.run_allow_status(
            cwd,
            &["merge-base", "--is-ancestor", "--", ancestor, descendant],
            &[0, 1],
        )
        .map(|output| output.status == 0)
    }

    fn run(&self, cwd: &Path, args: &[&str]) -> Result<CommandOutput, GitError> {
        self.run_allow_status(cwd, args, &[0])
    }

    fn run_allow_status(
        &self,
        cwd: &Path,
        args: &[&str],
        allowed: &[i32],
    ) -> Result<CommandOutput, GitError> {
        if !cwd.is_absolute() {
            return Err(error(
                "GIT_INVALID_CWD",
                "Git working directory must be absolute",
            ));
        }
        let mut child = Command::new(&self.executable)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| error("GIT_SPAWN_FAILED", "Git command could not be started"))?;

        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let limit = self.output_limit;
        let stdout_reader = thread::spawn(move || read_bounded(stdout, limit));
        let stderr_reader = thread::spawn(move || read_bounded(stderr, limit));
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child
                .try_wait()
                .map_err(|_| error("GIT_WAIT_FAILED", "Git command status was unavailable"))?
            {
                break status;
            }
            if started.elapsed() >= self.timeout {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(error("GIT_TIMEOUT", "Git command timed out"));
            }
            thread::sleep(Duration::from_millis(10));
        };
        let stdout = stdout_reader
            .join()
            .map_err(|_| error("GIT_OUTPUT_FAILED", "Git output reader failed"))??;
        let _stderr = stderr_reader
            .join()
            .map_err(|_| error("GIT_OUTPUT_FAILED", "Git output reader failed"))??;
        let exit_code = status.code().unwrap_or(-1);
        if !allowed.contains(&exit_code) {
            return Err(GitError {
                code: "GIT_COMMAND_FAILED",
                message: "Git command failed",
                exit_code: Some(exit_code),
            });
        }
        Ok(CommandOutput {
            status: exit_code,
            stdout,
        })
    }
}

struct CommandOutput {
    status: i32,
    stdout: Vec<u8>,
}

fn read_bounded(mut reader: impl Read, limit: usize) -> Result<Vec<u8>, GitError> {
    let mut captured = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| error("GIT_OUTPUT_FAILED", "Git output could not be read"))?;
        if count == 0 {
            return Ok(captured);
        }
        let remaining = limit.saturating_sub(captured.len());
        captured.extend_from_slice(&buffer[..count.min(remaining)]);
        if count > remaining {
            return Err(error(
                "GIT_OUTPUT_LIMIT",
                "Git output exceeded the safety limit",
            ));
        }
    }
}

fn parse_worktree_porcelain(bytes: &[u8]) -> Result<Vec<GitWorktree>, GitError> {
    let mut result = Vec::new();
    let mut current: Option<GitWorktree> = None;
    for raw in bytes.split(|byte| *byte == 0) {
        if raw.is_empty() {
            if let Some(item) = current.take() {
                result.push(item);
            }
            continue;
        }
        let field = std::str::from_utf8(raw)
            .map_err(|_| error("GIT_INVALID_OUTPUT", "Git returned invalid UTF-8 metadata"))?;
        if let Some(path) = field.strip_prefix("worktree ") {
            if let Some(item) = current.take() {
                result.push(item);
            }
            let raw_path = PathBuf::from(path);
            let path = std::fs::canonicalize(&raw_path).unwrap_or(raw_path);
            current = Some(GitWorktree {
                path,
                head: None,
                branch: None,
                detached: false,
                bare: false,
                locked: false,
                prunable: false,
            });
        } else if let Some(item) = current.as_mut() {
            if let Some(value) = field.strip_prefix("HEAD ") {
                item.head = Some(value.to_owned());
            } else if let Some(value) = field.strip_prefix("branch refs/heads/") {
                item.branch = Some(value.to_owned());
            } else if field == "detached" {
                item.detached = true;
            } else if field == "bare" {
                item.bare = true;
            } else if field == "locked" || field.starts_with("locked ") {
                item.locked = true;
            } else if field == "prunable" || field.starts_with("prunable ") {
                item.prunable = true;
            }
        }
    }
    if let Some(item) = current {
        result.push(item);
    }
    Ok(result)
}

fn required_line(bytes: &[u8]) -> Result<&str, GitError> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|value| value.lines().next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error("GIT_INVALID_OUTPUT", "Git returned incomplete metadata"))
}

fn canonical_directory(path: &Path) -> Result<PathBuf, GitError> {
    if !path.is_absolute() {
        return Err(error(
            "GIT_INVALID_PATH",
            "Repository path must be absolute",
        ));
    }
    let path = std::fs::canonicalize(path)
        .map_err(|_| error("GIT_REPOSITORY_NOT_FOUND", "Repository is not accessible"))?;
    path.is_dir()
        .then_some(path)
        .ok_or_else(|| error("GIT_INVALID_PATH", "Repository path is not a directory"))
}

fn error(code: &'static str, message: &'static str) -> GitError {
    GitError {
        code,
        message,
        exit_code: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_parser_keeps_locked_and_prunable_flags() {
        let parsed = parse_worktree_porcelain(
            b"worktree C:/repo\0HEAD abc\0branch refs/heads/main\0locked reason\0\0worktree C:/missing\0HEAD def\0detached\0prunable stale\0\0",
        )
        .unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].branch.as_deref(), Some("main"));
        assert!(parsed[0].locked);
        assert!(parsed[1].detached);
        assert!(parsed[1].prunable);
    }
}
