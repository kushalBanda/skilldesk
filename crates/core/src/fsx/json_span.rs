//! Byte spans inside JSON text, so edits touch only the entry they change and
//! every other byte of the user's file stays as it was. Tolerates
//! `//` and `/* */` comments (JSONC settings files).

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Key(String),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArraySpan {
    /// Byte index of `[`.
    pub open: usize,
    /// Byte index of `]`.
    pub close: usize,
    /// (start, end) of each element, end exclusive.
    pub elems: Vec<(usize, usize)>,
}

struct Scan<'a> {
    b: &'a [u8],
}

impl Scan<'_> {
    fn ws(&self, mut i: usize) -> usize {
        loop {
            while i < self.b.len() && self.b[i].is_ascii_whitespace() {
                i += 1;
            }
            if self.b[i..].starts_with(b"//") {
                while i < self.b.len() && self.b[i] != b'\n' {
                    i += 1;
                }
            } else if self.b[i..].starts_with(b"/*") {
                i += 2;
                while i + 1 < self.b.len() && !self.b[i..].starts_with(b"*/") {
                    i += 1;
                }
                i = (i + 2).min(self.b.len());
            } else {
                return i;
            }
        }
    }

    /// `i` at `"`; returns (end after closing quote, decoded text).
    fn string(&self, i: usize) -> Option<(usize, String)> {
        if self.b.get(i) != Some(&b'"') {
            return None;
        }
        let mut j = i + 1;
        while j < self.b.len() {
            match self.b[j] {
                b'\\' => j += 2,
                b'"' => {
                    let raw = std::str::from_utf8(&self.b[i..=j]).ok()?;
                    let s: String = serde_json::from_str(raw).ok()?;
                    return Some((j + 1, s));
                }
                _ => j += 1,
            }
        }
        None
    }

    /// `i` at the first byte of a value; returns its end (exclusive).
    fn value(&self, i: usize) -> Option<usize> {
        match *self.b.get(i)? {
            b'"' => self.string(i).map(|(e, _)| e),
            b'{' => self.object(i, |_, _, _| false).map(|(e, _)| e),
            b'[' => self.array(i).map(|a| a.close + 1),
            _ => {
                let mut j = i;
                while j < self.b.len() && !matches!(self.b[j], b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r' | b'/') {
                    j += 1;
                }
                (j > i).then_some(j)
            }
        }
    }

    /// Walks an object; `stop(key, value_start, value_end)` returning true stops
    /// and yields that value's span.
    fn object(&self, i: usize, mut stop: impl FnMut(&str, usize, usize) -> bool) -> Option<(usize, Option<(usize, usize)>)> {
        let mut j = self.ws(i + 1);
        if self.b.get(j) == Some(&b'}') {
            return Some((j + 1, None));
        }
        loop {
            let (after_key, key) = self.string(j)?;
            j = self.ws(after_key);
            if self.b.get(j) != Some(&b':') {
                return None;
            }
            let vs = self.ws(j + 1);
            let ve = self.value(vs)?;
            if stop(&key, vs, ve) {
                return Some((ve, Some((vs, ve))));
            }
            j = self.ws(ve);
            match self.b.get(j)? {
                b',' => {
                    j = self.ws(j + 1);
                    if self.b.get(j) == Some(&b'}') {
                        return Some((j + 1, None)); // trailing comma (JSONC)
                    }
                }
                b'}' => return Some((j + 1, None)),
                _ => return None,
            }
        }
    }

    fn array(&self, i: usize) -> Option<ArraySpan> {
        let mut elems = Vec::new();
        let mut j = self.ws(i + 1);
        if self.b.get(j) == Some(&b']') {
            return Some(ArraySpan { open: i, close: j, elems });
        }
        loop {
            let e = self.value(j)?;
            elems.push((j, e));
            j = self.ws(e);
            match self.b.get(j)? {
                b',' => {
                    j = self.ws(j + 1);
                    if self.b.get(j) == Some(&b']') {
                        return Some(ArraySpan { open: i, close: j, elems });
                    }
                }
                b']' => return Some(ArraySpan { open: i, close: j, elems }),
                _ => return None,
            }
        }
    }
}

/// Span of the value at `path`.
pub fn locate(text: &str, path: &[Seg]) -> Option<(usize, usize)> {
    let s = Scan { b: text.as_bytes() };
    let mut start = s.ws(0);
    let mut end = s.value(start)?;
    for seg in path {
        match seg {
            Seg::Key(k) => {
                if s.b.get(start) != Some(&b'{') {
                    return None;
                }
                let (_, found) = s.object(start, |key, _, _| key == k)?;
                (start, end) = found?;
            }
            Seg::Index(n) => {
                if s.b.get(start) != Some(&b'[') {
                    return None;
                }
                (start, end) = *s.array(start)?.elems.get(*n)?;
            }
        }
    }
    Some((start, end))
}

pub fn array_at(text: &str, path: &[Seg]) -> Option<ArraySpan> {
    let (start, _) = locate(text, path)?;
    let s = Scan { b: text.as_bytes() };
    (s.b.get(start) == Some(&b'[')).then(|| s.array(start)).flatten()
}

/// Removes element `n` with one neighbouring separator, so the array stays
/// valid. Returns (new text, element text, exact bytes cut).
pub fn remove_element(text: &str, path: &[Seg], n: usize) -> Option<(String, String, String)> {
    let a = array_at(text, path)?;
    let (s, e) = *a.elems.get(n)?;
    let removed = text[s..e].to_string();
    let (cut_s, cut_e) = if n + 1 < a.elems.len() {
        (s, a.elems[n + 1].0)
    } else if n > 0 {
        (a.elems[n - 1].1, e)
    } else {
        (a.open + 1, a.close)
    };
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..cut_s]);
    out.push_str(&text[cut_e..]);
    Some((out, removed, text[cut_s..cut_e].to_string()))
}

/// Inserts `value_text` as element `n` (clamped to the end). `cut` is what
/// `remove_element` cut, so putting an entry back restores the same bytes;
/// without it the array's own separator is copied.
pub fn insert_element(text: &str, path: &[Seg], n: usize, value_text: &str, cut: Option<&str>) -> Option<String> {
    let a = array_at(text, path)?;
    let from_cut = cut.and_then(|c| {
        c.strip_prefix(value_text).or_else(|| c.strip_suffix(value_text)).filter(|s| s.trim() == ",")
    });
    let sep = match (from_cut, a.elems.as_slice()) {
        (Some(s), _) => s.to_string(),
        (None, [x, y, ..]) => text[x.1..y.0].to_string(),
        _ => ", ".to_string(),
    };
    let mut out = String::with_capacity(text.len() + value_text.len() + sep.len());
    if a.elems.is_empty() {
        out.push_str(&text[..=a.open]);
        match cut.filter(|c| c.contains(value_text) && c.trim() == value_text.trim()) {
            Some(c) if text[a.open + 1..a.close].trim().is_empty() => {
                out.push_str(c);
                out.push_str(&text[a.close..]);
                return Some(out);
            }
            _ => out.push_str(value_text),
        }
        out.push_str(&text[a.close..]);
    } else if n < a.elems.len() {
        let at = a.elems[n].0;
        out.push_str(&text[..at]);
        out.push_str(value_text);
        out.push_str(&sep);
        out.push_str(&text[at..]);
    } else {
        let at = a.elems.last()?.1;
        out.push_str(&text[..at]);
        out.push_str(&sep);
        out.push_str(value_text);
        out.push_str(&text[at..]);
    }
    Some(out)
}

/// Adds `"key": value_text` as the first member of the object at `path`.
pub fn insert_member(text: &str, path: &[Seg], key: &str, value_text: &str) -> Option<String> {
    let (start, _) = locate(text, path)?;
    if text.as_bytes().get(start) != Some(&b'{') {
        return None;
    }
    let s = Scan { b: text.as_bytes() };
    let first = s.ws(start + 1);
    let empty = text.as_bytes().get(first) == Some(&b'}');
    let gap = &text[start + 1..first];
    let member = format!("{}: {}", serde_json::to_string(key).ok()?, value_text);
    let mut out = String::with_capacity(text.len() + member.len() + 4);
    out.push_str(&text[..=start]);
    if empty {
        out.push_str(&member);
        out.push_str(&text[first..]);
    } else {
        out.push_str(gap);
        out.push_str(&member);
        out.push(',');
        out.push_str(&text[start + 1..]);
    }
    Some(out)
}

/// Comments replaced by spaces (byte offsets unchanged) so serde can read JSONC.
pub fn strip_comments(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = b.to_vec();
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        if in_str {
            match b[i] {
                b'\\' => i += 1,
                b'"' => in_str = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        if b[i] == b'"' {
            in_str = true;
            i += 1;
        } else if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                out[i] = b' ';
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            while i < b.len() && !b[i..].starts_with(b"*/") {
                if b[i] != b'\n' {
                    out[i] = b' ';
                }
                i += 1;
            }
            let end = (i + 2).min(b.len());
            out[i..end].fill(b' ');
            i += 2;
        } else {
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectSpan {
    pub open: usize,
    pub close: usize,
    /// (key, member start = key's opening quote, value start, value end).
    pub members: Vec<(String, usize, usize, usize)>,
}

pub fn object_at(text: &str, path: &[Seg]) -> Option<ObjectSpan> {
    let (start, end) = locate(text, path)?;
    let s = Scan { b: text.as_bytes() };
    if s.b.get(start) != Some(&b'{') {
        return None;
    }
    let mut members = Vec::new();
    let mut j = s.ws(start + 1);
    if s.b.get(j) != Some(&b'}') {
        loop {
            let (after_key, key) = s.string(j)?;
            let colon = s.ws(after_key);
            let vs = s.ws(colon + 1);
            let ve = s.value(vs)?;
            members.push((key, j, vs, ve));
            j = s.ws(ve);
            if s.b.get(j) == Some(&b',') {
                j = s.ws(j + 1);
                if s.b.get(j) == Some(&b'}') {
                    break;
                }
            } else {
                break;
            }
        }
    }
    Some(ObjectSpan { open: start, close: end - 1, members })
}

/// Removes member `key` and one neighbouring separator. `None` when absent.
pub fn remove_member(text: &str, path: &[Seg], key: &str) -> Option<String> {
    let o = object_at(text, path)?;
    let n = o.members.iter().position(|m| m.0 == key)?;
    let (_, s, _, e) = o.members[n];
    let (cut_s, cut_e) = if n + 1 < o.members.len() {
        (s, o.members[n + 1].1)
    } else if n > 0 {
        (o.members[n - 1].3, e)
    } else {
        (o.open + 1, o.close)
    };
    Some(format!("{}{}", &text[..cut_s], &text[cut_e..]))
}

/// Sets member `key` to `value_text`: replaces the value in place, or appends
/// the member after the last one using the object's own spacing.
pub fn set_member(text: &str, path: &[Seg], key: &str, value_text: &str) -> Option<String> {
    let o = object_at(text, path)?;
    if let Some(m) = o.members.iter().find(|m| m.0 == key) {
        return Some(format!("{}{}{}", &text[..m.2], value_text, &text[m.3..]));
    }
    let member = format!("{}: {}", serde_json::to_string(key).ok()?, value_text);
    match o.members.last() {
        None => insert_member(text, path, key, value_text),
        Some(last) => {
            // Copy the gap before the last member ("\n    ") as the separator.
            let prev_end = if o.members.len() >= 2 { o.members[o.members.len() - 2].3 } else { o.open + 1 };
            let gap = text[prev_end..last.1].trim_start_matches(',');
            let gap = if gap.is_empty() { " " } else { gap };
            Some(format!("{},{}{}{}", &text[..last.3], gap, member, &text[last.3..]))
        }
    }
}

/// Like `remove_member`, also returning (member index, exact member text, exact cut).
pub fn remove_member_cut(text: &str, path: &[Seg], key: &str) -> Option<(String, usize, String, String)> {
    let o = object_at(text, path)?;
    let n = o.members.iter().position(|m| m.0 == key)?;
    let (_, s, _, e) = o.members[n];
    let (cut_s, cut_e) = if n + 1 < o.members.len() {
        (s, o.members[n + 1].1)
    } else if n > 0 {
        (o.members[n - 1].3, e)
    } else {
        (o.open + 1, o.close)
    };
    Some((format!("{}{}", &text[..cut_s], &text[cut_e..]), n, text[s..e].to_string(), text[cut_s..cut_e].to_string()))
}

/// Puts a member removed by `remove_member_cut` back at `n`, byte-exact when
/// the object around it is unchanged.
pub fn insert_member_at(text: &str, path: &[Seg], n: usize, member_text: &str, cut: &str) -> Option<String> {
    let o = object_at(text, path)?;
    let from_cut = cut.strip_prefix(member_text).or_else(|| cut.strip_suffix(member_text)).filter(|s| s.trim() == ",");
    let sep = match (from_cut, o.members.as_slice()) {
        (Some(s), _) => s.to_string(),
        (None, [x, y, ..]) => text[x.3..y.1].to_string(),
        _ => ", ".to_string(),
    };
    if o.members.is_empty() {
        let fill = if cut.trim() == member_text.trim() { cut } else { member_text };
        return Some(format!("{}{}{}", &text[..=o.open], fill, &text[o.close..]));
    }
    if n < o.members.len() {
        let at = o.members[n].1;
        Some(format!("{}{}{}{}", &text[..at], member_text, sep, &text[at..]))
    } else {
        let at = o.members.last()?.3;
        Some(format!("{}{}{}{}", &text[..at], sep, member_text, &text[at..]))
    }
}

/// Leading whitespace of the line where `at` sits.
pub fn indent_at(text: &str, at: usize) -> String {
    let line_start = text[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
    text[line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect()
}
