use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;
fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let terminal = io::stdout().is_terminal();
    match terms::execute(
        &args,
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        terminal,
    ) {
        Ok(status) => ExitCode::from(status),
        Err(error) => {
            let safe: String = error.chars().flat_map(char::escape_default).collect();
            let _ = writeln!(io::stderr().lock(), "terms: {safe}");
            ExitCode::from(2)
        }
    }
}
