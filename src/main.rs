//! Entry point: hands the command line to the linter and exits with its code.
//! Everything else lives in the `lintmatter` library alongside it.

fn main() {
    use std::io::IsTerminal;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let is_terminal = std::io::stdout().is_terminal();
    let code = lintmatter::cli::run(
        &args,
        &mut std::io::stdin().lock(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
        is_terminal,
    );
    std::process::exit(code);
}
