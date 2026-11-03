//! Original bounded recursive text search; no upstream search implementation is used.
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const LINE_LIMIT: usize = 16 * 1024 * 1024;
const WORK_LIMIT: usize = 100_000_000;
const HELP: &str = "Usage: terms search [-F] [-i] [-n] [-l | -c | --files] [--hidden] [--no-ignore] [--glob PATTERN] [--context N] [--null] [--] PATTERN [PATH...]\n--files accepts paths without a pattern. No stdin, symlinks, or special files. ASCII case folding only.\n";

#[derive(Default)]
struct Options {
    fixed: bool,
    insensitive: bool,
    numbers: bool,
    names: bool,
    counts: bool,
    files: bool,
    hidden: bool,
    no_ignore: bool,
    null: bool,
    context: usize,
    globs: Vec<(bool, Glob)>,
    paths: Vec<PathBuf>,
    pattern: Vec<u8>,
}

fn parse(args: &[OsString]) -> Result<Options, String> {
    let mut o = Options::default();
    let mut operands = Vec::new();
    let mut options = true;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let b = arg.as_bytes();
        if options && b == b"--" {
            options = false;
            continue;
        }
        if options && b.starts_with(b"-") && b != b"-" {
            match b {
                b"-F" => o.fixed = true,
                b"-i" => o.insensitive = true,
                b"-n" => o.numbers = true,
                b"-l" => o.names = true,
                b"-c" => o.counts = true,
                b"--files" => o.files = true,
                b"--hidden" => o.hidden = true,
                b"--no-ignore" => o.no_ignore = true,
                b"--null" => o.null = true,
                b"--context" => {
                    let n = it
                        .next()
                        .and_then(|x| x.to_str())
                        .ok_or("missing context size")?;
                    o.context = n.parse().map_err(|_| "invalid context size")?;
                    if o.context > 1000 {
                        return Err("context exceeds 1000 lines".into());
                    }
                }
                b"--glob" => {
                    let g = it.next().ok_or("missing glob")?.as_bytes();
                    let (negative, pat) = if g.starts_with(b"!") {
                        (true, &g[1..])
                    } else {
                        (false, g)
                    };
                    o.globs.push((negative, Glob::new(pat)?));
                }
                _ => return Err("unknown search option".into()),
            }
        } else {
            operands.push(arg.clone());
        }
    }
    if usize::from(o.names) + usize::from(o.counts) + usize::from(o.files) > 1 {
        return Err("conflicting search output modes".into());
    }
    if o.null && !(o.names || o.files) {
        return Err("--null requires -l or --files".into());
    }
    if o.files && (o.fixed || o.insensitive || o.numbers || o.context != 0) {
        return Err("pattern options cannot be used with --files".into());
    }
    if !o.files {
        if operands.is_empty() {
            return Err("search requires a pattern".into());
        }
        o.pattern = operands.remove(0).as_bytes().to_vec();
        if o.pattern.len() > 65536 {
            return Err("pattern exceeds 64 KiB".into());
        }
        if o.pattern.contains(&b'\n') {
            return Err("multiline search is unsupported".into());
        }
    }
    o.paths = operands.into_iter().map(PathBuf::from).collect();
    if o.paths.is_empty() {
        o.paths.push(PathBuf::from("."));
    }
    Ok(o)
}

/// Execute search with byte-safe filename output. Errors never become successful statuses.
pub fn execute(args: &[OsString], out: &mut dyn Write) -> Result<u8, String> {
    if args.len() == 1 && args[0] == "--help" {
        out.write_all(HELP.as_bytes()).map_err(ioerr)?;
        return Ok(0);
    }
    let options = parse(args)?;
    let matcher = if options.files {
        Matcher::Literal(Literal::new(&[], false))
    } else if options.fixed {
        Matcher::Literal(Literal::new(&options.pattern, options.insensitive))
    } else {
        Matcher::Regex(Regex::new(&options.pattern, options.insensitive)?)
    };
    let mut search = Search {
        options: &options,
        matcher,
        out,
        found: false,
        errors: Vec::new(),
    };
    for root in &options.paths {
        if let Err(e) = search.walk(root, root, &[], 0, true) {
            search.errors.push(e);
        }
    }
    if search.errors.is_empty() {
        Ok(if search.found { 0 } else { 1 })
    } else {
        Err(search.errors.join("; "))
    }
}
fn ioerr(e: std::io::Error) -> String {
    format!("search I/O error: {e}")
}
fn patherr(path: &Path, e: impl std::fmt::Display) -> String {
    format!("{}: {e}", escaped(path.as_os_str().as_bytes()))
}

struct Search<'a> {
    options: &'a Options,
    matcher: Matcher,
    out: &'a mut dyn Write,
    found: bool,
    errors: Vec<String>,
}
impl Search<'_> {
    fn walk(
        &mut self,
        path: &Path,
        root: &Path,
        inherited: &[Rule],
        depth: usize,
        explicit: bool,
    ) -> Result<(), String> {
        if depth > 256 {
            return Err(patherr(path, "traversal depth exceeds 256"));
        }
        let meta = fs::symlink_metadata(path).map_err(|e| patherr(path, e))?;
        if meta.file_type().is_symlink() || !(meta.is_dir() || meta.is_file()) {
            return if explicit {
                Err(patherr(path, "symlinks and special files are unsupported"))
            } else {
                Ok(())
            };
        }
        if meta.is_dir() && path.file_name().is_some_and(|name| name == ".git") {
            return Ok(());
        }
        let relative = path.strip_prefix(root).unwrap_or(path);
        if !explicit {
            let name = path.file_name().unwrap_or_default().as_bytes();
            if name == b".git" || (!self.options.hidden && name.starts_with(b".")) {
                return Ok(());
            }
            if ignored(path, meta.is_dir(), inherited)? {
                return Ok(());
            }
        }
        if meta.is_dir() {
            let mut rules = inherited.to_vec();
            if !self.options.no_ignore {
                for filename in [".gitignore", ".ignore"] {
                    let p = path.join(filename);
                    match fs::symlink_metadata(&p) {
                        Ok(m) => {
                            if !m.is_file() || m.file_type().is_symlink() {
                                return Err(patherr(
                                    &p,
                                    "ignore file must be a regular non-symlink file",
                                ));
                            }
                            if m.len() > 1024 * 1024 {
                                return Err(patherr(&p, "ignore file exceeds 1 MiB"));
                            }
                            let mut data = Vec::new();
                            File::open(&p)
                                .map_err(|e| patherr(&p, e))?
                                .take(1024 * 1024 + 1)
                                .read_to_end(&mut data)
                                .map_err(ioerr)?;
                            if data.len() > 1024 * 1024 {
                                return Err(patherr(&p, "ignore file exceeds 1 MiB"));
                            }
                            rules.extend(read_rules(path, &data).map_err(|e| patherr(&p, e))?);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => return Err(patherr(&p, e)),
                    }
                }
            }
            let mut entries = fs::read_dir(path)
                .map_err(|e| patherr(path, e))?
                .take(100_001)
                .collect::<Result<Vec<_>, _>>()
                .map_err(ioerr)?;
            if entries.len() > 100_000 {
                return Err("directory exceeds 100000 entries".into());
            }
            entries.sort_by(|a, b| a.file_name().as_bytes().cmp(b.file_name().as_bytes()));
            for entry in entries {
                if let Err(e) = self.walk(&entry.path(), root, &rules, depth + 1, false) {
                    if self.errors.len() < 100 {
                        self.errors.push(e);
                    }
                }
            }
            return Ok(());
        }
        let rel = if explicit {
            path.file_name().unwrap_or_default().as_bytes()
        } else {
            relative.as_os_str().as_bytes()
        };
        let mut positive = false;
        let mut accepted = false;
        for (negative, glob) in &self.options.globs {
            let m = glob.matches_path(rel)?;
            if *negative && m {
                return Ok(());
            }
            if !negative {
                positive = true;
                accepted |= m;
            }
        }
        if positive && !accepted {
            return Ok(());
        }
        if self.options.files {
            self.emit_path(path)?;
            self.found = true;
            return Ok(());
        }
        self.file(path)
    }
    fn emit_path(&mut self, path: &Path) -> Result<(), String> {
        if self.options.null {
            self.out
                .write_all(path.as_os_str().as_bytes())
                .map_err(ioerr)?;
            self.out.write_all(&[0]).map_err(ioerr)
        } else {
            writeln!(self.out, "{}", escaped(path.as_os_str().as_bytes())).map_err(ioerr)
        }
    }
    fn line(&mut self, path: &Path, n: usize, content: &[u8], matched: bool) -> Result<(), String> {
        let sep = if matched { ':' } else { '-' };
        write!(self.out, "{}{sep}", escaped(path.as_os_str().as_bytes())).map_err(ioerr)?;
        if self.options.numbers {
            write!(self.out, "{n}{sep}").map_err(ioerr)?;
        }
        writeln!(self.out, "{}", escaped(content)).map_err(ioerr)
    }
    fn file(&mut self, path: &Path) -> Result<(), String> {
        let mut file = File::open(path).map_err(|e| patherr(path, e))?;
        let before = file.metadata().map_err(ioerr)?;
        if !before.is_file() {
            return Err(patherr(path, "opened input is not a regular file"));
        }
        let mut binary = false;
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer).map_err(ioerr)?;
            if n == 0 {
                break;
            }
            binary |= buffer[..n].contains(&0);
        }
        if changed(&before, &file.metadata().map_err(ioerr)?) {
            return Err(patherr(path, "file changed during classification"));
        }
        if binary {
            return Ok(());
        }
        file.seek(SeekFrom::Start(0)).map_err(ioerr)?;
        let mut reader = BufReader::new(file);
        let mut line = Vec::new();
        let mut number = 0usize;
        let mut count = 0usize;
        let mut work = 0usize;
        let mut pending: VecDeque<(usize, Vec<u8>)> = VecDeque::new();
        let mut last = 0usize;
        let mut after = 0usize;
        loop {
            line.clear();
            let n = reader
                .by_ref()
                .take((LINE_LIMIT + 2) as u64)
                .read_until(b'\n', &mut line)
                .map_err(ioerr)?;
            if n == 0 {
                break;
            }
            if line.last() == Some(&b'\n') {
                line.pop();
            }
            if line.len() > LINE_LIMIT {
                return Err(patherr(path, "line exceeds 16 MiB"));
            }
            if line.contains(&0) {
                return Err(patherr(path, "file changed to binary during matching"));
            }
            number += 1;
            let matched = self.matcher.matches(&line, &mut work)?;
            if matched {
                count += 1;
                self.found = true;
            }
            if self.options.names || self.options.counts {
                continue;
            }
            if matched {
                let first = pending.front().map_or(number, |x| x.0);
                if last != 0 && first > last + 1 && self.options.context > 0 {
                    writeln!(self.out, "--").map_err(ioerr)?;
                }
                for (pn, pl) in pending.drain(..) {
                    if pn > last {
                        self.line(path, pn, &pl, false)?;
                        last = pn;
                    }
                }
                self.line(path, number, &line, true)?;
                last = number;
                after = self.options.context;
            } else if after > 0 {
                self.line(path, number, &line, false)?;
                last = number;
                after -= 1;
            } else if self.options.context > 0 {
                if pending
                    .iter()
                    .map(|(_, line)| line.len())
                    .sum::<usize>()
                    .saturating_add(line.len())
                    > 64 * 1024 * 1024
                {
                    return Err("search context exceeds 64 MiB".into());
                }
                pending.push_back((number, line.clone()));
                while pending.len() > self.options.context {
                    pending.pop_front();
                }
            }
        }
        if changed(&before, &reader.get_ref().metadata().map_err(ioerr)?) {
            return Err(patherr(path, "file changed during matching"));
        }
        if self.options.names && count > 0 {
            self.emit_path(path)?;
        }
        if self.options.counts {
            writeln!(self.out, "{}:{count}", escaped(path.as_os_str().as_bytes()))
                .map_err(ioerr)?;
        }
        Ok(())
    }
}
fn changed(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() != b.dev()
        || a.ino() != b.ino()
        || a.len() != b.len()
        || a.mtime() != b.mtime()
        || a.mtime_nsec() != b.mtime_nsec()
        || a.ctime() != b.ctime()
        || a.ctime_nsec() != b.ctime_nsec()
}
fn escaped(bytes: &[u8]) -> String {
    let mut result = String::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                escape_valid(s, &mut result);
                break;
            }
            Err(e) => {
                let (valid, tail) = rest.split_at(e.valid_up_to());
                if let Ok(s) = std::str::from_utf8(valid) {
                    escape_valid(s, &mut result);
                }
                let n = e.error_len().unwrap_or(tail.len());
                for byte in &tail[..n] {
                    result.push_str(&format!("\\x{byte:02x}"));
                }
                rest = &tail[n..];
            }
        }
    }
    result
}
fn escape_valid(s: &str, out: &mut String) {
    for c in s.chars() {
        if c.is_control()
            || matches!(c,'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
        {
            out.push_str(&format!("\\u{{{:x}}}", u32::from(c)));
        } else {
            out.push(c);
        }
    }
}

#[derive(Clone)]
struct Rule {
    base: PathBuf,
    negative: bool,
    directory: bool,
    glob: Glob,
}
fn read_rules(base: &Path, data: &[u8]) -> Result<Vec<Rule>, String> {
    let mut rules = Vec::new();
    for raw in data.split(|b| *b == b'\n') {
        let mut line = raw.strip_suffix(b"\r").unwrap_or(raw);
        while line.last() == Some(&b' ') {
            let n = line.len();
            let escapes = line[..n - 1]
                .iter()
                .rev()
                .take_while(|b| **b == b'\\')
                .count();
            if escapes % 2 == 1 {
                break;
            }
            line = &line[..n - 1];
        }
        if line.is_empty() || line[0] == b'#' {
            continue;
        }
        let negative = line[0] == b'!';
        if negative {
            line = &line[1..];
        }
        if line.is_empty() {
            return Err("empty ignore negation".into());
        }
        let directory = line.ends_with(b"/");
        if directory {
            line = &line[..line.len() - 1];
        }
        rules.push(Rule {
            base: base.to_path_buf(),
            negative,
            directory,
            glob: Glob::new(line)?,
        });
    }
    Ok(rules)
}
fn ignored(path: &Path, directory: bool, rules: &[Rule]) -> Result<bool, String> {
    let mut excluded = false;
    for rule in rules {
        if rule.directory && !directory {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(&rule.base) {
            if rule.glob.matches_path(rel.as_os_str().as_bytes())? {
                excluded = !rule.negative;
            }
        }
    }
    Ok(excluded)
}
#[derive(Clone)]
struct Glob {
    tokens: Vec<Token>,
    basename: bool,
}
#[derive(Clone)]
enum Token {
    Byte(u8),
    Any,
    Star,
    Cross,
    CrossSlash,
    Class(Box<[bool; 256]>),
}
impl Glob {
    fn new(pattern: &[u8]) -> Result<Self, String> {
        if pattern.is_empty() || pattern.len() > 65536 {
            return Err("glob must contain 1..65536 bytes".into());
        }
        let anchored = pattern.starts_with(b"/");
        let pattern = if anchored { &pattern[1..] } else { pattern };
        let basename = !anchored && !pattern.contains(&b'/');
        let mut tokens = Vec::new();
        let mut i = 0;
        while i < pattern.len() {
            match pattern[i] {
                b'\\' => {
                    i += 1;
                    let b = *pattern.get(i).ok_or("trailing glob escape")?;
                    tokens.push(Token::Byte(b));
                    i += 1;
                }
                b'?' => {
                    tokens.push(Token::Any);
                    i += 1;
                }
                b'*' => {
                    if pattern.get(i + 1) == Some(&b'*') {
                        if (i > 0 && pattern[i - 1] != b'/')
                            || (i + 2 < pattern.len() && pattern[i + 2] != b'/')
                        {
                            return Err("** must occupy a whole path component".into());
                        }
                        i += 2;
                        if pattern.get(i) == Some(&b'/') {
                            tokens.push(Token::CrossSlash);
                            i += 1;
                        } else {
                            tokens.push(Token::Cross);
                        }
                    } else {
                        tokens.push(Token::Star);
                        i += 1;
                    }
                }
                b'[' => {
                    let (class, next) = class(pattern, i, false, false)?;
                    tokens.push(Token::Class(Box::new(class)));
                    i = next;
                }
                b => {
                    tokens.push(Token::Byte(b));
                    i += 1;
                }
            }
        }
        Ok(Self { tokens, basename })
    }
    fn matches_path(&self, path: &[u8]) -> Result<bool, String> {
        if self.basename {
            self.matches(path.rsplit(|b| *b == b'/').next().unwrap_or(path))
        } else {
            self.matches(path)
        }
    }
    fn matches(&self, path: &[u8]) -> Result<bool, String> {
        if self
            .tokens
            .len()
            .saturating_mul(path.len().saturating_add(1))
            > 10_000_000
        {
            return Err("glob execution budget exceeded".into());
        }
        let mut current = vec![false; self.tokens.len() + 1];
        current[0] = true;
        for pos in 0..=path.len() {
            for (i, tok) in self.tokens.iter().enumerate() {
                if current[i]
                    && (matches!(tok, Token::Star | Token::Cross)
                        || matches!(tok, Token::CrossSlash) && (pos == 0 || path[pos - 1] == b'/'))
                {
                    current[i + 1] = true;
                }
            }
            if pos == path.len() {
                return Ok(current[self.tokens.len()]);
            }
            let b = path[pos];
            let mut next = vec![false; current.len()];
            for (i, tok) in self.tokens.iter().enumerate() {
                if !current[i] {
                    continue;
                }
                match tok {
                    Token::Byte(x) if *x == b => next[i + 1] = true,
                    Token::Any if b != b'/' => next[i + 1] = true,
                    Token::Class(c) if b != b'/' && c[b as usize] => next[i + 1] = true,
                    Token::Star if b != b'/' => next[i] = true,
                    Token::Cross | Token::CrossSlash => next[i] = true,
                    _ => {}
                }
            }
            current = next;
        }
        Ok(false)
    }
}
fn class(
    pattern: &[u8],
    start: usize,
    regex: bool,
    fold: bool,
) -> Result<([bool; 256], usize), String> {
    let mut result = [false; 256];
    let mut i = start + 1;
    let negative =
        matches!(pattern.get(i), Some(b'^')) || (!regex && pattern.get(i) == Some(&b'!'));
    if negative {
        i += 1;
    }
    let begin = i;
    while i < pattern.len() && pattern[i] != b']' {
        let left = class_byte(pattern, &mut i, regex)?;
        if pattern.get(i) == Some(&b'-') && pattern.get(i + 1).is_some_and(|b| *b != b']') {
            i += 1;
            let right = class_byte(pattern, &mut i, regex)?;
            if left > right {
                return Err("reversed character range".into());
            }
            for b in left..=right {
                result[b as usize] = true;
            }
        } else {
            result[left as usize] = true;
        }
    }
    if i == begin || pattern.get(i) != Some(&b']') {
        return Err("unterminated or empty character class".into());
    }
    if fold {
        for b in b'a'..=b'z' {
            let value = result[b as usize] || result[b.to_ascii_uppercase() as usize];
            result[b as usize] = value;
            result[b.to_ascii_uppercase() as usize] = value;
        }
    }
    if negative {
        for b in &mut result {
            *b = !*b;
        }
    }
    Ok((result, i + 1))
}
fn class_byte(p: &[u8], i: &mut usize, regex: bool) -> Result<u8, String> {
    let b = *p.get(*i).ok_or("unterminated class")?;
    *i += 1;
    let b = if b == b'\\' {
        let b = *p.get(*i).ok_or("trailing escape")?;
        *i += 1;
        if regex && !b"\\.^$|?*+()[]{}-".contains(&b) {
            return Err("unsupported regex escape".into());
        }
        b
    } else {
        b
    };
    if regex && !b.is_ascii() {
        return Err("regex classes support ASCII only".into());
    }
    Ok(b)
}

enum Matcher {
    Literal(Literal),
    Regex(Regex),
}
impl Matcher {
    fn matches(&self, line: &[u8], work: &mut usize) -> Result<bool, String> {
        match self {
            Self::Literal(l) => Ok(l.matches(line)),
            Self::Regex(r) => r.matches(line, work),
        }
    }
}
struct Literal {
    pattern: Vec<u8>,
    table: Vec<usize>,
    fold: bool,
}
impl Literal {
    fn new(pattern: &[u8], fold: bool) -> Self {
        let p: Vec<_> = pattern
            .iter()
            .map(|b| if fold { b.to_ascii_lowercase() } else { *b })
            .collect();
        let mut table = vec![0; p.len()];
        let mut n = 0;
        for i in 1..p.len() {
            while n > 0 && p[i] != p[n] {
                n = table[n - 1];
            }
            if p[i] == p[n] {
                n += 1;
            }
            table[i] = n;
        }
        Self {
            pattern: p,
            table,
            fold,
        }
    }
    fn matches(&self, line: &[u8]) -> bool {
        if self.pattern.is_empty() {
            return true;
        }
        let mut n = 0;
        for b in line {
            let b = if self.fold {
                b.to_ascii_lowercase()
            } else {
                *b
            };
            while n > 0 && b != self.pattern[n] {
                n = self.table[n - 1];
            }
            if b == self.pattern[n] {
                n += 1;
            }
            if n == self.pattern.len() {
                return true;
            }
        }
        false
    }
}
#[derive(Debug)]
enum Node {
    Empty,
    Byte(u8),
    Any,
    Start,
    End,
    Class(Box<[bool; 256]>),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Repeat(Box<Node>, u8),
}
struct Parser<'a> {
    p: &'a [u8],
    i: usize,
    fold: bool,
}
impl Parser<'_> {
    fn expression(&mut self, depth: usize) -> Result<Node, String> {
        if depth > 128 {
            return Err("regex nesting exceeds 128".into());
        }
        let mut branches = vec![self.sequence(depth)?];
        while self.p.get(self.i) == Some(&b'|') {
            self.i += 1;
            branches.push(self.sequence(depth)?);
        }
        Ok(if branches.len() == 1 {
            branches.remove(0)
        } else {
            Node::Alt(branches)
        })
    }
    fn sequence(&mut self, depth: usize) -> Result<Node, String> {
        let mut seq = Vec::new();
        while self.i < self.p.len() && !matches!(self.p[self.i], b'|' | b')') {
            let b = self.p[self.i];
            self.i += 1;
            let mut node = match b {
                b'(' => {
                    let n = self.expression(depth + 1)?;
                    if self.p.get(self.i) != Some(&b')') {
                        return Err("unclosed regex group".into());
                    }
                    self.i += 1;
                    n
                }
                b'[' => {
                    let (c, i) = class(self.p, self.i - 1, true, self.fold)?;
                    self.i = i;
                    Node::Class(Box::new(c))
                }
                b'.' => Node::Any,
                b'^' => Node::Start,
                b'$' => Node::End,
                b'\\' => {
                    let b = *self.p.get(self.i).ok_or("trailing regex escape")?;
                    self.i += 1;
                    if !b"\\.^$|?*+()[]{}-".contains(&b) {
                        return Err("unsupported regex escape".into());
                    }
                    Node::Byte(b)
                }
                b'*' | b'+' | b'?' | b'{' | b'}' | b']' => {
                    return Err("unsupported regex syntax or misplaced quantifier".into());
                }
                _ => Node::Byte(b),
            };
            if let Some(q @ (b'*' | b'+' | b'?')) = self.p.get(self.i) {
                let q = *q;
                self.i += 1;
                if matches!(node, Node::Start | Node::End) {
                    return Err("cannot repeat an anchor".into());
                }
                node = Node::Repeat(Box::new(node), q);
            }
            seq.push(node);
        }
        Ok(if seq.is_empty() {
            Node::Empty
        } else if seq.len() == 1 {
            seq.remove(0)
        } else {
            Node::Seq(seq)
        })
    }
}
enum State {
    Byte(u8, usize),
    Any(usize),
    Class(Box<[bool; 256]>, usize),
    Start(usize),
    End(usize),
    Split(usize, usize),
    Match,
}
struct Regex {
    states: Vec<State>,
    start: usize,
    fold: bool,
}
impl Regex {
    fn new(pattern: &[u8], fold: bool) -> Result<Self, String> {
        let mut p = Parser {
            p: pattern,
            i: 0,
            fold,
        };
        let node = p.expression(0)?;
        if p.i != pattern.len() {
            return Err("unexpected regex closing group".into());
        }
        let mut r = Self {
            states: vec![State::Match],
            start: 0,
            fold,
        };
        r.start = r.compile(&node, 0)?;
        Ok(r)
    }
    fn push(&mut self, s: State) -> Result<usize, String> {
        if self.states.len() >= 16384 {
            return Err("regex exceeds 16384 states".into());
        }
        let i = self.states.len();
        self.states.push(s);
        Ok(i)
    }
    fn compile(&mut self, n: &Node, next: usize) -> Result<usize, String> {
        match n {
            Node::Empty => Ok(next),
            Node::Byte(b) => self.push(State::Byte(*b, next)),
            Node::Any => self.push(State::Any(next)),
            Node::Start => self.push(State::Start(next)),
            Node::End => self.push(State::End(next)),
            Node::Class(c) => self.push(State::Class(c.clone(), next)),
            Node::Seq(seq) => {
                let mut start = next;
                for n in seq.iter().rev() {
                    start = self.compile(n, start)?;
                }
                Ok(start)
            }
            Node::Alt(branches) => {
                let mut start = self.compile(&branches[0], next)?;
                for n in &branches[1..] {
                    let other = self.compile(n, next)?;
                    start = self.push(State::Split(start, other))?;
                }
                Ok(start)
            }
            Node::Repeat(n, q) => match q {
                b'?' => {
                    let start = self.compile(n, next)?;
                    self.push(State::Split(start, next))
                }
                b'*' | b'+' => {
                    let split = self.push(State::Split(next, next))?;
                    let start = self.compile(n, split)?;
                    self.states[split] = State::Split(start, next);
                    Ok(if *q == b'*' { split } else { start })
                }
                _ => Err("invalid internal quantifier".into()),
            },
        }
    }
    fn matches(&self, line: &[u8], work: &mut usize) -> Result<bool, String> {
        let mut current = Vec::new();
        let mut seeds = Vec::new();
        let mut visited = vec![0usize; self.states.len()];
        let mut generation = 0;
        for pos in 0..=line.len() {
            generation += 1;
            seeds.push(self.start);
            current.clear();
            while let Some(i) = seeds.pop() {
                if visited[i] == generation {
                    continue;
                }
                visited[i] = generation;
                *work += 1;
                if *work > WORK_LIMIT {
                    return Err("regex execution budget exceeded".into());
                }
                match self.states[i] {
                    State::Match => return Ok(true),
                    State::Split(a, b) => {
                        seeds.push(a);
                        seeds.push(b);
                    }
                    State::Start(n) if pos == 0 => seeds.push(n),
                    State::End(n) if pos == line.len() => seeds.push(n),
                    State::Byte(..) | State::Any(..) | State::Class(..) => current.push(i),
                    _ => {}
                }
            }
            if pos == line.len() {
                break;
            }
            let byte = line[pos];
            for i in &current {
                match &self.states[*i] {
                    State::Byte(b, n)
                        if if self.fold {
                            b.eq_ignore_ascii_case(&byte)
                        } else {
                            *b == byte
                        } =>
                    {
                        seeds.push(*n)
                    }
                    State::Any(n) if byte != b'\n' => seeds.push(*n),
                    State::Class(c, n) if c[byte as usize] => seeds.push(*n),
                    _ => {}
                }
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;
    #[test]
    fn folded_negative_class() {
        let regex = Regex::new(b"[^a]", true).unwrap();
        assert!(!regex.matches(b"A", &mut 0).unwrap());
        assert!(regex.matches(b"B", &mut 0).unwrap());
    }
    #[test]
    fn filesystem_binary_ignore_and_context() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("terms-search-test-{}-{unique}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join(".ignore"), b"ignored\n").unwrap();
        fs::write(root.join("ignored"), b"match\n").unwrap();
        fs::write(root.join("binary"), b"match\nlate\0").unwrap();
        fs::write(root.join("text"), b"before\nmatch\nafter\n").unwrap();
        let mut out = Vec::new();
        let args = vec![
            OsString::from("-F"),
            OsString::from("-n"),
            OsString::from("--context"),
            OsString::from("1"),
            OsString::from("match"),
            root.as_os_str().to_owned(),
        ];
        assert_eq!(execute(&args, &mut out).unwrap(), 0);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("text-1-before"));
        assert!(text.contains("text:2:match"));
        assert!(text.contains("text-3-after"));
        assert!(!text.contains("ignored"));
        assert!(!text.contains("binary"));
        let name = OsString::from_vec(b"name\n".to_vec());
        fs::write(root.join(&name), b"match").unwrap();
        let mut out = Vec::new();
        execute(
            &[
                OsString::from("-F"),
                OsString::from("-l"),
                OsString::from("--null"),
                OsString::from("match"),
                root.join(&name).into_os_string(),
            ],
            &mut out,
        )
        .unwrap();
        assert!(out.ends_with(b"name\n\0"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn regex_engine() {
        for (p, line, want) in [
            ("a(b|c)+d?", "zzabcb", true),
            ("^ab$", "xab", false),
            ("^ab$", "ab", true),
            ("a*", "z", true),
            ("[^a]+", "aab", true),
            ("x?y", "y", true),
            ("a(b|c)*d", "abcbcd", true),
        ] {
            assert_eq!(
                Regex::new(p.as_bytes(), false)
                    .unwrap()
                    .matches(line.as_bytes(), &mut 0)
                    .unwrap(),
                want,
                "{p}"
            );
        }
        for p in ["(?=x)", "a{2}", "a*?", "\\w", "[z-a]", "(", "a)"] {
            assert!(Regex::new(p.as_bytes(), false).is_err(), "{p}");
        }
    }
    #[test]
    fn globs() {
        for (p, path, want) in [
            ("*.rs", "sub/a.rs", true),
            ("/a", "sub/a", false),
            ("**/a", "a", true),
            ("**/a", "sub/a", true),
            ("a/**/b", "a/b", true),
            ("a/**/b", "a/x/y/b", true),
            ("a/*/b", "a/x/y/b", false),
            ("[a-z].rs", "a.rs", true),
        ] {
            assert_eq!(
                Glob::new(p.as_bytes())
                    .unwrap()
                    .matches_path(path.as_bytes())
                    .unwrap(),
                want,
                "{p} {path}"
            );
        }
    }
    #[test]
    fn ignore_order() {
        let rules = read_rules(Path::new("root"), b"*.tmp\n!important.tmp\n/sub/\n").unwrap();
        assert!(ignored(Path::new("root/a.tmp"), false, &rules).unwrap());
        assert!(!ignored(Path::new("root/important.tmp"), false, &rules).unwrap());
        assert!(ignored(Path::new("root/sub"), true, &rules).unwrap());
    }
    #[test]
    fn literal_and_escaping() {
        assert!(Literal::new(b"ababaca", false).matches(b"xxabababacazz"));
        assert!(Literal::new(b"HELLO", true).matches(b"Hello"));
        assert_eq!(escaped(b"a\x1b\xff"), "a\\u{1b}\\xff");
    }
}
