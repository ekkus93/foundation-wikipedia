//! Bounded incremental XML markup-nesting guard for provisional public dumps.
//!
//! This deliberately accepts a conservative ASCII-element-name subset used by
//! Wikimedia dump envelopes. It checks nesting, quoted attributes, comments,
//! CDATA, and one nonempty <mediawiki> root. It rejects DTDs and external
//! entities. It is NOT a complete XML parser: namespace semantics, page
//! metadata and publisher completeness require independent validation.
//! Use behind XmlTextGuard and never publish provisional output on its own.

use std::collections::BTreeSet;
use std::io::{self, Write};

const MAX_TAG_BYTES: usize = 64 * 1024;
const MAX_DEPTH: usize = 64;
const MAX_WRITE: usize = 8192;
const MAX_ENTITY_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Text,
    Tag,
    Comment,
    Cdata,
    ProcessingInstruction,
}

pub struct XmlNestingGuard<W> {
    output: W,
    mode: Mode,
    token: Vec<u8>,
    entity: Vec<u8>,
    stack: Vec<String>,
    root_seen: bool,
    root_closed: bool,
    pages: u64,
    // XML 1.0 forbids the CDATA terminator in ordinary character data.
    text_right_brackets: u8,
    failed: bool,
}

impl<W: Write> XmlNestingGuard<W> {
    pub fn new(output: W) -> Self {
        Self {
            output,
            mode: Mode::Text,
            token: Vec::new(),
            entity: Vec::new(),
            stack: Vec::new(),
            root_seen: false,
            root_closed: false,
            pages: 0,
            text_right_brackets: 0,
            failed: false,
        }
    }

    fn invalid(&mut self) -> io::Error {
        self.failed = true;
        io::Error::new(io::ErrorKind::InvalidData, "invalid XML markup envelope")
    }

    fn name(bytes: &[u8]) -> bool {
        !bytes.is_empty()
            && matches!(bytes[0], b'a'..=b'z' | b'A'..=b'Z' | b'_' | b':')
            && bytes.iter().skip(1).all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(*byte, b'_' | b':' | b'.' | b'-')
            })
    }

    fn valid_entity(bytes: &[u8]) -> bool {
        if [b"&amp;".as_slice(), b"&lt;", b"&gt;", b"&apos;", b"&quot;"].contains(&bytes) {
            return true;
        }
        let Some(raw) = bytes.strip_prefix(b"&#").and_then(|b| b.strip_suffix(b";")) else {
            return false;
        };
        let (digits, radix) = if let Some(hex) = raw.strip_prefix(b"x") {
            (hex, 16)
        } else {
            (raw, 10)
        };
        if digits.is_empty()
            || !digits.iter().all(|b| {
                if radix == 16 {
                    b.is_ascii_hexdigit()
                } else {
                    b.is_ascii_digit()
                }
            })
        {
            return false;
        }
        let Ok(text) = std::str::from_utf8(digits) else {
            return false;
        };
        let Ok(code) = u32::from_str_radix(text, radix) else {
            return false;
        };
        matches!(code, 9 | 10 | 13)
            || (0x20..=0xd7ff).contains(&code)
            || (0xe000..=0xfffd).contains(&code)
            || (0x10000..=0x10ffff).contains(&code)
    }

    fn valid_attribute_value(bytes: &[u8]) -> bool {
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'<' {
                return false;
            }
            if bytes[index] == b'&' {
                let Some(end) = bytes[index..].iter().position(|b| *b == b';') else {
                    return false;
                };
                let end = index + end + 1;
                if end - index > MAX_ENTITY_BYTES || !Self::valid_entity(&bytes[index..end]) {
                    return false;
                }
                index = end;
            } else {
                index += 1;
            }
        }
        true
    }

    /// The XML text guard only accepts UTF-8 and XML 1.0, so a declaration
    /// must not advertise a different encoding or XML character model.
    fn valid_xml_declaration(token: &[u8]) -> bool {
        let Some(mut rest) = token
            .strip_prefix(b"<?xml")
            .and_then(|body| body.strip_suffix(b"?>"))
        else {
            return false;
        };
        let mut stage = 0u8;
        while !rest.is_empty() {
            let spaces = rest.iter().take_while(|b| b.is_ascii_whitespace()).count();
            if spaces == 0 {
                return false;
            }
            rest = &rest[spaces..];
            if rest.is_empty() {
                break;
            }
            let end = rest
                .iter()
                .position(|b| b.is_ascii_whitespace() || *b == b'=')
                .unwrap_or(rest.len());
            let name = &rest[..end];
            rest = &rest[end..];
            let spaces = rest.iter().take_while(|b| b.is_ascii_whitespace()).count();
            rest = &rest[spaces..];
            if !rest.starts_with(b"=") {
                return false;
            }
            rest = &rest[1..];
            let spaces = rest.iter().take_while(|b| b.is_ascii_whitespace()).count();
            rest = &rest[spaces..];
            let Some(&quote) = rest.first() else {
                return false;
            };
            if quote != b'\'' && quote != b'"' {
                return false;
            }
            rest = &rest[1..];
            let Some(end) = rest.iter().position(|b| *b == quote) else {
                return false;
            };
            let value = &rest[..end];
            rest = &rest[end + 1..];
            match name {
                b"version" if stage == 0 && value == b"1.0" => stage = 1,
                b"encoding" if stage == 1 && value.eq_ignore_ascii_case(b"utf-8") => stage = 2,
                b"standalone" if (stage == 1 || stage == 2) && (value == b"yes" || value == b"no") => {
                    stage = 3;
                }
                _ => return false,
            }
        }
        stage != 0
    }

    fn attributes(mut bytes: &[u8]) -> bool {
        let mut seen = BTreeSet::new();
        while !bytes.is_empty() {
            let whitespace = bytes.iter().take_while(|b| b.is_ascii_whitespace()).count();
            if whitespace == 0 {
                return false;
            }
            bytes = &bytes[whitespace..];
            if bytes.is_empty() {
                return true;
            }
            let name_end = bytes
                .iter()
                .position(|b| b.is_ascii_whitespace() || *b == b'=')
                .unwrap_or(bytes.len());
            if !Self::name(&bytes[..name_end]) || !seen.insert(&bytes[..name_end]) {
                return false;
            }
            bytes = &bytes[name_end..];
            let whitespace = bytes.iter().take_while(|b| b.is_ascii_whitespace()).count();
            bytes = &bytes[whitespace..];
            if !bytes.starts_with(b"=") {
                return false;
            }
            bytes = &bytes[1..];
            let whitespace = bytes.iter().take_while(|b| b.is_ascii_whitespace()).count();
            bytes = &bytes[whitespace..];
            let Some(&quote) = bytes.first() else {
                return false;
            };
            if quote != b'\'' && quote != b'"' {
                return false;
            }
            bytes = &bytes[1..];
            let Some(end) = bytes.iter().position(|b| *b == quote) else {
                return false;
            };
            if !Self::valid_attribute_value(&bytes[..end]) {
                return false;
            }
            bytes = &bytes[end + 1..];
        }
        true
    }

    fn parse_tag(&mut self, token: &[u8]) -> io::Result<()> {
        let body = &token[1..token.len() - 1];
        let closing = body.starts_with(b"/");
        let body = if closing { &body[1..] } else { body };
        let body = body.trim_ascii_end();
        let self_closing = !closing && body.ends_with(b"/");
        let body = if self_closing {
            body[..body.len() - 1].trim_ascii_end()
        } else {
            body
        };
        let end = body
            .iter()
            .position(|b| b.is_ascii_whitespace())
            .unwrap_or(body.len());
        let name = &body[..end];
        if !Self::name(name)
            || (closing && end != body.len())
            || (!closing && !Self::attributes(&body[end..]))
        {
            return Err(self.invalid());
        }
        let name = std::str::from_utf8(name).map_err(|_| self.invalid())?;
        if closing {
            if self.stack.pop().as_deref() != Some(name) {
                return Err(self.invalid());
            }
            if self.stack.is_empty() {
                self.root_closed = true;
            }
            return Ok(());
        }
        if self.stack.is_empty() {
            if self.root_seen || name != "mediawiki" || self_closing {
                return Err(self.invalid());
            }
            self.root_seen = true;
        } else if self.stack.len() == 1 && name == "page" {
            if self_closing {
                return Err(self.invalid());
            }
            self.pages = self.pages.checked_add(1).ok_or_else(|| self.invalid())?;
        }
        if !self_closing {
            if self.stack.len() >= MAX_DEPTH {
                return Err(self.invalid());
            }
            self.stack.push(name.to_owned());
        }
        Ok(())
    }

    fn byte(&mut self, byte: u8) -> io::Result<()> {
        match self.mode {
            Mode::Text => {
                // Track raw text across write/chunk boundaries. Entity references
                // and markup reset the sequence; CDATA is handled separately.
                if byte == b'>' && self.text_right_brackets == 2 {
                    return Err(self.invalid());
                }
                self.text_right_brackets = if byte == b']' {
                    (self.text_right_brackets + 1).min(2)
                } else {
                    0
                };
                if !self.entity.is_empty() {
                    self.entity.push(byte);
                    if self.entity.len() > MAX_ENTITY_BYTES {
                        return Err(self.invalid());
                    }
                    if byte == b';' {
                        if !Self::valid_entity(&self.entity) {
                            return Err(self.invalid());
                        }
                        self.entity.clear();
                    } else if !byte.is_ascii_alphanumeric() && byte != b'#' {
                        return Err(self.invalid());
                    }
                } else if byte == b'&' {
                    if self.stack.is_empty() {
                        return Err(self.invalid());
                    }
                    self.entity.push(byte);
                } else if byte == b'<' {
                    self.mode = Mode::Tag;
                    self.token.clear();
                    self.token.push(byte);
                } else if self.stack.is_empty() && !byte.is_ascii_whitespace() {
                    return Err(self.invalid());
                }
            }
            Mode::Tag => {
                self.token.push(byte);
                if self.token.len() > MAX_TAG_BYTES {
                    return Err(self.invalid());
                }
                if self.token == b"<!--" {
                    self.mode = Mode::Comment;
                    self.token.clear();
                } else if self.token == b"<![CDATA[" {
                    if self.stack.is_empty() {
                        return Err(self.invalid());
                    }
                    self.mode = Mode::Cdata;
                    self.token.clear();
                } else if self.token.starts_with(b"<!") {
                    if !b"<!--".starts_with(&self.token) && !b"<![CDATA[".starts_with(&self.token) {
                        return Err(self.invalid());
                    }
                } else if self.token.starts_with(b"<?") {
                    self.mode = Mode::ProcessingInstruction;
                } else if byte == b'>' {
                    let mut quote = None;
                    for &b in &self.token[1..self.token.len() - 1] {
                        if let Some(q) = quote {
                            if b == q {
                                quote = None;
                            }
                        } else if b == b'\'' || b == b'"' {
                            quote = Some(b);
                        }
                    }
                    if quote.is_none() {
                        let token = std::mem::take(&mut self.token);
                        self.parse_tag(&token)?;
                        self.mode = Mode::Text;
                    }
                }
            }
            Mode::ProcessingInstruction => {
                self.token.push(byte);
                if self.token.len() > MAX_TAG_BYTES {
                    return Err(self.invalid());
                }
                if self.token.ends_with(b"?>") {
                    if self.root_seen {
                        return Err(self.invalid());
                    }
                    let body = &self.token[2..self.token.len() - 2];
                    let target_end = body
                        .iter()
                        .position(|b| b.is_ascii_whitespace() || *b == b'?')
                        .unwrap_or(body.len());
                    let target = &body[..target_end];
                    if target.eq_ignore_ascii_case(b"xml")
                        && (target != b"xml" || !Self::valid_xml_declaration(&self.token))
                    {
                        return Err(self.invalid());
                    }
                    self.mode = Mode::Text;
                    self.token.clear();
                }
            }
            Mode::Comment | Mode::Cdata => {
                if self.mode == Mode::Comment && self.token.ends_with(b"--") && byte != b'>' {
                    return Err(self.invalid());
                }
                self.token.push(byte);
                let terminator = if self.mode == Mode::Comment {
                    b"-->".as_slice()
                } else {
                    b"]]>".as_slice()
                };
                if self.token.ends_with(terminator) {
                    self.mode = Mode::Text;
                    self.token.clear();
                } else if self.token.len() > 3 {
                    self.token.remove(0);
                }
            }
        }
        Ok(())
    }

    /// The output is provisional; only a successful finish certifies this
    /// limited envelope grammar. A failure permanently poisons the writer.
    pub fn finish(mut self) -> io::Result<W> {
        if self.failed
            || self.mode != Mode::Text
            || !self.root_seen
            || !self.root_closed
            || !self.stack.is_empty()
            || !self.entity.is_empty()
            || self.pages == 0
        {
            return Err(self.invalid());
        }
        self.output.flush().inspect_err(|_| self.failed = true)?;
        Ok(self.output)
    }
}

impl<W: Write> Write for XmlNestingGuard<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.failed {
            return Err(self.invalid());
        }
        let count = bytes.len().min(MAX_WRITE);
        for &byte in &bytes[..count] {
            self.byte(byte)?;
        }
        if let Err(error) = self.output.write_all(&bytes[..count]) {
            self.failed = true;
            return Err(error);
        }
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.failed {
            return Err(self.invalid());
        }
        self.output.flush().inspect_err(|_| self.failed = true)
    }
}

/// Compose bounded compressed-input/output receipts with incremental UTF-8,
/// XML-character and markup-envelope checks. The callback still must perform
/// actual bzip2 decoding and full XML/page semantic validation. No bytes may
/// be published until a complete, authenticated multi-member import qualifies.
pub fn decode_checked_member_with_xml_envelope<R, W, F, T, E>(
    input: R,
    member: &crate::public_dump::DumpChecksum,
    max_compressed_bytes: u64,
    output: W,
    max_decoded_bytes: u64,
    decoder: F,
) -> Result<(W, u64, T), crate::public_dump_checked_read::CheckedMemberError<E>>
where
    R: std::io::Read,
    W: Write,
    F: FnOnce(&mut dyn std::io::Read, &mut dyn Write) -> Result<T, E>,
{
    use crate::public_dump_checked_read::{
        decode_checked_member_with_bounded_output, CheckedMemberError,
    };
    use crate::public_dump_xml_text::XmlTextGuard;

    let mut invalid_document = false;
    let result = decode_checked_member_with_bounded_output(
        input,
        member,
        max_compressed_bytes,
        output,
        max_decoded_bytes,
        |reader, writer| {
            let mut text = XmlTextGuard::new(XmlNestingGuard::new(writer));
            let decoded = decoder(reader, &mut text);
            match text.finish() {
                Ok((structure, _)) => {
                    if structure.finish().is_err() {
                        invalid_document = true;
                    }
                }
                Err(_) => invalid_document = true,
            }
            decoded
        },
    );
    match result {
        Ok(_) if invalid_document => Err(CheckedMemberError::InvalidXmlDocument),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::public_dump::DumpChecksum;
    use crate::public_dump_checked_read::CheckedMemberError;
    use sha2::{Digest, Sha256};
    use std::io::{Cursor, Read};

    const VALID: &str = "<?xml version=\"1.0\"?><mediawiki lang=\"ar\"><siteinfo/>\
        <!-- comment > --> <page><title>日本語</title><revision><text><![CDATA[a > b]]>\
        </text></revision></page></mediawiki>";

    #[test]
    fn validates_chunked_document_and_returns_provisional_bytes() {
        for chunk in [1, 2, 7, 8192] {
            let mut guard = XmlNestingGuard::new(Vec::new());
            for part in VALID.as_bytes().chunks(chunk) {
                guard.write_all(part).unwrap();
            }
            assert_eq!(guard.finish().unwrap(), VALID.as_bytes());
        }
    }

    #[test]
    fn rejects_truncated_unbalanced_duplicate_root_and_dtd() {
        for bad in [
            "<mediawiki><page></mediawiki>",
            "<mediawiki><page></page>",
            "<mediawiki><page></page></mediawiki><mediawiki/>",
            "<!DOCTYPE mediawiki><mediawiki><page/></mediawiki>",
            "<mediawiki><page/></mediawiki>",
            "<mediawiki><page></page></mediawiki>trailer",
            "<mediawiki><page><x></page></mediawiki>",
            "<mediawiki><page a='1' a='2'></page></mediawiki>",
            "<mediawiki><page a=1></page></mediawiki>",
            "<mediawiki><page><!-- unfinished",
        ] {
            let mut guard = XmlNestingGuard::new(Vec::new());
            let written = guard.write_all(bad.as_bytes());
            assert!(written.is_err() || guard.finish().is_err(), "{bad}");
        }
    }

    #[test]
    fn accepts_named_and_numeric_entities_split_at_all_boundaries() {
        let xml = "<mediawiki lang='fr&amp;ar'><page><text>&lt;&gt;&quot;&apos;&amp;&#65;&#x1F600;</text></page></mediawiki>";
        for size in [1, 2, 3, 11] {
            let mut guard = XmlNestingGuard::new(Vec::new());
            for chunk in xml.as_bytes().chunks(size) {
                guard.write_all(chunk).unwrap();
            }
            assert_eq!(guard.finish().unwrap(), xml.as_bytes());
        }
    }

    #[test]
    fn xml_declaration_must_match_utf8_xml10_decoder() {
        for good in [
            "<?xml version='1.0'?><mediawiki><page></page></mediawiki>",
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone='yes'?><mediawiki><page></page></mediawiki>",
            "<?xml-stylesheet href='x'?><mediawiki><page></page></mediawiki>",
        ] {
            let mut guard = XmlNestingGuard::new(Vec::new());
            for chunk in good.as_bytes().chunks(1) {
                guard.write_all(chunk).unwrap();
            }
            assert_eq!(guard.finish().unwrap(), good.as_bytes());
        }
        for bad in [
            "<?xml version='1.1'?><mediawiki><page></page></mediawiki>",
            "<?xml version='1.0' encoding='ISO-8859-1'?><mediawiki><page></page></mediawiki>",
            "<?xml version='1.0' encoding='UTF-16'?><mediawiki><page></page></mediawiki>",
            "<?xml encoding='utf-8'?><mediawiki><page></page></mediawiki>",
            "<?xml version='1.0' standalone='maybe'?><mediawiki><page></page></mediawiki>",
            "<?xml version='1.0' version='1.0'?><mediawiki><page></page></mediawiki>",
            "<?XML version='1.0'?><mediawiki><page></page></mediawiki>",
        ] {
            let mut guard = XmlNestingGuard::new(Vec::new());
            let result = bad
                .as_bytes()
                .chunks(2)
                .try_for_each(|chunk| guard.write_all(chunk));
            assert!(result.is_err() || guard.finish().is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_raw_cdata_terminator_in_text_across_chunks() {
        for bad in [
            "<mediawiki><page><text>bad ]]></text></page></mediawiki>",
            "<mediawiki><page><text>bad ]]]></text></page></mediawiki>",
        ] {
            for size in [1, 2, 3, 8192] {
                let mut guard = XmlNestingGuard::new(Vec::new());
                let result = bad
                    .as_bytes()
                    .chunks(size)
                    .try_for_each(|chunk| guard.write_all(chunk));
                assert!(result.is_err() || guard.finish().is_err(), "{bad}");
            }
        }
        for good in [
            "<mediawiki><page><text>literal ]]&gt;</text></page></mediawiki>",
            "<mediawiki><page><text><![CDATA[literal ]]>]]&gt;</text></page></mediawiki>",
        ] {
            let mut guard = XmlNestingGuard::new(Vec::new());
            for chunk in good.as_bytes().chunks(1) {
                guard.write_all(chunk).unwrap();
            }
            assert_eq!(guard.finish().unwrap(), good.as_bytes());
        }
    }

    #[test]
    fn rejects_invalid_entities_and_comment_double_hyphen() {
        for bad in [
            "<mediawiki><page><text>bare &amp</text></page></mediawiki>",
            "<mediawiki><page><text>&bogus;</text></page></mediawiki>",
            "<mediawiki><page><text>&#0;</text></page></mediawiki>",
            "<mediawiki><page><text>&#xD800;</text></page></mediawiki>",
            "<mediawiki><page><text>&#x110000;</text></page></mediawiki>",
            "<mediawiki><page><text>&#xGG;</text></page></mediawiki>",
            "<mediawiki><page><text>&#;</text></page></mediawiki>",
            "<mediawiki><page title='bare &amp'></page></mediawiki>",
            "<mediawiki><page title='&bogus;'></page></mediawiki>",
            "<mediawiki><!-- illegal -- text --><page></page></mediawiki>",
        ] {
            let mut guard = XmlNestingGuard::new(Vec::new());
            let written = guard.write_all(bad.as_bytes());
            assert!(written.is_err() || guard.finish().is_err(), "{bad}");
        }
    }

    #[test]
    fn checked_composition_catches_swallowed_structure_and_utf8_errors() {
        let compressed = b"synthetic, not bzip2";
        let member = DumpChecksum {
            relative_path: "enwiki-2026-10-01-p1p100.xml.bz2".into(),
            sha256: format!("{:x}", Sha256::digest(compressed)),
        };
        let decode = |xml: &'static [u8]| {
            move |reader: &mut dyn Read, writer: &mut dyn Write| {
                let mut compressed_bytes = Vec::new();
                reader.read_to_end(&mut compressed_bytes)?;
                let _ = writer.write_all(xml);
                Ok::<_, io::Error>(())
            }
        };
        let valid = decode_checked_member_with_xml_envelope(
            Cursor::new(compressed),
            &member,
            1024,
            Vec::new(),
            4096,
            decode(VALID.as_bytes()),
        )
        .unwrap();
        assert_eq!(valid.0, VALID.as_bytes());
        for invalid in [
            b"<mediawiki><page></mediawiki>".as_slice(),
            b"<mediawiki><page>\xff</page></mediawiki>",
            b"<mediawiki><page></page></mediawiki>\xe2",
        ] {
            assert!(matches!(
                decode_checked_member_with_xml_envelope(
                    Cursor::new(compressed),
                    &member,
                    1024,
                    Vec::new(),
                    4096,
                    decode(invalid),
                ),
                Err(CheckedMemberError::InvalidXmlDocument)
            ));
        }
    }
}
