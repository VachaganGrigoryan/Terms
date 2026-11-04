//! Deterministic, read-only file listings with escaped display names.
use std::ffi::OsString;
use std::fs::{self, Metadata};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

const HELP: &str = "Usage: terms ls [--all] [--long] [--tree] [--level N] [--sort name|size|modified] [--reverse] [--header] [--color auto|never|always] [--icons none|emoji|nerd|phosphor] [--] [PATH...]\nOne entry per line; symlinks are displayed, never intentionally followed.\nIcons are off by default. emoji needs no font; nerd and phosphor need that font installed.\nTERMS_ICONS=<set> enables icons on terminals only; --icons overrides it.\nLong columns: TYPE MODE UID GID BYTES MODIFIED NAME\n";
const MAX_ENTRIES: usize = 1_000_000;
const MAX_NAMES: usize = 64 * 1024 * 1024;

#[derive(Default)]
struct Options {
    all: bool,
    long: bool,
    tree: bool,
    level: Option<usize>,
    sort: Sort,
    reverse: bool,
    header: bool,
    color: bool,
    icons: Option<IconSet>,
    paths: Vec<PathBuf>,
}
#[derive(Default)]
enum Sort {
    #[default]
    Name,
    Size,
    Modified,
}
struct Entry {
    path: PathBuf,
    name: OsString,
    metadata: Metadata,
}

/// List paths without changing filesystem contents or following directory links.
pub fn execute(args: &[OsString], out: &mut dyn Write, terminal: bool) -> Result<u8, String> {
    if args.len() == 1 && args[0] == "--help" {
        out.write_all(HELP.as_bytes())
            .map_err(|_| "cannot write listing help".to_string())?;
        return Ok(0);
    }
    let options = parse(args, terminal)?;
    let mut errors = Vec::new();
    if options.header {
        writeln!(out, "TYPE MODE UID GID BYTES MODIFIED NAME").map_err(write_error)?;
    }
    for path in &options.paths {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                record_error(&mut errors, path_error(path, &error))?;
                continue;
            }
        };
        if options.tree {
            let entry = Entry {
                path: path.clone(),
                name: path.as_os_str().to_owned(),
                metadata,
            };
            if let Err(error) = show(&entry, "", &options, out) {
                record_error(&mut errors, error)?;
            }
            if entry.metadata.is_dir() {
                walk(path, 0, "", &options, out, &mut errors)?;
            }
        } else if metadata.is_dir() {
            if options.paths.len() > 1 {
                writeln!(out, "{}:", escape(path.as_os_str().as_bytes())).map_err(write_error)?;
            }
            match entries(path, &options) {
                Ok(entries) => {
                    for entry in entries {
                        if let Err(error) = show(&entry, "", &options, out) {
                            record_error(&mut errors, error)?;
                        }
                    }
                }
                Err(error) => record_error(&mut errors, error)?,
            }
        } else {
            let entry = Entry {
                path: path.clone(),
                name: path.as_os_str().to_owned(),
                metadata,
            };
            if let Err(error) = show(&entry, "", &options, out) {
                record_error(&mut errors, error)?;
            }
        }
    }
    if errors.is_empty() {
        Ok(0)
    } else {
        Err(errors.join("\n"))
    }
}
fn record_error(errors: &mut Vec<String>, error: String) -> Result<(), String> {
    if error == "cannot write listing output" {
        return Err(error);
    }
    if errors.len() < 64 {
        errors.push(error);
    } else if errors.len() == 64 {
        errors.push("additional listing errors omitted".into());
    }
    Ok(())
}
fn write_error(_: std::io::Error) -> String {
    "cannot write listing output".into()
}
fn path_error(path: &Path, error: &std::io::Error) -> String {
    format!(
        "{}: {}",
        escape(path.as_os_str().as_bytes()),
        escape(error.to_string().as_bytes())
    )
}
fn parse(args: &[OsString], terminal: bool) -> Result<Options, String> {
    let mut options = Options {
        color: false,
        ..Options::default()
    };
    let mut end = false;
    let mut pos = 0;
    let mut color = None;
    let mut icons = None;
    while pos < args.len() {
        let arg = &args[pos];
        if !end && arg == "--" {
            end = true;
        } else if !end && arg.as_bytes().starts_with(b"-") {
            match arg.to_str() {
                Some("--all") => options.all = true,
                Some("--long") => options.long = true,
                Some("--tree") => options.tree = true,
                Some("--reverse") => options.reverse = true,
                Some("--header") => options.header = true,
                Some(flag @ ("--level" | "--sort" | "--color" | "--icons")) => {
                    pos += 1;
                    let value = args
                        .get(pos)
                        .and_then(|s| s.to_str())
                        .ok_or_else(|| format!("{flag} requires a value"))?;
                    match flag {
                        "--level" => {
                            let level = value
                                .parse::<usize>()
                                .map_err(|_| "--level requires an integer from 0 to 256")?;
                            if level > 256 {
                                return Err("--level exceeds 256".into());
                            }
                            options.level = Some(level);
                        }
                        "--sort" => {
                            options.sort = match value {
                                "name" => Sort::Name,
                                "size" => Sort::Size,
                                "modified" => Sort::Modified,
                                _ => return Err("--sort requires name, size, or modified".into()),
                            }
                        }
                        "--icons" => {
                            icons = Some(
                                parse_icons(value)
                                    .ok_or("--icons requires none, emoji, nerd, or phosphor")?,
                            )
                        }
                        _ => {
                            let setting = match value {
                                "auto" => styled(terminal),
                                "always" => true,
                                "never" => false,
                                _ => return Err("--color requires auto, never, or always".into()),
                            };
                            color = Some(setting);
                        }
                    }
                }
                _ => return Err("unknown listing option; use terms ls --help".into()),
            }
        } else {
            options.paths.push(PathBuf::from(arg));
        }
        pos += 1;
    }
    options.color = color.unwrap_or_else(|| styled(terminal));
    // TERMS_ICONS is only a default for interactive terminals; an explicit --icons wins everywhere.
    options.icons = icons.unwrap_or_else(|| {
        let capable = terminal && std::env::var_os("TERM").is_some_and(|v| v != "dumb");
        std::env::var("TERMS_ICONS")
            .ok()
            .and_then(|v| parse_icons(&v))
            .flatten()
            .filter(|_| capable)
    });
    if options.header && !options.long {
        return Err("--header requires --long".into());
    }
    if options.level.is_some() && !options.tree {
        return Err("--level requires --tree".into());
    }
    if options.paths.is_empty() {
        options.paths.push(PathBuf::from("."));
    }
    Ok(options)
}
/// `Some(None)` is an explicit "none"; `None` is an unknown value.
fn parse_icons(value: &str) -> Option<Option<IconSet>> {
    match value {
        "none" => Some(None),
        "emoji" => Some(Some(IconSet::Emoji)),
        "phosphor" => Some(Some(IconSet::Phosphor)),
        "nerd" => Some(Some(IconSet::Nerd)),
        _ => None,
    }
}
/// Styling is allowed on a capable terminal unless `NO_COLOR` is set.
fn styled(terminal: bool) -> bool {
    terminal
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var_os("TERM").is_some_and(|v| v != "dumb")
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Icon {
    Folder,
    Git,
    Link,
    Pipe,
    Socket,
    Device,
    Unknown,
    Package,
    Config,
    Book,
    Rust,
    Python,
    Js,
    Ts,
    Html,
    Css,
    Markdown,
    Csv,
    Sql,
    C,
    Cpp,
    Data,
    Shell,
    Code,
    Image,
    Video,
    Audio,
    Zip,
    Archive,
    Pdf,
    Word,
    Excel,
    Slides,
    Text,
    Database,
    Key,
    Lock,
    File,
    Go,
    Java,
    Ruby,
    Php,
    Swift,
    Kotlin,
    Lua,
    CSharp,
    Vue,
    Jsx,
    Tsx,
    Svelte,
    Dart,
    Scala,
    Haskell,
    Elixir,
    Zig,
    Nix,
    Perl,
    R,
    Docker,
    Terraform,
    WinScript,
    Svg,
    Xml,
    Font,
    Binary,
    Diff,
    Subtitle,
    Notebook,
    Disc,
    Cert,
    Calendar,
    Contact,
    LockFile,
}
#[derive(Clone, Copy)]
enum IconSet {
    Emoji,
    Phosphor,
    Nerd,
}
/// (emoji, Phosphor regular web 2.1.1, Nerd Font v3.4.0 glyphnames) per kind.
fn glyph(icon: Icon, set: IconSet) -> &'static str {
    let (emoji, phosphor, nerd) = match icon {
        Icon::Folder => ("📁", "\u{e24a}", "\u{f07b}"),
        Icon::Git => ("🌱", "\u{e278}", "\u{e702}"),
        Icon::Link => ("🔗", "\u{e2e2}", "\u{f0c1}"),
        Icon::Pipe => ("🚰", "\u{ed86}", "\u{f07e5}"),
        Icon::Socket => ("🔌", "\u{e946}", "\u{f06a5}"),
        Icon::Device => ("💽", "\u{e29e}", "\u{f02ca}"),
        Icon::Unknown => ("❓", "\u{e3e8}", "\u{f128}"),
        Icon::Package => ("📦", "\u{e390}", "\u{f03d6}"),
        Icon::Config => ("🔧", "\u{e270}", "\u{f013}"),
        Icon::Book => ("📖", "\u{e0e6}", "\u{f02d}"),
        Icon::Rust => ("🦀", "\u{eb28}", "\u{e7a8}"),
        Icon::Python => ("🐍", "\u{eb2c}", "\u{e73c}"),
        Icon::Js => ("🟨", "\u{eb24}", "\u{e781}"),
        Icon::Ts => ("🟦", "\u{eb26}", "\u{e8ca}"),
        Icon::Html => ("🌐", "\u{eb38}", "\u{e736}"),
        Icon::Css => ("🎨", "\u{eb34}", "\u{e749}"),
        Icon::Markdown => ("📝", "\u{ed50}", "\u{e73e}"),
        Icon::Csv => ("📊", "\u{eb1c}", "\u{e64a}"),
        Icon::Sql => ("💾", "\u{ed4e}", "\u{f1c0}"),
        Icon::C => ("🔷", "\u{eb32}", "\u{e61e}"),
        Icon::Cpp => ("🔶", "\u{eb2e}", "\u{e61d}"),
        Icon::Data => ("🧾", "\u{e860}", "\u{e60b}"),
        Icon::Shell => ("🐚", "\u{eae8}", "\u{e795}"),
        Icon::Code => ("💻", "\u{e914}", "\u{f121}"),
        Icon::Image => ("📷", "\u{ea24}", "\u{f1c5}"),
        Icon::Video => ("🎬", "\u{ea22}", "\u{f1c8}"),
        Icon::Audio => ("🎵", "\u{ea20}", "\u{f1c7}"),
        Icon::Zip => ("🤐", "\u{e958}", "\u{f1c6}"),
        Icon::Archive => ("🧳", "\u{eb2a}", "\u{f1c6}"),
        Icon::Pdf => ("📕", "\u{e702}", "\u{f1c1}"),
        Icon::Word => ("📘", "\u{eb1e}", "\u{f1c2}"),
        Icon::Excel => ("📗", "\u{eb22}", "\u{f1c3}"),
        Icon::Slides => ("📙", "\u{eb20}", "\u{f1c4}"),
        Icon::Text => ("📃", "\u{e23a}", "\u{f15c}"),
        Icon::Database => ("📀", "\u{e1de}", "\u{f1c0}"),
        Icon::Key => ("🔑", "\u{e2d6}", "\u{f084}"),
        Icon::Lock => ("🔒", "\u{e2fe}", "\u{f023}"),
        Icon::File => ("📄", "\u{e230}", "\u{f15b}"),
        Icon::Go => ("🐹", "\u{e914}", "\u{e724}"),
        Icon::Java => ("☕", "\u{e1c2}", "\u{e738}"),
        Icon::Ruby => ("💎", "\u{e1ec}", "\u{e739}"),
        Icon::Php => ("🐘", "\u{e914}", "\u{e73d}"),
        Icon::Swift => ("🐦", "\u{e914}", "\u{e755}"),
        Icon::Kotlin => ("🟪", "\u{e914}", "\u{e634}"),
        Icon::Lua => ("🌙", "\u{e914}", "\u{e620}"),
        Icon::CSharp => ("🟩", "\u{eb30}", "\u{e648}"),
        Icon::Vue => ("💚", "\u{eb3e}", "\u{e6a0}"),
        Icon::Jsx => ("🔵", "\u{eb3a}", "\u{e7ba}"),
        Icon::Tsx => ("🟦", "\u{eb3c}", "\u{e7ba}"),
        Icon::Svelte => ("🧡", "\u{e914}", "\u{e697}"),
        Icon::Dart => ("🎯", "\u{e914}", "\u{e798}"),
        Icon::Scala => ("🔺", "\u{e914}", "\u{e737}"),
        Icon::Haskell => ("🟣", "\u{ebe4}", "\u{e777}"),
        Icon::Elixir => ("💧", "\u{e914}", "\u{e62d}"),
        Icon::Zig => ("⚡", "\u{e2de}", "\u{e6a9}"),
        Icon::Nix => ("🧊", "\u{e914}", "\u{f313}"),
        Icon::Perl => ("🐪", "\u{e914}", "\u{e769}"),
        Icon::R => ("📈", "\u{e914}", "\u{e68a}"),
        Icon::Docker => ("🐳", "\u{e1da}", "\u{e7b0}"),
        Icon::Terraform => ("🌍", "\u{e95e}", "\u{e69a}"),
        Icon::WinScript => ("🪟", "\u{e692}", "\u{e683}"),
        Icon::Svg => ("📐", "\u{ed08}", "\u{f1c5}"),
        Icon::Xml => ("🧩", "\u{e862}", "\u{e619}"),
        Icon::Font => ("🔤", "\u{e6ee}", "\u{f031}"),
        Icon::Binary => ("🧱", "\u{ee60}", "\u{f471}"),
        Icon::Diff => ("🩹", "\u{e27c}", "\u{f440}"),
        Icon::Subtitle => ("💬", "\u{e1a8}", "\u{f0a16}"),
        Icon::Notebook => ("📓", "\u{e34e}", "\u{f082e}"),
        Icon::Disc => ("💿", "\u{e564}", "\u{f05ee}"),
        Icon::Cert => ("📜", "\u{e766}", "\u{f0a3}"),
        Icon::Calendar => ("📅", "\u{e108}", "\u{f00ed}"),
        Icon::Contact => ("📇", "\u{e6f8}", "\u{f06cb}"),
        Icon::LockFile => ("🔐", "\u{e95c}", "\u{f023}"),
    };
    match set {
        IconSet::Emoji => emoji,
        IconSet::Phosphor => phosphor,
        IconSet::Nerd => nerd,
    }
}
fn by_name(name: &[u8]) -> Option<Icon> {
    Some(match name {
        b"Dockerfile"
        | b"Containerfile"
        | b"docker-compose.yml"
        | b"docker-compose.yaml"
        | b"compose.yml"
        | b"compose.yaml" => Icon::Docker,
        b".gitignore" | b".gitattributes" | b".gitmodules" | b".mailmap" => Icon::Git,
        b"package-lock.json" | b"pnpm-lock.yaml" => Icon::LockFile,
        b"Makefile" | b"GNUmakefile" | b"CMakeLists.txt" | b"justfile" | b"Rakefile"
        | b"Gemfile" | b"go.mod" | b"go.sum" => Icon::Config,
        b"Cargo.toml" | b"package.json" | b"pyproject.toml" => Icon::Package,
        _ if [
            &b"LICENSE"[..],
            b"COPYING",
            b"README",
            b"CHANGELOG",
            b"AUTHORS",
            b"CONTRIBUTING",
        ]
        .iter()
        .any(|p| name.starts_with(p)) =>
        {
            Icon::Book
        }
        _ if name == b".env" || name.starts_with(b".env.") => Icon::Key,
        _ => return None,
    })
}
fn by_extension(extension: &[u8]) -> Option<Icon> {
    Some(match extension {
        b"rs" => Icon::Rust,
        b"py" | b"pyi" | b"pyw" => Icon::Python,
        b"js" | b"mjs" | b"cjs" => Icon::Js,
        b"ts" | b"mts" | b"cts" => Icon::Ts,
        b"jsx" => Icon::Jsx,
        b"tsx" => Icon::Tsx,
        b"html" | b"htm" | b"xhtml" => Icon::Html,
        b"css" | b"scss" | b"sass" | b"less" | b"styl" => Icon::Css,
        b"md" | b"mdx" | b"markdown" | b"rst" | b"adoc" | b"org" => Icon::Markdown,
        b"csv" | b"tsv" => Icon::Csv,
        b"sql" => Icon::Sql,
        b"c" | b"h" => Icon::C,
        b"cpp" | b"cc" | b"cxx" | b"hpp" | b"hh" | b"hxx" => Icon::Cpp,
        b"json" | b"jsonc" | b"json5" | b"jsonl" | b"ndjson" | b"yaml" | b"yml" | b"toml"
        | b"msgpack" | b"map" => Icon::Data,
        b"xml" | b"xsd" | b"xsl" | b"plist" => Icon::Xml,
        b"svg" => Icon::Svg,
        b"sh" | b"bash" | b"zsh" | b"fish" | b"ksh" | b"csh" | b"nu" => Icon::Shell,
        b"bat" | b"cmd" | b"ps1" | b"psm1" => Icon::WinScript,
        b"go" => Icon::Go,
        b"java" | b"gradle" => Icon::Java,
        b"rb" | b"erb" | b"gemspec" | b"rake" => Icon::Ruby,
        b"php" => Icon::Php,
        b"swift" => Icon::Swift,
        b"kt" | b"kts" => Icon::Kotlin,
        b"lua" => Icon::Lua,
        b"cs" | b"csx" => Icon::CSharp,
        b"vue" => Icon::Vue,
        b"svelte" => Icon::Svelte,
        b"dart" => Icon::Dart,
        b"scala" | b"sc" => Icon::Scala,
        b"hs" | b"lhs" => Icon::Haskell,
        b"ex" | b"exs" | b"heex" => Icon::Elixir,
        b"zig" => Icon::Zig,
        b"nix" => Icon::Nix,
        b"pl" | b"pm" => Icon::Perl,
        b"r" | b"rmd" => Icon::R,
        b"tf" | b"tfvars" | b"hcl" => Icon::Terraform,
        b"m" | b"mm" | b"ml" | b"clj" | b"erl" | b"jl" | b"vb" | b"asm" | b"s" | b"f90"
        | b"proto" | b"graphql" | b"gql" | b"cmake" | b"mk" | b"tpl" | b"j2" | b"hbs" | b"ejs"
        | b"tape" | b"slint" | b"sol" | b"el" | b"lisp" | b"groovy" | b"tcl" | b"awk" | b"vim" => {
            Icon::Code
        }
        b"png" | b"jpg" | b"jpeg" | b"gif" | b"webp" | b"ico" | b"bmp" | b"tif" | b"tiff"
        | b"heic" | b"heif" | b"avif" | b"psd" | b"ai" | b"xcf" | b"raw" | b"cr2" | b"nef"
        | b"dng" | b"sketch" | b"fig" | b"icns" | b"xwd" | b"rgba" => Icon::Image,
        b"mp4" | b"mov" | b"mkv" | b"webm" | b"avi" | b"wmv" | b"flv" | b"m4v" | b"mpg"
        | b"mpeg" | b"3gp" => Icon::Video,
        b"mp3" | b"wav" | b"flac" | b"ogg" | b"m4a" | b"aac" | b"aiff" | b"aif" | b"opus"
        | b"wma" | b"mid" | b"midi" | b"pcm" | b"m3u" | b"m3u8" => Icon::Audio,
        b"zip" => Icon::Zip,
        b"tar" | b"gz" | b"tgz" | b"xz" | b"bz2" | b"7z" | b"zst" | b"txz" | b"tbz2" | b"rar"
        | b"lz4" | b"lzma" | b"cab" | b"z" => Icon::Archive,
        b"deb" | b"rpm" | b"apk" | b"whl" | b"gem" | b"crate" | b"jar" | b"war" | b"nupkg"
        | b"pkg" | b"msi" | b"appimage" | b"snap" => Icon::Package,
        b"pdf" => Icon::Pdf,
        b"doc" | b"docx" | b"odt" | b"rtf" | b"pages" => Icon::Word,
        b"xls" | b"xlsx" | b"ods" | b"numbers" => Icon::Excel,
        b"ppt" | b"pptx" | b"odp" | b"key" => Icon::Slides,
        b"txt" | b"log" | b"text" | b"out" => Icon::Text,
        b"epub" | b"mobi" | b"azw3" => Icon::Book,
        b"db" | b"sqlite" | b"sqlite3" | b"mdb" | b"parquet" => Icon::Database,
        b"ini" | b"cfg" | b"conf" | b"config" | b"properties" | b"editorconfig" | b"iml"
        | b"example" | b"sample" | b"eslintrc" | b"npmrc" | b"nvmrc" => Icon::Config,
        b"pem" | b"pub" | b"asc" | b"env" | b"keystore" | b"sig" => Icon::Key,
        b"gpg" | b"age" | b"kdbx" => Icon::Lock,
        b"lock" => Icon::LockFile,
        b"so" | b"dylib" | b"a" | b"o" | b"obj" | b"lib" | b"dll" | b"exe" | b"bin" | b"pyc"
        | b"pyo" | b"class" | b"wasm" | b"dat" | b"pth" | b"rkyv" | b"cache" => Icon::Binary,
        b"diff" | b"patch" | b"rej" => Icon::Diff,
        b"srt" | b"vtt" | b"ass" | b"ssa" | b"sub" => Icon::Subtitle,
        b"ipynb" => Icon::Notebook,
        b"iso" | b"dmg" | b"img" | b"vhd" | b"vmdk" | b"qcow2" => Icon::Disc,
        b"ttf" | b"otf" | b"woff" | b"woff2" | b"eot" => Icon::Font,
        b"crt" | b"cer" | b"der" | b"p12" | b"pfx" | b"csr" => Icon::Cert,
        b"ics" => Icon::Calendar,
        b"vcf" => Icon::Contact,
        _ => return None,
    })
}
/// Pick the icon kind from metadata and name only; files are never opened.
fn classify(entry: &Entry) -> Icon {
    let file_type = entry.metadata.file_type();
    let name = entry.name.as_bytes();
    if file_type.is_dir() {
        return match name {
            b".git" | b".github" => Icon::Git,
            b"node_modules" => Icon::Package,
            _ => Icon::Folder,
        };
    }
    if file_type.is_symlink() {
        return Icon::Link;
    }
    if file_type.is_fifo() {
        return Icon::Pipe;
    }
    if file_type.is_socket() {
        return Icon::Socket;
    }
    if file_type.is_block_device() || file_type.is_char_device() {
        return Icon::Device;
    }
    if !file_type.is_file() {
        return Icon::Unknown;
    }
    by_name(name)
        .or_else(|| {
            let extension = Path::new(&entry.name).extension()?;
            by_extension(&extension.as_bytes().to_ascii_lowercase())
        })
        .unwrap_or(if name.starts_with(b".") {
            Icon::Config
        } else if entry.metadata.mode() & 0o111 != 0 {
            Icon::Shell
        } else {
            Icon::File
        })
}
fn entries(path: &Path, options: &Options) -> Result<Vec<Entry>, String> {
    let iterator = fs::read_dir(path).map_err(|error| path_error(path, &error))?;
    let mut result = Vec::new();
    let mut name_bytes = 0usize;
    for item in iterator {
        let item = item.map_err(|error| path_error(path, &error))?;
        let name = item.file_name();
        if !options.all && name.as_bytes().starts_with(b".") {
            continue;
        }
        name_bytes = name_bytes
            .checked_add(name.as_bytes().len())
            .ok_or("listing name budget exceeded")?;
        if result.len() >= MAX_ENTRIES || name_bytes > MAX_NAMES {
            return Err("listing directory budget exceeded".into());
        }
        let path = item.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| path_error(&path, &error))?;
        result.push(Entry {
            path,
            name,
            metadata,
        });
    }
    result.sort_by(|a, b| {
        let primary = match options.sort {
            Sort::Name => std::cmp::Ordering::Equal,
            Sort::Size => a.metadata.len().cmp(&b.metadata.len()),
            Sort::Modified => a
                .metadata
                .mtime()
                .cmp(&b.metadata.mtime())
                .then(a.metadata.mtime_nsec().cmp(&b.metadata.mtime_nsec())),
        };
        primary.then(a.name.as_bytes().cmp(b.name.as_bytes()))
    });
    if options.reverse {
        result.reverse();
    }
    Ok(result)
}
fn walk(
    path: &Path,
    depth: usize,
    prefix: &str,
    options: &Options,
    out: &mut dyn Write,
    errors: &mut Vec<String>,
) -> Result<(), String> {
    if depth >= options.level.unwrap_or(256) {
        return Ok(());
    }
    let children = match entries(path, options) {
        Ok(v) => v,
        Err(error) => {
            record_error(errors, error)?;
            return Ok(());
        }
    };
    let count = children.len();
    for (index, child) in children.into_iter().enumerate() {
        let last = index + 1 == count;
        let branch = format!("{prefix}{}", if last { "`-- " } else { "|-- " });
        if let Err(error) = show(&child, &branch, options, out) {
            record_error(errors, error)?;
        }
        if child.metadata.is_dir() {
            if depth + 1 == 256 && options.level.is_none() {
                record_error(
                    errors,
                    format!(
                        "{}: tree depth limit reached",
                        escape(child.path.as_os_str().as_bytes())
                    ),
                )?;
            } else {
                let next = format!("{prefix}{}", if last { "    " } else { "|   " });
                walk(&child.path, depth + 1, &next, options, out, errors)?;
            }
        }
    }
    Ok(())
}
fn show(entry: &Entry, prefix: &str, options: &Options, out: &mut dyn Write) -> Result<(), String> {
    let file_type = entry.metadata.file_type();
    let kind = if file_type.is_dir() {
        'd'
    } else if file_type.is_symlink() {
        'l'
    } else if file_type.is_file() {
        '-'
    } else if file_type.is_fifo() {
        'p'
    } else if file_type.is_socket() {
        's'
    } else if file_type.is_block_device() {
        'b'
    } else if file_type.is_char_device() {
        'c'
    } else {
        '?'
    };
    let mut line = String::new();
    if options.long {
        let timestamp = timestamp(entry.metadata.mtime())?;
        line.push_str(&format!(
            "{kind} {:04o} {} {} {} {timestamp} ",
            entry.metadata.mode() & 0o7777,
            entry.metadata.uid(),
            entry.metadata.gid(),
            entry.metadata.len()
        ));
    }
    line.push_str(prefix);
    if let Some(set) = options.icons {
        line.push_str(glyph(classify(entry), set));
        line.push(' ');
    }
    if options.color {
        line.push_str(if file_type.is_dir() {
            "\x1b[34m"
        } else if file_type.is_symlink() {
            "\x1b[36m"
        } else {
            "\x1b[0m"
        });
    }
    line.push_str(&escape(entry.name.as_bytes()));
    if options.color {
        line.push_str("\x1b[0m");
    }
    if file_type.is_symlink() {
        let target = fs::read_link(&entry.path).map_err(|error| path_error(&entry.path, &error))?;
        line.push_str(" -> ");
        line.push_str(&escape(target.as_os_str().as_bytes()));
    }
    line.push('\n');
    out.write_all(line.as_bytes()).map_err(write_error)
}

/// Escape untrusted bytes without lossy replacement or terminal control interpretation.
fn escape(bytes: &[u8]) -> String {
    let mut result = String::new();
    let mut remaining = bytes;
    while !remaining.is_empty() {
        match std::str::from_utf8(remaining) {
            Ok(text) => {
                escape_text(text, &mut result);
                break;
            }
            Err(error) => {
                if let Ok(text) = std::str::from_utf8(&remaining[..error.valid_up_to()]) {
                    escape_text(text, &mut result);
                }
                let bad = error
                    .error_len()
                    .unwrap_or(remaining.len() - error.valid_up_to());
                for byte in &remaining[error.valid_up_to()..error.valid_up_to() + bad] {
                    result.push_str(&format!("\\x{byte:02x}"));
                }
                remaining = &remaining[error.valid_up_to() + bad..];
            }
        }
    }
    result
}
fn escape_text(text: &str, out: &mut String) {
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            c if c.is_control()
                || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{2028}' | '\u{2029}') =>
            {
                if (c as u32) < 256 {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                } else {
                    out.push_str(&format!("\\u{{{:x}}}", c as u32));
                }
            }
            c => out.push(c),
        }
    }
}
fn timestamp(seconds: i64) -> Result<String, String> {
    let days = seconds.div_euclid(86_400);
    let clock = seconds.rem_euclid(86_400);
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_march + 2) / 5 + 1;
    let month = month_march + if month_march < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    if !(0..=9999).contains(&year) {
        return Err("timestamp is outside RFC 3339 year range".into());
    }
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        clock / 3600,
        clock / 60 % 60,
        clock % 60
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dates_and_hostile_bytes() {
        assert_eq!(timestamp(0).unwrap(), "1970-01-01T00:00:00Z");
        assert_eq!(timestamp(-1).unwrap(), "1969-12-31T23:59:59Z");
        assert_eq!(timestamp(951782400).unwrap(), "2000-02-29T00:00:00Z");
        assert_eq!(escape(b"a\x1b\n\xff\\"), "a\\x1b\\x0a\\xff\\\\");
        assert_eq!(escape("x\u{202e}y".as_bytes()), "x\\u{202e}y");
    }
    #[test]
    fn files_tree_links_and_read_only_output() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!(
            "terms-listing-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        fs::write(root.join("z"), b"unchanged").unwrap();
        fs::write(root.join("a"), b"x").unwrap();
        fs::write(root.join(".hidden"), b"private").unwrap();
        fs::write(root.join("bad\nname"), b"bytes").unwrap();
        fs::create_dir(root.join("dir")).unwrap();
        fs::write(root.join("dir/child"), b"child").unwrap();
        symlink("dir", root.join("link")).unwrap();
        let mut out = Vec::new();
        execute(&[root.as_os_str().to_owned()], &mut out, false).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("a\nbad\\x0aname\n"));
        assert!(text.contains("link -> dir\n"));
        assert!(!text.contains(".hidden"));
        let mut out = Vec::new();
        execute(
            &[
                "--tree".into(),
                "--level".into(),
                "1".into(),
                root.as_os_str().to_owned(),
            ],
            &mut out,
            false,
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("child"));
        assert_eq!(text.matches("link -> dir").count(), 1);
        let mut out = Vec::new();
        execute(
            &[
                "--long".into(),
                "--header".into(),
                root.join("a").into_os_string(),
            ],
            &mut out,
            false,
        )
        .unwrap();
        assert!(
            String::from_utf8(out)
                .unwrap()
                .starts_with("TYPE MODE UID GID BYTES MODIFIED NAME\n- ")
        );
        assert_eq!(fs::read(root.join("z")).unwrap(), b"unchanged");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 6);
    }
    #[test]
    fn writer_failure_stops_listing() {
        struct Broken;
        impl std::io::Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert_eq!(
            execute(&["Cargo.toml".into()], &mut Broken, false).unwrap_err(),
            "cannot write listing output"
        );
    }
    #[test]
    fn invalid_combinations() {
        for args in [
            vec!["--header"],
            vec!["--level", "1"],
            vec!["--tree", "--level", "257"],
            vec!["--sort", "bad"],
            vec!["--icons"],
            vec!["--icons", "bad"],
            vec!["--icons", "always"],
            vec!["--icon-set", "nerd"],
        ] {
            assert!(
                execute(
                    &args.into_iter().map(OsString::from).collect::<Vec<_>>(),
                    &mut Vec::new(),
                    false
                )
                .is_err()
            );
        }
    }
    #[test]
    fn icons_by_type_and_name() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = std::env::temp_dir().join(format!("terms-icons-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir(root.join("d")).unwrap();
        for f in ["x.rs", "X.PNG", "Cargo.toml", "run", "o.zzz"] {
            fs::write(root.join(f), b"").unwrap();
        }
        fs::set_permissions(root.join("run"), fs::Permissions::from_mode(0o755)).unwrap();
        symlink("d", root.join("l")).unwrap();
        let get = |n: &str| {
            let path = root.join(n);
            glyph(
                classify(&Entry {
                    metadata: fs::symlink_metadata(&path).unwrap(),
                    name: n.into(),
                    path,
                }),
                IconSet::Phosphor,
            )
        };
        assert_eq!(get("d"), "\u{e24a}");
        assert_eq!(get(".git"), "\u{e278}");
        assert_eq!(get("x.rs"), "\u{eb28}");
        assert_eq!(get("X.PNG"), "\u{ea24}");
        assert_eq!(get("Cargo.toml"), "\u{e390}");
        assert_eq!(get("run"), "\u{eae8}");
        assert_eq!(get("o.zzz"), "\u{e230}");
        assert_eq!(get("l"), "\u{e2e2}");
        let list = |extra: &[&str]| {
            let mut args: Vec<OsString> = extra.iter().map(OsString::from).collect();
            args.push(root.join("x.rs").into_os_string());
            let mut out = Vec::new();
            execute(&args, &mut out, false).unwrap();
            String::from_utf8(out).unwrap()
        };
        assert!(list(&["--icons", "emoji"]).starts_with("🦀 "));
        assert!(list(&["--icons", "phosphor"]).starts_with("\u{eb28} "));
        assert!(list(&["--icons", "nerd"]).starts_with("\u{e7a8} "));
        assert_eq!(
            list(&[]).chars().count(),
            list(&["--icons", "none"]).chars().count()
        );
        assert!(!list(&["--icons", "none"]).contains("🦀"));
        fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn icon_kinds_by_name() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("terms-kinds-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("node_modules")).unwrap();
        let cases = [
            ("a.go", Icon::Go),
            ("A.JAVA", Icon::Java),
            ("a.vue", Icon::Vue),
            ("a.tsx", Icon::Tsx),
            ("a.ttf", Icon::Font),
            ("a.pyc", Icon::Binary),
            ("a.srt", Icon::Subtitle),
            ("a.patch", Icon::Diff),
            ("a.ipynb", Icon::Notebook),
            ("a.iso", Icon::Disc),
            ("a.crt", Icon::Cert),
            ("a.bat", Icon::WinScript),
            ("a.svg", Icon::Svg),
            ("a.xml", Icon::Xml),
            ("a.tf", Icon::Terraform),
            ("Dockerfile", Icon::Docker),
            ("Cargo.lock", Icon::LockFile),
            ("package-lock.json", Icon::LockFile),
            (".env.local", Icon::Key),
            (".gitignore", Icon::Git),
            (".zshrc", Icon::Config),
            ("README.md", Icon::Book),
            ("Makefile", Icon::Config),
            ("noext", Icon::File),
            ("node_modules", Icon::Package),
        ];
        for (name, _) in cases {
            if name != "node_modules" {
                fs::write(root.join(name), b"").unwrap();
            }
        }
        fs::write(root.join("tool"), b"").unwrap();
        fs::set_permissions(root.join("tool"), fs::Permissions::from_mode(0o755)).unwrap();
        for (name, expected) in cases.into_iter().chain([("tool", Icon::Shell)]) {
            let path = root.join(name);
            let entry = Entry {
                metadata: fs::symlink_metadata(&path).unwrap(),
                name: name.into(),
                path,
            };
            assert_eq!(classify(&entry), expected, "{name}");
        }
        fs::remove_dir_all(&root).unwrap();
    }
}
