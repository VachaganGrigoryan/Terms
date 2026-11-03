//! Streaming command orchestration for Terms.
use std::ffi::OsString;
use std::io::{Read, Write};
mod git;
mod help;
mod history;
mod json;
mod jump;
mod listing;
mod native;
mod pick;
pub use pick::{InteractiveRuntime, execute_with_runtime as execute_pick_with_runtime};
mod search;

/// Parse and execute a command against caller-owned streams.
pub fn execute(
    args: &[OsString],
    input: &mut dyn Read,
    out: &mut dyn Write,
    terminal: bool,
) -> Result<u8, String> {
    let write = |out: &mut dyn Write, text: &str| {
        out.write_all(text.as_bytes())
            .map_err(|e| format!("output: {e}"))
    };
    if args.is_empty() || (args.len() == 1 && (args[0] == "--help" || args[0] == "-h")) {
        write(
            out,
            "Terms — independent terminal toolkit\nUsage: terms <command> [options]\nCommands: ls, help, search, json, pick, jump, history, git\nUse terms <command> --help for usage\n",
        )?;
        return Ok(0);
    }
    if args.len() == 1 && (args[0] == "--version" || args[0] == "-V") {
        write(out, concat!("terms ", env!("CARGO_PKG_VERSION"), "\n"))?;
        return Ok(0);
    }
    let command = args[0].to_str().ok_or("invalid command token")?;
    let rest = &args[1..];
    if rest.len() == 1 && rest[0] == "--help" {
        let usage = match command {
            "ls" => {
                "ls [--all] [--long] [--tree] [--level N] [--sort name|size|modified] [--reverse] [--header] [--color auto|never|always] [--] [PATH...]"
            }
            "help" => {
                "help [--list] [--search TEXT] [--platform macos|linux|common] [--plain] [COMMAND...]"
            }
            "search" => {
                "search [-F] [-i] [-n] [-l|-c|--files] [--hidden] [--no-ignore] [--glob PATTERN] [--context N] [--null] [--] PATTERN [PATH...]"
            }
            "json" => "json [-c] [-r] [--] FILTER [FILE...]",
            "pick" => "pick [--exact] [--multi] [--read0] [--print0] [--query TEXT] [--hex]",
            "git" => {
                "git [--git-bin ABS] [-C PATH] status|diff|log|show|branches|stashes|identity|browse|stage|unstage|commit"
            }
            "history" => {
                "history init|unlock|lock|status|agent|search|pick|import|export|delete|clear|retention|exclude|repair"
            }
            "jump" => "jump add|remove|clear|list|query|exclude [options] [PATH|TERM...]",
            _ => return Err("command unavailable or unknown".into()),
        };
        write(out, usage)?;
        write(out, "\n")?;
        return Ok(0);
    }
    match command {
        "ls" => listing::execute(rest, out, terminal),
        "help" => help::execute(rest, out, terminal),
        "search" => search::execute(rest, out),
        "json" => json::execute(rest, input, out, terminal),
        "pick" => pick::execute(rest, input, out),
        "jump" => jump::execute(rest, out),
        "history" => history::execute(rest, input, out, terminal),
        "git" => git::execute(rest, input, out, terminal),
        _ => Err("unknown command or option; use --help".into()),
    }
}
