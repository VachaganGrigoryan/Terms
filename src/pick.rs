//! Independent byte-preserving selector.
use crate::native::Terminal;
use std::collections::{BTreeSet, VecDeque};
use std::ffi::OsString;
use std::io::{IsTerminal, Read, Write};
const TOTAL: usize = 64 * 1024 * 1024;
const RECORD: usize = 16 * 1024 * 1024;
fn escaped(v: &[u8]) -> String {
    v.iter()
        .map(|b| {
            if (32..127).contains(b) {
                (*b as char).to_string()
            } else {
                format!("\\x{b:02x}")
            }
        })
        .collect()
}
/// Injectable terminal/readiness boundary; tests need no controlling terminal.
pub trait InteractiveRuntime {
    fn write(&mut self, bytes: &[u8]) -> Result<(), String>;
    fn dimensions(&self) -> (usize, usize);
    fn wait(
        &mut self,
        candidate_input: bool,
        matching_pending: bool,
    ) -> Result<(bool, bool, bool), String>;
    fn key(&mut self) -> Result<Vec<u8>, String>;
    fn suspend(&mut self);
}
impl InteractiveRuntime for Terminal {
    fn write(&mut self, b: &[u8]) -> Result<(), String> {
        Terminal::write(self, b)
    }
    fn dimensions(&self) -> (usize, usize) {
        Terminal::dimensions(self)
    }
    fn wait(&mut self, input: bool, busy: bool) -> Result<(bool, bool, bool), String> {
        Terminal::wait(self, input, busy)
    }
    fn key(&mut self) -> Result<Vec<u8>, String> {
        Terminal::key(self)
    }
    fn suspend(&mut self) {
        Terminal::suspend(self)
    }
}
type Rank = (usize, usize, usize, usize);
struct Matching {
    query: Vec<u8>,
    prefix: Vec<usize>,
    fold: bool,
    exact: bool,
    preserve: bool,
    record: usize,
    position: usize,
    matched: usize,
    first: usize,
    ranked: BTreeSet<Rank>,
}
impl Matching {
    fn new(query: &str, exact: bool, preserve: bool) -> Self {
        let query = query.as_bytes().to_vec();
        let fold = !query.iter().any(u8::is_ascii_uppercase);
        let eq = |a: u8, b: u8| {
            if fold {
                a.eq_ignore_ascii_case(&b)
            } else {
                a == b
            }
        };
        let mut prefix = vec![0; query.len()];
        for i in 1..query.len() {
            let mut j = prefix[i - 1];
            while j > 0 && !eq(query[i], query[j]) {
                j = prefix[j - 1];
            }
            if eq(query[i], query[j]) {
                j += 1;
            }
            prefix[i] = j;
        }
        Self {
            query,
            prefix,
            fold,
            exact,
            preserve,
            record: 0,
            position: 0,
            matched: 0,
            first: 0,
            ranked: BTreeSet::new(),
        }
    }
    fn finish(&mut self, len: usize, result: Option<(usize, usize)>) {
        if let Some((gap, start)) = result {
            self.ranked.insert(if self.preserve {
                (0, 0, 0, self.record)
            } else {
                (gap, start, len, self.record)
            });
        }
        self.record += 1;
        self.position = 0;
        self.matched = 0;
        self.first = 0;
    }
    // Bound each scheduling quantum by both bytes and records. State resumes inside
    // a 16 MiB record, so a single pathological record cannot monopolize the UI.
    fn advance(&mut self, records: &[Vec<u8>]) {
        let (mut work, mut finished) = (0, 0);
        while self.record < records.len() && work < 65536 && finished < 256 {
            let record = &records[self.record];
            if self.query.is_empty() {
                self.finish(record.len(), Some((0, 0)));
                finished += 1;
                continue;
            }
            if self.position == record.len() {
                self.finish(record.len(), None);
                finished += 1;
                continue;
            }
            let byte = record[self.position];
            let eq = |a: u8, b: u8| {
                if self.fold {
                    a.eq_ignore_ascii_case(&b)
                } else {
                    a == b
                }
            };
            if self.exact {
                while self.matched > 0 && !eq(byte, self.query[self.matched]) {
                    self.matched = self.prefix[self.matched - 1];
                }
                if eq(byte, self.query[self.matched]) {
                    self.matched += 1;
                }
            } else if eq(byte, self.query[self.matched]) {
                if self.matched == 0 {
                    self.first = self.position;
                }
                self.matched += 1;
            }
            self.position += 1;
            work += 1;
            if self.matched == self.query.len() {
                let result = if self.exact {
                    (0, self.position - self.query.len())
                } else {
                    (self.position - self.first - self.query.len(), self.first)
                };
                self.finish(record.len(), Some(result));
                finished += 1;
            }
        }
    }
    fn visible(&self) -> Vec<usize> {
        self.ranked.iter().map(|r| r.3).collect()
    }
}
#[cfg(test)]
fn matches(records: &[Vec<u8>], query: &str, exact: bool, preserve: bool) -> Vec<usize> {
    let mut matching = Matching::new(query, exact, preserve);
    while matching.record < records.len() {
        matching.advance(records);
    }
    matching.visible()
}
struct Settings {
    exact: bool,
    multi: bool,
    read0: bool,
    print0: bool,
    hex: bool,
    query: String,
    preserve: bool,
}
fn select(
    records: Vec<Vec<u8>>,
    s: Settings,
    source: Option<&mut dyn Read>,
) -> Result<Option<Vec<Vec<u8>>>, String> {
    if source.is_some() && std::io::stdin().is_terminal() {
        return Err(
            "pick requires piped candidates; try: printf 'apple\\nbanana\\n' | terms pick".into(),
        );
    }
    let mut terminal = Terminal::open()?;
    select_with_runtime(records, s, source, &mut terminal)
}
fn select_with_runtime(
    mut records: Vec<Vec<u8>>,
    s: Settings,
    mut source: Option<&mut dyn Read>,
    terminal: &mut dyn InteractiveRuntime,
) -> Result<Option<Vec<Vec<u8>>>, String> {
    let mut query = s.query;
    let mut selected = vec![false; records.len()];
    let mut pending = Vec::new();
    let mut total: usize = records.iter().map(Vec::len).sum();
    if total > TOTAL || records.len() > 100_000 || records.iter().any(|r| r.len() > RECORD) {
        return Err("candidate limit exceeded".into());
    }
    let mut cursor = 0;
    let mut escape = Vec::new();
    let mut utf8 = Vec::new();
    let mut queued_keys = VecDeque::new();
    let mut escape_started = std::time::Instant::now();
    let mut matching = Matching::new(&query, s.exact, s.preserve);
    loop {
        if matching.query != query.as_bytes() {
            matching = Matching::new(&query, s.exact, s.preserve);
        }
        matching.advance(&records);
        let visible = matching.visible();
        cursor = cursor.min(visible.len().saturating_sub(1));
        let (rows, columns) = terminal.dimensions();
        let mut lines = vec![format!("> {}", escaped(query.as_bytes()))];
        if rows > 1 {
            lines.push(format!(
                "{} matches / {} records",
                visible.len(),
                records.len()
            ));
        }
        let start = cursor.saturating_sub(rows.saturating_sub(3));
        for (position, &id) in visible
            .iter()
            .enumerate()
            .skip(start)
            .take(rows.saturating_sub(2))
        {
            lines.push(format!(
                "{}{} {}",
                if position == cursor { ">" } else { " " },
                if selected[id] { "*" } else { " " },
                escaped(&records[id][..records[id].len().min(columns)])
            ));
        }
        let width = columns.saturating_sub(1);
        for line in &mut lines {
            line.truncate(line.len().min(width));
        }
        let screen = format!("\x1b[H\x1b[2J{}", lines.join("\r\n"));
        terminal.write(screen.as_bytes())?;
        if source.is_none() && records.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let (keys, data, cancel) = terminal.wait(
            source.is_some() && queued_keys.is_empty(),
            matching.record < records.len() || !queued_keys.is_empty(),
        )?;
        if cancel {
            return Ok(None);
        }
        if data {
            let mut buf = [0; 8192];
            let n = source
                .as_mut()
                .expect("source readiness")
                .read(&mut buf)
                .map_err(|_| "candidate read failed")?;
            if n == 0 {
                if !pending.is_empty() {
                    records.push(std::mem::take(&mut pending));
                    selected.push(false);
                }
                if records.len() > 100_000 {
                    return Err("too many candidates".into());
                }
                source = None;
            } else {
                for &b in &buf[..n] {
                    total += 1;
                    if total > TOTAL {
                        return Err("candidate bytes exceed 64 MiB".into());
                    }
                    if b == if s.read0 { 0 } else { b'\n' } {
                        records.push(std::mem::take(&mut pending));
                        selected.push(false);
                        if records.len() > 100_000 {
                            return Err("too many candidates".into());
                        }
                    } else {
                        pending.push(b);
                        if pending.len() > RECORD {
                            return Err("candidate exceeds 16 MiB".into());
                        }
                    }
                }
            }
        }
        if keys {
            queued_keys.extend(terminal.key()?);
            if queued_keys.len() > 8192 {
                return Err("terminal input queue exceeds 8 KiB".into());
            }
        }
        if queued_keys.contains(&3) {
            return Ok(None);
        }
        let defer_action = queued_keys
            .front()
            .is_some_and(|b| matches!(b, b'\r' | b'\n' | 9 | 14 | 16))
            && matching.record < records.len();
        if !queued_keys.is_empty() && !defer_action {
            for b in [queued_keys.pop_front().expect("nonempty key queue")] {
                if !escape.is_empty() {
                    escape.push(b);
                    if escape == b"\x1b[A" || escape == b"\x1bOA" {
                        cursor = cursor.saturating_sub(1);
                        escape.clear();
                    } else if escape == b"\x1b[B" || escape == b"\x1bOB" {
                        cursor = (cursor + 1).min(visible.len().saturating_sub(1));
                        escape.clear();
                    } else if escape.len() > 2 && !matches!(escape[1], b'[' | b'O')
                        || escape.len() > 3
                    {
                        escape.clear();
                    }
                    continue;
                }
                match b {
                    3 => return Ok(None),
                    26 => terminal.suspend(),
                    27 => {
                        escape.push(b);
                        escape_started = std::time::Instant::now();
                    }
                    b'\r' | b'\n' => {
                        let mut ids: Vec<_> = selected
                            .iter()
                            .enumerate()
                            .filter_map(|(i, &s)| s.then_some(i))
                            .collect();
                        if ids.is_empty() {
                            if let Some(&id) = visible.get(cursor) {
                                ids.push(id);
                            }
                        }
                        return Ok(Some(ids.into_iter().map(|i| records[i].clone()).collect()));
                    }
                    14 => cursor = (cursor + 1).min(visible.len().saturating_sub(1)),
                    16 => cursor = cursor.saturating_sub(1),
                    9 if s.multi => {
                        if let Some(&id) = visible.get(cursor) {
                            selected[id] = !selected[id];
                        }
                    }
                    21 => {
                        query.clear();
                        utf8.clear();
                        cursor = 0;
                    }
                    8 | 127 => {
                        query.pop();
                        utf8.clear();
                        cursor = 0;
                    }
                    b if b >= 32 => {
                        utf8.push(b);
                        if query.len() + utf8.len() > 4096 {
                            return Err("query exceeds 4 KiB".into());
                        }
                        match std::str::from_utf8(&utf8) {
                            Ok(text) => {
                                query.push_str(text);
                                utf8.clear();
                                cursor = 0;
                            }
                            Err(e) if e.error_len().is_some() => utf8.clear(),
                            Err(_) => {}
                        }
                    }
                    _ => {}
                }
            }
        } else if queued_keys.is_empty()
            && escape == b"\x1b"
            && escape_started.elapsed() >= std::time::Duration::from_millis(50)
        {
            return Ok(None);
        }
    }
}
/// Open a selector over already-ranked directory candidates.
pub fn choose(
    records: Vec<Vec<u8>>,
    query: &str,
    multi: bool,
    preserve_order: bool,
) -> Result<Option<Vec<Vec<u8>>>, String> {
    select(
        records,
        Settings {
            exact: false,
            multi,
            read0: true,
            print0: true,
            hex: false,
            query: query.into(),
            preserve: preserve_order,
        },
        None,
    )
}
/// Execute the interactive selector; input is consumed only after readiness notifications.
pub fn execute(args: &[OsString], input: &mut dyn Read, out: &mut dyn Write) -> Result<u8, String> {
    execute_internal(args, input, out, None)
}
/// Execute with a caller-supplied readiness/UI implementation.
pub fn execute_with_runtime(
    args: &[OsString],
    input: &mut dyn Read,
    out: &mut dyn Write,
    runtime: &mut dyn InteractiveRuntime,
) -> Result<u8, String> {
    execute_internal(args, input, out, Some(runtime))
}
fn execute_internal(
    args: &[OsString],
    input: &mut dyn Read,
    out: &mut dyn Write,
    runtime: Option<&mut dyn InteractiveRuntime>,
) -> Result<u8, String> {
    let mut s = Settings {
        exact: false,
        multi: false,
        read0: false,
        print0: false,
        hex: false,
        query: String::new(),
        preserve: false,
    };
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        match a.to_str() {
            Some("--exact") => s.exact = true,
            Some("--multi") => s.multi = true,
            Some("--read0") => s.read0 = true,
            Some("--print0") => s.print0 = true,
            Some("--hex") => s.hex = true,
            Some("--query") => {
                s.query = iter
                    .next()
                    .and_then(|a| a.to_str())
                    .ok_or("missing UTF-8 query")?
                    .into()
            }
            _ => return Err("unknown pick option".into()),
        }
    }
    if s.query.len() > 4096 {
        return Err("query exceeds 4 KiB".into());
    }
    if s.hex && s.print0 {
        return Err("--hex conflicts with --print0".into());
    }
    let (hex, zero) = (s.hex, s.print0);
    let result = if let Some(runtime) = runtime {
        select_with_runtime(Vec::new(), s, Some(input), runtime)
    } else {
        select(Vec::new(), s, Some(input))
    };
    let Some(records) = result? else {
        return Ok(130);
    };
    if records.is_empty() {
        return Ok(1);
    }
    if hex && records.iter().map(Vec::len).sum::<usize>() > 1024 * 1024 {
        return Err("shell output exceeds 1 MiB".into());
    }
    for record in records {
        if hex {
            for b in record {
                write!(out, "{b:02x}").map_err(|_| "selection write failed")?;
            }
            out.write_all(b"\n").map_err(|_| "selection write failed")?;
        } else {
            out.write_all(&record)
                .and_then(|_| out.write_all(if zero { b"\0" } else { b"\n" }))
                .map_err(|_| "selection write failed")?;
        }
    }
    Ok(0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matching() {
        assert_eq!(matches(&[b"alpha".to_vec()], "ap", false, false), vec![0]);
        assert!(matches(&[b"alpha".to_vec()], "ap", true, false).is_empty());
        assert!(matches(&[b"alpha".to_vec()], "A", false, false).is_empty());
        assert_eq!(
            matches(&[b"ab".to_vec(), b"a_b".to_vec()], "ab", false, false),
            vec![0, 1]
        );
    }
    #[test]
    fn display() {
        assert_eq!(escaped(b"a\n\xff"), "a\\x0a\\xff");
    }

    struct FakeRuntime {
        steps: usize,
        keys: Vec<u8>,
        cancel_at: Option<usize>,
        dimensions: (usize, usize),
    }
    impl InteractiveRuntime for FakeRuntime {
        fn write(&mut self, _: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn dimensions(&self) -> (usize, usize) {
            self.dimensions
        }
        fn wait(&mut self, input: bool, _: bool) -> Result<(bool, bool, bool), String> {
            self.steps += 1;
            Ok((!input, input, self.cancel_at == Some(self.steps)))
        }
        fn key(&mut self) -> Result<Vec<u8>, String> {
            Ok(std::mem::take(&mut self.keys))
        }
        fn suspend(&mut self) {}
    }
    #[test]
    fn injected_input_and_acceptance() {
        let mut runtime = FakeRuntime {
            steps: 0,
            keys: vec![b'\r'],
            cancel_at: None,
            dimensions: (3, 20),
        };
        let mut input = std::io::Cursor::new(b"alpha\0beta\0");
        let mut output = Vec::new();
        assert_eq!(
            execute_with_runtime(
                &["--read0".into(), "--print0".into()],
                &mut input,
                &mut output,
                &mut runtime
            )
            .unwrap(),
            0
        );
        assert_eq!(output, b"beta\0");
    }
    #[test]
    fn both_arrow_protocols_navigate() {
        for keys in [b"\x1b[B\r".to_vec(), b"\x1bOB\r".to_vec()] {
            let mut runtime = FakeRuntime {
                steps: 0,
                keys,
                cancel_at: Some(100),
                dimensions: (10, 80),
            };
            let mut input = std::io::Cursor::new(b"one\ntwo\n");
            let mut output = Vec::new();
            assert_eq!(
                execute_with_runtime(&[], &mut input, &mut output, &mut runtime).unwrap(),
                0
            );
            assert_eq!(output, b"two\n");
        }
    }
    #[test]
    fn cancellation_interrupts_one_large_record() {
        let records = vec![vec![b'a'; RECORD]];
        let settings = Settings {
            exact: true,
            multi: false,
            read0: true,
            print0: true,
            hex: false,
            query: "z".into(),
            preserve: false,
        };
        let mut runtime = FakeRuntime {
            steps: 0,
            keys: Vec::new(),
            cancel_at: Some(1),
            dimensions: (1, 1),
        };
        assert!(
            select_with_runtime(records, settings, None, &mut runtime)
                .unwrap()
                .is_none()
        );
        assert_eq!(runtime.steps, 1);
    }
    #[test]
    fn matching_resumes_with_fixed_byte_quantum() {
        let records = vec![vec![b'a'; RECORD]];
        let mut m = Matching::new("z", true, false);
        m.advance(&records);
        assert_eq!(m.record, 0);
        assert_eq!(m.position, 65536);
    }

    #[test]
    fn batched_query_and_enter_never_accept_stale_match() {
        let mut runtime = FakeRuntime {
            steps: 0,
            keys: b"beta\r".to_vec(),
            cancel_at: None,
            dimensions: (3, 20),
        };
        let mut input = std::io::Cursor::new(b"alpha\0betamax\0");
        let mut output = Vec::new();
        assert_eq!(
            execute_with_runtime(
                &["--read0".into(), "--print0".into()],
                &mut input,
                &mut output,
                &mut runtime
            )
            .unwrap(),
            0
        );
        assert_eq!(output, b"betamax\0");
    }
}
