//! The XML the GPX and TCX readers walk: the file's bytes as text, and a
//! forward-only pull reader over it that reports only what they use (start
//! tags with their attributes, end tags, the end of the document).
//!
//! The reader follows the Android platform's xmlpull parser (kXML, namespace
//! processing off) the shell imported with before: a tag's name keeps its
//! prefix as written (`gpxtpx:hr`), an attribute is found by its name as
//! written, entities are expanded (the five predefined ones and character
//! references; an undefined one is malformed), text split by entities, CDATA
//! sections, comments and processing instructions reads as one, and every
//! well-formedness error it reports is malformed here too. A document that
//! declares a DTD never reaches it ([`declares_dtd`]).

use std::borrow::Cow;

/// Deepest element nesting an import reads. A GPX or TCX track nests about ten
/// levels deep (TCX `…/Trackpoint/Extensions/TPX/Speed`); a deeper file is
/// refused as damaged as soon as it gets there.
pub(super) const MAX_IMPORT_XML_DEPTH: usize = 64;

/// How much of a file the format sniff decodes to find its root element.
pub(super) const SNIFF_BYTES: usize = 4 * 1024;

/// Malformed import XML: the reader's parse error, or a document refused by
/// [`pull_tags`] (too deep, cut off). The import calls it damaged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Malformed(pub(super) &'static str);

/// The text encodings an import file is read in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Charset {
    Utf8,
    Utf16Be,
    Utf16Le,
}

/// The encoding of `bytes` and the length of its byte order mark. XML 1.0
/// §4.3.3: a UTF-16 entity starts with its byte order mark; anything else is
/// read as UTF-8 (its mark, if any, skipped).
pub(super) fn charset_of(bytes: &[u8]) -> (Charset, usize) {
    match bytes {
        [0xFE, 0xFF, ..] => (Charset::Utf16Be, 2),
        [0xFF, 0xFE, ..] => (Charset::Utf16Le, 2),
        [0xEF, 0xBB, 0xBF, ..] => (Charset::Utf8, 3),
        _ => (Charset::Utf8, 0),
    }
}

/// `bytes` as `charset` text, malformed input replaced by U+FFFD.
pub(super) fn decode(bytes: &[u8], charset: Charset) -> Cow<'_, str> {
    match charset {
        Charset::Utf8 => String::from_utf8_lossy(bytes),
        Charset::Utf16Be => Cow::Owned(decode_utf16(bytes, true)),
        Charset::Utf16Le => Cow::Owned(decode_utf16(bytes, false)),
    }
}

/// UTF-16 (`big_endian` or little-endian) as the JDK's decoder reads it with
/// replacement: a byte order mark stays a character, an unpaired surrogate (a
/// high one swallows the unit after it) and trailing bytes too short for a
/// character each read as one U+FFFD.
pub(super) fn decode_utf16(bytes: &[u8], big_endian: bool) -> String {
    let unit = |at: usize| {
        let (a, b) = (u16::from(bytes[at]), u16::from(bytes[at + 1]));
        if big_endian {
            (a << 8) | b
        } else {
            (b << 8) | a
        }
    };
    let end = bytes.len();
    let mut out = String::with_capacity(end / 2);
    let mut i = 0;
    while end - i >= 2 {
        let c = unit(i);
        match c {
            0xDC00..=0xDFFF => {
                out.push('\u{FFFD}');
                i += 2;
            }
            0xD800..=0xDBFF => {
                if end - i < 4 {
                    break;
                }
                let low = unit(i + 2);
                if (0xDC00..=0xDFFF).contains(&low) {
                    let cp = 0x10000 + ((u32::from(c) - 0xD800) << 10) + (u32::from(low) - 0xDC00);
                    out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                } else {
                    out.push('\u{FFFD}');
                }
                i += 4;
            }
            _ => {
                out.push(char::from_u32(u32::from(c)).unwrap_or('\u{FFFD}'));
                i += 2;
            }
        }
    }
    if i < end {
        out.push('\u{FFFD}');
    }
    out
}

/// Whether `text`'s prolog (what precedes the root element: the XML
/// declaration, comments, processing instructions) holds a `<!DOCTYPE`. No GPX
/// or TCX exporter writes one, and a DTD's internal entities nest into
/// exponential text (billion laughs) while its external ones read other files
/// (XXE), so such a file is refused before it is parsed (OWASP: disable DTDs
/// entirely). A `<!DOCTYPE` anywhere else is no declaration (XML 1.0 §2.8
/// allows it only in the prolog), and the reader refuses it as malformed.
pub(super) fn declares_dtd(text: &str) -> bool {
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        i = if let Some(c) = rest.chars().next().filter(|&c| super::kt_is_whitespace(c)) {
            i + c.len_utf8()
        } else if let Some(pi) = rest.strip_prefix("<?") {
            match pi.find("?>") {
                Some(k) => i + 2 + k + 2,
                None => return false,
            }
        } else if let Some(comment) = rest.strip_prefix("<!--") {
            match comment.find("-->") {
                Some(k) => i + 4 + k + 3,
                None => return false,
            }
        } else {
            return rest.starts_with("<!DOCTYPE");
        };
    }
    false
}

/// The events an import reads; text, comments and the like are passed over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Event {
    Start,
    End,
    EndDocument,
}

/// A forward-only reader over one XML document. Every call returns
/// [`Malformed`] on malformed XML.
pub(super) struct Reader<'a> {
    s: &'a str,
    pos: usize,
    /// The open elements' names, outermost first.
    stack: Vec<&'a str>,
    /// The current tag's name.
    name: &'a str,
    /// The current start tag's attributes, as written and expanded.
    attrs: Vec<(&'a str, Cow<'a, str>)>,
    /// The current tag's depth: the root element is 1, and an end tag has its
    /// start tag's depth.
    depth: usize,
    /// The current start tag closed itself (`<x/>`): its end tag comes next.
    self_closed: bool,
    current: Option<Event>,
    /// The XML declaration has been looked for (only at the very start).
    started: bool,
}

impl<'a> Reader<'a> {
    pub(super) fn new(s: &'a str) -> Self {
        Self {
            s,
            pos: 0,
            stack: Vec::new(),
            name: "",
            attrs: Vec::new(),
            depth: 0,
            self_closed: false,
            current: None,
            started: false,
        }
    }

    /// The current tag's name as written, prefix included (`gpxtpx:hr`).
    pub(super) fn name(&self) -> &'a str {
        self.name
    }

    /// The current tag's nesting depth (root 1; an end tag has its start
    /// tag's depth).
    pub(super) fn depth(&self) -> usize {
        self.depth
    }

    /// The current start tag's attribute named exactly `name` (prefix
    /// included), or `None`. A duplicated attribute reads as its last value.
    pub(super) fn attribute(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_ref())
    }

    fn rest(&self) -> &'a str {
        &self.s[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn eat(&mut self, prefix: &str) -> bool {
        let hit = self.rest().starts_with(prefix);
        if hit {
            self.pos += prefix.len();
        }
        hit
    }

    fn expect(&mut self, c: char, what: &'static str) -> Result<(), Malformed> {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            Ok(())
        } else {
            Err(Malformed(what))
        }
    }

    /// Skip XML whitespace (characters at or below U+0020, as kXML does).
    fn skip_space(&mut self) {
        while let Some(c) = self.peek() {
            if c > ' ' {
                break;
            }
            self.pos += 1;
        }
    }

    /// Advance to the next start tag, end tag or the document's end.
    pub(super) fn next(&mut self) -> Result<Event, Malformed> {
        let event = self.advance()?;
        self.current = Some(event);
        Ok(event)
    }

    fn advance(&mut self) -> Result<Event, Malformed> {
        if !self.started {
            self.started = true;
            if self.rest().starts_with("<?xml")
                && self.rest()[5..].chars().next().is_some_and(|c| c <= ' ')
            {
                self.xml_declaration()?;
            }
        }
        if self.self_closed {
            self.self_closed = false;
            self.stack.pop();
            return Ok(Event::End);
        }
        loop {
            if self.pos >= self.s.len() {
                return Ok(Event::EndDocument);
            }
            if self.eat("</") {
                return self.end_tag().map(|()| Event::End);
            }
            if self.eat("<!--") {
                self.comment()?;
            } else if self.eat("<![CDATA[") {
                if self.stack.is_empty() {
                    return Err(Malformed("CDATA outside the root element"));
                }
                self.cdata()?;
            } else if self.rest().starts_with("<!") {
                return Err(Malformed("unexpected declaration"));
            } else if self.eat("<?") {
                self.processing_instruction()?;
            } else if self.eat("<") {
                return self.start_tag().map(|()| Event::Start);
            } else {
                // Character data outside the root element is malformed unless it
                // is whitespace (characters at or below U+0020, as kXML counts it).
                let outside = self.stack.is_empty();
                let mut stray = false;
                self.text(|t| stray |= outside && t.chars().any(|c| c > ' '))?;
                if stray {
                    return Err(Malformed("text outside the root element"));
                }
            }
        }
    }

    /// `<?xml version="…" encoding="…" standalone="…"?>`: kXML's checks (the
    /// version first, then optionally the encoding and the standalone flag,
    /// nothing else). The encoding is not acted on: the text is decoded already.
    fn xml_declaration(&mut self) -> Result<(), Malformed> {
        self.pos += "<?xml".len();
        let mut attrs: Vec<(&str, Cow<'_, str>)> = Vec::new();
        loop {
            self.skip_space();
            if self.eat("?") {
                self.expect('>', "> expected")?;
                break;
            }
            let name = self.read_name()?;
            self.skip_space();
            self.expect('=', "Attr.value missing")?;
            self.skip_space();
            let value = self.attr_value()?;
            attrs.push((name, value));
        }
        let mut at = 0;
        if attrs.first().is_none_or(|(n, _)| *n != "version") {
            return Err(Malformed("version expected"));
        }
        at += 1;
        if attrs.get(at).is_some_and(|(n, _)| *n == "encoding") {
            at += 1;
        }
        if let Some((n, v)) = attrs.get(at)
            && *n == "standalone"
        {
            if v != "yes" && v != "no" {
                return Err(Malformed("illegal standalone value"));
            }
            at += 1;
        }
        if at != attrs.len() {
            return Err(Malformed("unexpected attributes in XML declaration"));
        }
        Ok(())
    }

    fn comment(&mut self) -> Result<(), Malformed> {
        match self.rest().find("--") {
            Some(k) => {
                self.pos += k + 2;
                self.expect('>', "illegal comment delimiter: --")
            }
            None => Err(Malformed("unterminated comment")),
        }
    }

    fn cdata(&mut self) -> Result<&'a str, Malformed> {
        let rest = self.rest();
        match rest.find("]]>") {
            Some(k) => {
                self.pos += k + 3;
                Ok(&rest[..k])
            }
            None => Err(Malformed("unterminated CDATA")),
        }
    }

    /// A processing instruction, passed over. Its target may not be `xml`: the
    /// XML declaration belongs only at the very start of the document.
    fn processing_instruction(&mut self) -> Result<(), Malformed> {
        let rest = self.rest();
        let target_end = rest
            .find(|c: char| c <= ' ' || c == '?')
            .unwrap_or(rest.len());
        if rest[..target_end].eq_ignore_ascii_case("xml") {
            return Err(Malformed("processing instruction 'xml' not allowed here"));
        }
        match rest.find("?>") {
            Some(k) => {
                self.pos += k + 2;
                Ok(())
            }
            None => Err(Malformed("unterminated processing instruction")),
        }
    }

    /// A name as kXML reads one: a first character `[A-Za-z_:]` or from
    /// U+00C0 on, then `[A-Za-z0-9_\-:.]` or from U+00B7 on.
    fn read_name(&mut self) -> Result<&'a str, Malformed> {
        let rest = self.rest();
        let mut chars = rest.char_indices();
        match chars.next() {
            Some((_, c)) if c.is_ascii_alphabetic() || c == '_' || c == ':' || c >= '\u{C0}' => {}
            _ => return Err(Malformed("name expected")),
        }
        let end = chars
            .find(|&(_, c)| {
                !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ':' | '.') || c >= '\u{B7}')
            })
            .map_or(rest.len(), |(k, _)| k);
        self.pos += end;
        Ok(&rest[..end])
    }

    fn start_tag(&mut self) -> Result<(), Malformed> {
        let name = self.read_name()?;
        self.attrs.clear();
        loop {
            self.skip_space();
            match self.peek() {
                None => return Err(Malformed("unexpected EOF")),
                Some('/') => {
                    self.pos += 1;
                    self.skip_space();
                    self.expect('>', "> expected")?;
                    self.self_closed = true;
                    break;
                }
                Some('>') => {
                    self.pos += 1;
                    break;
                }
                Some(_) => {}
            }
            let attr = self.read_name()?;
            self.skip_space();
            self.expect('=', "Attr.value missing")?;
            self.skip_space();
            let value = self.attr_value()?;
            self.attrs.push((attr, value));
        }
        self.stack.push(name);
        self.name = name;
        self.depth = self.stack.len();
        Ok(())
    }

    /// A quoted attribute value, entities expanded and line breaks read as
    /// spaces; a `<` inside it is malformed.
    fn attr_value(&mut self) -> Result<Cow<'a, str>, Malformed> {
        let quote = match self.peek() {
            Some(q @ ('"' | '\'')) => q,
            _ => return Err(Malformed("attr value delimiter missing!")),
        };
        self.pos += 1;
        let start = self.pos;
        let mut out: Option<String> = None;
        loop {
            let Some(c) = self.peek() else {
                return Err(Malformed("unexpected EOF"));
            };
            match c {
                c if c == quote => {
                    let raw = &self.s[start..self.pos];
                    self.pos += 1;
                    return Ok(out.map_or(Cow::Borrowed(raw), Cow::Owned));
                }
                '<' => return Err(Malformed("Illegal: \"<\" inside attribute value")),
                '&' => {
                    let buf = out.get_or_insert_with(|| self.s[start..self.pos].to_owned());
                    let expanded = self.entity()?;
                    buf.push_str(&expanded);
                }
                '\r' | '\n' | '\t' => {
                    let buf = out.get_or_insert_with(|| self.s[start..self.pos].to_owned());
                    if !(c == '\n' && buf.ends_with(' ') && self.s[..self.pos].ends_with('\r')) {
                        buf.push(' ');
                    }
                    self.pos += 1;
                }
                _ => {
                    if let Some(buf) = out.as_mut() {
                        buf.push(c);
                    }
                    self.pos += c.len_utf8();
                }
            }
        }
    }

    /// `&…;` at the read position: the predefined entities and character
    /// references; anything else is malformed.
    fn entity(&mut self) -> Result<Cow<'static, str>, Malformed> {
        self.pos += 1;
        let rest = self.rest();
        let Some(end) = rest.find(';') else {
            return Err(Malformed("unterminated entity ref"));
        };
        let body = &rest[..end];
        if body.is_empty()
            || !body.chars().all(|c| {
                c.is_ascii_alphanumeric()
                    || matches!(c, '_' | '-' | ':' | '.' | '#')
                    || c >= '\u{B7}'
            })
        {
            return Err(Malformed("unterminated entity ref"));
        }
        self.pos += end + 1;
        let expanded = match body {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            _ => {
                let code = if let Some(hex) = body.strip_prefix("#x") {
                    u32::from_str_radix(hex, 16).ok()
                } else if let Some(dec) = body.strip_prefix('#') {
                    dec.parse::<u32>().ok()
                } else {
                    return Err(Malformed("unresolved entity"));
                };
                let code = code
                    .filter(|&c| c <= 0x10_FFFF)
                    .ok_or(Malformed("bad character reference"))?;
                // A lone surrogate is a valid Java char but no Rust char.
                return Ok(Cow::Owned(
                    char::from_u32(code).unwrap_or('\u{FFFD}').to_string(),
                ));
            }
        };
        Ok(Cow::Borrowed(expanded))
    }

    /// Character data up to the next `<`, entities expanded, handed to `sink`.
    fn text(&mut self, mut sink: impl FnMut(&str)) -> Result<(), Malformed> {
        loop {
            let rest = self.rest();
            let stop = rest.find(['<', '&']).unwrap_or(rest.len());
            let chunk = &rest[..stop];
            if chunk.contains("]]>") {
                return Err(Malformed("Illegal: \"]]>\" outside CDATA section"));
            }
            sink(chunk);
            self.pos += stop;
            if self.peek() == Some('&') {
                let expanded = self.entity()?;
                sink(&expanded);
            } else {
                return Ok(());
            }
        }
    }

    fn end_tag(&mut self) -> Result<(), Malformed> {
        let name = self.read_name()?;
        self.skip_space();
        self.expect('>', "> expected")?;
        match self.stack.last() {
            Some(open) if *open == name => {}
            _ => return Err(Malformed("end tag name does not match")),
        }
        self.name = name;
        self.depth = self.stack.len();
        self.stack.pop();
        Ok(())
    }

    /// The current start tag's text content, leaving the reader on its end
    /// tag. A child element inside it is malformed for this call.
    pub(super) fn next_text(&mut self) -> Result<String, Malformed> {
        if self.current != Some(Event::Start) {
            return Err(Malformed("precondition: START_TAG"));
        }
        let mut out = String::new();
        if self.self_closed {
            self.self_closed = false;
            self.stack.pop();
            self.current = Some(Event::End);
            return Ok(out);
        }
        loop {
            if self.pos >= self.s.len() {
                return Err(Malformed("END_TAG expected"));
            }
            if self.eat("</") {
                self.end_tag()?;
                self.current = Some(Event::End);
                return Ok(normalize_breaks(out));
            }
            if self.eat("<!--") {
                self.comment()?;
            } else if self.eat("<![CDATA[") {
                out.push_str(self.cdata()?);
            } else if self.eat("<?") {
                self.processing_instruction()?;
            } else if self.rest().starts_with('<') {
                return Err(Malformed("END_TAG expected"));
            } else {
                self.text(|t| out.push_str(t))?;
            }
        }
    }
}

/// `\r\n` and a lone `\r` read as `\n` (XML 1.0 §2.11).
fn normalize_breaks(s: String) -> String {
    if s.contains('\r') {
        s.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        s
    }
}

/// Pulls `text` through `on_tag`: every start and end tag with its local name
/// (the prefix stripped, so `gpxtpx:hr` reads as `hr`). A file nesting deeper
/// than [`MAX_IMPORT_XML_DEPTH`] is malformed. A cut-off download often ends
/// right after a tag, where the reader reaches the document's end without an
/// error, so only a closed `root` proves the file is whole; anything else is
/// malformed too.
pub(super) fn pull_tags<'a>(
    text: &'a str,
    root: &str,
    mut on_tag: impl FnMut(&mut Reader<'a>, Event, &'a str) -> Result<(), Malformed>,
) -> Result<(), Malformed> {
    let mut reader = Reader::new(text);
    let mut root_closed = false;
    let mut event = reader.next()?;
    while event != Event::EndDocument {
        if event == Event::Start && reader.depth() > MAX_IMPORT_XML_DEPTH {
            return Err(Malformed("elements nest too deep"));
        }
        let name = reader.name();
        let local = name.rsplit_once(':').map_or(name, |(_, l)| l);
        if event == Event::End && local == root && reader.depth() == 1 {
            root_closed = true;
        }
        on_tag(&mut reader, event, local)?;
        event = reader.next()?;
    }
    if root_closed {
        Ok(())
    } else {
        Err(Malformed(
            "the document ends before its root element closes",
        ))
    }
}
