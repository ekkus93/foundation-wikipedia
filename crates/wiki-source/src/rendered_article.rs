//! SRC-003: turn one verified rendered revision (MediaWiki `action=parse`
//! HTML for an exact revision, e.g. a revision named by a public dump) into
//! canonical article blocks.
//!
//! The upstream HTML is untrusted. It is only *read* here: every block is
//! plain text (or MathML rebuilt from an allowlist), and the stored
//! `rendered_html` is the separately sanitized copy. Page furniture
//! (navigation boxes, edit links, citation markers, hidden metadata, styles)
//! is skipped by class.
//!
//! Images are not downloaded here. Each figure, infobox or gallery image
//! becomes a `Media` block pointing at an [`ImageReference`]; the caller
//! resolves each reference to real Commons metadata (licence, creator,
//! attribution) and passes it to [`assemble_article`]. A `MediaAsset` with
//! no `sha256` is a required image whose bytes are not installed yet.

use std::cell::RefCell;
use std::collections::HashMap;

use html5ever::tokenizer::states::RawKind;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use wiki_model::{
    Article, Block, BlockContent, MediaAsset, Revision, Section, ARTICLE_SCHEMA_VERSION,
};

use crate::online_article::OnlineRevision;
use crate::reader_document::{MATHML_ATTRIBUTES, MATHML_TAGS};

/// Bytes of raw HTML accepted for one article.
const MAX_HTML_BYTES: usize = 8 * 1024 * 1024;
/// Images recorded for one article.
const MAX_IMAGES: usize = 2_000;
/// Element nesting tracked before the document is refused.
const MAX_DEPTH: usize = 512;

/// Classes whose whole element is page furniture, not article content.
const SKIPPED_CLASSES: [&str; 22] = [
    "mw-editsection",
    "navbox",
    "navbox-styles",
    "reference",
    "mw-cite-backlink",
    "noprint",
    "metadata",
    "sistersitebox",
    "side-box",
    "hatnote",
    "shortdescription",
    "mw-empty-elt",
    "toc",
    "mw-jump-link",
    "catlinks",
    "ambox",
    "printfooter",
    "authority-control",
    "portalbox",
    "mw-references-columns-styles",
    "plainlinks-print",
    "box-Multiple_issues",
];

/// Elements that never contain article text.
const SKIPPED_TAGS: [&str; 8] = [
    "style", "script", "link", "meta", "noscript", "template", "svg", "button",
];

/// Elements that have no end tag.
const VOID_TAGS: [&str; 12] = [
    "img", "br", "hr", "meta", "link", "wbr", "input", "source", "track", "area", "col", "embed",
];

/// One image the article requires offline, in first-appearance order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageReference {
    /// `File:` page title with spaces, e.g. `File:Hypotenuse.svg`.
    pub file_title: String,
    pub caption: Option<String>,
    /// The file is audio or video; the required image is its still
    /// (MOBPACK-003), stored as an A/V preview.
    pub av_preview: bool,
}

/// Blocks of one revision before image metadata is attached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedArticle {
    pub lead: Vec<Block>,
    pub sections: Vec<Section>,
    /// `Media { media_index }` blocks index into this list.
    pub images: Vec<ImageReference>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderedArticleError {
    TooLarge,
    TooDeep,
    TooManyImages,
    /// No readable text was found.
    Empty,
    /// The number of media records does not match the image references.
    MediaMismatch,
    /// A media record does not name the referenced file.
    MediaNotForFile(usize),
    InvalidArticle(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Plain,
    Skip,
    Heading(usize),
    Paragraph,
    List,
    Item,
    Table,
    Infobox,
    Row,
    Cell { header: bool },
    Quote,
    Figure,
    Caption,
    Gallery,
    Math,
}

struct Element {
    name: String,
    kind: Kind,
}

/// A MathML element being linearized to text (`a^2 + b^2`).
struct MathFrame {
    name: String,
    children: usize,
}

#[derive(Default)]
struct Walker {
    stack: Vec<Element>,
    text: String,
    lead: Vec<Block>,
    /// Open sections by depth (level 1 = h2).
    open: Vec<(usize, Section)>,
    done: Vec<Section>,
    images: Vec<ImageReference>,
    image_index: HashMap<String, usize>,
    /// Images seen in the current figure/infobox/gallery, emitted at its end.
    pending_media: Vec<usize>,
    caption: String,
    list_items: Vec<String>,
    rows: Vec<Vec<String>>,
    row: Vec<(bool, String)>,
    /// Text of the current math element: (linear text, MathML).
    math: Option<(String, String)>,
    math_frames: Vec<MathFrame>,
    /// The last finished math element, to detect math-only paragraphs.
    last_math: Option<(String, String)>,
    math_only: bool,
    error: Option<RenderedArticleError>,
}

fn has_class(tag: &Tag, wanted: &str) -> bool {
    tag.attrs.iter().any(|attr| {
        &*attr.name.local == "class" && attr.value.split_ascii_whitespace().any(|c| c == wanted)
    })
}

fn attr<'a>(tag: &'a Tag, name: &str) -> Option<&'a str> {
    tag.attrs
        .iter()
        .find(|attr| &*attr.name.local == name)
        .map(|attr| &*attr.value)
}

fn collapse(text: &str) -> String {
    // Invisible MathML operators (function application, invisible times,
    // separator, plus) carry no reading text.
    let text: String = text
        .chars()
        .filter(|c| !matches!(c, '\u{2061}'..='\u{2064}' | '\u{00ad}'))
        .collect();
    text.split(|c: char| c.is_whitespace() || c == '\u{2060}' || c == '\u{200b}')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `/wiki/File:Some_name.svg` → `File:Some name.svg`.
fn file_title(href: &str) -> Option<String> {
    let rest = href.strip_prefix("/wiki/")?;
    let decoded = url::form_urlencoded::parse(format!("t={rest}").as_bytes())
        .next()
        .map(|(_, value)| value.into_owned())?;
    let (namespace, name) = decoded.split_once(':')?;
    let name = name.replace('_', " ");
    let valid = matches!(namespace, "File" | "Image")
        && !name.trim().is_empty()
        && name.len() <= 512
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '#' | '|'));
    valid.then(|| format!("File:{}", name.trim()))
}

/// `//upload.wikimedia.org/…/thumb/a/ab/Some_name.svg/250px-….png` →
/// `File:Some name.svg`; `None` for anything that is not a Wikimedia file.
fn source_file_title(src: &str) -> Option<String> {
    let name = crate::media_download::media_file_name(src)?.replace('_', " ");
    let valid = !name.trim().is_empty()
        && name.len() <= 512
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '#' | '|'));
    valid.then(|| format!("File:{}", name.trim()))
}

impl Walker {
    fn inside(&self, kind: Kind) -> bool {
        self.stack.iter().any(|element| element.kind == kind)
    }

    fn inside_cell(&self) -> bool {
        self.stack
            .iter()
            .any(|element| matches!(element.kind, Kind::Cell { .. }))
    }

    fn skipping(&self) -> bool {
        self.inside(Kind::Skip)
    }

    /// A block element that is not nested in a list, table or other block.
    fn top_level_block(&self) -> bool {
        !self.stack.iter().any(|element| {
            matches!(
                element.kind,
                Kind::Paragraph
                    | Kind::List
                    | Kind::Table
                    | Kind::Infobox
                    | Kind::Quote
                    | Kind::Figure
                    | Kind::Gallery
                    | Kind::Heading(_)
            )
        })
    }

    fn blocks(&mut self) -> &mut Vec<Block> {
        match self.open.last_mut() {
            Some((_, section)) => &mut section.blocks,
            None => &mut self.lead,
        }
    }

    fn push_block(&mut self, content: BlockContent) {
        let blocks = self.blocks();
        let ordinal = u32::try_from(blocks.len()).unwrap_or(u32::MAX);
        blocks.push(Block { ordinal, content });
    }

    fn take_text(&mut self) -> String {
        collapse(&std::mem::take(&mut self.text))
    }

    /// Emit loose text outside any block (rare) as its own paragraph.
    fn flush_loose_text(&mut self) {
        if self.top_level_block() {
            let text = self.take_text();
            if !text.is_empty() {
                self.push_block(BlockContent::Paragraph(text));
            }
        }
    }

    fn close_sections_to(&mut self, level: usize) {
        while self.open.last().is_some_and(|(open, _)| *open >= level) {
            let (_, section) = self.open.pop().expect("checked");
            match self.open.last_mut() {
                Some((_, parent)) => {
                    let ordinal = u32::try_from(parent.subsections.len()).unwrap_or(u32::MAX);
                    parent.subsections.push(Section { ordinal, ..section });
                }
                None => {
                    let ordinal = u32::try_from(self.done.len()).unwrap_or(u32::MAX);
                    self.done.push(Section { ordinal, ..section });
                }
            }
        }
    }

    /// Record a required image; `block` also places it as a media block
    /// (figures, infoboxes, galleries), otherwise it is required for the
    /// rendered page only (an inline flag or symbol).
    fn image(&mut self, title: String, block: bool) {
        let index = match self.image_index.get(&title) {
            Some(index) => *index,
            None => {
                if self.images.len() >= MAX_IMAGES {
                    self.error = Some(RenderedArticleError::TooManyImages);
                    return;
                }
                self.images.push(ImageReference {
                    file_title: title.clone(),
                    caption: None,
                    av_preview: false,
                });
                self.image_index.insert(title, self.images.len() - 1);
                self.images.len() - 1
            }
        };
        if block && !self.pending_media.contains(&index) {
            self.pending_media.push(index);
        }
    }

    fn emit_media(&mut self, caption: Option<String>) {
        for index in std::mem::take(&mut self.pending_media) {
            if let Some(caption) = caption.clone().filter(|c| !c.is_empty()) {
                self.images[index].caption.get_or_insert(caption);
            }
            self.push_block(BlockContent::Media { media_index: index });
        }
    }

    fn start(&mut self, tag: Tag) -> TokenSinkResult<()> {
        let name = tag.name.to_string();
        if self.math.is_some() {
            self.math_start(&tag, &name);
            return TokenSinkResult::Continue;
        }
        if self.skipping() {
            if !VOID_TAGS.contains(&name.as_str()) && !tag.self_closing {
                self.stack.push(Element {
                    name,
                    kind: Kind::Plain,
                });
            }
            return TokenSinkResult::Continue;
        }
        // Images are recorded only where they are article content.
        if name == "a"
            && (has_class(&tag, "mw-file-description") || has_class(&tag, "image"))
            && (self.inside(Kind::Figure)
                || self.inside(Kind::Infobox)
                || self.inside(Kind::Gallery))
        {
            if let Some(title) = attr(&tag, "href").and_then(file_title) {
                self.image(title, true);
            }
        }
        // Every displayed Wikimedia file is required offline (PACK-003),
        // including figures without a file link (`link=`) and inline images
        // such as flags. Furniture was skipped above; formula fallback images
        // are not Wikimedia files (the MathML is kept instead).
        if name == "img" {
            if let Some(title) = attr(&tag, "src").and_then(source_file_title) {
                let block = self.inside(Kind::Figure)
                    || self.inside(Kind::Infobox)
                    || self.inside(Kind::Gallery);
                self.image(title, block);
            }
        }
        // A video is shown offline as its still image (MOBPACK-003): the
        // player's own poster for the file it names.
        if name == "video" {
            let still = attr(&tag, "poster")
                .and_then(|poster| poster.split(['?', '#']).next().map(str::to_owned))
                .and_then(|poster| source_file_title(&poster))
                .filter(|title| {
                    attr(&tag, "data-mwtitle").is_some_and(|name| {
                        crate::media_download::file_title_key(title)
                            == name.trim().replace(' ', "_")
                    })
                });
            if let Some(title) = still {
                let block = self.inside(Kind::Figure)
                    || self.inside(Kind::Infobox)
                    || self.inside(Kind::Gallery);
                self.image(title.clone(), block);
                if let Some(index) = self.image_index.get(&title) {
                    self.images[*index].av_preview = true;
                }
            }
        }
        // Keep exponents readable: 10<sup>20</sup> is 10^20, not 1020.
        if name == "sup" && !SKIPPED_CLASSES.iter().any(|class| has_class(&tag, class)) {
            self.text.push('^');
        }
        if name == "sub" {
            self.text.push('_');
        }
        if name == "br" && (self.inside_cell() || self.inside(Kind::Item)) {
            self.text.push(' ');
        }
        if VOID_TAGS.contains(&name.as_str()) || tag.self_closing {
            return TokenSinkResult::Continue;
        }
        if self.stack.len() >= MAX_DEPTH {
            self.error = Some(RenderedArticleError::TooDeep);
            return TokenSinkResult::Continue;
        }
        let hidden = attr(&tag, "style").is_some_and(|style| {
            style
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
                .contains("display:none")
        });
        let kind = if SKIPPED_TAGS.contains(&name.as_str())
            || hidden
            || SKIPPED_CLASSES.iter().any(|class| has_class(&tag, class))
        {
            Kind::Skip
        } else {
            match name.as_str() {
                "math" => Kind::Math,
                "h2" | "h3" | "h4" | "h5" | "h6" if self.top_level_block() => {
                    Kind::Heading(usize::from(name.as_bytes()[1] - b'1'))
                }
                "p" | "dd" | "dt" if self.top_level_block() => Kind::Paragraph,
                "ul" | "ol" if has_class(&tag, "gallery") && self.top_level_block() => {
                    Kind::Gallery
                }
                "ul" | "ol" if self.top_level_block() => Kind::List,
                "li" if self.stack.last().is_some_and(|e| e.kind == Kind::List) => Kind::Item,
                "table" if has_class(&tag, "infobox") && self.top_level_block() => Kind::Infobox,
                "table" if self.top_level_block() => Kind::Table,
                "tr" if self.inside(Kind::Table) || self.inside(Kind::Infobox) => {
                    if self.inside_cell() {
                        Kind::Plain
                    } else {
                        Kind::Row
                    }
                }
                "th" | "td" if self.stack.last().is_some_and(|e| e.kind == Kind::Row) => {
                    Kind::Cell {
                        header: name == "th",
                    }
                }
                "blockquote" if self.top_level_block() => Kind::Quote,
                "figure" if self.top_level_block() => Kind::Figure,
                "figcaption" if self.inside(Kind::Figure) => Kind::Caption,
                _ if (has_class(&tag, "gallerytext") || has_class(&tag, "infobox-caption"))
                    && (self.inside(Kind::Gallery) || self.inside(Kind::Infobox)) =>
                {
                    Kind::Caption
                }
                _ => Kind::Plain,
            }
        };
        if kind != Kind::Plain && kind != Kind::Skip && kind != Kind::Math {
            if matches!(
                kind,
                Kind::Heading(_)
                    | Kind::Paragraph
                    | Kind::List
                    | Kind::Table
                    | Kind::Infobox
                    | Kind::Quote
                    | Kind::Figure
                    | Kind::Gallery
            ) {
                self.flush_loose_text();
                self.text.clear();
                self.math_only = true;
                self.last_math = None;
            }
            if matches!(kind, Kind::Item | Kind::Cell { .. }) {
                self.text.clear();
            }
            if kind == Kind::Caption {
                self.caption.clear();
            }
        }
        if kind == Kind::Math {
            self.math = Some((String::new(), String::from("<math>")));
            self.math_frames.clear();
            self.stack.push(Element { name, kind });
            return TokenSinkResult::Continue;
        }
        let raw = matches!(name.as_str(), "style" | "script" | "noscript" | "template");
        self.stack.push(Element { name, kind });
        if raw {
            TokenSinkResult::RawData(RawKind::Rawtext)
        } else {
            TokenSinkResult::Continue
        }
    }

    fn math_start(&mut self, tag: &Tag, name: &str) {
        let (linear, mathml) = self.math.as_mut().expect("in math");
        if let Some(parent) = self.math_frames.last_mut() {
            let index = parent.children;
            parent.children += 1;
            let prefix = match (parent.name.as_str(), index) {
                ("msup", 1) | ("msubsup", 2) | ("mover", 1) => "^",
                ("msub", 1) | ("msubsup", 1) | ("munder", 1) => "_",
                ("munderover", 1) => "_",
                ("munderover", 2) => "^",
                ("mfrac", 0) => "(",
                ("mfrac", 1) => ")/(",
                ("mroot", 1) => ")^(1/",
                _ => "",
            };
            linear.push_str(prefix);
        }
        match name {
            "msqrt" => linear.push_str("√("),
            "mroot" => linear.push_str("√("),
            _ => {}
        }
        if MATHML_TAGS.contains(&name) {
            mathml.push('<');
            mathml.push_str(name);
            for attribute in &tag.attrs {
                let key = &*attribute.name.local;
                if MATHML_ATTRIBUTES.contains(&key) {
                    mathml.push_str(&format!(" {key}=\"{}\"", escape(&attribute.value)));
                }
            }
            mathml.push('>');
        }
        if !tag.self_closing {
            self.math_frames.push(MathFrame {
                name: name.to_owned(),
                children: 0,
            });
        }
    }

    fn end(&mut self, name: &str) {
        if self.math.is_some() && name != "math" {
            if let Some(position) = self.math_frames.iter().rposition(|f| f.name == name) {
                self.math_frames.truncate(position);
                let (linear, mathml) = self.math.as_mut().expect("in math");
                match name {
                    "msqrt" | "mroot" | "mfrac" => linear.push(')'),
                    "mo" => {}
                    _ => {}
                }
                if MATHML_TAGS.contains(&name) {
                    mathml.push_str(&format!("</{name}>"));
                }
            }
            return;
        }
        let Some(position) = self.stack.iter().rposition(|element| element.name == name) else {
            return;
        };
        while self.stack.len() > position {
            let element = self.stack.pop().expect("checked");
            self.close(element.kind);
        }
    }

    fn close(&mut self, kind: Kind) {
        if self.skipping() && kind != Kind::Skip {
            return;
        }
        match kind {
            Kind::Plain | Kind::Skip | Kind::Row | Kind::Cell { .. } | Kind::Item => {
                self.close_container(kind)
            }
            Kind::Math => {
                let (linear, mathml) = self.math.take().unwrap_or_default();
                let linear = collapse(&linear);
                let mathml = format!("{mathml}</math>");
                if self.inside_cell() || self.inside(Kind::Item) || self.inside(Kind::Caption) {
                    self.text.push_str(&linear);
                } else {
                    self.text.push_str(&linear);
                    self.last_math = Some((linear, mathml));
                }
            }
            Kind::Heading(level) => {
                let heading = self.take_text();
                if !heading.is_empty() {
                    self.close_sections_to(level);
                    self.open.push((
                        level,
                        Section {
                            ordinal: 0,
                            heading,
                            blocks: Vec::new(),
                            subsections: Vec::new(),
                        },
                    ));
                }
            }
            Kind::Paragraph => {
                let text = self.take_text();
                match self.last_math.take() {
                    Some((linear, html)) if self.math_only && collapse(&linear) == text => self
                        .push_block(BlockContent::Math {
                            source: linear,
                            html,
                        }),
                    _ if !text.is_empty() => self.push_block(BlockContent::Paragraph(text)),
                    _ => {}
                }
            }
            Kind::Quote => {
                let text = self.take_text();
                if !text.is_empty() {
                    self.push_block(BlockContent::Quote(text));
                }
            }
            Kind::List => {
                let items: Vec<String> = std::mem::take(&mut self.list_items)
                    .into_iter()
                    .filter(|item| !item.is_empty())
                    .collect();
                if !items.is_empty() {
                    self.push_block(BlockContent::List(items));
                }
                self.text.clear();
            }
            Kind::Table => {
                let rows: Vec<Vec<String>> = std::mem::take(&mut self.rows)
                    .into_iter()
                    .filter(|row| row.iter().any(|cell| !cell.is_empty()))
                    .collect();
                if !rows.is_empty() {
                    self.push_block(BlockContent::Table(rows));
                }
                self.text.clear();
            }
            Kind::Infobox => {
                let caption = collapse(&std::mem::take(&mut self.caption));
                self.emit_media(Some(caption));
                let fields: Vec<(String, String)> = std::mem::take(&mut self.rows)
                    .into_iter()
                    .filter_map(|row| match row.as_slice() {
                        [label, value] if !label.is_empty() => Some((label.clone(), value.clone())),
                        _ => None,
                    })
                    .collect();
                if !fields.is_empty() {
                    self.push_block(BlockContent::Infobox(fields));
                }
                self.text.clear();
            }
            Kind::Figure => {
                let caption = collapse(&std::mem::take(&mut self.caption));
                self.emit_media(Some(caption));
                self.text.clear();
            }
            Kind::Gallery => {
                self.emit_media(None);
                self.text.clear();
            }
            Kind::Caption => {
                let caption = self.take_text();
                if self.inside(Kind::Gallery) {
                    if let Some(index) = self.pending_media.last().copied() {
                        self.images[index].caption.get_or_insert(caption);
                    }
                } else {
                    self.caption = caption;
                }
            }
        }
    }

    fn close_container(&mut self, kind: Kind) {
        match kind {
            Kind::Item => {
                let text = self.take_text();
                self.list_items.push(text);
            }
            Kind::Cell { header } => {
                let text = self.take_text();
                self.row.push((header, text));
            }
            Kind::Row => {
                let cells = std::mem::take(&mut self.row);
                if self.inside(Kind::Infobox) {
                    // Label/value rows only: a header cell then a data cell.
                    if let [(true, label), (false, value)] = cells.as_slice() {
                        self.rows.push(vec![label.clone(), value.clone()]);
                    }
                } else if !cells.is_empty() {
                    self.rows
                        .push(cells.into_iter().map(|(_, text)| text).collect());
                }
            }
            _ => {}
        }
    }

    fn characters(&mut self, text: &str) {
        if self.skipping() {
            return;
        }
        if let Some((linear, _)) = self.math.as_mut() {
            linear.push_str(text);
            let (_, mathml) = self.math.as_mut().expect("in math");
            mathml.push_str(&escape(text));
            return;
        }
        if self.inside(Kind::Caption) {
            self.text.push_str(text);
            return;
        }
        if !text.trim().is_empty() && self.inside(Kind::Paragraph) {
            // Text outside a math element: not a math-only paragraph.
            self.math_only = false;
        }
        self.text.push_str(text);
    }

    fn finish(mut self) -> Result<ExtractedArticle, RenderedArticleError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        while let Some(element) = self.stack.pop() {
            self.close(element.kind);
        }
        self.flush_loose_text();
        self.close_sections_to(0);
        if self.lead.is_empty() && self.done.is_empty() {
            return Err(RenderedArticleError::Empty);
        }
        Ok(ExtractedArticle {
            lead: self.lead,
            sections: self.done,
            images: self.images,
        })
    }
}

struct Sink(RefCell<Walker>);

impl TokenSink for Sink {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut walker = self.0.borrow_mut();
        match token {
            Token::TagToken(tag) => match tag.kind {
                TagKind::StartTag => walker.start(tag),
                TagKind::EndTag => {
                    walker.end(&tag.name);
                    TokenSinkResult::Continue
                }
            },
            Token::CharacterTokens(text) => {
                walker.characters(&text);
                TokenSinkResult::Continue
            }
            _ => TokenSinkResult::Continue,
        }
    }
}

/// Read the blocks and required images of one rendered revision. `html` is
/// the raw `parse.text` (it is never echoed back as markup).
pub fn extract_rendered_article(html: &str) -> Result<ExtractedArticle, RenderedArticleError> {
    if html.len() > MAX_HTML_BYTES {
        return Err(RenderedArticleError::TooLarge);
    }
    let tokenizer = Tokenizer::new(
        Sink(RefCell::new(Walker {
            math_only: true,
            ..Walker::default()
        })),
        TokenizerOpts::default(),
    );
    let input = BufferQueue::default();
    input.push_back(html.into());
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let walker = std::mem::take(&mut *tokenizer.sink.0.borrow_mut());
    walker.finish()
}

/// Build the canonical article from extracted blocks, the sanitized display
/// HTML and one `MediaAsset` per image reference (same order), each naming
/// the referenced file in its `source_url`.
pub fn assemble_article(
    revision: &OnlineRevision,
    language: &str,
    extracted: ExtractedArticle,
    sanitized_html: String,
    mut media: Vec<MediaAsset>,
) -> Result<Article, RenderedArticleError> {
    if media.len() != extracted.images.len() {
        return Err(RenderedArticleError::MediaMismatch);
    }
    for (index, (asset, image)) in media.iter().zip(&extracted.images).enumerate() {
        let name = image
            .file_title
            .trim_start_matches("File:")
            .replace(' ', "_");
        let encoded: String = url::form_urlencoded::byte_serialize(name.as_bytes()).collect();
        let tail = asset.source_url.rsplit('/').next().unwrap_or_default();
        if tail != name && tail != encoded && tail != encoded.replace("%2C", ",") {
            return Err(RenderedArticleError::MediaNotForFile(index));
        }
    }
    // A video's image is its still: the video itself is online only.
    for (asset, image) in media.iter_mut().zip(&extracted.images) {
        asset.is_av_preview = image.av_preview;
    }
    let mut article = Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: revision.key.clone(),
        revision: Revision {
            revision_id: revision.revision_id,
            timestamp: revision.timestamp.clone(),
            content_sha256: "0".repeat(64),
        },
        title: revision.title.clone(),
        display_title: revision.title.clone(),
        language: language.to_owned(),
        namespace: revision.namespace,
        aliases: Vec::new(),
        wikidata_id: None,
        lead: extracted.lead,
        sections: extracted.sections,
        references: Vec::new(),
        links: Vec::new(),
        media,
        rendered_html: sanitized_html,
        is_disambiguation: false,
    };
    article.revision.content_sha256 =
        crate::enterprise_verified::enterprise_normalize::digest_article(&article)
            .map_err(|error| RenderedArticleError::InvalidArticle(format!("{error:?}")))?;
    article
        .validate()
        .map_err(|error| RenderedArticleError::InvalidArticle(format!("{error:?}")))?;
    Ok(article)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online_article::parse_rendered_revision_with_source;
    use wiki_model::ArticleKey;

    const HYPOTENUSE: &str =
        include_str!("../../../fixtures/wikimedia/enwiki/hypotenuse-1383416967.parse.json");

    fn pinned() -> OnlineRevision {
        OnlineRevision {
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 251366,
            },
            title: "Hypotenuse".into(),
            namespace: 0,
            revision_id: 1383416967,
            timestamp: "2026-11-02T15:48:37Z".into(),
        }
    }

    fn all_text(extracted: &ExtractedArticle) -> String {
        fn blocks(out: &mut String, blocks: &[Block]) {
            for block in blocks {
                out.push_str(&format!("{:?}\n", block.content));
            }
        }
        fn sections(out: &mut String, list: &[Section], depth: usize) {
            for section in list {
                out.push_str(&format!("{}# {}\n", "  ".repeat(depth), section.heading));
                blocks(out, &section.blocks);
                sections(out, &section.subsections, depth + 1);
            }
        }
        let mut out = String::new();
        blocks(&mut out, &extracted.lead);
        sections(&mut out, &extracted.sections, 0);
        out
    }

    fn paragraphs(blocks: &[Block]) -> Vec<&str> {
        blocks
            .iter()
            .filter_map(|block| match &block.content {
                BlockContent::Paragraph(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_real_revision_reads_as_sections_with_math_and_required_images() {
        let (rendered, raw) = parse_rendered_revision_with_source(HYPOTENUSE, &pinned()).unwrap();
        let extracted = extract_rendered_article(&raw).unwrap();
        let text = all_text(&extracted);

        // Lead: the figure first, then the opening sentence.
        assert_eq!(
            extracted.lead[0].content,
            BlockContent::Media { media_index: 0 }
        );
        assert!(paragraphs(&extracted.lead)[0].starts_with(
            "In geometry, a hypotenuse is the side of a right triangle that is opposite"
        ));
        let headings: Vec<&str> = extracted
            .sections
            .iter()
            .map(|s| s.heading.as_str())
            .collect();
        assert_eq!(
            headings,
            [
                "Etymology",
                "Properties",
                "In computer programming",
                "See also",
                "References",
                "External links"
            ]
        );
        let properties = &extracted.sections[1];
        let nested: Vec<&str> = properties
            .subsections
            .iter()
            .map(|s| s.heading.as_str())
            .collect();
        assert_eq!(nested, ["Pythagorean theorem", "Sine and cosine"]);

        // Math is readable text; invisible operators are gone.
        assert!(text.contains("a^2+b^2=c^2"));
        assert!(text.contains("−2abcosθ=c^2"));
        assert!(!text.contains('\u{2061}'));
        // Furniture is skipped: no citation markers, styles or side boxes.
        assert!(!text.contains("[4]"));
        assert!(!text.contains("mw-parser-output"));
        assert!(!text.contains("Wiktionary"));

        let files: Vec<&str> = extracted
            .images
            .iter()
            .map(|i| i.file_title.as_str())
            .collect();
        assert_eq!(
            files,
            [
                "File:Hypotenuse.svg",
                "File:Pythagorean.svg",
                "File:Trigono sine en2.svg"
            ]
        );
        assert_eq!(
            extracted.images[0].caption.as_deref(),
            Some("A right-angled triangle and its hypotenuse")
        );

        // Assembled with one media record per image (bytes not installed).
        let media = files
            .iter()
            .map(|file| MediaAsset {
                source_url: format!(
                    "https://upload.wikimedia.org/wikipedia/commons/x/xx/{}",
                    file.trim_start_matches("File:").replace(' ', "_")
                ),
                mime_type: "image/svg+xml".into(),
                sha256: None,
                license: "CC BY-SA 3.0".into(),
                creator: "Example".into(),
                attribution: "Example, CC BY-SA 3.0".into(),
                is_av_preview: false,
                derivation: None,
            })
            .collect::<Vec<_>>();
        let article = assemble_article(
            &pinned(),
            "en",
            extracted.clone(),
            rendered.html.clone(),
            media.clone(),
        )
        .unwrap();
        article.validate().unwrap();
        assert_eq!(article.revision.revision_id, 1383416967);
        assert_ne!(article.revision.content_sha256, "0".repeat(64));
        assert_eq!(article.media.len(), 3);

        // Media must match the referenced files, one for one.
        assert_eq!(
            assemble_article(
                &pinned(),
                "en",
                extracted.clone(),
                rendered.html.clone(),
                media[..2].to_vec()
            ),
            Err(RenderedArticleError::MediaMismatch)
        );
        let mut swapped = media;
        swapped.swap(0, 1);
        assert_eq!(
            assemble_article(&pinned(), "en", extracted, rendered.html, swapped),
            Err(RenderedArticleError::MediaNotForFile(0))
        );
    }

    #[test]
    fn infoboxes_tables_display_math_and_galleries_become_blocks() {
        let html = concat!(
            "<div class=\"mw-parser-output\">",
            "<style>.x{color:red}</style>",
            "<table class=\"infobox\"><tbody>",
            "<tr><th colspan=\"2\">Ceres</th></tr>",
            "<tr><td colspan=\"2\"><span typeof=\"mw:File\"><a href=\"/wiki/File:Ceres_-_RC3.jpg\" class=\"mw-file-description\"><img src=\"//upload.wikimedia.org/a.jpg\"></a></span>",
            "<div class=\"infobox-caption\">Ceres seen by <i>Dawn</i></div></td></tr>",
            "<tr><th>Discovered by</th><td>Giuseppe Piazzi<sup class=\"reference\">[1]</sup></td></tr>",
            "<tr><th>Mass</th><td>9.38<span>×10</span><sup>20</sup> kg</td></tr>",
            "</tbody></table>",
            "<p>Ceres is a dwarf planet.</p>",
            "<div class=\"mw-heading mw-heading2\"><h2 id=\"Orbit\">Orbit<span class=\"mw-editsection\">edit</span></h2></div>",
            "<dl><dd><math display=\"block\"><mrow><mi>T</mi><mo>=</mo><mfrac><mn>1</mn><mn>2</mn></mfrac></mrow></math></dd></dl>",
            "<table class=\"wikitable\"><tr><th>Body</th><th>Days</th></tr><tr><td>Ceres</td><td>1682</td></tr></table>",
            "<ul class=\"gallery\"><li class=\"gallerybox\"><a href=\"/wiki/File:Ahuna_Mons.jpg\" class=\"mw-file-description\"><img src=\"//upload.wikimedia.org/b.jpg\"></a><div class=\"gallerytext\">Ahuna Mons</div></li></ul>",
            "<div class=\"navbox\"><p>Dwarf planets navigation</p></div>",
            "<p>Ends here.<span style=\"display: none\">hidden</span></p>",
            "</div>"
        );
        let extracted = extract_rendered_article(html).unwrap();
        assert_eq!(
            extracted.lead[0].content,
            BlockContent::Media { media_index: 0 }
        );
        assert_eq!(
            extracted.lead[1].content,
            BlockContent::Infobox(vec![
                ("Discovered by".into(), "Giuseppe Piazzi".into()),
                ("Mass".into(), "9.38×10^20 kg".into()),
            ])
        );
        assert_eq!(
            extracted.lead[2].content,
            BlockContent::Paragraph("Ceres is a dwarf planet.".into())
        );
        let orbit = &extracted.sections[0];
        assert_eq!(orbit.heading, "Orbit");
        let BlockContent::Math { source, html } = &orbit.blocks[0].content else {
            panic!("expected display math, got {:?}", orbit.blocks[0].content);
        };
        assert_eq!(source, "T=(1)/(2)");
        assert!(html.starts_with("<math><mrow><mi>T</mi>"));
        assert_eq!(
            orbit.blocks[1].content,
            BlockContent::Table(vec![
                vec!["Body".into(), "Days".into()],
                vec!["Ceres".into(), "1682".into()],
            ])
        );
        assert_eq!(
            orbit.blocks[2].content,
            BlockContent::Media { media_index: 1 }
        );
        assert_eq!(
            orbit.blocks[3].content,
            BlockContent::Paragraph("Ends here.".into())
        );
        assert_eq!(orbit.blocks.len(), 4, "navbox skipped");
        assert_eq!(
            extracted.images,
            vec![
                ImageReference {
                    file_title: "File:Ceres - RC3.jpg".into(),
                    caption: Some("Ceres seen by Dawn".into()),
                    av_preview: false,
                },
                ImageReference {
                    file_title: "File:Ahuna Mons.jpg".into(),
                    caption: Some("Ahuna Mons".into()),
                    av_preview: false,
                },
            ]
        );
    }

    /// MOBPACK-003: a video's still is a required image, marked as an A/V
    /// preview; audio requires nothing; a poster of another file is ignored.
    #[test]
    fn a_video_requires_its_still_as_an_av_preview() {
        let html = concat!(
            "<p>Films are moving pictures.</p>",
            "<figure typeof=\"mw:File/Thumb\"><span><video poster=\"//thumb.wikimedia.org/wikipedia/commons/thumb/a/ab/Film.webm/250px-Film.webm.jpg?utm_source=x\" ",
            "data-durationhint=\"95\" data-mwtitle=\"Film.webm\"><source src=\"//upload.wikimedia.org/wikipedia/commons/transcoded/a/ab/Film.webm/Film.webm.240p.vp9.webm\"></video></span>",
            "<figcaption>A film</figcaption></figure>",
            "<audio data-mwtitle=\"Speech.ogg\"><source src=\"//upload.wikimedia.org/wikipedia/commons/transcoded/1/12/Speech.ogg/Speech.ogg.mp3\"></audio>",
            "<video poster=\"//thumb.wikimedia.org/wikipedia/commons/thumb/c/cd/Other.webm/250px-Other.webm.jpg\" data-mwtitle=\"Mismatch.webm\"></video>",
        );
        let extracted = extract_rendered_article(html).unwrap();
        assert_eq!(
            extracted.images,
            vec![ImageReference {
                file_title: "File:Film.webm".into(),
                caption: Some("A film".into()),
                av_preview: true,
            }]
        );
        // Assembled, its image is recorded as the video's preview.
        let still = MediaAsset {
            source_url: "https://upload.wikimedia.org/wikipedia/commons/a/ab/Film.webm".into(),
            mime_type: "image/jpeg".into(),
            sha256: None,
            license: "CC BY 3.0".into(),
            creator: "Example".into(),
            attribution: "Example, CC BY 3.0".into(),
            is_av_preview: false,
            derivation: None,
        };
        let stored = crate::reader_document::sanitize_online_article_html(html);
        let article = assemble_article(&pinned(), "en", extracted, stored, vec![still]).unwrap();
        assert!(article.media[0].is_av_preview);
        assert!(article
            .rendered_html
            .contains("Play online (needs Internet)"));
    }

    #[test]
    fn every_displayed_wikimedia_file_is_required_even_without_a_file_link() {
        let html = concat!(
            // A figure set to link= has no file link, only the image.
            "<figure typeof=\"mw:File/Thumb\"><span><img src=\"//upload.wikimedia.org/wikipedia/commons/thumb/a/ab/Right_triangle.svg/250px-Right_triangle.svg.png\" alt=\"A right triangle\"></span>",
            "<figcaption>A right triangle</figcaption></figure>",
            "<p>The hypotenuse is the longest side.</p>",
            // An inline flag in a table: required, not a media block.
            "<table class=\"wikitable\"><tr><td><span class=\"flagicon\"><img src=\"//upload.wikimedia.org/wikipedia/en/thumb/a/a4/Flag_of_the_United_States.svg/23px-Flag_of_the_United_States.svg.png\" alt=\"United States\"></span> ENIAC</td><td>1945</td></tr></table>",
            // A formula fallback image is not a Wikimedia file.
            "<p>So <span class=\"mwe-math-element\"><img src=\"https://wikimedia.org/api/rest_v1/media/math/render/svg/abc\" class=\"mwe-math-fallback-image-inline\" alt=\"c\"></span>.</p>",
            // Furniture icons stay skipped.
            "<div class=\"sistersitebox\"><img src=\"//upload.wikimedia.org/wikipedia/commons/thumb/9/99/Wiktionary-logo-en-v2.svg/40px-Wiktionary-logo-en-v2.svg.png\" alt=\"Wiktionary logo\"></div>",
        );
        let article = extract_rendered_article(html).unwrap();
        let titles: Vec<&str> = article
            .images
            .iter()
            .map(|image| image.file_title.as_str())
            .collect();
        assert_eq!(
            titles,
            [
                "File:Right triangle.svg",
                "File:Flag of the United States.svg"
            ]
        );
        assert_eq!(
            article.images[0].caption.as_deref(),
            Some("A right triangle")
        );
        let media_blocks = article
            .lead
            .iter()
            .filter(|block| matches!(block.content, BlockContent::Media { .. }))
            .count();
        assert_eq!(media_blocks, 1, "only the figure is a media block");
    }

    #[test]
    fn unsafe_or_empty_input_is_refused() {
        assert_eq!(
            extract_rendered_article("<div><style>p{}</style><div class=\"navbox\">x</div></div>"),
            Err(RenderedArticleError::Empty)
        );
        assert_eq!(
            extract_rendered_article(&"x".repeat(MAX_HTML_BYTES + 1)),
            Err(RenderedArticleError::TooLarge)
        );
        let deep = "<div>".repeat(MAX_DEPTH + 1) + "text";
        assert_eq!(
            extract_rendered_article(&deep),
            Err(RenderedArticleError::TooDeep)
        );
        // File links outside the wiki namespace are not images.
        for href in [
            "https://evil.example/File:A.png",
            "/wiki/File:",
            "/wiki/Talk:A.png",
            "/wiki/File:a/b.png",
        ] {
            assert_eq!(file_title(href), None, "{href}");
        }
        assert_eq!(
            file_title("/wiki/File:Caf%C3%A9_x.jpg").as_deref(),
            Some("File:Café x.jpg")
        );
        // Script text never becomes article text.
        let extracted =
            extract_rendered_article("<p>Safe</p><script>alert('<p>x</p>')</script>").unwrap();
        assert_eq!(all_text(&extracted), "Paragraph(\"Safe\")\n");
    }
}
