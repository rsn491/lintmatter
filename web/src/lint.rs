//! Handles `POST /lint`: validates a GitHub URL, clones it into a temp
//! directory, runs the `lintmatter` binary against the clone, and forwards its
//! JSON report to the caller.

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use lintmatter::config::{
    DEFAULT_MAX_AGENTS_TOKENS, DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS, DEFAULT_MAX_SKILL_NAME_TOKENS,
    DEFAULT_MAX_SKILL_TOKENS,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const CLONE_TIMEOUT: Duration = Duration::from_secs(60);
const LINT_TIMEOUT: Duration = Duration::from_secs(30);

/// Caps a budget well above any real file so a typo in the form cannot turn
/// into an unbounded number on the command line. Zero stays meaningful: it is
/// how lintmatter spells "skip this check".
const MAX_BUDGET: i64 = 1_000_000;

const GENERIC_ERROR: &str =
    "Something went wrong while linting that repository. Please try again later.";
const INVALID_URL_ERROR: &str = "That doesn't look like a valid GitHub repository URL. Expected format: https://github.com/<owner>/<repo>";
const INVALID_BUDGET_ERROR: &str =
    "Token budgets must be whole numbers from 0 to 1000000 (0 turns that check off).";

/// The token budgets a run enforces, named after the flags they become.
///
/// Every field is optional, and an omitted one is not passed to `lintmatter` at
/// all, so the clone's own `.lintmatter.yaml` still decides it. A budget that is
/// present wins over that file, the same way the flag does on the command
/// line -- which is what makes the form authoritative when it sends one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Budgets {
    max_agents_tokens: Option<i64>,
    max_skill_tokens: Option<i64>,
    max_skill_name_tokens: Option<i64>,
    max_skill_description_tokens: Option<i64>,
}

impl Budgets {
    /// What a bare `lintmatter` run enforces. The page starts the form from
    /// these, so the numbers on screen are the linter's own rather than a
    /// second opinion that could drift from it.
    pub fn defaults() -> Self {
        Budgets {
            max_agents_tokens: Some(DEFAULT_MAX_AGENTS_TOKENS),
            max_skill_tokens: Some(DEFAULT_MAX_SKILL_TOKENS),
            max_skill_name_tokens: Some(DEFAULT_MAX_SKILL_NAME_TOKENS),
            max_skill_description_tokens: Some(DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS),
        }
    }

    /// Each budget paired with the flag that carries it, in the order the
    /// usage text lists them. Backs both validation and the argument list, so
    /// a budget can never be checked but not passed, or the reverse.
    fn entries(&self) -> [(&'static str, Option<i64>); 4] {
        [
            ("--max-agents-tokens", self.max_agents_tokens),
            ("--max-skill-tokens", self.max_skill_tokens),
            ("--max-skill-name-tokens", self.max_skill_name_tokens),
            (
                "--max-skill-description-tokens",
                self.max_skill_description_tokens,
            ),
        ]
    }

    fn validate(&self) -> Result<(), String> {
        for (flag, value) in self.entries() {
            let Some(n) = value else { continue };
            if !(0..=MAX_BUDGET).contains(&n) {
                return Err(format!("{flag} is {n}, outside 0..={MAX_BUDGET}"));
            }
        }
        Ok(())
    }

    fn flags(&self) -> Vec<String> {
        let mut args = Vec::new();
        for (flag, value) in self.entries() {
            if let Some(n) = value {
                args.push(flag.to_string());
                args.push(n.to_string());
            }
        }
        args
    }
}

/// What the page needs to build the budget form: the numbers to open on and
/// the ceiling to enforce on them. Serving it from here keeps the form and
/// the server agreeing on both without either restating the other's numbers.
#[derive(Serialize)]
struct BudgetSettings {
    defaults: Budgets,
    max: i64,
}

/// The budget form's settings as JSON, for embedding in the page.
pub fn budget_settings_json() -> String {
    let settings = BudgetSettings {
        defaults: Budgets::defaults(),
        max: MAX_BUDGET,
    };
    serde_json::to_string(&settings).expect("budget settings are plain integers")
}

#[derive(Deserialize)]
pub struct LintRequest {
    url: String,
    /// Absent for a caller that only sends a URL, which then gets whatever
    /// the repository and lintmatter's defaults work out between them.
    #[serde(default)]
    budgets: Budgets,
}

/// The detail behind an error never reaches the client: it may contain
/// internal paths, subprocess stderr, or other implementation details.
/// It's only ever logged server-side; the client gets `public_message`.
pub struct ApiError {
    status: StatusCode,
    log_detail: String,
    public_message: &'static str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.public_message }))).into_response()
    }
}

pub async fn handle_lint(Json(req): Json<LintRequest>) -> Result<Json<Value>, ApiError> {
    let url = req.url.clone();
    // The budgets are logged too: a report only makes sense next to the
    // limits it was measured against.
    let budgets = req.budgets.flags().join(" ");
    println!("POST /lint url={url:?} budgets={budgets:?}");

    let result = handle_lint_inner(req).await;

    match &result {
        Ok(_) => println!("POST /lint url={url:?} status={}", StatusCode::OK.as_u16()),
        Err(err) => println!(
            "POST /lint url={url:?} status={} error={:?}",
            err.status.as_u16(),
            err.log_detail
        ),
    }

    result
}

async fn handle_lint_inner(req: LintRequest) -> Result<Json<Value>, ApiError> {
    let clone_url =
        validate_github_url(&req.url).map_err(|detail| bad_request(detail, INVALID_URL_ERROR))?;
    // Checked before the clone: there is no reason to spend a network fetch
    // on a request that cannot be linted.
    req.budgets
        .validate()
        .map_err(|detail| bad_request(detail, INVALID_BUDGET_ERROR))?;

    let dir = tempfile::TempDir::new()
        .map_err(|e| server_error(format!("failed to create temp dir: {e}")))?;

    clone_repo(&clone_url, dir.path()).await?;
    let mut report = run_lintmatter(dir.path(), &req.budgets).await?;

    if let Some(obj) = report.as_object_mut() {
        obj.insert("url".to_string(), Value::String(clone_url));
        // Echoed back so a saved report says which limits produced it.
        obj.insert(
            "budgets".to_string(),
            serde_json::to_value(&req.budgets).unwrap_or(Value::Null),
        );
    }
    Ok(Json(report))
}

/// Accepts only `https://github.com/<owner>/<repo>[.git][/]`, rejecting
/// anything else. This is a security boundary as much as a UX one: git
/// supports transports like `ext::` that can execute arbitrary commands, so
/// the server must never hand user input to `git clone` unvalidated. Returns
/// a canonicalized clone URL rebuilt from the validated parts (rather than
/// the raw input) so stray query strings, fragments, or credentials in the
/// original can't slip through.
fn validate_github_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    let without_slash = trimmed.strip_suffix('/').unwrap_or(trimmed);
    let rest = without_slash
        .strip_prefix("https://github.com/")
        .ok_or("URL must start with https://github.com/")?;

    let parts: Vec<&str> = rest.split('/').collect();
    let [owner, repo] = parts[..] else {
        return Err("URL must look like https://github.com/<owner>/<repo>".to_string());
    };
    let repo = repo.strip_suffix(".git").unwrap_or(repo);

    let valid_segment = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    };
    if !valid_segment(owner) || !valid_segment(repo) {
        return Err("owner/repo contains invalid characters".to_string());
    }

    Ok(format!("https://github.com/{owner}/{repo}"))
}

async fn clone_repo(url: &str, dest: &Path) -> Result<(), ApiError> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(["clone", "--depth", "1", "--", url, &dest.to_string_lossy()])
        // Blocks git's non-http(s) transports (e.g. `ext::`, which can run
        // arbitrary commands) even if validation above were ever bypassed.
        .env("GIT_ALLOW_PROTOCOL", "https")
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true);

    let output = tokio::time::timeout(CLONE_TIMEOUT, cmd.output())
        .await
        .map_err(|_| {
            bad_request(
                "cloning the repository timed out",
                "Cloning that repository timed out. Please try again.",
            )
        })?
        .map_err(|e| server_error(format!("failed to run git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(bad_request(
            format!("git clone failed: {}", stderr.trim()),
            "Could not clone that repository. Make sure the URL points to a public GitHub repository.",
        ));
    }
    Ok(())
}

async fn run_lintmatter(repo_dir: &Path, budgets: &Budgets) -> Result<Value, ApiError> {
    let mut cmd = tokio::process::Command::new(lintmatter_binary_path());
    cmd.args(["--format", "json"])
        .args(budgets.flags())
        .current_dir(repo_dir)
        .kill_on_drop(true);

    let output = tokio::time::timeout(LINT_TIMEOUT, cmd.output())
        .await
        .map_err(|_| server_error("linting the repository timed out".to_string()))?
        .map_err(|e| server_error(format!("failed to run lintmatter: {e}")))?;

    // lintmatter exits 0 (clean) or 1 (findings reported) on a successful run;
    // anything else (2 = usage error, or killed by signal) means the run
    // itself failed and stdout won't be valid JSON.
    match output.status.code() {
        Some(0) | Some(1) => serde_json::from_slice(&output.stdout)
            .map_err(|e| server_error(format!("failed to parse lintmatter output: {e}"))),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(server_error(format!(
                "lintmatter failed: {}",
                stderr.trim()
            )))
        }
    }
}

/// Locates the `lintmatter` binary as a sibling of this binary, since both are
/// built into the same workspace `target/` directory; falls back to `PATH`.
fn lintmatter_binary_path() -> PathBuf {
    let name = if cfg!(windows) {
        "lintmatter.exe"
    } else {
        "lintmatter"
    };
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from(name)
}

fn bad_request(log_detail: impl Into<String>, public_message: &'static str) -> ApiError {
    ApiError {
        status: StatusCode::BAD_REQUEST,
        log_detail: log_detail.into(),
        public_message,
    }
}

fn server_error(log_detail: impl Into<String>) -> ApiError {
    ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        log_detail: log_detail.into(),
        public_message: GENERIC_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(body: &str) -> LintRequest {
        serde_json::from_str(body).unwrap_or_else(|e| panic!("{body}: {e}"))
    }

    #[test]
    fn budgets_become_flags_in_flag_order() {
        let req = request(
            r#"{"url":"https://github.com/o/r","budgets":{
                "max_agents_tokens":1200,
                "max_skill_tokens":0,
                "max_skill_name_tokens":8,
                "max_skill_description_tokens":64}}"#,
        );
        assert_eq!(
            req.budgets.flags(),
            [
                "--max-agents-tokens",
                "1200",
                "--max-skill-tokens",
                "0",
                "--max-skill-name-tokens",
                "8",
                "--max-skill-description-tokens",
                "64",
            ]
        );
    }

    /// A budget the caller left out must not reach the command line at all:
    /// that is what leaves the clone's own `.lintmatter.yaml` in charge of it.
    #[test]
    fn omitted_budgets_pass_no_flags() {
        assert_eq!(
            request(r#"{"url":"https://github.com/o/r"}"#)
                .budgets
                .flags(),
            Vec::<String>::new()
        );

        let partial =
            request(r#"{"url":"https://github.com/o/r","budgets":{"max_skill_tokens":10}}"#);
        assert_eq!(partial.budgets.flags(), ["--max-skill-tokens", "10"]);
    }

    #[test]
    fn validate_rejects_budgets_outside_the_allowed_range() {
        assert!(Budgets::defaults().validate().is_ok());
        assert!(
            Budgets {
                max_skill_tokens: Some(0),
                ..Budgets::default()
            }
            .validate()
            .is_ok(),
            "0 disables a check and must stay accepted"
        );

        for bad in [-1, MAX_BUDGET + 1] {
            let err = Budgets {
                max_agents_tokens: Some(bad),
                ..Budgets::default()
            }
            .validate()
            .unwrap_err();
            assert!(err.contains("--max-agents-tokens"), "{err}");
        }
    }

    /// The message spells the ceiling out, so it has to be the ceiling the
    /// server actually enforces.
    #[test]
    fn the_budget_error_names_the_real_ceiling() {
        assert!(
            INVALID_BUDGET_ERROR.contains(&MAX_BUDGET.to_string()),
            "{INVALID_BUDGET_ERROR:?} does not name {MAX_BUDGET}"
        );
    }

    /// The form starts from these, so they are the linter's defaults or the
    /// page is quietly lying about what it enforces.
    #[test]
    fn defaults_are_the_linters_own() {
        let json = serde_json::to_value(Budgets::defaults()).unwrap();
        assert_eq!(json["max_agents_tokens"], DEFAULT_MAX_AGENTS_TOKENS);
        assert_eq!(json["max_skill_tokens"], DEFAULT_MAX_SKILL_TOKENS);
        assert_eq!(json["max_skill_name_tokens"], DEFAULT_MAX_SKILL_NAME_TOKENS);
        assert_eq!(
            json["max_skill_description_tokens"],
            DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS
        );
    }
}
