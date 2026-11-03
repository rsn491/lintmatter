//! Use cases: resolving a run's settings and linting a set of paths. They
//! reach the outside world only through [`ports`], which the adapters
//! implement.

pub mod lint_run;
pub mod ports;
pub mod settings;

pub use lint_run::{LintRun, Outcome, ReadError};
pub use ports::{SourceReader, TargetFinder};
pub use settings::{RunSettings, Settings, UnknownRules};
