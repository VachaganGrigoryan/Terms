use zeroize::Zeroize;
#[derive(Clone, Debug)]
pub struct Record {
    pub id: String,
    pub session: String,
    pub command: Vec<u8>,
    pub cwd: Vec<u8>,
    pub time: u64,
    pub duration: Option<u64>,
    pub exit: Option<i32>,
}
impl Drop for Record {
    fn drop(&mut self) {
        self.command.zeroize();
        self.cwd.zeroize();
    }
}
#[derive(Clone)]
pub struct Model {
    pub records: Vec<Record>,
    pub days: u64,
    pub max: usize,
    pub prefixes: Vec<Vec<u8>>,
    pub dirs: Vec<Vec<u8>>,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            days: 90,
            max: 10000,
            prefixes: Vec::new(),
            dirs: Vec::new(),
        }
    }
}
impl Drop for Model {
    fn drop(&mut self) {
        for x in self.prefixes.iter_mut().chain(self.dirs.iter_mut()) {
            x.zeroize();
        }
    }
}
pub fn hex(v: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(v.len() * 2);
    for &b in v {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}
pub struct Hex<'a>(pub &'a [u8]);
impl std::fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}
pub fn unhex(v: &str) -> Result<Vec<u8>, String> {
    if v.len() % 2 != 0 || v.len() > 2 * 1024 * 1024 {
        return Err("invalid hex length".into());
    }
    let mut out = zeroize::Zeroizing::new(Vec::with_capacity(v.len() / 2));
    for b in v.as_bytes().chunks_exact(2) {
        let s = std::str::from_utf8(b).map_err(|_| "invalid hex")?;
        out.push(u8::from_str_radix(s, 16).map_err(|_| "invalid hex")?);
    }
    Ok(std::mem::take(&mut *out))
}

pub fn number<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.parse().map_err(|_| "invalid numeric field".into())
}
impl Model {
    pub fn apply(&mut self, event: &[u8]) -> Result<(), String> {
        let text = std::str::from_utf8(event).map_err(|_| "invalid event")?;
        let f: Vec<_> = text.split('\t').collect();
        match f.as_slice() {
            ["start", id, session, cwd, command, time] => {
                let mut c = zeroize::Zeroizing::new(unhex(command)?);
                let mut d = zeroize::Zeroizing::new(unhex(cwd)?);
                if c.len() > 65536 || d.len() > 16384 || self.records.iter().any(|r| r.id == *id) {
                    return Err("invalid history start".into());
                }
                let time = number(time)?;
                self.records.push(Record {
                    id: id.to_string(),
                    session: session.to_string(),
                    command: std::mem::take(&mut *c),
                    cwd: std::mem::take(&mut *d),
                    time,
                    duration: None,
                    exit: None,
                });
            }
            ["finish", id, duration, exit] => {
                let r = self
                    .records
                    .iter_mut()
                    .find(|r| r.id == *id)
                    .ok_or("unknown command")?;
                if r.exit.is_some() {
                    return Err("duplicate completion".into());
                }
                r.duration = Some(number(duration)?);
                r.exit = Some(number(exit)?);
            }
            ["delete", id] => self.records.retain(|r| r.id != *id),
            ["clear"] => self.records.clear(),
            ["retention", days, max] => {
                self.days = number(days)?;
                self.max = number(max)?;
                if self.days > 36500 || self.max == 0 || self.max > 100000 {
                    return Err("invalid retention".into());
                }
            }
            ["exclude", kind, action, value] => {
                let mut v = zeroize::Zeroizing::new(unhex(value)?);
                let list = match *kind {
                    "prefix" => &mut self.prefixes,
                    "dir" => &mut self.dirs,
                    _ => return Err("unknown exclusion".into()),
                };
                match *action {
                    "add" => {
                        if list.len() >= 1000 {
                            return Err("exclusion limit".into());
                        }
                        if !list.iter().any(|x| x.as_slice() == v.as_slice()) {
                            list.push(std::mem::take(&mut *v))
                        }
                    }
                    "remove" => list.retain_mut(|x| {
                        if x.as_slice() == v.as_slice() {
                            x.zeroize();
                            false
                        } else {
                            true
                        }
                    }),
                    _ => return Err("invalid exclusion action".into()),
                }
            }
            _ => return Err("unsupported history event".into()),
        }
        if self.records.len() > 100000
            || self
                .records
                .iter()
                .map(|r| r.command.len() + r.cwd.len())
                .sum::<usize>()
                > 64 * 1024 * 1024
        {
            return Err("decrypted index exceeds limit".into());
        }
        Ok(())
    }
    pub fn excluded(&self, c: &[u8], cwd: &[u8]) -> bool {
        c.starts_with(b" ")
            || c.starts_with(b"terms history ")
            || self.prefixes.iter().any(|p| c.starts_with(p))
            || self
                .dirs
                .iter()
                .any(|d| cwd == d || cwd.starts_with(&[d.as_slice(), b"/"].concat()))
    }
    pub fn prune(&mut self, now: u64) {
        let days = self.days;
        self.records
            .retain(|r| days == 0 || now.saturating_sub(r.time) <= days * 86400);
        self.records
            .sort_by(|a, b| b.time.cmp(&a.time).then(a.id.cmp(&b.id)));
        self.records.truncate(self.max);
    }
    pub fn events(&self) -> Vec<Vec<u8>> {
        let mut e = vec![format!("retention\t{}\t{}", self.days, self.max).into_bytes()];
        for (kind, list) in [("prefix", &self.prefixes), ("dir", &self.dirs)] {
            for v in list {
                e.push(format!("exclude\t{kind}\tadd\t{}", Hex(v)).into_bytes());
            }
        }
        for r in &self.records {
            e.push(
                format!(
                    "start\t{}\t{}\t{}\t{}\t{}",
                    r.id,
                    r.session,
                    Hex(&r.cwd),
                    Hex(&r.command),
                    r.time
                )
                .into_bytes(),
            );
            if let (Some(d), Some(x)) = (r.duration, r.exit) {
                e.push(format!("finish\t{}\t{d}\t{x}", r.id).into_bytes())
            }
        }
        e
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn events_roundtrip() {
        let mut m = Model::default();
        m.apply(b"start\t1\t2\t2f746d70\t6c73\t1").unwrap();
        m.apply(b"finish\t1\t2\t0").unwrap();
        let mut n = Model::default();
        for e in m.events() {
            n.apply(&e).unwrap()
        }
        assert_eq!(n.records[0].command, b"ls");
        assert_eq!(n.records[0].exit, Some(0));
        assert!(n.apply(b"finish\t1\t2\t0").is_err());
    }
}
