//! Inert, authored, offline command examples embedded in the binary.
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::Write;

const USAGE: &str = "Usage: terms help [--list | --search TEXT] [--platform macos|linux|common] [--plain] [--] [COMMAND...]\nOriginal offline examples; examples are displayed and never executed.\n";
struct Page {
    name: &'static str,
    platform: &'static str,
    markdown: &'static str,
}
const PAGES: &[Page] = &[
    Page {
        name: "terms ls",
        platform: "common",
        markdown: "# terms ls\nList entries without following directory symlinks.\n\n- Show hidden files with metadata:\n```sh\nterms ls --all --long --header\n```\n- Limit a directory tree to two levels:\n```sh\nterms ls --tree --level 2 -- ./project\n```\n",
    },
    Page {
        name: "terms help",
        platform: "common",
        markdown: "# terms help\nRead original, offline examples.\n\n- List available topics:\n```sh\nterms help --list\n```\n- Search example text:\n```sh\nterms help --search directory\n```\n- Read a multiword command topic:\n```sh\nterms help terms ls\n```\n",
    },
    Page {
        name: "terms search",
        platform: "common",
        markdown: "# terms search\nSearch regular text files recursively. Hidden files and ignored paths are excluded by default.\n\n- Find literal text with line numbers:\n```sh\nterms search -F -n -- 'TODO' ./src\n```\n- Emit matching filenames as NUL-delimited records:\n```sh\nterms search -F -l --null -- 'needle' ./project\n```\n",
    },
    Page {
        name: "terms json",
        platform: "common",
        markdown: "# terms json\nValidate, format and filter JSON values from standard input or explicit files.\n\n- Format a file:\n```sh\nterms json '.' ./data.json\n```\n- Select matching array entries:\n```sh\nterms json '.[] | select(.fish==\"tuna\")' ./orders.json\n```\n- Extract a raw string:\n```sh\nterms json -r '.name' ./data.json\n```\n",
    },
    Page {
        name: "cd",
        platform: "common",
        markdown: "# cd\nChange the current shell's directory. This is a shell builtin.\n\n- Enter a directory with spaces:\n```sh\ncd -- './my project'\n```\n- Return to the previous directory:\n```sh\ncd -\n```\n",
    },
    Page {
        name: "pwd",
        platform: "common",
        markdown: "# pwd\nPrint the current working directory.\n\n- Print the physical path with symlinks resolved:\n```sh\npwd -P\n```\n",
    },
    Page {
        name: "mkdir",
        platform: "common",
        markdown: "# mkdir\nCreate directories.\n\n- Create missing parents as well:\n```sh\nmkdir -p './project/assets'\n```\n",
    },
    Page {
        name: "cp",
        platform: "common",
        markdown: "# cp\nCopy files or directories.\n\n- Ask before replacing an existing destination:\n```sh\ncp -i './source.txt' './destination.txt'\n```\n- Copy a directory recursively:\n```sh\ncp -R './source-directory' './backup-directory'\n```\n",
    },
    Page {
        name: "mv",
        platform: "common",
        markdown: "# mv\nMove or rename a file.\n\n- Ask before replacing an existing destination:\n```sh\nmv -i './old name.txt' './new name.txt'\n```\n",
    },
    Page {
        name: "cat",
        platform: "common",
        markdown: "# cat\nConcatenate files to standard output. File contents may contain terminal controls.\n\n- Combine files into a new file:\n```sh\ncat './first.txt' './second.txt' > './combined.txt'\n```\n",
    },
    Page {
        name: "find",
        platform: "common",
        markdown: "# find\nWalk a directory tree and select paths.\n\n- Find regular files by name:\n```sh\nfind './project' -type f -name '*.rs'\n```\n",
    },
    Page {
        name: "find",
        platform: "linux",
        markdown: "# find\nLinux examples for walking a directory tree.\n\n- Emit matching paths with NUL separators:\n```sh\nfind './project' -type f -name '*.rs' -print0\n```\n- Print regular-file sizes in bytes (GNU find):\n```sh\nfind './project' -type f -printf '%s %p\\n'\n```\n",
    },
    Page {
        name: "find",
        platform: "macos",
        markdown: "# find\nmacOS examples for walking a directory tree.\n\n- Emit matching paths with NUL separators:\n```sh\nfind './project' -type f -name '*.rs' -print0\n```\n",
    },
];

/// Display embedded pages. No page path is ever opened and examples remain inert.
pub fn execute(args: &[OsString], out: &mut dyn Write, terminal: bool) -> Result<u8, String> {
    if args.len() == 1 && args[0] == "--help" {
        out.write_all(USAGE.as_bytes()).map_err(write_error)?;
        return Ok(0);
    }
    let mut list = false;
    let mut search = None;
    let mut platform = if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let mut plain = false;
    let mut names = Vec::new();
    let mut end = false;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].to_str().ok_or("help arguments must be UTF-8")?;
        if !end && arg == "--" {
            end = true;
        } else if !end && arg.starts_with('-') {
            match arg {
                "--list" => list = true,
                "--plain" => plain = true,
                "--search" | "--platform" => {
                    index += 1;
                    let value = args
                        .get(index)
                        .and_then(|v| v.to_str())
                        .ok_or("help option requires a UTF-8 value")?;
                    if arg == "--search" {
                        if value.len() > 64 * 1024 {
                            return Err("help query exceeds 64 KiB".into());
                        }
                        search = Some(value.to_ascii_lowercase());
                    } else {
                        platform = match value {
                            "macos" => "macos",
                            "linux" => "linux",
                            "common" => "common",
                            _ => return Err("--platform requires macos, linux, or common".into()),
                        };
                    }
                }
                _ => return Err("unknown help option; use terms help --help".into()),
            }
        } else {
            if !arg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'))
                || arg.is_empty()
            {
                return Err("help topic must contain only ASCII command words, not paths".into());
            }
            names.push(arg);
        }
        index += 1;
    }
    if (list && search.is_some()) || (!names.is_empty() && (list || search.is_some())) {
        return Err("help list, search, and topic modes are mutually exclusive".into());
    }
    let topics: BTreeSet<&str> = PAGES
        .iter()
        .filter(|p| p.platform == "common" || p.platform == platform)
        .map(|p| p.name)
        .collect();
    if list || search.is_some() {
        let mut matched = false;
        for name in topics {
            let page = resolve(name, platform).ok_or("internal help page missing")?;
            if search
                .as_ref()
                .is_none_or(|query| page.markdown.to_ascii_lowercase().contains(query))
            {
                writeln!(out, "{name}").map_err(write_error)?;
                matched = true;
            }
        }
        return Ok(if matched { 0 } else { 1 });
    }
    if names.is_empty() {
        out.write_all(USAGE.as_bytes()).map_err(write_error)?;
        return Ok(0);
    }
    let joined = names.join(" ");
    if joined.len() > 1024 {
        return Err("help topic exceeds 1024 bytes".into());
    }
    let normalized = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = match normalized.as_str() {
        "ls" => "terms ls",
        "help" => "terms help",
        "search" => "terms search",
        "json" => "terms json",
        other => other,
    };
    let Some(page) = resolve(name, platform) else {
        return Ok(1);
    };
    let styled = terminal
        && !plain
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var_os("TERM").is_some_and(|v| v != "dumb");
    render(page.markdown, out, styled)?;
    Ok(0)
}
fn resolve<'a>(name: &str, platform: &str) -> Option<&'a Page> {
    PAGES
        .iter()
        .find(|p| p.name == name && p.platform == platform)
        .or_else(|| {
            PAGES
                .iter()
                .find(|p| p.name == name && p.platform == "common")
        })
}
fn render(markdown: &str, out: &mut dyn Write, styled: bool) -> Result<(), String> {
    let mut code = false;
    for line in markdown.lines() {
        if line.starts_with("```") {
            code = !code;
            continue;
        }
        if let Some(heading) = line.strip_prefix("# ") {
            if styled {
                writeln!(out, "\x1b[1m{heading}\x1b[0m").map_err(write_error)?;
            } else {
                writeln!(out, "{heading}").map_err(write_error)?;
            }
        } else if code {
            writeln!(out, "    {line}").map_err(write_error)?;
        } else {
            writeln!(out, "{line}").map_err(write_error)?;
        }
    }
    Ok(())
}
fn write_error(_: std::io::Error) -> String {
    "cannot write help output".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(args: &[&str]) -> (u8, String) {
        let mut out = Vec::new();
        let status = execute(
            &args.iter().map(OsString::from).collect::<Vec<_>>(),
            &mut out,
            false,
        )
        .unwrap();
        (status, String::from_utf8(out).unwrap())
    }
    #[test]
    fn listing_resolution_and_search() {
        assert!(run(&["--list"]).1.contains("terms json\n"));
        assert!(run(&["terms", "json"]).1.contains("select(.fish"));
        assert!(run(&["--platform", "linux", "find"]).1.contains("-printf"));
        assert!(!run(&["--platform", "macos", "find"]).1.contains("-printf"));
        assert!(!run(&["--platform", "common", "find"]).1.contains("-printf"));
        assert_eq!(run(&["--search", "this query cannot match"]).0, 1);
        assert_eq!(run(&["nonexistent"]).0, 1);
        assert!(!run(&["json"]).1.contains('\x1b'));
    }
    #[test]
    fn bad_paths_and_conflicts() {
        for args in [
            vec!["../json"],
            vec!["--list", "json"],
            vec!["--list", "--search", "x"],
            vec!["--platform", "windows"],
        ] {
            assert!(
                execute(
                    &args.iter().map(OsString::from).collect::<Vec<_>>(),
                    &mut Vec::new(),
                    false
                )
                .is_err()
            );
        }
    }
}
