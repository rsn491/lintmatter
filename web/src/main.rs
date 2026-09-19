mod lint;

use std::sync::LazyLock;

use axum::Router;
use axum::body::Body;
use axum::http::{Response, header};
use axum::response::Html;
use axum::routing::{get, post};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 3000;
const SCORECARD_GIF: &[u8] = include_bytes!("../../docs/img/scorecard.gif");
const PAGE_CLASS_PLACEHOLDER: &str = "{{PAGE_CLASS}}";
const PAGE_TITLE_PLACEHOLDER: &str = "{{PAGE_TITLE}}";

/// Where the page expects its token budget settings to be substituted in. The
/// form has to open on real numbers rather than blanks, and the only numbers
/// worth showing are the linter's own, so they are baked into the page here
/// instead of being fetched afterwards -- the form is never briefly wrong.
const BUDGETS_PLACEHOLDER: &str = "{{BUDGET_SETTINGS}}";

/// The page, rendered once at first request.
static INDEX_HTML: LazyLock<String> =
    LazyLock::new(|| render_page(include_str!("index.html"), "landing", "lintmatter"));
static DEMO_HTML: LazyLock<String> =
    LazyLock::new(|| render_page(include_str!("index.html"), "demo", "Try lintmatter"));

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(index))
        .route("/demo", get(demo))
        .route("/scorecard.gif", get(scorecard))
        .route("/lint", post(lint::handle_lint));

    let addr = bind_addr();
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("failed to bind {addr}: {e}"));
    println!(
        "listening on http://{}",
        listener.local_addr().expect("socket has no local addr")
    );
    axum::serve(listener, app).await.expect("server error");
}

/// Builds the listen address from `HOST` and `PORT`. Platforms like Render
/// route traffic to the port they hand the process in `PORT`, and reach it
/// only if the server listens on every interface, so a deploy there sets
/// `HOST=0.0.0.0`. The default stays loopback: `/lint` shells out to
/// `git clone` on a URL the caller supplies, which is not something a dev
/// machine should offer the rest of its network by accident.
fn bind_addr() -> String {
    let host = std::env::var("HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());
    let port = match std::env::var("PORT") {
        Ok(raw) => raw
            .parse::<u16>()
            .unwrap_or_else(|e| panic!("PORT must be a port number, got {raw:?}: {e}")),
        Err(_) => DEFAULT_PORT,
    };
    format!("{host}:{port}")
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML.as_str())
}

async fn demo() -> Html<&'static str> {
    Html(DEMO_HTML.as_str())
}

async fn scorecard() -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, "image/gif")
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(SCORECARD_GIF))
        .expect("static scorecard response is valid")
}

/// Fills the page's placeholder with the budget settings as JSON. Panics on a
/// page that lost the placeholder: serving a form with no budgets in it would
/// look like a styling bug while every run went out against nothing.
fn render_index(template: &str) -> String {
    assert!(
        template.contains(BUDGETS_PLACEHOLDER),
        "index.html no longer contains {BUDGETS_PLACEHOLDER}"
    );
    template.replace(BUDGETS_PLACEHOLDER, &lint::budget_settings_json())
}

fn render_page(template: &str, page_class: &str, page_title: &str) -> String {
    assert!(
        template.contains(PAGE_CLASS_PLACEHOLDER),
        "index.html no longer contains {PAGE_CLASS_PLACEHOLDER}"
    );
    assert!(
        template.contains(PAGE_TITLE_PLACEHOLDER),
        "index.html no longer contains {PAGE_TITLE_PLACEHOLDER}"
    );
    render_index(template)
        .replace(PAGE_CLASS_PLACEHOLDER, page_class)
        .replace(PAGE_TITLE_PLACEHOLDER, page_title)
}

#[cfg(test)]
mod tests {
    use lintmatter::config::{
        DEFAULT_MAX_AGENTS_TOKENS, DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS,
        DEFAULT_MAX_SKILL_NAME_TOKENS, DEFAULT_MAX_SKILL_TOKENS,
    };

    use super::*;

    /// The page reads the report's scores straight out of the `/lint` JSON, so
    /// a rename on the Rust side would silently blank the score widgets. Pin
    /// the field names and the band classes the page depends on.
    #[test]
    fn index_renders_the_report_scores() {
        let html = include_str!("index.html");
        for needle in [
            "summary.score",
            "file.score",
            "score-card",
            "score-badge",
            "score-green",
            "score-yellow",
            "score-orange",
            "score-red",
        ] {
            assert!(html.contains(needle), "index.html is missing {needle:?}");
        }
    }

    /// The form builds itself from the budget names the server sends, so an
    /// input that lost its id would leave that budget with no field at all.
    #[test]
    fn index_has_an_input_for_every_budget() {
        let html = include_str!("index.html");
        for field in [
            "max_agents_tokens",
            "max_skill_tokens",
            "max_skill_name_tokens",
            "max_skill_description_tokens",
        ] {
            assert!(
                html.contains(&format!("id=\"{field}\"")),
                "index.html has no input for {field:?}"
            );
        }
    }

    #[test]
    fn index_has_install_methods_and_plain_wordmark() {
        let html = include_str!("index.html");
        let curl_command = "curl --proto '=https' --tlsv1.2 -LsSf https://github.com/rsn491/lintmatter/releases/latest/download/lintmatter-installer.sh | sh";
        let brew_command = "brew install rsn491/tap/lintmatter";

        assert!(
            html.contains(curl_command),
            "index.html is missing the curl installer command"
        );
        assert!(
            html.contains(brew_command),
            "index.html is missing the Homebrew installer command"
        );
        assert!(
            html.contains(">Curl</button>") && html.contains(">Homebrew</button>"),
            "index.html is missing an installation method"
        );
        assert!(
            html.contains("class=\"shell-prompt\" aria-hidden=\"true\">$ </span>"),
            "index.html is missing the shell prompt"
        );
        assert!(
            html.contains("aria-label=\"Copy installation command\""),
            "index.html is missing the accessible copy control"
        );
        assert!(
            html.contains("src=\"/scorecard.gif\""),
            "index.html is missing the scorecard demo"
        );
        assert!(
            html.contains("class=\"tool-description\""),
            "index.html is missing the compact tool description"
        );
        assert!(
            html.find("src=\"/scorecard.gif\"") < html.find("class=\"features\""),
            "feature cards should follow the scorecard demo"
        );
        assert_eq!(&SCORECARD_GIF[..6], b"GIF89a");
        assert!(
            !html.contains(".wordmark::before"),
            "the wordmark still has a leading decoration"
        );
    }

    /// The served page must carry the linter's real defaults, not a
    /// placeholder the form would then read as `undefined`.
    #[test]
    fn render_index_substitutes_the_budget_settings() {
        let rendered = render_index(include_str!("index.html"));
        assert!(
            !rendered.contains(BUDGETS_PLACEHOLDER),
            "the placeholder survived rendering"
        );
        for needle in [
            &format!("\"max_agents_tokens\":{DEFAULT_MAX_AGENTS_TOKENS}"),
            &format!("\"max_skill_tokens\":{DEFAULT_MAX_SKILL_TOKENS}"),
            &format!("\"max_skill_name_tokens\":{DEFAULT_MAX_SKILL_NAME_TOKENS}"),
            &format!("\"max_skill_description_tokens\":{DEFAULT_MAX_SKILL_DESCRIPTION_TOKENS}"),
        ] {
            assert!(
                rendered.contains(needle),
                "rendered page is missing {needle}"
            );
        }
    }

    #[test]
    fn page_variants_select_the_right_view() {
        let template = include_str!("index.html");
        let landing = render_page(template, "landing", "lintmatter");
        let demo = render_page(template, "demo", "Try lintmatter");

        assert!(landing.contains("<body class=\"landing\">"));
        assert!(landing.contains("<title>lintmatter</title>"));
        assert!(landing.contains("A linter for agentic context"));
        assert!(landing.contains("class=\"landing-cta\" href=\"/demo\""));
        assert!(demo.contains("<body class=\"demo\">"));
        assert!(demo.contains("<title>Try lintmatter</title>"));
        assert!(demo.contains("id=\"runner-heading\">Lint a repository"));
        assert!(!landing.contains(PAGE_CLASS_PLACEHOLDER));
        assert!(!demo.contains(PAGE_TITLE_PLACEHOLDER));
    }

    #[test]
    #[should_panic(expected = "no longer contains")]
    fn render_index_rejects_a_page_without_the_placeholder() {
        render_index("<html>no budgets here</html>");
    }
}
