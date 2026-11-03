//! `terms init`: print the shell adapters embedded in the binary for `eval "$(terms init zsh)"`.
//!
//! Output only. Nothing is installed, no startup file is touched, and history capture is never
//! enabled here: `--history` defines its functions, and the user still runs `terms_history_enable`.
use std::ffi::OsString;
use std::io::Write;

pub const USAGE: &str = "init zsh|bash [--track] [--history]";
const ZSH: &str = include_str!("../shell/terms.zsh");
const BASH: &str = include_str!("../shell/terms.bash");
const HISTORY_ZSH: &str = include_str!("../shell/history.zsh");
const HISTORY_BASH: &str = include_str!("../shell/history.bash");

fn push_line_terminated(text: &mut String, part: &str) {
    text.push_str(part);
    if !part.ends_with('\n') {
        text.push('\n');
    }
}

pub fn execute(args: &[OsString], out: &mut dyn Write) -> Result<u8, String> {
    let usage = || format!("usage: terms {USAGE}");
    let (mut shell, mut track, mut history) = (None, false, false);
    for arg in args {
        match arg.to_str() {
            Some("--track") => track = true,
            Some("--history") => history = true,
            Some(name @ ("zsh" | "bash")) if shell.is_none() => shell = Some(name),
            _ => return Err(usage()),
        }
    }
    let (adapter, history_adapter) = match shell.ok_or_else(usage)? {
        "zsh" => (ZSH, HISTORY_ZSH),
        _ => (BASH, HISTORY_BASH),
    };
    let mut text = String::new();
    push_line_terminated(&mut text, adapter);
    if history {
        push_line_terminated(&mut text, history_adapter);
    }
    push_line_terminated(
        &mut text,
        if track {
            "terms_enable --track"
        } else {
            "terms_enable"
        },
    );
    out.write_all(text.as_bytes())
        .map_err(|e| format!("output: {e}"))?;
    Ok(0)
}
