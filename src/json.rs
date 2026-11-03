//! Bounded, independently implemented JSON parsing and filtering.
use std::collections::HashSet;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::rc::Rc;

const VALUE_LIMIT: usize = 64 * 1024 * 1024;
const WORK_LIMIT: usize = 1_000_000;
type V = Rc<Value>;
#[derive(Debug)]
enum Value {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<V>),
    Object(Vec<(String, V)>),
}
fn value(v: Value) -> V {
    Rc::new(v)
}
fn error(msg: &str) -> String {
    format!("json: {msg}")
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    nodes: usize,
}
impl<'a> Parser<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            nodes: 0,
        }
    }
    fn ws(&mut self) {
        while self
            .bytes
            .get(self.pos)
            .is_some_and(|b| b" \r\n\t".contains(b))
        {
            self.pos += 1;
        }
    }
    fn take(&mut self, b: u8) -> bool {
        self.ws();
        if self.bytes.get(self.pos) == Some(&b) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn need(&mut self, b: u8) -> Result<(), String> {
        if self.take(b) {
            Ok(())
        } else {
            Err(error("unexpected token"))
        }
    }
    fn string(&mut self) -> Result<String, String> {
        self.need(b'"')?;
        let mut out = String::new();
        let mut start = self.pos;
        loop {
            let b = *self
                .bytes
                .get(self.pos)
                .ok_or_else(|| error("unterminated string"))?;
            if b == b'"' || b == b'\\' {
                out.push_str(
                    std::str::from_utf8(&self.bytes[start..self.pos])
                        .map_err(|_| error("invalid UTF-8"))?,
                );
                self.pos += 1;
                if b == b'"' {
                    return Ok(out);
                }
                let esc = *self
                    .bytes
                    .get(self.pos)
                    .ok_or_else(|| error("incomplete escape"))?;
                self.pos += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let high = self.hex()?;
                        let cp = if (0xd800..=0xdbff).contains(&high) {
                            if self.bytes.get(self.pos..self.pos + 2) != Some(b"\\u") {
                                return Err(error("unpaired surrogate"));
                            }
                            self.pos += 2;
                            let low = self.hex()?;
                            if !(0xdc00..=0xdfff).contains(&low) {
                                return Err(error("unpaired surrogate"));
                            }
                            0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00)
                        } else {
                            high
                        };
                        out.push(
                            char::from_u32(cp).ok_or_else(|| error("invalid Unicode scalar"))?,
                        );
                    }
                    _ => return Err(error("invalid escape")),
                }
                start = self.pos;
            } else {
                if b < 0x20 {
                    return Err(error("unescaped control character"));
                }
                self.pos += 1;
            }
        }
    }
    fn hex(&mut self) -> Result<u32, String> {
        let mut n = 0;
        for _ in 0..4 {
            let c = *self
                .bytes
                .get(self.pos)
                .ok_or_else(|| error("incomplete Unicode escape"))?;
            self.pos += 1;
            n = n * 16
                + (c as char)
                    .to_digit(16)
                    .ok_or_else(|| error("invalid Unicode escape"))?;
        }
        Ok(n)
    }
    fn parse(&mut self, depth: usize) -> Result<V, String> {
        if depth > 128 {
            return Err(error("nesting exceeds 128"));
        }
        self.nodes += 1;
        if self.nodes > WORK_LIMIT {
            return Err(error("too many JSON nodes"));
        }
        self.ws();
        let result = match self.bytes.get(self.pos).copied() {
            Some(b'"') => Value::String(self.string()?),
            Some(b'[') => {
                self.pos += 1;
                let mut a = Vec::new();
                if !self.take(b']') {
                    loop {
                        a.push(self.parse(depth + 1)?);
                        if self.take(b']') {
                            break;
                        }
                        self.need(b',')?;
                    }
                }
                Value::Array(a)
            }
            Some(b'{') => {
                self.pos += 1;
                let mut a = Vec::new();
                let mut keys = HashSet::new();
                if !self.take(b'}') {
                    loop {
                        let k = self.string()?;
                        if !keys.insert(k.clone()) {
                            return Err(error("duplicate object key"));
                        }
                        self.need(b':')?;
                        a.push((k, self.parse(depth + 1)?));
                        if self.take(b'}') {
                            break;
                        }
                        self.need(b',')?;
                    }
                }
                Value::Object(a)
            }
            Some(b'n') => {
                self.keyword(b"null")?;
                Value::Null
            }
            Some(b't') => {
                self.keyword(b"true")?;
                Value::Bool(true)
            }
            Some(b'f') => {
                self.keyword(b"false")?;
                Value::Bool(false)
            }
            Some(b'-' | b'0'..=b'9') => Value::Number(self.number()?),
            _ => return Err(error("expected JSON value")),
        };
        Ok(value(result))
    }
    fn keyword(&mut self, s: &[u8]) -> Result<(), String> {
        if self.bytes.get(self.pos..self.pos + s.len()) == Some(s) {
            self.pos += s.len();
            Ok(())
        } else {
            Err(error("invalid literal"))
        }
    }
    fn number(&mut self) -> Result<String, String> {
        let start = self.pos;
        if self.bytes.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        match self.bytes.get(self.pos) {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.pos += 1;
                self.digits();
            }
            _ => return Err(error("invalid number")),
        }
        if self.bytes.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            let p = self.pos;
            self.digits();
            if p == self.pos {
                return Err(error("missing fraction"));
            }
        }
        if matches!(self.bytes.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.bytes.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let p = self.pos;
            self.digits();
            if p == self.pos {
                return Err(error("missing exponent"));
            }
        }
        Ok(std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| error("invalid number"))?
            .into())
    }
    fn digits(&mut self) {
        while self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit) {
            self.pos += 1;
        }
    }
}

#[derive(Debug)]
enum Step {
    Field(String),
    Index(usize),
    Iterate,
}
#[derive(Debug)]
enum Filter {
    Path(Vec<Step>),
    Pipe(Vec<Filter>),
    Select(Vec<Step>, bool, V),
    Array(Box<Filter>),
    Object(Vec<(String, Filter)>),
    Map(Box<Filter>),
    Length,
}
struct FilterParser<'a> {
    parser: Parser<'a>,
}
impl FilterParser<'_> {
    fn filter(&mut self, depth: usize) -> Result<Filter, String> {
        if depth > 128 {
            return Err(error("filter nesting exceeds 128"));
        }
        let mut filters = vec![self.atom(depth + 1)?];
        while self.parser.take(b'|') {
            filters.push(self.atom(depth + 1)?);
        }
        if filters.len() == 1 {
            Ok(filters.remove(0))
        } else {
            Ok(Filter::Pipe(filters))
        }
    }
    fn word(&mut self, w: &[u8]) -> bool {
        self.parser.ws();
        if self
            .parser
            .bytes
            .get(self.parser.pos..self.parser.pos + w.len())
            == Some(w)
        {
            self.parser.pos += w.len();
            true
        } else {
            false
        }
    }
    fn atom(&mut self, depth: usize) -> Result<Filter, String> {
        if self.word(b"select") {
            self.parser.need(b'(')?;
            let path = self.path()?;
            let equal = if self.word(b"==") {
                true
            } else if self.word(b"!=") {
                false
            } else {
                return Err(error("expected == or !="));
            };
            let literal = self.parser.parse(0)?;
            if matches!(&*literal, Value::Array(_) | Value::Object(_)) {
                return Err(error("select requires scalar literal"));
            }
            self.parser.need(b')')?;
            Ok(Filter::Select(path, equal, literal))
        } else if self.word(b"map") {
            self.parser.need(b'(')?;
            let f = self.filter(depth)?;
            self.parser.need(b')')?;
            Ok(Filter::Map(Box::new(f)))
        } else if self.word(b"length") {
            Ok(Filter::Length)
        } else if self.parser.take(b'[') {
            let f = self.filter(depth)?;
            self.parser.need(b']')?;
            Ok(Filter::Array(Box::new(f)))
        } else if self.parser.take(b'{') {
            let mut fields = Vec::new();
            let mut keys = HashSet::new();
            if !self.parser.take(b'}') {
                loop {
                    self.parser.ws();
                    let k = if self.parser.bytes.get(self.parser.pos) == Some(&b'"') {
                        self.parser.string()?
                    } else {
                        self.ident()?
                    };
                    if !keys.insert(k.clone()) {
                        return Err(error("duplicate constructor key"));
                    }
                    self.parser.need(b':')?;
                    let f = self.filter(depth)?;
                    fields.push((k, f));
                    if self.parser.take(b'}') {
                        break;
                    }
                    self.parser.need(b',')?;
                }
            }
            Ok(Filter::Object(fields))
        } else {
            Ok(Filter::Path(self.path()?))
        }
    }
    fn ident(&mut self) -> Result<String, String> {
        self.parser.ws();
        let start = self.parser.pos;
        let b = self
            .parser
            .bytes
            .get(start)
            .copied()
            .ok_or_else(|| error("expected identifier"))?;
        if !b.is_ascii_alphabetic() && b != b'_' {
            return Err(error("expected identifier"));
        }
        self.parser.pos += 1;
        while self
            .parser
            .bytes
            .get(self.parser.pos)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            self.parser.pos += 1;
        }
        String::from_utf8(self.parser.bytes[start..self.parser.pos].into())
            .map_err(|_| error("invalid identifier"))
    }
    fn path(&mut self) -> Result<Vec<Step>, String> {
        self.parser.need(b'.')?;
        let mut steps = Vec::new();
        self.parser.ws();
        if self
            .parser
            .bytes
            .get(self.parser.pos)
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        {
            steps.push(Step::Field(self.ident()?));
        }
        loop {
            if self.parser.take(b'[') {
                if self.parser.take(b']') {
                    steps.push(Step::Iterate)
                } else {
                    self.parser.ws();
                    if self.parser.bytes.get(self.parser.pos) == Some(&b'"') {
                        steps.push(Step::Field(self.parser.string()?));
                    } else {
                        let start = self.parser.pos;
                        self.parser.digits();
                        if start == self.parser.pos {
                            return Err(error("expected nonnegative index"));
                        }
                        let n = std::str::from_utf8(&self.parser.bytes[start..self.parser.pos])
                            .map_err(|_| error("invalid index"))?
                            .parse()
                            .map_err(|_| error("index too large"))?;
                        steps.push(Step::Index(n));
                    }
                    self.parser.need(b']')?;
                }
            } else if self.parser.take(b'.') {
                steps.push(Step::Field(self.ident()?));
            } else {
                break;
            }
        }
        Ok(steps)
    }
}
fn work(budget: &mut usize, n: usize) -> Result<(), String> {
    *budget = budget
        .checked_sub(n)
        .ok_or_else(|| error("evaluation work exhausted"))?;
    Ok(())
}
fn path(input: &V, steps: &[Step], budget: &mut usize) -> Result<Vec<V>, String> {
    let mut values = vec![input.clone()];
    for step in steps {
        let mut next = Vec::new();
        for v in values {
            work(budget, 1)?;
            match (step, &*v) {
                (Step::Field(k), Value::Object(fields)) => {
                    work(budget, fields.len())?;
                    next.push(
                        fields
                            .iter()
                            .find(|(name, _)| name == k)
                            .map_or_else(|| value(Value::Null), |(_, v)| v.clone()),
                    );
                }
                (Step::Field(_) | Step::Index(_), Value::Null) => next.push(v.clone()),
                (Step::Index(n), Value::Array(a)) => {
                    next.push(a.get(*n).cloned().unwrap_or_else(|| value(Value::Null)))
                }
                (Step::Iterate, Value::Array(a)) => {
                    work(budget, a.len())?;
                    next.extend(a.iter().cloned());
                }
                (Step::Iterate, Value::Object(a)) => {
                    work(budget, a.len())?;
                    next.extend(a.iter().map(|(_, v)| v.clone()));
                }
                _ => return Err(error("invalid path operand type")),
            }
        }
        values = next;
    }
    Ok(values)
}
fn eval(f: &Filter, input: &V, budget: &mut usize) -> Result<Vec<V>, String> {
    let results = eval_inner(f, input, budget)?;
    let mut pending: Vec<_> = results.iter().map(|v| (v, 0usize)).collect();
    while let Some((v, depth)) = pending.pop() {
        work(budget, 1)?;
        if depth > 128 {
            return Err(error("constructed value exceeds nesting limit"));
        }
        match &**v {
            Value::Array(a) => pending.extend(a.iter().map(|v| (v, depth + 1))),
            Value::Object(a) => pending.extend(a.iter().map(|(_, v)| (v, depth + 1))),
            _ => {}
        }
    }
    Ok(results)
}
fn eval_inner(f: &Filter, input: &V, budget: &mut usize) -> Result<Vec<V>, String> {
    work(budget, 1)?;
    match f {
        Filter::Path(p) => path(input, p, budget),
        Filter::Pipe(fs) => {
            let mut vs = vec![input.clone()];
            for f in fs {
                let mut next = Vec::new();
                for v in vs {
                    next.extend(eval(f, &v, budget)?);
                }
                vs = next;
            }
            Ok(vs)
        }
        Filter::Select(p, equal, literal) => {
            let vals = path(input, p, budget)?;
            let mut out = Vec::new();
            for v in vals {
                let comparison_bytes = match &*v {
                    Value::String(s) | Value::Number(s) => s.len(),
                    _ => 0,
                };
                work(budget, comparison_bytes.saturating_add(1))?;
                if equivalent(&v, literal) == *equal {
                    out.push(input.clone())
                }
            }
            Ok(out)
        }
        Filter::Array(f) => Ok(vec![value(Value::Array(eval(f, input, budget)?))]),
        Filter::Object(fs) => {
            let mut out = Vec::new();
            for (k, f) in fs {
                work(budget, k.len())?;
                let mut v = eval(f, input, budget)?;
                if v.len() != 1 {
                    return Err(error("constructor fields require one result"));
                }
                out.push((k.clone(), v.remove(0)));
            }
            Ok(vec![value(Value::Object(out))])
        }
        Filter::Map(f) => {
            let Value::Array(a) = &**input else {
                return Err(error("map requires array"));
            };
            let mut out = Vec::new();
            for v in a {
                out.extend(eval(f, v, budget)?);
            }
            Ok(vec![value(Value::Array(out))])
        }
        Filter::Length => {
            let n = match &**input {
                Value::Array(a) => a.len(),
                Value::Object(a) => a.len(),
                Value::String(s) => {
                    work(budget, s.len())?;
                    s.chars().count()
                }
                _ => return Err(error("length requires array, object or string")),
            };
            Ok(vec![value(Value::Number(n.to_string()))])
        }
    }
}

// Signed arbitrary-precision decimal addition is used only for exponent normalization.
// Its allocation depends on token length, never on the exponent's numeric value.
fn signed_add(a: &str, delta: i64) -> String {
    fn normalized(s: &str) -> (bool, String) {
        let neg = s.starts_with('-');
        let digits = s.trim_start_matches(['-', '+']).trim_start_matches('0');
        (
            neg && !digits.is_empty(),
            if digits.is_empty() {
                "0".into()
            } else {
                digits.into()
            },
        )
    }
    fn add(a: &str, b: &str) -> String {
        let (mut i, mut j, mut carry) = (a.len(), b.len(), 0u8);
        let mut out = Vec::new();
        while i > 0 || j > 0 || carry > 0 {
            let x = if i > 0 {
                i -= 1;
                a.as_bytes()[i] - b'0'
            } else {
                0
            };
            let y = if j > 0 {
                j -= 1;
                b.as_bytes()[j] - b'0'
            } else {
                0
            };
            let n = x + y + carry;
            out.push(b'0' + n % 10);
            carry = n / 10;
        }
        out.reverse();
        String::from_utf8(out).unwrap_or_default()
    }
    fn subtract(a: &str, b: &str) -> String {
        let (mut i, mut j, mut borrow) = (a.len(), b.len(), 0i16);
        let mut out = Vec::new();
        while i > 0 {
            i -= 1;
            let x = i16::from(a.as_bytes()[i] - b'0');
            let y = if j > 0 {
                j -= 1;
                i16::from(b.as_bytes()[j] - b'0')
            } else {
                0
            };
            let mut n = x - y - borrow;
            if n < 0 {
                n += 10;
                borrow = 1
            } else {
                borrow = 0
            }
            out.push(b'0' + n as u8);
        }
        while out.len() > 1 && out.last() == Some(&b'0') {
            out.pop();
        }
        out.reverse();
        String::from_utf8(out).unwrap_or_default()
    }
    let (an, a) = normalized(a);
    let (bn, b) = normalized(&delta.to_string());
    let (negative, digits) = if an == bn {
        (an, add(&a, &b))
    } else {
        let cmp = a.len().cmp(&b.len()).then_with(|| a.cmp(&b));
        if cmp.is_ge() {
            (an, subtract(&a, &b))
        } else {
            (bn, subtract(&b, &a))
        }
    };
    if negative && digits != "0" {
        format!("-{digits}")
    } else {
        digits
    }
}
fn normalized_number(s: &str) -> (bool, String, String) {
    let neg = s.starts_with('-');
    let s = s.trim_start_matches('-');
    let (mantissa, exponent) = s.split_once(['e', 'E']).unwrap_or((s, "0"));
    let fractional = mantissa.split_once('.').map_or(0, |(_, f)| f.len());
    let digits = mantissa.replace('.', "");
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return (false, "0".into(), "0".into());
    }
    let trimmed = digits.trim_end_matches('0');
    let adjustment = (digits.len() - trimmed.len()) as i64 - fractional as i64;
    (neg, trimmed.into(), signed_add(exponent, adjustment))
}
fn equivalent(a: &V, b: &V) -> bool {
    match (&**a, &**b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Number(a), Value::Number(b)) => normalized_number(a) == normalized_number(b),
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equivalent(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(k, v)| {
                    b.iter()
                        .find(|(name, _)| name == k)
                        .is_some_and(|(_, other)| equivalent(v, other))
                })
        }
        _ => false,
    }
}

struct Output<'a> {
    writer: &'a mut dyn Write,
    remaining: usize,
}
impl Output<'_> {
    fn put(&mut self, s: &str) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(s.len())
            .ok_or_else(|| error("output exceeds 64 MiB per value"))?;
        self.writer
            .write_all(s.as_bytes())
            .map_err(|_| error("output write failed"))
    }
}
fn quoted(out: &mut Output<'_>, s: &str, quotes: bool) -> Result<(), String> {
    if quotes {
        out.put("\"")?;
    }
    let mut start = 0;
    for (i, c) in s.char_indices() {
        let escaped = match c {
            '"' if quotes => Some("\\\"".into()),
            '\\' => Some("\\\\".into()),
            '\n' => Some("\\n".into()),
            '\r' => Some("\\r".into()),
            '\t' => Some("\\t".into()),
            c if c.is_control()
                || matches!(c,'\u{061c}'|'\u{200e}'|'\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') =>
            {
                Some(format!("\\u{:04x}", c as u32))
            }
            _ => None,
        };
        if let Some(escaped) = escaped {
            out.put(&s[start..i])?;
            out.put(&escaped)?;
            start = i + c.len_utf8();
        }
    }
    out.put(&s[start..])?;
    if quotes {
        out.put("\"")?;
    }
    Ok(())
}
fn serialize(out: &mut Output<'_>, v: &V, pretty: bool, depth: usize) -> Result<(), String> {
    match &**v {
        Value::Null => out.put("null"),
        Value::Bool(b) => out.put(if *b { "true" } else { "false" }),
        Value::Number(n) => out.put(n),
        Value::String(s) => quoted(out, s, true),
        Value::Array(a) => {
            out.put("[")?;
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.put(",")?;
                }
                if pretty {
                    out.put("\n")?;
                    out.put(&"  ".repeat(depth + 1))?;
                }
                serialize(out, v, pretty, depth + 1)?;
            }
            if pretty && !a.is_empty() {
                out.put("\n")?;
                out.put(&"  ".repeat(depth))?;
            }
            out.put("]")
        }
        Value::Object(a) => {
            out.put("{")?;
            for (i, (k, v)) in a.iter().enumerate() {
                if i > 0 {
                    out.put(",")?;
                }
                if pretty {
                    out.put("\n")?;
                    out.put(&"  ".repeat(depth + 1))?;
                }
                quoted(out, k, true)?;
                out.put(if pretty { ": " } else { ":" })?;
                serialize(out, v, pretty, depth + 1)?;
            }
            if pretty && !a.is_empty() {
                out.put("\n")?;
                out.put(&"  ".repeat(depth))?;
            }
            out.put("}")
        }
    }
}

fn read_byte(input: &mut dyn BufRead) -> Result<Option<u8>, String> {
    let b = input
        .fill_buf()
        .map_err(|_| error("input read failed"))?
        .first()
        .copied();
    if b.is_some() {
        input.consume(1);
    }
    Ok(b)
}
fn framed(input: &mut dyn BufRead) -> Result<Option<Vec<u8>>, String> {
    let first = loop {
        match read_byte(input)? {
            Some(b) if b" \r\n\t".contains(&b) => {}
            other => break other,
        }
    };
    let Some(first) = first else { return Ok(None) };
    let mut bytes = vec![first];
    let structured = matches!(first, b'{' | b'[');
    let mut depth = if structured { 1usize } else { 0 };
    let mut string = first == b'"';
    let mut escaped = false;
    let mut done = false;
    loop {
        if !structured && !string && first != b'"' {
            let peek = input
                .fill_buf()
                .map_err(|_| error("input read failed"))?
                .first()
                .copied();
            if peek.is_none_or(|b| b" \r\n\t".contains(&b)) {
                break;
            }
        }
        if done {
            break;
        }
        let Some(b) = read_byte(input)? else { break };
        if bytes.len() >= VALUE_LIMIT {
            return Err(error("input value exceeds 64 MiB"));
        }
        bytes.push(b);
        if string {
            if escaped {
                escaped = false
            } else if b == b'\\' {
                escaped = true
            } else if b == b'"' {
                string = false;
                if !structured {
                    done = true;
                }
            }
        } else if b == b'"' {
            string = true
        } else if structured {
            match b {
                b'{' | b'[' => {
                    depth += 1;
                    if depth > 129 {
                        return Err(error("nesting exceeds 128"));
                    }
                }
                b'}' | b']' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        done = true
                    }
                }
                _ => {}
            }
        }
    }
    if done
        && input
            .fill_buf()
            .map_err(|_| error("input read failed"))?
            .first()
            .is_some_and(|b| !b" \r\n\t".contains(b))
    {
        return Err(error("input values require whitespace separation"));
    }
    Ok(Some(bytes))
}
fn process(
    input: &mut dyn Read,
    out: &mut dyn Write,
    filter: &Filter,
    compact: bool,
    raw: bool,
    terminal: bool,
) -> Result<bool, String> {
    let mut input = BufReader::new(input);
    let mut emitted = false;
    while let Some(bytes) = framed(&mut input)? {
        let mut parser = Parser::new(&bytes);
        let input = parser.parse(0)?;
        parser.ws();
        if parser.pos != bytes.len() {
            return Err(error("trailing data in JSON value"));
        }
        let mut budget = WORK_LIMIT;
        let results = eval(filter, &input, &mut budget)?;
        let mut output = Output {
            writer: out,
            remaining: VALUE_LIMIT,
        };
        for v in results {
            if raw {
                if let Value::String(s) = &*v {
                    if terminal {
                        quoted(&mut output, s, false)?
                    } else {
                        output.put(s)?
                    }
                } else {
                    serialize(&mut output, &v, !compact, 0)?
                }
            } else {
                serialize(&mut output, &v, !compact, 0)?
            }
            output.put("\n")?;
            emitted = true;
        }
    }
    Ok(emitted)
}

/// Execute the supported JSON filter language with bounded parsing and output.
pub fn execute(
    args: &[OsString],
    input: &mut dyn Read,
    out: &mut dyn Write,
    terminal: bool,
) -> Result<u8, String> {
    let (mut compact, mut raw, mut options) = (false, false, true);
    let mut operands = Vec::new();
    for arg in args {
        if options && arg == "--" {
            options = false;
            continue;
        }
        if options && arg == "-c" {
            compact = true;
            continue;
        }
        if options && arg == "-r" {
            raw = true;
            continue;
        }
        if options && arg != "-" && arg.as_encoded_bytes().starts_with(b"-") {
            return Err(error("unknown option"));
        }
        operands.push(arg);
    }
    let filter = operands
        .first()
        .ok_or_else(|| error("missing filter"))?
        .to_str()
        .ok_or_else(|| error("filter must be UTF-8"))?;
    if filter.len() > 65536 {
        return Err(error("filter exceeds 64 KiB"));
    }
    let mut parser = FilterParser {
        parser: Parser::new(filter.as_bytes()),
    };
    let filter = parser.filter(0)?;
    parser.parser.ws();
    if parser.parser.pos != parser.parser.bytes.len() {
        return Err(error("unsupported filter syntax"));
    }
    let mut emitted = false;
    if operands.len() == 1 {
        emitted = process(input, out, &filter, compact, raw, terminal)?;
    } else {
        for path in &operands[1..] {
            let metadata =
                std::fs::symlink_metadata(path).map_err(|_| error("cannot inspect input file"))?;
            if !metadata.is_file() {
                return Err(error("input must be a regular file"));
            }
            let mut file =
                std::fs::File::open(path).map_err(|_| error("cannot open input file"))?;
            emitted |= process(&mut file, out, &filter, compact, raw, terminal)?;
        }
    }
    Ok(if emitted { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(filter: &str, input: &str) -> Result<(u8, String), String> {
        let mut output = Vec::new();
        let status = execute(
            &["-c".into(), filter.into()],
            &mut input.as_bytes(),
            &mut output,
            false,
        )?;
        Ok((status, String::from_utf8(output).unwrap()))
    }
    #[test]
    fn screenshot_and_constructors() {
        assert_eq!(
            run(
                ".[] | select(.fish==\"tuna\")",
                r#"[{"fish":"cod"},{"fish":"tuna"}]"#
            )
            .unwrap()
            .1,
            "{\"fish\":\"tuna\"}\n"
        );
        assert_eq!(
            run("map({name: .cat, n: .qty})", r#"[{"cat":"Kitty","qty":2}]"#)
                .unwrap()
                .1,
            "[{\"name\":\"Kitty\",\"n\":2}]\n"
        );
    }
    #[test]
    fn strict_input() {
        for input in [
            "{\"a\":1,\"a\":2}",
            "01",
            "[1,]",
            "\"\\ud800\"",
            "truefalse",
            "{}{}",
            "\"a\n\"",
            "1.",
        ] {
            assert!(run(".", input).is_err(), "{input}");
        }
        assert_eq!(run(".", "1 true null").unwrap().1, "1\ntrue\nnull\n");
    }
    #[test]
    fn numeric_comparison() {
        for (a, b) in [
            ("100", "1e2"),
            ("-0", "0"),
            ("1.00", "10e-1"),
            (
                "1e999999999999999999999999999",
                "10e999999999999999999999999998",
            ),
            ("0.01", "1e-2"),
        ] {
            assert_eq!(normalized_number(a), normalized_number(b));
        }
        assert_ne!(
            normalized_number("1e999999999999999999999"),
            normalized_number("1e999999999999999999998")
        );
        assert_eq!(run("select(. == 1)", "1.0").unwrap().1, "1.0\n");
    }
    #[test]
    fn unicode_and_limits() {
        assert_eq!(run("length", "\"\\ud83d\\ude00\"").unwrap().1, "1\n");
        assert!(run(".", &format!("{}0{}", "[".repeat(130), "]".repeat(130))).is_err());
        assert!(run("reduce .[]", "[]").is_err());
    }
    #[test]
    fn raw_terminal_safety() {
        for terminal in [false, true] {
            let mut out = Vec::new();
            execute(
                &["-r".into(), ".".into()],
                &mut b"\"\\u001b[31m\"".as_slice(),
                &mut out,
                terminal,
            )
            .unwrap();
            assert_eq!(
                out,
                if terminal {
                    b"\\u001b[31m\n".to_vec()
                } else {
                    b"\x1b[31m\n".to_vec()
                }
            );
        }
    }
}
