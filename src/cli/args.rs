//! Turns the argument list into [`Flags`].

use crate::config::Settings;

/// What the command line asked for. The settings a config file can also
/// carry sit in `settings`, with budgets left `None` when unset so the file
/// can supply them instead. Run-behavior flags (`strict`, `quiet`, `format`,
/// `color`) are not configurable via the file, so they keep their concrete
/// defaults here.
#[derive(Debug)]
pub(super) struct Flags {
    pub settings: Settings,
    /// Checked against the known formats once the config is read, so a bad
    /// value is reported after the config file's own errors.
    pub format: String,
    pub color: String,
    pub strict: bool,
    pub quiet: bool,
    pub show_version: bool,
    pub list_rules: bool,
    pub config: Option<String>,
    pub no_config: bool,
    pub paths: Vec<String>,
}

impl Default for Flags {
    fn default() -> Self {
        Flags {
            settings: Settings::default(),
            format: "text".to_string(),
            color: "auto".to_string(),
            strict: false,
            quiet: false,
            show_version: false,
            list_rules: false,
            config: None,
            no_config: false,
            paths: Vec::new(),
        }
    }
}

pub(super) enum ParseOutcome {
    Flags(Flags),
    Help,
    Err(String),
}

pub(super) fn parse_args(args: &[String]) -> ParseOutcome {
    match parse_flags(args) {
        Ok(outcome) => outcome,
        Err(msg) => ParseOutcome::Err(msg),
    }
}

/// Splits a token's flag name from any inline `=value`, stripping one or two
/// leading dashes.
fn split_flag(token: &str) -> (&str, Option<&str>) {
    let stripped = token
        .strip_prefix("--")
        .or_else(|| token.strip_prefix('-'))
        .unwrap_or(token);
    match stripped.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (stripped, None),
    }
}

struct ArgParser<'a> {
    args: &'a [String],
    i: usize,
    negative: Vec<String>,
}

impl<'a> ArgParser<'a> {
    fn new(args: &'a [String]) -> Self {
        ArgParser {
            args,
            i: 0,
            negative: Vec::new(),
        }
    }

    fn next_value(&mut self, name: &str, inline: Option<&str>) -> Result<String, String> {
        if let Some(v) = inline {
            return Ok(v.to_string());
        }
        self.i += 1;
        self.args
            .get(self.i)
            .cloned()
            .ok_or_else(|| format!("flag needs an argument: --{name}"))
    }

    fn next_nonempty(&mut self, name: &str, inline: Option<&str>) -> Result<String, String> {
        let v = self.next_value(name, inline)?;
        if v.is_empty() {
            return Err(format!("--{name} value must not be empty"));
        }
        Ok(v)
    }

    fn budget(&mut self, name: &str, inline: Option<&str>) -> Result<i64, String> {
        let raw = self.next_value(name, inline)?;
        let n: i64 = raw
            .parse()
            .map_err(|_| format!("invalid value {raw:?} for flag --{name}: not an integer"))?;
        if n < 0 {
            self.negative.push(format!("--{name}"));
        }
        Ok(n)
    }
}

fn parse_flags(args: &[String]) -> Result<ParseOutcome, String> {
    let mut f = Flags::default();
    let mut p = ArgParser::new(args);

    while p.i < args.len() {
        let arg = &args[p.i];
        if arg == "--" {
            p.i += 1;
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            break;
        }

        let (name, inline) = split_flag(arg);
        if let Some(slot) = f.settings.budgets.get_mut(name) {
            *slot = Some(p.budget(name, inline)?);
            p.i += 1;
            continue;
        }

        match name {
            "h" | "help" => return Ok(ParseOutcome::Help),
            "version" => f.show_version = true,
            "list-rules" => f.list_rules = true,
            "strict" => f.strict = parse_bool_inline(inline),
            "quiet" => f.quiet = parse_bool_inline(inline),
            "no-config" => f.no_config = parse_bool_inline(inline),
            "format" => f.format = p.next_value(name, inline)?,
            "color" => f.color = p.next_value(name, inline)?,
            "config" => f.config = Some(p.next_nonempty(name, inline)?),
            "exclude" => f.settings.excludes.push(p.next_nonempty(name, inline)?),
            "disable" => f.settings.disabled.push(p.next_nonempty(name, inline)?),
            other => return Err(format!("flag provided but not defined: -{other}")),
        }
        p.i += 1;
    }

    f.paths = args[p.i..].to_vec();

    if !p.negative.is_empty() {
        p.negative.sort();
        return Err(format!(
            "{} must be zero or more (0 disables the check)",
            p.negative.join(", ")
        ));
    }

    Ok(ParseOutcome::Flags(f))
}

fn parse_bool_inline(inline: Option<&str>) -> bool {
    match inline {
        Some(v) => v != "false" && v != "0",
        None => true,
    }
}
