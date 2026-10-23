//! Streaming extraction of current-content page identities from validated dump XML.
//!
//! This is a narrow MediaWiki page-event adapter, not a general XML parser.
//! It must be downstream of XmlTextGuard and XmlNestingGuard; those guards
//! validate UTF-8, markup and entity syntax. This adapter distinguishes
//! page/id, revision/id and contributor/id without buffering revision text.
//! Callbacks are PROVISIONAL until compressed digest, complete XML, shard
//! census and upstream authenticity have all been verified.

use std::collections::{BTreeMap, BTreeSet};
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
    DatabaseName,
}

/// Converts validated XML bytes into provisional, bounded page records.
/// A rejected semantic field or callback permanently poisons the writer.
pub struct XmlPageMetadataWriter<F> {
    callback: F,
    collector: PageMetadataCollector,
    expected_project: Option<String>,
    siteinfo_seen: bool,
    dbname_seen: bool,
    pages_started: bool,
    path: Vec<String>,
    // Namespace prefix scopes are kept in lockstep with open elements.
    prefixes: Vec<BTreeMap<Vec<u8>, Vec<u8>>>,
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
            expected_project: None,
            siteinfo_seen: false,
            dbname_seen: false,
            pages_started: false,
            path: Vec::new(),
            prefixes: Vec::new(),
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

    /// Bind optional siteinfo database identity to the pinned export project.
    /// This is a consistency check, not authentication of the publication.
    pub fn with_expected_project(callback: F, project: &str) -> io::Result<Self> {
        if !crate::valid_wiki_project(project) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid Wikimedia project",
            ));
        }
        let mut writer = Self::new(callback);
        writer.expected_project = Some(project.to_owned());
        Ok(writer)
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
            // Match the strict public-dump diagnostic schema. Unknown
            // redirect attributes must not be silently discarded because
            // downstream navigation identity is taken only from "title".
            if key != b"title" || title.is_some() {
                return None;
            }
            title = Some(Self::decode_entities(&attrs[..end])?);
            attrs = &attrs[end + 1..];
        }
        title
    }

    /// Preserve the Wikimedia export namespace identity. A foreign default
    /// namespace or a descendant rebinding must never be interpreted as an
    /// ordinary MediaWiki page, even if its local tag names match.
    ///
    /// Namespace syntax is already checked by XmlNestingGuard. This is an
    /// additional semantic check, not proof of upstream source authenticity.
    fn official_export_namespace(value: &[u8]) -> bool {
        let Some(version) = value
            .strip_prefix(b"http://www.mediawiki.org/xml/export-0.")
            .and_then(|suffix| suffix.strip_suffix(b"/"))
        else {
            return false;
        };
        !version.is_empty() && version.len() <= 3 && version.iter().all(u8::is_ascii_digit)
    }

    fn namespace_scope(
        mut attrs: &[u8],
        root: bool,
        inherited: &BTreeMap<Vec<u8>, Vec<u8>>,
    ) -> Option<BTreeMap<Vec<u8>, Vec<u8>>> {
        let mut scope = inherited.clone();
        let mut names = Vec::new();
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
            let &quote = attrs.first()?;
            if quote != b'\'' && quote != b'"' {
                return None;
            }
            attrs = &attrs[1..];
            let end = attrs.iter().position(|byte| *byte == quote)?;
            let value = &attrs[..end];
            if key == b"xmlns" {
                // Namespace URIs are XML attribute values, so character
                // references must be expanded before comparing their identity.
                let decoded = Self::decode_entities(value)?;
                if !root || !Self::official_export_namespace(decoded.as_bytes()) {
                    return None;
                }
            } else if let Some(prefix) = key.strip_prefix(b"xmlns:") {
                let decoded = Self::decode_entities(value)?;
                let value = decoded.as_bytes();
                // XML Namespace 1.0 reserves the xml/xmlns prefixes and URIs.
                const XML_URI: &[u8] = b"http://www.w3.org/XML/1998/namespace";
                const XMLNS_URI: &[u8] = b"http://www.w3.org/2000/xmlns/";
                if prefix.is_empty()
                    || prefix.contains(&b':')
                    || prefix == b"xmlns"
                    || value.is_empty()
                    || value == XMLNS_URI
                    || (prefix == b"xml" && value != XML_URI)
                    || (prefix != b"xml" && value == XML_URI)
                {
                    return None;
                }
                scope.insert(prefix.to_vec(), value.to_vec());
            }
            names.push(key);
            attrs = &attrs[end + 1..];
        }
        // Namespace declarations apply to the entire opening tag, even if
        // they appear after a prefixed attribute in source order. XML
        // forbids duplicate attributes by expanded (namespace URI, local)
        // name, even when two different prefixes alias the same URI.
        let mut expanded = BTreeSet::new();
        for name in names {
            if !Self::qualified_name_bound(name, &scope, true) {
                return None;
            }
            if name == b"xmlns" || name.starts_with(b"xmlns:") {
                continue;
            }
            let (uri, local): (&[u8], &[u8]) = if let Some(colon) =
                name.iter().position(|byte| *byte == b':')
            {
                let prefix = &name[..colon];
                let uri = if prefix == b"xml" {
                    b"http://www.w3.org/XML/1998/namespace".as_slice()
                } else {
                    scope.get(prefix)?.as_slice()
                };
                (uri, &name[colon + 1..])
            } else {
                (b"", name)
            };
            if !expanded.insert((uri.to_vec(), local.to_vec())) {
                return None;
            }
        }
        Some(scope)
    }

    fn qualified_name_bound(name: &[u8], scope: &BTreeMap<Vec<u8>, Vec<u8>>, attribute: bool) -> bool {
        let Some(colon) = name.iter().position(|byte| *byte == b':') else {
            return true;
        };
        let prefix = &name[..colon];
        let local = &name[colon + 1..];
        if prefix.is_empty() || local.is_empty() || local.contains(&b':') {
            return false;
        }
        if attribute && prefix == b"xmlns" {
            return true;
        }
        prefix != b"xmlns" && (prefix == b"xml" || scope.contains_key(prefix))
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
            Field::DatabaseName => {
                if self.dbname_seen || !crate::valid_wiki_project(&value) {
                    return Err(self.invalid());
                }
                if self
                    .expected_project
                    .as_deref()
                    .is_some_and(|expected| expected != value)
                {
                    return Err(self.invalid());
                }
                self.dbname_seen = true;
                Ok(())
            }
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
        let scope = if closing {
            None
        } else {
            let inherited = self.prefixes.last().cloned().unwrap_or_default();
            let scope = Self::namespace_scope(&body[end..], self.path.is_empty(), &inherited)
                .ok_or_else(|| self.invalid())?;
            if !Self::qualified_name_bound(name.as_bytes(), &scope, false) {
                return Err(self.invalid());
            }
            Some(scope)
        };
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
            self.prefixes.pop();
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
        if self.at(&["mediawiki"]) {
            match name {
                "siteinfo" => {
                    if self_closing || self.siteinfo_seen || self.pages_started {
                        return Err(self.invalid());
                    }
                    self.siteinfo_seen = true;
                }
                "page" => {
                    if self_closing {
                        return Err(self.invalid());
                    }
                    self.pages_started = true;
                    self.emit(PageEvent::BeginPage)?;
                }
                _ => return Err(self.invalid()),
            }
        } else if self.at(&["mediawiki", "page"]) && name == "revision" {
            if self_closing {
                return Err(self.invalid());
            }
            self.emit(PageEvent::BeginRevision)?;
        }

        let field = if self.at(&["mediawiki", "siteinfo"]) && name == "dbname" {
            Some(Field::DatabaseName)
        } else if self.at(&["mediawiki", "page"]) {
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
            let scope = scope.ok_or_else(|| self.invalid())?;
            self.path.push(name.to_owned());
            self.prefixes.push(scope);
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
    fn checks_optional_siteinfo_database_against_pinned_project() {
        let base = VALID.replace("<sitename>Wiki</sitename>", "<dbname>enwiki</dbname>");
        for chunk_size in [1, 2, 7, 8192] {
            let mut records = Vec::new();
            let mut writer = XmlPageMetadataWriter::with_expected_project(
                |page| {
                    records.push(page);
                    Ok(())
                },
                "enwiki",
            )
            .unwrap();
            for chunk in base.as_bytes().chunks(chunk_size) {
                writer.write_all(chunk).unwrap();
            }
            assert_eq!(writer.finish().unwrap().1, 1);
            assert_eq!(records[0].page_id, 42);
        }
        for bad in [
            base.replace("enwiki</dbname>", "frwiki</dbname>"),
            base.replace("enwiki</dbname>", "EnWiki</dbname>"),
            base.replace("</dbname>", "</dbname><dbname>enwiki</dbname>"),
            base.replace("</siteinfo>", "</siteinfo><siteinfo/>"),
        ] {
            let mut writer =
                XmlPageMetadataWriter::with_expected_project(|_| Ok(()), "enwiki").unwrap();
            let result = writer.write_all(bad.as_bytes());
            assert!(result.is_err() || writer.finish().is_err(), "{bad}");
        }
        assert!(XmlPageMetadataWriter::with_expected_project(|_| Ok(()), "enwiki_").is_err());

        // Legacy test dumps can omit optional siteinfo database metadata;
        // absence does not by itself prove project identity.
        let mut legacy =
            XmlPageMetadataWriter::with_expected_project(|_| Ok(()), "enwiki").unwrap();
        legacy.write_all(VALID.as_bytes()).unwrap();
        assert_eq!(legacy.finish().unwrap().1, 1);
    }

    #[test]
    fn redirect_attributes_must_match_strict_diagnostic_contract() {
        for invalid in [
            r#"<redirect title="Science" extra="ignored"/>"#,
            r#"<redirect bogus="Science" title="Science"/>"#,
            r#"<redirect title="Science" title="Physics"/>"#,
            r#"<redirect xml:lang="en"/>"#,
        ] {
            let xml = VALID.replace(r#"<redirect title="Science &amp; Math"/>"#, invalid);
            for chunk_size in [1, 3, 8192] {
                let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
                let mut observed_error = false;
                for chunk in xml.as_bytes().chunks(chunk_size) {
                    if writer.write_all(chunk).is_err() {
                        observed_error = true;
                        break;
                    }
                }
                assert!(
                    observed_error || writer.finish().is_err(),
                    "accepted {invalid}"
                );
            }
        }
        let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
        writer.write_all(VALID.as_bytes()).unwrap();
        assert_eq!(writer.finish().unwrap().1, 1);
    }

    #[test]
    fn rejects_foreign_and_rebound_default_xml_namespaces() {
        let official = VALID.replace(
            "<mediawiki>",
            "<mediawiki xmlns=\"http://www.mediawiki.org/xml/export-0.11/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">",
        );
        for size in [1, 2, 7, 8192] {
            let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
            for chunk in official.as_bytes().chunks(size) {
                writer.write_all(chunk).unwrap();
            }
            assert_eq!(writer.finish().unwrap().1, 1);
        }

        for bad in [
            official.replace("export-0.11/", "other-0.11/"),
            official.replace("<page>", "<page xmlns=\"\">"),
            official.replace("<title>", "<title xmlns=\"urn:forged\">"),
            VALID.replace("<page>", "<page xmlns=\"urn:foreign\">"),
            VALID.replace("<mediawiki>", "<mediawiki xmlns=\"urn:foreign\">"),
        ] {
            for size in [1, 2, 7, 8192] {
                let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
                let result = bad
                    .as_bytes()
                    .chunks(size)
                    .try_for_each(|chunk| writer.write_all(chunk));
                assert!(
                    result.is_err() || writer.finish().is_err(),
                    "foreign namespace accepted"
                );
            }
        }
    }

    #[test]
    fn rejects_unknown_root_children_and_siteinfo_after_page() {
        let without_siteinfo = VALID.replace("<siteinfo><sitename>Wiki</sitename></siteinfo>", "");
        for invalid in [
            VALID.replace(
                "<siteinfo><sitename>Wiki</sitename></siteinfo>",
                "<unknown/>",
            ),
            without_siteinfo.replace(
                "</page></mediawiki>",
                "</page><siteinfo><dbname>enwiki</dbname></siteinfo></mediawiki>",
            ),
            VALID.replace("</page></mediawiki>", "</page><other/></mediawiki>"),
        ] {
            for chunk_size in [1, 3, 8192] {
                let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
                let result = invalid
                    .as_bytes()
                    .chunks(chunk_size)
                    .try_for_each(|chunk| writer.write_all(chunk));
                assert!(
                    result.is_err() || writer.finish().is_err(),
                    "invalid root child accepted"
                );
            }
        }
        let mut legacy = XmlPageMetadataWriter::new(|_| Ok(()));
        legacy.write_all(without_siteinfo.as_bytes()).unwrap();
        assert_eq!(legacy.finish().unwrap().1, 1);
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
    #[test]
    fn rejects_reserved_namespace_prefix_rebindings() {
        let official = VALID.replace(
            "<mediawiki>",
            "<mediawiki xmlns=\"http://www.mediawiki.org/xml/export-0.11/\" \
             xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">",
        );
        let valid_explicit_xml = official.replace(
            "<page>",
            "<page xmlns:xml=\"http://www.w3.org/XML/1998/namespace\">",
        );
        for xml in [&official, &valid_explicit_xml] {
            for chunk_size in [1, 2, 7, 8192] {
                let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
                for chunk in xml.as_bytes().chunks(chunk_size) {
                    writer.write_all(chunk).unwrap();
                }
                assert_eq!(writer.finish().unwrap().1, 1);
            }
        }

        for bad in [
            official.replace("<page>", "<page xmlns:xml=\"urn:forged\">"),
            official.replace("<page>", "<page xmlns:xmlns=\"urn:forged\">"),
            official.replace(
                "<page>",
                "<page xmlns:evil=\"http://www.w3.org/XML/1998/namespace\">",
            ),
            official.replace(
                "<page>",
                "<page xmlns:evil=\"http://www.w3.org/2000/xmlns/\">",
            ),
            official.replace("<page>", "<page xmlns:evil=\"\">"),
            official.replace("<page>", "<page xmlns:evil:extra=\"urn:forged\">"),
        ] {
            for chunk_size in [1, 2, 7, 8192] {
                let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
                let result = bad
                    .as_bytes()
                    .chunks(chunk_size)
                    .try_for_each(|chunk| writer.write_all(chunk));
                assert!(
                    result.is_err() || writer.finish().is_err(),
                    "reserved namespace rebinding accepted: {bad}"
                );
            }
        }
    }

    #[test]
    fn namespace_uri_character_references_cannot_hide_reserved_bindings() {
        let official = VALID.replace(
            "<mediawiki>",
            r#"<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/">"#,
        );
        let encoded_default = official.replace("export-0.11/", "export-0.&#x31;1/");
        let encoded_xml = official.replace(
            "<page>",
            r#"<page xmlns:xml="http://www.w3.org/XML/1998/namesp&#x61;ce">"#,
        );
        let encoded_extension = official.replace(
            "<page>",
            r#"<page xmlns:ext="urn:foo&amp;bar" ext:note="ok">"#,
        );
        for valid in [&encoded_default, &encoded_xml, &encoded_extension] {
            for chunk_size in [1, 2, 7, 8192] {
                let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
                for chunk in valid.as_bytes().chunks(chunk_size) {
                    writer.write_all(chunk).unwrap();
                }
                assert_eq!(writer.finish().unwrap().1, 1);
            }
        }

        for bad in [
            official.replace(
                "<page>",
                r#"<page xmlns:evil="http://www.w3.org/XML/1998/namesp&#x61;ce">"#,
            ),
            official.replace(
                "<page>",
                r#"<page xmlns:evil="http://www.w3.org/2000/xmln&#x73;/">"#,
            ),
            official.replace(
                "<page>",
                r#"<page xmlns:xml="http://www.w3.org/XML/1998/namesp&#x61;ce-evil">"#,
            ),
        ] {
            for chunk_size in [1, 2, 7, 8192] {
                let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
                let result = bad
                    .as_bytes()
                    .chunks(chunk_size)
                    .try_for_each(|chunk| writer.write_all(chunk));
                assert!(
                    result.is_err() || writer.finish().is_err(),
                    "encoded reserved namespace binding accepted: {bad}"
                );
            }
        }
    }

    #[test]
    fn rejects_duplicate_expanded_attribute_names_even_with_alias_prefixes() {
        let base = VALID.replace(
            "<mediawiki>",
            r#"<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/">"#,
        );
        let distinct = base.replace(
            "<page>",
            r#"<page xmlns:a="urn:a" xmlns:b="urn:b" a:hint="one" b:hint="two">"#,
        );
        let same_uri_distinct_local = base.replace(
            "<page>",
            r#"<page xmlns:a="urn:shared" xmlns:b="urn:shared" a:first="one" b:second="two">"#,
        );
        for valid in [&distinct, &same_uri_distinct_local] {
            for chunk_size in [1, 2, 7, 8192] {
                let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
                for chunk in valid.as_bytes().chunks(chunk_size) {
                    writer.write_all(chunk).unwrap();
                }
                assert_eq!(writer.finish().unwrap().1, 1);
            }
        }

        let aliased = base.replace(
            "<page>",
            r#"<page xmlns:a="urn:shared" xmlns:b="urn:shared" a:hint="one" b:hint="two">"#,
        );
        let escaped_alias = base.replace(
            "<page>",
            r#"<page xmlns:a="urn:shared" xmlns:b="urn:sh&#x61;red" a:hint="one" b:hint="two">"#,
        );
        let inherited_alias = base
            .replace(
                r#"<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/">"#,
                r#"<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/" xmlns:a="urn:shared" xmlns:b="urn:shared">"#,
            )
            .replace("<page>", r#"<page a:hint="one" b:hint="two">"#);
        for bad in [&aliased, &escaped_alias, &inherited_alias] {
            for chunk_size in [1, 2, 7, 8192] {
                let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
                let result = bad
                    .as_bytes()
                    .chunks(chunk_size)
                    .try_for_each(|chunk| writer.write_all(chunk));
                assert!(
                    result.is_err() || writer.finish().is_err(),
                    "duplicate expanded attribute accepted: {bad}"
                );
            }
        }
    }

    #[test]
    fn validates_qualified_xml_names_in_lexical_namespace_scope() {
        let base = VALID.replace(
            "<mediawiki>",
            "<mediawiki xmlns=\"http://www.mediawiki.org/xml/export-0.11/\">",
        );
        let valid = base.replace(
            "<siteinfo>",
            "<siteinfo xmlns:xsi=\"urn:extension\"><xsi:extra/><xsi:extra xsi:hint=\"yes\"/>",
        );
        let valid_late_declaration = base.replace(
            "<siteinfo>",
            "<siteinfo xsi:hint=\"yes\" xmlns:xsi=\"urn:extension\">",
        );
        for xml in [&valid, &valid_late_declaration] {
            for chunk_size in [1, 2, 7, 8192] {
                let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
                for chunk in xml.as_bytes().chunks(chunk_size) {
                    writer.write_all(chunk).unwrap();
                }
                assert_eq!(writer.finish().unwrap().1, 1);
            }
        }

        for bad in [
            base.replace("<siteinfo>", "<siteinfo><xsi:extra/>"),
            base.replace("<siteinfo>", "<siteinfo xsi:hint=\"unbound\">"),
            base.replace("</siteinfo><page>", "</siteinfo><page xsi:hint=\"leaked\">")
                .replace("<siteinfo>", "<siteinfo xmlns:xsi=\"urn:extension\">"),
            base.replace("<siteinfo>", "<siteinfo><xsi:extra xmlns:xsi=\"\"/>"),
            base.replace(
                "<siteinfo>",
                "<siteinfo><xsi:extra:bad xmlns:xsi=\"urn:extension\"/>",
            ),
            base.replace("<siteinfo>", "<siteinfo><xmlns:extra/>"),
        ] {
            for chunk_size in [1, 2, 7, 8192] {
                let mut writer = XmlPageMetadataWriter::new(|_| Ok(()));
                let result = bad
                    .as_bytes()
                    .chunks(chunk_size)
                    .try_for_each(|chunk| writer.write_all(chunk));
                assert!(
                    result.is_err() || writer.finish().is_err(),
                    "unbound or malformed qualified name accepted: {bad}"
                );
            }
        }
    }
}
