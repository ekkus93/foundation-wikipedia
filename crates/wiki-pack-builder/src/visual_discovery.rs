//! Bounded, read-only discovery of visual URL *candidates* in rendered HTML.
//!
//! Portable Rust counterpart of `scripts/inventory_rendered_visuals.py`, built
//! on the spec-compliant `html5ever` tokenizer and the WHATWG `url` crate. It
//! performs no network transfer, authentication, byte hashing, sanitization or
//! licensing. Candidates are NOT verified media: `required_media_verified` and
//! `css_and_font_dependencies_verified` are always false, and the result must
//! never be used on its own to call a pack offline-complete. External
//! stylesheets are not followed; unresolved dynamic surfaces are only counted.
//!
//! Every candidate is resolved to an absolute http(s) URL against the first
//! HTML `<base href>` or, absent one, the caller's document URL. Ambiguous or
//! unsafe shapes fail closed instead of being guessed.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use url::Url;

pub const MAX_URL_CHARS: usize = 4096;
pub const MAX_REFERENCES: usize = 10_000;
pub const MAX_INLINE_CSS_CHARS: usize = 2 * 1024 * 1024;
pub const MAX_MARKUP_BYTES: usize = 64 * 1024 * 1024;

/// Variants are declared in alphabetical order of [`CandidateKind::as_str`] so
/// that the derived ordering matches the Python diagnostic's sort order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CandidateKind {
    AvPreview,
    CssImport,
    CssUrl,
    EmbeddedImage,
    Image,
    MathGlyph,
    PictureVariant,
    PreloadFont,
    PreloadImage,
    PreloadResponsiveImage,
    PreloadStyle,
    ResponsiveImage,
    Stylesheet,
    SvgFilterImage,
    SvgImage,
    SvgUse,
}

impl CandidateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AvPreview => "av_preview",
            Self::CssImport => "css_import",
            Self::CssUrl => "css_url",
            Self::EmbeddedImage => "embedded_image",
            Self::Image => "image",
            Self::MathGlyph => "math_glyph",
            Self::PictureVariant => "picture_variant",
            Self::PreloadFont => "preload_font",
            Self::PreloadImage => "preload_image",
            Self::PreloadResponsiveImage => "preload_responsive_image",
            Self::PreloadStyle => "preload_style",
            Self::ResponsiveImage => "responsive_image",
            Self::Stylesheet => "stylesheet",
            Self::SvgFilterImage => "svg_filter_image",
            Self::SvgImage => "svg_image",
            Self::SvgUse => "svg_use",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Candidate {
    pub kind: CandidateKind,
    /// Absolute http(s) URL, normalized by the WHATWG URL parser.
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryError {
    InvalidBudget,
    MarkupTooLarge,
    InvalidDocumentUrl,
    UnsafeCandidateUrl,
    InvalidCandidateUrl,
    UnsafeCandidateAuthority,
    InvalidCandidatePort,
    DuplicateResourceAttribute,
    MultipleBaseHref,
    BaseHrefAfterResourceReference,
    UnsafeBaseHref,
    InvalidResponsiveImage(&'static str),
    ConflictingSvgHref,
    InlineCssBudget,
    UnclosedStyleElement,
    Budget(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisualInventory {
    /// Sorted by (kind string, URL) and deduplicated.
    pub candidates: Vec<Candidate>,
    pub observed_references: usize,
    pub inline_styles: usize,
    pub inline_style_attributes: usize,
    pub inline_css_urls: usize,
    pub inline_css_imports: usize,
    /// `canvas`, `iframe` and SVG `foreignObject`: counted, never resolved.
    pub unresolved_dynamic_surfaces: usize,
    pub unsupported_schemes: usize,
    pub missing_sources: usize,
    pub html_base_href_count: usize,
    pub document_url: String,
    pub html_base_url: Option<String>,
    /// Always false: external CSS and fonts are not inspected here.
    pub css_and_font_dependencies_verified: bool,
    /// Always false: candidates are not acquired, hashed or licensed.
    pub required_media_verified: bool,
}

fn url_text_is_unsafe(text: &str) -> bool {
    text.chars().count() > MAX_URL_CHARS
        || text != text.trim()
        || text.contains('\\')
        || text
            .chars()
            .any(|c| c.is_whitespace() || (c as u32) < 32 || c as u32 == 127)
}

/// Absolute http(s) URL with a host, no credentials and a non-zero port.
fn validate_http_url(url: &Url) -> Result<(), DiscoveryError> {
    if url.as_str().chars().count() > MAX_URL_CHARS {
        return Err(DiscoveryError::UnsafeCandidateUrl);
    }
    if url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(DiscoveryError::UnsafeCandidateAuthority);
    }
    if url.port() == Some(0) {
        return Err(DiscoveryError::InvalidCandidatePort);
    }
    Ok(())
}

fn parse_absolute_http(text: &str) -> Option<Url> {
    if text.is_empty() || url_text_is_unsafe(text) {
        return None;
    }
    let url = Url::parse(text).ok()?;
    (matches!(url.scheme(), "http" | "https") && validate_http_url(&url).is_ok()).then_some(url)
}

struct State {
    limit: usize,
    document: Url,
    base: Option<Url>,
    urls: BTreeSet<Candidate>,
    observed_references: usize,
    picture_depth: usize,
    svg_depth: usize,
    math_depth: usize,
    unresolved_dynamic_surfaces: usize,
    inline_styles: usize,
    inline_style_attributes: usize,
    inline_css_urls: usize,
    inline_css_imports: usize,
    inline_css_chars: usize,
    pending_css_chars: usize,
    style_fragments: Vec<String>,
    unsupported_schemes: usize,
    missing_sources: usize,
    html_base_href_count: usize,
    error: Option<DiscoveryError>,
}

type Attrs = HashMap<String, String>;

impl State {
    fn add(&mut self, kind: CandidateKind, source: Option<&str>) -> Result<(), DiscoveryError> {
        // Repeated, missing and unsupported references consume the budget.
        self.observed_references += 1;
        if self.observed_references > self.limit {
            return Err(DiscoveryError::Budget("visual reference count"));
        }
        let Some(source) = source.filter(|s| !s.is_empty()) else {
            self.missing_sources += 1;
            return Ok(());
        };
        if url_text_is_unsafe(source) {
            return Err(DiscoveryError::UnsafeCandidateUrl);
        }
        if source.starts_with('#') {
            return Ok(()); // Same-document fragment; no separate transport URL.
        }
        let resolved = self
            .base
            .as_ref()
            .unwrap_or(&self.document)
            .join(source)
            .map_err(|_| DiscoveryError::InvalidCandidateUrl)?;
        if !matches!(resolved.scheme(), "http" | "https") {
            self.unsupported_schemes += 1;
            return Ok(());
        }
        validate_http_url(&resolved)?;
        self.urls.insert(Candidate {
            kind,
            url: resolved.into(),
        });
        if self.urls.len() > self.limit {
            return Err(DiscoveryError::Budget("visual candidate count"));
        }
        Ok(())
    }

    fn set_base(&mut self, href: &str) -> Result<(), DiscoveryError> {
        // Browsers and preload scanners disagree about references seen before
        // a late <base>, so that shape fails closed.
        if self.observed_references > 0 {
            return Err(DiscoveryError::BaseHrefAfterResourceReference);
        }
        if href.is_empty() || href != href.trim() || href.starts_with('#') {
            return Err(DiscoveryError::UnsafeBaseHref);
        }
        let resolved = self
            .document
            .join(href)
            .map_err(|_| DiscoveryError::UnsafeBaseHref)?;
        if !matches!(resolved.scheme(), "http" | "https") || url_text_is_unsafe(resolved.as_str()) {
            return Err(DiscoveryError::UnsafeBaseHref);
        }
        validate_http_url(&resolved).map_err(|_| DiscoveryError::UnsafeBaseHref)?;
        self.base = Some(resolved);
        Ok(())
    }

    fn srcset(&mut self, kind: CandidateKind, value: Option<&str>) -> Result<(), DiscoveryError> {
        let value = value.unwrap_or("");
        // Diagnostic split: data URIs containing commas would need a complete
        // srcset parser, so data: candidates are rejected below.
        if !value.is_empty() && value.split(',').any(|part| part.trim().is_empty()) {
            return Err(DiscoveryError::InvalidResponsiveImage("empty candidate"));
        }
        let mut seen: BTreeSet<(char, String)> = BTreeSet::new();
        let mut family: Option<char> = None;
        for part in value.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (url, descriptor) = match part.find(char::is_whitespace) {
                Some(i) => (&part[..i], part[i..].trim_start()),
                None => (part, "1x"),
            };
            let key = descriptor_key(descriptor)?;
            if family.is_some_and(|f| f != key.0) {
                return Err(DiscoveryError::InvalidResponsiveImage("mixed descriptors"));
            }
            if !seen.insert(key.clone()) {
                return Err(DiscoveryError::InvalidResponsiveImage(
                    "duplicate descriptor",
                ));
            }
            family = Some(key.0);
            if url
                .get(..5)
                .is_some_and(|p| p.eq_ignore_ascii_case("data:"))
            {
                return Err(DiscoveryError::InvalidResponsiveImage("data URL srcset"));
            }
            self.add(kind, Some(url))?;
        }
        Ok(())
    }

    fn scan_css(&mut self, css: &str) -> Result<(), DiscoveryError> {
        self.inline_css_chars += css.chars().count();
        if self.inline_css_chars > MAX_INLINE_CSS_CHARS {
            return Err(DiscoveryError::InlineCssBudget);
        }
        for (is_import, source) in css_url_tokens(css) {
            if is_import {
                self.inline_css_imports += 1;
                self.add(CandidateKind::CssImport, Some(&source))?;
            } else {
                self.inline_css_urls += 1;
                self.add(CandidateKind::CssUrl, Some(&source))?;
            }
        }
        Ok(())
    }

    fn character_data(&mut self, data: &str) -> Result<(), DiscoveryError> {
        if let Some(fragment) = self.style_fragments.last_mut() {
            self.pending_css_chars += data.chars().count();
            if self.inline_css_chars + self.pending_css_chars > MAX_INLINE_CSS_CHARS {
                return Err(DiscoveryError::InlineCssBudget);
            }
            fragment.push_str(data);
        }
        Ok(())
    }

    fn start_tag(&mut self, tag: &str, attrs: &Attrs) -> Result<(), DiscoveryError> {
        if let Some(style) = attrs.get("style") {
            self.inline_style_attributes += 1;
            if self.inline_style_attributes > self.limit {
                return Err(DiscoveryError::Budget("inline style attribute count"));
            }
            self.scan_css(style)?;
        }
        let html_context = self.svg_depth == 0 && self.math_depth == 0;
        if tag == "base" && html_context {
            if let Some(href) = attrs.get("href") {
                self.html_base_href_count += 1;
                if self.html_base_href_count > 1 {
                    return Err(DiscoveryError::MultipleBaseHref);
                }
                self.set_base(href)?;
            }
        }
        let get = |name: &str| attrs.get(name).map(String::as_str);
        match tag {
            "picture" => self.picture_depth += 1,
            "svg" => self.svg_depth += 1,
            "math" => self.math_depth += 1,
            "canvas" | "iframe" | "foreignobject" => {
                // Dynamic drawing, embedded documents and arbitrary foreign
                // markup are not a verified offline resource graph.
                self.unresolved_dynamic_surfaces += 1;
                if self.unresolved_dynamic_surfaces > self.limit {
                    return Err(DiscoveryError::Budget("unresolved visual surface count"));
                }
            }
            "mglyph" if self.math_depth > 0 => self.add(CandidateKind::MathGlyph, get("src"))?,
            "img" => {
                self.add(CandidateKind::Image, get("src"))?;
                self.srcset(CandidateKind::ResponsiveImage, get("srcset"))?;
            }
            "source" if self.picture_depth > 0 => {
                self.srcset(CandidateKind::PictureVariant, get("srcset"))?;
            }
            "link" => self.link(attrs)?,
            "object" | "embed" => {
                // Only explicitly image-typed embeds can be classified safely.
                let media_type = get("type")
                    .unwrap_or("")
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                if media_type.starts_with("image/") {
                    let key = if tag == "object" { "data" } else { "src" };
                    self.add(CandidateKind::EmbeddedImage, get(key))?;
                }
            }
            "video" if get("poster").is_some_and(|p| !p.is_empty()) => {
                self.add(CandidateKind::AvPreview, get("poster"))?;
            }
            "image" | "feimage" | "use" if self.svg_depth > 0 => {
                // Tokenizer names are lowercased; feImage becomes "feimage".
                let (href, legacy) = (get("href"), get("xlink:href"));
                if let (Some(a), Some(b)) = (href, legacy) {
                    if !a.is_empty() && !b.is_empty() && a != b {
                        return Err(DiscoveryError::ConflictingSvgHref);
                    }
                }
                let kind = match tag {
                    "image" => CandidateKind::SvgImage,
                    "feimage" => CandidateKind::SvgFilterImage,
                    _ => CandidateKind::SvgUse,
                };
                let chosen = href.filter(|h| !h.is_empty()).or(legacy);
                self.add(kind, chosen)?;
            }
            "style" => {
                self.inline_styles += 1;
                if self.inline_styles > self.limit {
                    return Err(DiscoveryError::Budget("inline style element count"));
                }
                self.style_fragments.push(String::new());
            }
            _ => {}
        }
        Ok(())
    }

    fn link(&mut self, attrs: &Attrs) -> Result<(), DiscoveryError> {
        let get = |name: &str| attrs.get(name).map(String::as_str);
        let rel = get("rel").unwrap_or("").to_ascii_lowercase();
        let relations: BTreeSet<&str> = rel.split_whitespace().collect();
        if relations.contains("stylesheet") {
            self.add(CandidateKind::Stylesheet, get("href"))?;
        } else if relations.contains("preload") {
            let resource_type = get("as").unwrap_or("").trim().to_ascii_lowercase();
            let kind = match resource_type.as_str() {
                "font" => CandidateKind::PreloadFont,
                "image" => CandidateKind::PreloadImage,
                "style" => CandidateKind::PreloadStyle,
                _ => return Ok(()),
            };
            self.add(kind, get("href"))?;
            if resource_type == "image" && attrs.contains_key("imagesrcset") {
                self.srcset(CandidateKind::PreloadResponsiveImage, get("imagesrcset"))?;
            }
        }
        Ok(())
    }

    fn end_tag(&mut self, tag: &str) -> Result<(), DiscoveryError> {
        match tag {
            "style" => {
                if let Some(css) = self.style_fragments.pop() {
                    self.pending_css_chars =
                        self.pending_css_chars.saturating_sub(css.chars().count());
                    self.scan_css(&css)?;
                }
            }
            "picture" => self.picture_depth = self.picture_depth.saturating_sub(1),
            "svg" => self.svg_depth = self.svg_depth.saturating_sub(1),
            "math" => self.math_depth = self.math_depth.saturating_sub(1),
            _ => {}
        }
        Ok(())
    }
}

/// `1x`-style or `100w` descriptor, validated like the Python diagnostic.
fn descriptor_key(descriptor: &str) -> Result<(char, String), DiscoveryError> {
    let invalid = || DiscoveryError::InvalidResponsiveImage("invalid descriptor");
    if descriptor.len() > 32 || !descriptor.is_ascii() {
        return Err(invalid());
    }
    let (number, family) = descriptor.split_at(descriptor.len().saturating_sub(1));
    match family {
        "w" => {
            let digits_ok = !number.is_empty()
                && number.bytes().all(|b| b.is_ascii_digit())
                && !number.starts_with('0');
            digits_ok
                .then(|| ('w', number.to_owned()))
                .ok_or_else(invalid)
        }
        "x" => {
            let (whole, fraction) = match number.split_once('.') {
                Some((w, f)) => (w, Some(f)),
                None => (number, None),
            };
            let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
            let shape_ok = all_digits(whole)
                && fraction.is_none_or(all_digits)
                && (!whole.is_empty() || fraction.is_some_and(|f| !f.is_empty()));
            let value: f64 = number.parse().map_err(|_| invalid())?;
            if !shape_ok || value <= 0.0 || !value.is_finite() {
                return Err(invalid());
            }
            Ok(('x', value.to_bits().to_string()))
        }
        _ => Err(invalid()),
    }
}

fn is_css_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// Diagnostic subset of CSS URL tokens, not a CSS parser. Comments and quoted
/// strings are skipped before matching `url(...)` and `@import`. Returns
/// `(is_import, url)` pairs in source order.
fn css_url_tokens(css: &str) -> Vec<(bool, String)> {
    let chars: Vec<char> = css.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let prev_is_word = i > 0 && is_css_word(chars[i - 1]);
        if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
            let mut j = i + 2;
            let mut closed = false;
            while j + 1 < chars.len() {
                if chars[j] == '*' && chars[j + 1] == '/' {
                    closed = true;
                    break;
                }
                j += 1;
            }
            if closed {
                i = j + 2;
                continue;
            }
        } else if chars[i] == '@' && !prev_is_word {
            if let Some((source, end)) = match_import(&chars, i) {
                out.push((true, source));
                i = end;
                continue;
            }
        } else if chars[i] == '"' || chars[i] == '\'' {
            if let Some(end) = match_string(&chars, i) {
                i = end;
                continue;
            }
        } else if !prev_is_word {
            if let Some((source, end)) = match_url_function(&chars, i) {
                out.push((false, source));
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn starts_with_ignore_case(chars: &[char], at: usize, word: &str) -> bool {
    word.chars().enumerate().all(|(k, w)| {
        chars
            .get(at + k)
            .is_some_and(|c| c.to_ascii_lowercase() == w)
    })
}

fn skip_ws(chars: &[char], mut at: usize) -> usize {
    while chars.get(at).is_some_and(|c| c.is_whitespace()) {
        at += 1;
    }
    at
}

/// A complete quoted string including escapes; `None` if unterminated.
fn match_string(chars: &[char], start: usize) -> Option<usize> {
    let quote = chars[start];
    let mut j = start + 1;
    while j < chars.len() {
        match chars[j] {
            '\\' if j + 1 < chars.len() => j += 2,
            '\\' => return None,
            c if c == quote => return Some(j + 1),
            _ => j += 1,
        }
    }
    None
}

/// A quoted value without escapes: `"..."` or `'...'`.
fn match_plain_quoted(chars: &[char], start: usize) -> Option<(String, usize)> {
    let quote = *chars.get(start).filter(|c| **c == '"' || **c == '\'')?;
    let mut j = start + 1;
    while j < chars.len() {
        match chars[j] {
            '\\' => return None,
            c if c == quote => return Some((chars[start + 1..j].iter().collect(), j + 1)),
            _ => j += 1,
        }
    }
    None
}

/// The argument of `url(` starting at `at` (just after the opening paren).
fn match_url_argument(chars: &[char], at: usize) -> Option<(String, usize)> {
    let at = skip_ws(chars, at);
    let (value, end) = if let Some(quoted) = match_plain_quoted(chars, at) {
        quoted
    } else {
        let mut j = at;
        while chars
            .get(j)
            .is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | ')' | '"' | '\'' | '\\'))
        {
            j += 1;
        }
        if j == at {
            return None;
        }
        (chars[at..j].iter().collect(), j)
    };
    let end = skip_ws(chars, end);
    (chars.get(end) == Some(&')')).then_some((value, end + 1))
}

fn match_url_function(chars: &[char], at: usize) -> Option<(String, usize)> {
    if !starts_with_ignore_case(chars, at, "url") {
        return None;
    }
    let paren = skip_ws(chars, at + 3);
    if chars.get(paren) != Some(&'(') {
        return None;
    }
    match_url_argument(chars, paren + 1)
}

fn match_import(chars: &[char], at: usize) -> Option<(String, usize)> {
    if !starts_with_ignore_case(chars, at + 1, "import") {
        return None;
    }
    let after = at + 7;
    if !chars.get(after).is_some_and(|c| c.is_whitespace()) {
        return None;
    }
    let value_at = skip_ws(chars, after);
    match_url_function(chars, value_at).or_else(|| match_plain_quoted(chars, value_at))
}

struct Sink<'a>(&'a RefCell<State>);

impl TokenSink for Sink<'_> {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<()> {
        let mut state = self.0.borrow_mut();
        if state.error.is_some() {
            return TokenSinkResult::Continue;
        }
        let mut raw = None;
        let result = match token {
            Token::CharacterTokens(text) => state.character_data(&text),
            Token::NullCharacterToken => state.character_data("\u{FFFD}"),
            Token::TagToken(tag) => {
                let name = tag.name.to_string();
                match tag.kind {
                    TagKind::StartTag => {
                        let attrs: Attrs = tag
                            .attrs
                            .iter()
                            .map(|a| (a.name.local.to_string(), a.value.to_string()))
                            .collect();
                        // Duplicates are dropped by the tokenizer, which could
                        // hide a required dependency, so fail closed.
                        let resource_tag = matches!(
                            name.as_str(),
                            "img"
                                | "source"
                                | "link"
                                | "video"
                                | "image"
                                | "feimage"
                                | "use"
                                | "object"
                                | "embed"
                                | "mglyph"
                                | "base"
                        );
                        let html_context = state.svg_depth == 0 && state.math_depth == 0;
                        raw = raw_kind(&name, html_context);
                        let result = if tag.had_duplicate_attributes
                            && (resource_tag || attrs.contains_key("style"))
                        {
                            Err(DiscoveryError::DuplicateResourceAttribute)
                        } else {
                            state.start_tag(&name, &attrs)
                        };
                        // A self-closing flag is ignored for raw-text elements
                        // exactly as in HTML; otherwise it closes the element.
                        if result.is_ok() && tag.self_closing && raw.is_none() {
                            state.end_tag(&name)
                        } else {
                            result
                        }
                    }
                    TagKind::EndTag => state.end_tag(&name),
                }
            }
            Token::DoctypeToken(_)
            | Token::CommentToken(_)
            | Token::EOFToken
            | Token::ParseError(_) => Ok(()),
        };
        if let Err(error) = result {
            state.error = Some(error);
            return TokenSinkResult::Continue;
        }
        match raw {
            Some(kind) => TokenSinkResult::RawData(kind),
            None => TokenSinkResult::Continue,
        }
    }
}

/// Content models the HTML tree builder would select. Only `<style>` is raw
/// text in foreign content too, so its CSS is still scanned inside SVG.
fn raw_kind(tag: &str, html_context: bool) -> Option<RawKind> {
    match tag {
        "style" => Some(RawKind::Rawtext),
        "script" if html_context => Some(RawKind::ScriptData),
        "title" | "textarea" if html_context => Some(RawKind::Rcdata),
        "xmp" | "iframe" | "noembed" | "noframes" if html_context => Some(RawKind::Rawtext),
        _ => None,
    }
}

/// List visual URL candidates in `markup`, resolved against `document_url` and
/// the first HTML `<base href>`. See the module documentation for non-claims.
pub fn inventory(
    markup: &str,
    document_url: &str,
    max_references: usize,
) -> Result<VisualInventory, DiscoveryError> {
    if max_references == 0 {
        return Err(DiscoveryError::InvalidBudget);
    }
    if markup.len() > MAX_MARKUP_BYTES {
        return Err(DiscoveryError::MarkupTooLarge);
    }
    let document = parse_absolute_http(document_url).ok_or(DiscoveryError::InvalidDocumentUrl)?;
    let state = RefCell::new(State {
        limit: max_references,
        document,
        base: None,
        urls: BTreeSet::new(),
        observed_references: 0,
        picture_depth: 0,
        svg_depth: 0,
        math_depth: 0,
        unresolved_dynamic_surfaces: 0,
        inline_styles: 0,
        inline_style_attributes: 0,
        inline_css_urls: 0,
        inline_css_imports: 0,
        inline_css_chars: 0,
        pending_css_chars: 0,
        style_fragments: Vec::new(),
        unsupported_schemes: 0,
        missing_sources: 0,
        html_base_href_count: 0,
        error: None,
    });
    {
        let tokenizer = Tokenizer::new(Sink(&state), TokenizerOpts::default());
        let queue = BufferQueue::default();
        queue.push_back(StrTendril::from_slice(markup));
        let _ = tokenizer.feed(&queue);
        tokenizer.end();
    }
    let state = state.into_inner();
    if let Some(error) = state.error {
        return Err(error);
    }
    if !state.style_fragments.is_empty() {
        return Err(DiscoveryError::UnclosedStyleElement);
    }
    Ok(VisualInventory {
        candidates: state.urls.into_iter().collect(),
        observed_references: state.observed_references,
        inline_styles: state.inline_styles,
        inline_style_attributes: state.inline_style_attributes,
        inline_css_urls: state.inline_css_urls,
        inline_css_imports: state.inline_css_imports,
        unresolved_dynamic_surfaces: state.unresolved_dynamic_surfaces,
        unsupported_schemes: state.unsupported_schemes,
        missing_sources: state.missing_sources,
        html_base_href_count: state.html_base_href_count,
        document_url: document_url.to_owned(),
        html_base_url: state.base.map(String::from),
        css_and_font_dependencies_verified: false,
        required_media_verified: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "https://en.wikipedia.org/wiki/Page";

    fn run(markup: &str) -> Result<VisualInventory, DiscoveryError> {
        inventory(markup, DOC, MAX_REFERENCES)
    }

    fn urls(markup: &str) -> Vec<(&'static str, String)> {
        run(markup)
            .unwrap()
            .candidates
            .into_iter()
            .map(|c| (c.kind.as_str(), c.url))
            .collect()
    }

    fn pair(kind: &'static str, url: &str) -> (&'static str, String) {
        (kind, url.to_owned())
    }

    #[test]
    fn resolves_against_document_without_base_and_never_claims_verification() {
        let result = run(r#"<img src="a.png"><img src="//c.test/b.png">"#).unwrap();
        let found: Vec<_> = result.candidates.iter().map(|c| c.url.as_str()).collect();
        assert_eq!(
            vec![
                "https://c.test/b.png",
                "https://en.wikipedia.org/wiki/a.png"
            ],
            found
        );
        assert_eq!(None, result.html_base_url);
        assert!(!result.required_media_verified);
        assert!(!result.css_and_font_dependencies_verified);
    }

    #[test]
    fn base_href_applies_absolute_relative_and_scheme_relative() {
        assert_eq!(
            vec![pair("image", "https://upload.wikimedia.org/x/a.png")],
            urls(r#"<base href="https://upload.wikimedia.org/x/"><img src="a.png">"#)
        );
        let result = run(r#"<base href="/static/"><img src="a.png">"#).unwrap();
        assert_eq!(
            Some("https://en.wikipedia.org/static/".to_owned()),
            result.html_base_url
        );
        assert_eq!(
            vec![pair("image", "http://c.test/a.png")],
            urls(r#"<base href="http://h.test/"><img src="//c.test/a.png">"#)
        );
    }

    #[test]
    fn base_without_href_and_foreign_content_base_are_ignored() {
        assert_eq!(
            0,
            run(r#"<base target="_blank"><img src="a.png">"#)
                .unwrap()
                .html_base_href_count
        );
        for page in [
            r#"<svg><base href="/evil/"></svg><img src="a.png">"#,
            r#"<math><base href="/evil/"></math><img src="a.png">"#,
        ] {
            let result = run(page).unwrap();
            assert_eq!(0, result.html_base_href_count, "{page}");
            assert_eq!(
                "https://en.wikipedia.org/wiki/a.png", result.candidates[0].url,
                "{page}"
            );
        }
    }

    #[test]
    fn unsafe_late_duplicate_or_multiple_base_fails_closed() {
        assert_eq!(
            Err(DiscoveryError::BaseHrefAfterResourceReference),
            run(r#"<img src="a.png"><base href="/x/">"#).map(|_| ())
        );
        for href in [
            "data:,",
            "javascript:alert(1)",
            "",
            "ftp://h/",
            "#f",
            "blob:https://a.test/x",
            " /x/",
            "https://u@h.test/",
        ] {
            let page = format!(r#"<base href="{href}"><img src="a.png">"#);
            assert!(run(&page).is_err(), "base {href:?}");
        }
        assert_eq!(
            Err(DiscoveryError::DuplicateResourceAttribute),
            run(r#"<base href="/a/" href="/b/">"#).map(|_| ())
        );
        assert_eq!(
            Err(DiscoveryError::MultipleBaseHref),
            run(r#"<base href="/a/"><base href="/b/">"#).map(|_| ())
        );
        assert!(run(r#"<base href="/a/"><base target="x">"#).is_ok());
    }

    #[test]
    fn invalid_document_url_budget_and_length_fail_closed() {
        for document in [
            "/wiki/Page",
            "ftp://h.test/x",
            "data:,",
            "",
            "https://u@h.test/x",
        ] {
            assert_eq!(
                Err(DiscoveryError::InvalidDocumentUrl),
                inventory("<img src=a.png>", document, 10).map(|_| ()),
                "{document:?}"
            );
        }
        assert_eq!(
            Err(DiscoveryError::InvalidBudget),
            inventory("", DOC, 0).map(|_| ())
        );
        let long = format!(
            r#"<base href="/{}/"><img src="{}.png">"#,
            "d".repeat(2500),
            "f".repeat(2500)
        );
        assert_eq!(
            Err(DiscoveryError::UnsafeCandidateUrl),
            run(&long).map(|_| ())
        );
        assert_eq!(
            vec![pair("image", "https://en.wikipedia.org/a.png")],
            urls(r#"<img src="../../../a.png">"#)
        );
    }

    #[test]
    fn inline_css_uses_html_base_and_skips_comments_and_strings() {
        let page = concat!(
            r#"<base href="/b/"><style>/* url(no.png) */ .x{background:url(i.png)}"#,
            r#" .y::after{content:"url(no2.png)"} @import "t.css"; "#,
            r#"@IMPORT URL( 'u.css' ) ;</style><p style="background:url('p.png')">"#
        );
        let result = run(page).unwrap();
        let found: Vec<_> = result
            .candidates
            .iter()
            .map(|c| (c.kind.as_str(), c.url.as_str()))
            .collect();
        assert_eq!(
            vec![
                ("css_import", "https://en.wikipedia.org/b/t.css"),
                ("css_import", "https://en.wikipedia.org/b/u.css"),
                ("css_url", "https://en.wikipedia.org/b/i.png"),
                ("css_url", "https://en.wikipedia.org/b/p.png"),
            ],
            found
        );
        assert_eq!(
            (1, 1),
            (result.inline_styles, result.inline_style_attributes)
        );
        assert_eq!((2, 2), (result.inline_css_imports, result.inline_css_urls));
        assert_eq!(
            Err(DiscoveryError::UnclosedStyleElement),
            run("<style>.x{}").map(|_| ())
        );
    }

    #[test]
    fn css_tokens_ignore_word_prefixed_and_malformed_functions() {
        let tokens =
            css_url_tokens("x-url(a.png) myurl(b.png) url(c.png) url( 'd.png' ) url(e.png");
        let found: Vec<_> = tokens.iter().map(|(_, u)| u.as_str()).collect();
        assert_eq!(vec!["c.png", "d.png"], found);
    }

    #[test]
    fn responsive_images_picture_and_preload_are_candidates() {
        let page = concat!(
            r#"<img src="a.png" srcset="a1.png 1x, a2.png 2x">"#,
            r#"<picture><source srcset="p.webp 100w, q.webp 200w"></picture>"#,
            r#"<source srcset="outside.png">"#,
            r#"<link rel="stylesheet" href="s.css">"#,
            r#"<link rel="preload" as="font" href="f.woff2">"#,
            r#"<link rel="preload" as="image" href="pi.png" imagesrcset="pr.png 1x">"#,
            r#"<link rel="preload" as="script" href="no.js">"#,
            r#"<object type="image/svg+xml; charset=x" data="o.svg"></object>"#,
            r#"<embed type="text/html" src="no.html">"#,
            r#"<video poster="v.jpg"></video><canvas></canvas><iframe src="x"></iframe>"#
        );
        let result = run(page).unwrap();
        let found: Vec<_> = result
            .candidates
            .iter()
            .map(|c| c.kind.as_str().to_owned() + " " + c.url.rsplit('/').next().unwrap())
            .collect();
        assert_eq!(
            vec![
                "av_preview v.jpg",
                "embedded_image o.svg",
                "image a.png",
                "picture_variant p.webp",
                "picture_variant q.webp",
                "preload_font f.woff2",
                "preload_image pi.png",
                "preload_responsive_image pr.png",
                "responsive_image a1.png",
                "responsive_image a2.png",
                "stylesheet s.css",
            ],
            found
        );
        assert_eq!(2, result.unresolved_dynamic_surfaces);
    }

    #[test]
    fn invalid_srcset_shapes_fail_closed() {
        for set in [
            "a.png 1x,, b.png 2x",
            "a.png 1x, b.png 100w",
            "a.png 1x, b.png 1.0x",
            "a.png 0x",
            "a.png 0w",
            "a.png 1x 2x",
            "a.png 1y",
            "data:image/png;base64,AAAA 1x",
        ] {
            let page = format!(r#"<img srcset="{set}">"#);
            assert!(
                matches!(run(&page), Err(DiscoveryError::InvalidResponsiveImage(_))),
                "{set:?}"
            );
        }
        assert_eq!(
            0,
            run(r#"<img srcset="">"#).unwrap().observed_references - 1
        );
    }

    #[test]
    fn svg_math_and_dynamic_surfaces() {
        let page = concat!(
            r#"<svg><filter id="blur"><feImage href="/filter.png"/></filter>"#,
            r#"<image xlink:href="/diagram.svg"/>"#,
            r##"<use href="#local"/><feImage href="#internal"/></svg>"##,
            r#"<math><mglyph src="g.png"></mglyph></math><mglyph src="no.png">"#
        );
        let result = run(page).unwrap();
        let found: Vec<_> = result
            .candidates
            .iter()
            .map(|c| (c.kind.as_str(), c.url.rsplit('/').next().unwrap()))
            .collect();
        assert_eq!(
            vec![
                ("math_glyph", "g.png"),
                ("svg_filter_image", "filter.png"),
                ("svg_image", "diagram.svg"),
            ],
            found
        );
        assert_eq!(5, result.observed_references);
        assert_eq!(
            Err(DiscoveryError::ConflictingSvgHref),
            run(r#"<svg><image href="/a.png" xlink:href="/b.png"/></svg>"#).map(|_| ())
        );
        assert!(run(r#"<svg><feImage href="/s.png" xlink:href="/s.png"/></svg>"#).is_ok());
    }

    #[test]
    fn duplicate_resource_attributes_fail_closed_but_plain_ones_do_not() {
        for page in [
            r#"<img src="a.png" src="b.png">"#,
            r#"<link rel="stylesheet" href="/a.css" href="/b.css">"#,
            r#"<div style="background:url(a.png)" style="background:url(b.png)">"#,
            r#"<svg><feImage href="a.png" href="b.png"/></svg>"#,
        ] {
            assert_eq!(
                Err(DiscoveryError::DuplicateResourceAttribute),
                run(page).map(|_| ()),
                "{page}"
            );
        }
        assert_eq!(
            0,
            run(r#"<div class="a" class="b">"#)
                .unwrap()
                .candidates
                .len()
        );
    }

    #[test]
    fn scripts_comments_and_text_do_not_produce_candidates() {
        let page = concat!(
            r#"<script>document.write('<img src="s.png">')</script>"#,
            r#"<!-- <img src="c.png"> --><title><img src="t.png"></title>"#,
            r#"<textarea><img src="ta.png"></textarea>&lt;img src="e.png"&gt;"#
        );
        assert!(run(page).unwrap().candidates.is_empty());
    }

    #[test]
    fn unsupported_missing_and_budgeted_references() {
        let result =
            run(r#"<img src="data:image/png;base64,AAAA"><img><img src="mailto:x">"#).unwrap();
        assert_eq!(
            (2, 1, 3),
            (
                result.unsupported_schemes,
                result.missing_sources,
                result.observed_references
            )
        );
        assert!(result.candidates.is_empty());
        let repeated =
            r#"<embed type="image/png" src="same.png"><embed type="image/png" src="same.png">"#;
        assert_eq!(1, run(repeated).unwrap().candidates.len());
        assert_eq!(
            Err(DiscoveryError::Budget("visual reference count")),
            inventory(repeated, DOC, 1).map(|_| ())
        );
        assert!(run(r#"<img src="a b.png">"#).is_err());
        assert!(run(r#"<img src="a\b.png">"#).is_err());
        assert!(run(r#"<img src="http://h.test:0/a.png">"#).is_err());
        assert!(run(r#"<img src="http://u@h.test/a.png">"#).is_err());
    }

    #[test]
    fn kind_order_matches_string_order() {
        let kinds = [
            CandidateKind::AvPreview,
            CandidateKind::CssImport,
            CandidateKind::CssUrl,
            CandidateKind::EmbeddedImage,
            CandidateKind::Image,
            CandidateKind::MathGlyph,
            CandidateKind::PictureVariant,
            CandidateKind::PreloadFont,
            CandidateKind::PreloadImage,
            CandidateKind::PreloadResponsiveImage,
            CandidateKind::PreloadStyle,
            CandidateKind::ResponsiveImage,
            CandidateKind::Stylesheet,
            CandidateKind::SvgFilterImage,
            CandidateKind::SvgImage,
            CandidateKind::SvgUse,
        ];
        let strings: Vec<_> = kinds.iter().map(|k| k.as_str()).collect();
        let mut sorted = strings.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, strings);
    }
}
