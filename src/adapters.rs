//! Everything that touches the real world: the command line (the driving
//! adapter and composition root), the filesystem, the config file and the
//! reports.

pub mod cli;
pub mod config_file;
pub mod discover;
pub mod fs;
pub mod report;
