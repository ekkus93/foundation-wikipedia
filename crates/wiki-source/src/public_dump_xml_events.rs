//! Streaming extraction of current-content page identities from validated dump XML.
//!
//! This is a narrow MediaWiki page-event adapter, not a general XML parser.
//! It must be downstream of XmlTextGuard and XmlNestingGuard; those guards
//! validate UTF-8, markup and entity syntax. This adapter distinguishes
//! page/id, revision/id and contributor/id without buffering revision text.
//! Callbacks are PROVISIONAL until compressed digest, complete XML, shard
//! census and upstream authenticity have all been verified.

use std::io::{self, Write};

use crate::public_dump_page_meta::{PageEvent, PageMetadataCollector, PublicDumpPage};

const MAX_MARKUP_BYTES: usize = 64 * 1024;
const MAX_FIELD_BYTES: usize = 16 * 1024;
const MAX_ENTITY_BYTES: usize = 32;
const MAX_WRITE_BYTES: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Text,
    Markup,
    Comment,
    Cdata,
    Instruction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Title,
    Namespace,
    PageId,
    RevisionId,
    Timestamp,
}

/// Converts validated XML bytes into provisional, bounded page records.
/// A rejected semantic field or callback permanently poisons the writer.
pub struct XmlPageMetadataWriter<F> {
    callback: F,
    collector: PageMetadataCollector,
    path: Vec<String>,
    mode: Mode,
    markup: Vec<u8>,
    quote: Option<u8>,
    field: Option<Field>,
    field_bytes: Vec<u8>,
    tail: Vec<u8>,
    pages: u64,
    failed: bool,
}

impl<F: FnMut(PublicDumpPage) -> io::Result<()>> XmlPageMetadataWriter<F> {
    pub fn new(callback: F) -> Self {
        Self {
            callback,
            collector: PageMetadataCollector::new(),
            path: Vec::new(),
            mode: Mode::Text,
            markup: Vec::new(),
            quote: None,
            field: None,
            field_bytes: Vec::new(),
            tail: Vec::new(),
            pages: 0,
            failed: false,
        }
    }

    fn invalid(&mut self) -> io::Error {
        self.failed = true;
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid MediaWiki page semantics",
        )
    }

    fn at(&self, path: &[&str]) -> bool {
        self.path.len() == path.len()
            && self
                .path
                .iter()
                .zip(path)
                .all(|(actual, expected)| actual.as_str() == *expected)
    }

    fn emit(&mut self, event: PageEvent<'_>) -> io::Result<()> {
        let page = match self.collector.push(event) {
            Ok(page) => page,
            Err(_) => return Err(self.invalid()),
        };
        if let Some(page) = page {
            if let Err(error) = (self.callback)(page) {
                self.failed = true;
                return Err(error);
            }
            self.pages = self.pages.checked_add(1).ok_or_else(|| self.invalid())?;
        }
        Ok(())
    }

    fn xml_character(code: u32) -> bool {
        matches!(code, 9 | 10 | 13)
            || (0x20..=0xd7ff).contains(&code)
            || (0xe000..=0xfffd).contains(&code)
            || (0x10000..=0x10ffff).contains(&code)
    }

    fn decode_entities(raw: &[u8]) -> Option<String> {
        let text = std::str::from_utf8(raw).ok()?;
        let mut result = String::new();
        let mut rest = text;
        while let Some(start) = rest.find('&') {
            result.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            let end = after.find(';')?;
            if end + 2 > MAX_ENTITY_BYTES {
                return None;
            }
            let entity = &after[..end];
            let decoded = match entity {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let (digits, radix) = if let Some(digits) = entity.strip_prefix("#x") {
                        (digits, 16)
                    } else if let Some(digits) = entity.strip_prefix('#') {
                        (digits, 10)
                    } else {
                        return None;
                    };
                    if digits.is_empty()
                        || !digits.bytes().all(|byte| {
                            if radix == 16 {
                                byte.is_ascii_hexdigit()
                            } else {
                                byte.is_ascii_digit()
                            }
                        })
                    {
                        return None;
                    }
                    let code = u32::from_str_radix(digits, radix).ok()?;
                    if !Self::xml_character(code) {
                        return None;
                    }
                    char::from_u32(code)?
                }
            };
            result.push(decoded);
            rest = &after[end + 1..];
        }
        result.push_str(rest);
        Some(result)
    }

    fn redirect_title(mut attrs: &[u8]) -> Option<String> {
        let mut title = None;
        while !attrs.is_empty() {
            attrs = attrs.trim_ascii_start();
            if attrs.is_empty() {
                break;
            }
            let end = attrs
                .iter()
                .position(|byte| byte.is_ascii_whitespace() || *byte == b'=')
                .unwrap_or(attrs.len());
            if end == 0 {
                return None;
            }
            let key = &attrs[..end];
            attrs = attrs[end..].trim_ascii_start();
            attrs = attrs.strip_prefix(b"=")?.trim_ascii_start();
            let quote = *attrs.first()?;
            if quote != b'\'' && quote != b'"' {
                return None;
            }
            attrs = &attrs[1..];
            let end = attrs.iter().position(|byte| *byte == quote)?;
            if key == b"title" {
                if title.is_some() {
                    return None;
                }
                title = Some(Self::decode_entities(&attrs[..end])?);
            }
            attrs = &attrs[end + 1..];
        }
        title
    }

    fn finish_field(&mut self) -> io::Result<()> {
        let field = self.field.take().ok_or_else(|| self.invalid())?;
        let raw = std::mem::take(&mut self.field_bytes);
        let value = Self::decode_entities(&raw).ok_or_else(|| self.invalid())?;
        match field {
            Field::Title => self.emit(PageEvent::Title(&value)),
            Field::Namespace => {
                if !value.bytes().all(|byte| byte.is_ascii_digit()) || value.is_empty() {
                    return Err(self.invalid());
                }
                let number = value.parse::<i32>().map_err(|_| self.invalid())?;
                self.emit(PageEvent::Namespace(number))
            }
            Field::PageId | Field::RevisionId => {
                if !value.bytes().all(|byte| byte.is_ascii_digit()) || value.is_empty() {
                    return Err(self.invalid());
                }
                let number = value.parse::<u64>().map_err(|_| self.invalid())?;
                if field == Field::PageId {
                    self.emit(PageEvent::PageId(number))
                } else {
                    self.emit(PageEvent::RevisionId(number))
                }
            }
            Field::Timestamp => self.emit(PageEvent::Timestamp(&value)),
        }
    }

    fn tag(&mut self, token: &[u8]) -> io::Result<()> {
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
            .position(u8::is_ascii_whitespace)
            .unwrap_or(body.len());
        let name = std::str::from_utf8(&body[..end]).map_err(|_| self.invalid())?;
        if name.is_empty() {
            return Err(self.invalid());
        }
        if closing {
            if self.path.last().is_none_or(|last| last != name) {
                return Err(self.invalid());
            }
            if self.field.is_some() {
                self.finish_field()?;
            } else if self.at(&["mediawiki", "page", "revision"]) {
                self.emit(PageEvent::EndRevision)?;
            } else if self.at(&["mediawiki", "page"]) {
                self.emit(PageEvent::EndPage)?;
            }
            self.path.pop();
            return Ok(());
        }

        if self.field.is_some() {
            return Err(self.invalid());
        }
        if self.at(&["mediawiki", "page"]) && name == "redirect" {
            if !self_closing {
                return Err(self.invalid());
            }
            let target = Self::redirect_title(&body[end..]).ok_or_else(|| self.invalid())?;
            return self.emit(PageEvent::RedirectTarget(&target));
        }
        if self.at(&["mediawiki"]) && name == "page" {
            if self_closing {
                return Err(self.invalid());
            }
            self.emit(PageEvent::BeginPage)?;
        } else if self.at(&["mediawiki", "page"]) && name == "revision" {
            if self_closing {
                return Err(self.invalid());
            }
            self.emit(PageEvent::BeginRevision)?;
        }

        let field = if self.at(&["mediawiki", "page"]) {
            match name {
                "title" => Some(Field::Title),
                "ns" => Some(Field::Namespace),
                "id" => Some(Field::PageId),
                _ => None,
            }
        } else if self.at(&["mediawiki", "page", "revision"]) {
            match name {
                "id" => Some(Field::RevisionId),
                "timestamp" => Some(Field::Timestamp),
                _ => None,
            }
        } else {
            None
        };
        if field.is_some() && self_closing {
            return Err(self.invalid());
        }
        if !self_closing {
            if self.path.len() >= 64 {
                return Err(self.invalid());
            }
            self.path.push(name.to_owned());
            self.field = field;
            if field.is_some() {
                self.field_bytes.clear();
            }
        }
        Ok(())
    }

    fn byte(&mut self, byte: u8) -> io::Result<()> {
        match self.mode {
            Mode::Text => {
                if byte == b'<' {
                    self.mode = Mode::Markup;
                    self.markup.clear();
                    self.markup.push(byte);
                    self.quote = None;
                } else if self.field.is_some() {
                    if self.field_bytes.len() >= MAX_FIELD_BYTES {
                        return Err(self.invalid());
                    }
                    self.field_bytes.push(byte);
                }
            }
            Mode::Markup => {
                self.markup.push(byte);
                if self.markup.len() > MAX_MARKUP_BYTES {
                    return Err(self.invalid());
                }
                if self.markup == b"<!--" {
                    if self.field.is_some() {
                        return Err(self.invalid());
                    }
                    self.mode = Mode::Comment;
                    self.tail.clear();
                    self.markup.clear();
                } else if self.markup == b"<![CDATA[" {
                    if self.field.is_some() {
                        return Err(self.invalid());
                    }
                    self.mode = Mode::Cdata;
                    self.tail.clear();
                    self.markup.clear();
                } else if self.markup.starts_with(b"<!") {
                    if !b"<!--".starts_with(&self.markup) && !b"<![CDATA[".starts_with(&self.markup)
                    {
                        return Err(self.invalid());
                    }
                } else if self.markup == b"<?" {
                    if self.field.is_some() {
                        return Err(self.invalid());
                    }
                    self.mode = Mode::Instruction;
                    self.tail.clear();
                    self.markup.clear();
                } else if let Some(quote) = self.quote {
                    if byte == quote {
                        self.quote = None;
                    }
                } else if byte == b'\'' || byte == b'"' {
                    self.quote = Some(byte);
                } else if byte == b'>' {
                    let token = std::mem::take(&mut self.markup);
                    self.tag(&token)?;
                    self.mode = Mode::Text;
                }
            }
            Mode::Comment | Mode::Cdata | Mode::Instruction => {
                self.tail.push(byte);
                let ending = match self.mode {
                    Mode::Comment => b"-->".as_slice(),
                    Mode::Cdata => b"]]>".as_slice(),
                    Mode::Instruction => b"?>".as_slice(),
                    _ => unreachable!(),
                };
                if self.tail.ends_with(ending) {
                    self.mode = Mode::Text;
                    self.tail.clear();
                } else if self.tail.len() > ending.len() {
                    self.tail.remove(0);
                }
            }
        }
        Ok(())
    }

    /// Only call after the upstream XML and compressed-member guards succeed.
    /// The returned count is provisional until the complete source qualifies.
    pub fn finish(mut self) -> io::Result<(F, u64)> {
        if self.failed
            || self.mode != Mode::Text
            || !self.path.is_empty()
            || self.field.is_some()
            || self.pages == 0
        {
            return Err(self.invalid());
        }
        self.collector
            .finish()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "incomplete page metadata"))?;
        Ok((self.callback, self.pages))
    }
}

impl<F: FnMut(PublicDumpPage) -> io::Result<()>> Write for XmlPageMetadataWriter<F> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.failed {
            return Err(self.invalid());
        }
        let count = bytes.len().min(MAX_WRITE_BYTES);
        for &byte in &bytes[..count] {
            self.byte(byte)?;
        }
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.failed {
            Err(self.invalid())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"<?xml version="1.0"?><mediawiki><siteinfo><sitename>Wiki</sitename></siteinfo><page><title>Physics &amp; 日本語</title><ns>0</ns><id>42</id><redirect title="Science &amp; Math"/><revision><id>900</id><timestamp>2026-10-01T12:00:00Z</timestamp><contributor><id>9999</id></contributor><text xml:space="preserve"><![CDATA[{{Template}}]]></text></revision></page></mediawiki>"#;

    #[test]
    fn extracts_only_outer_page_and_revision_ids_across_chunk_boundaries() {
        for chunk_size in [1, 2, 3, 7, 8192] {
            let mut pages = Vec::new();
            {
                let mut writer = XmlPageMetadataWriter::new(|page| {
                    pages.push(page);
                    Ok(())
                });
                for chunk in VALID.as_bytes().chunks(chunk_size) {
                    writer.write_all(chunk).unwrap();
                }
                assert_eq!(writer.finish().unwrap().1, 1);
            }
            assert_eq!(pages.len(), 1);
            assert_eq!(pages[0].page_id, 42);
            assert_eq!(pages[0].revision_id, 900);
            assert_eq!(pages[0].title, "Physics & 日本語");
            assert_eq!(pages[0].redirect_target.as_deref(), Some("Science & Math"));
        }
    }

    #[test]
    fn rejects_missing_duplicate_or_forged_page_semantics() {
        let invalid = [
            "<mediawiki><page><title>X</title><ns>0</ns><id>1</id><revision><id>2</id></revision></page></mediawiki>",
            "<mediawiki><page><title>X</title><ns>0</ns><id>1</id><id>2</id><revision><id>3</id><timestamp>2026-10-01T12:00:00Z</timestamp></revision></page></mediawiki>",
            "<mediawiki><page><title>X</title><ns>0</ns><revision><id>2</id><timestamp>2026-10-01T12:00:00Z</timestamp><contributor><id>1</id></contributor></revision></page></mediawiki>",
            "<mediawiki><page><title>X</title><ns>0</ns><id>1</id><redirect/><revision><id>2</id><timestamp>2026-10-01T12:00:00Z</timestamp></revision></page></mediawiki>",
            "<mediawiki><page><title>&#xD800;</title><ns>0</ns><id>1</id><revision><id>2</id><timestamp>2026-10-01T12:00:00Z</timestamp></revision></page></mediawiki>",
        ];
        for xml in invalid {
            let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
            let written = writer.write_all(xml.as_bytes());
            assert!(written.is_err() || writer.finish().is_err(), "{xml}");
        }
    }

    #[test]
    fn poisoned_after_callback_failure_even_if_caller_swallows_it() {
        let mut writer = XmlPageMetadataWriter::new(|_| Err(io::Error::other("sink failed")));
        assert!(writer.write_all(VALID.as_bytes()).is_err());
        assert!(writer.write_all(b"<mediawiki/>").is_err());
        assert!(writer.finish().is_err());
    }

    #[test]
    fn rejects_unfinished_fields_and_nested_markup_in_scalar() {
        for xml in [
            "<mediawiki><page><title>X<bad/>Y</title></page></mediawiki>",
            "<mediawiki><page><title>X",
        ] {
            let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
            let result = writer.write_all(xml.as_bytes());
            assert!(result.is_err() || writer.finish().is_err());
        }
    }
}
