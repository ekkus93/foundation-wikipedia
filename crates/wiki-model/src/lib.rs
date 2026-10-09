//! Versioned, platform-neutral Wikipedia article domain model.
//!
//! This is a **partial** implementation of MOD-001: stable typed data and
//! validation. Upstream adapters and serialized record formats remain pending.

use std::collections::HashSet;
use std::error::Error;
use std::fmt;

pub const ARTICLE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArticleKey {
    pub project: String,
    pub page_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    pub revision_id: u64,
    pub timestamp: String,
    /// Hex-encoded SHA-256 of canonical content bytes.
    pub content_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Article {
    pub schema_version: u32,
    pub key: ArticleKey,
    pub revision: Revision,
    pub title: String,
    pub display_title: String,
    pub language: String,
    pub wikidata_id: Option<String>,
    pub lead: Vec<Block>,
    pub sections: Vec<Section>,
    pub references: Vec<Reference>,
    pub links: Vec<ArticleLink>,
    pub media: Vec<MediaAsset>,
    /// Sanitization is a renderer/security concern; never evaluate as script.
    pub rendered_html: String,
    pub is_disambiguation: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageRecord {
    Article(Box<Article>),
    Redirect(Redirect),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Redirect {
    pub from: ArticleKey,
    pub title: String,
    pub to: ArticleKey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub ordinal: u32,
    pub heading: String,
    pub blocks: Vec<Block>,
    pub subsections: Vec<Section>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub ordinal: u32,
    pub content: BlockContent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockContent {
    Paragraph(String),
    List(Vec<String>),
    Table(Vec<Vec<String>>),
    Quote(String),
    Math { source: String, html: String },
    Media { media_index: usize },
    Infobox(Vec<(String, String)>),
    HtmlFallback(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub id: String,
    pub label: String,
    pub source_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArticleLink {
    pub label: String,
    pub target: ArticleKey,
    pub fragment: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaAsset {
    pub source_url: String,
    pub mime_type: String,
    pub sha256: Option<String>,
    pub license: String,
    pub creator: String,
    pub attribution: String,
    /// Audio/video are online-on-demand; the offline asset is its preview.
    pub is_av_preview: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelError {
    InvalidIdentity,
    InvalidRevision,
    UnsupportedSchema(u32),
    InvalidHash,
    MissingTitle,
    InvalidSectionHeading,
    InvalidReferenceLabel,
    DuplicateOrdinal,
    InvalidMediaIndex(usize),
    InvalidMediaMetadata(usize),
    DuplicateReference(String),
    InvalidRedirect,
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for ModelError {}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl ArticleKey {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.page_id == 0
            || self.project.is_empty()
            || !self
                .project
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(ModelError::InvalidIdentity);
        }
        Ok(())
    }

    /// Revision IDs and block ordinals are included to make citations
    /// unequivocally revision-scoped rather than transferable bookmarks.
    pub fn block_id(&self, revision_id: u64, section_path: &[u32], ordinal: u32) -> String {
        let mut id = format!("wkb:{}:{}:{}", self.project, self.page_id, revision_id);
        for section in section_path {
            id.push(':');
            id.push_str(&section.to_string());
        }
        id.push_str(&format!(":b{ordinal}"));
        id
    }

    /// Stable hard citation for one Wikipedia reference in one exact revision.
    /// Hex encoding avoids collisions between Unicode labels and delimiters.
    /// This is not a relocatable bookmark or an external-source endorsement.
    pub fn reference_id(&self, revision_id: u64, source_ref_id: &str) -> String {
        let mut id = format!("wkr:{}:{}:{}:", self.project, self.page_id, revision_id);
        for byte in source_ref_id.bytes() {
            use std::fmt::Write;
            write!(&mut id, "{byte:02x}").expect("writing into String cannot fail");
        }
        id
    }
}

impl Redirect {
    /// Redirects must preserve distinct, valid page identities and a title.
    pub fn validate(&self) -> Result<(), ModelError> {
        self.from.validate()?;
        self.to.validate()?;
        // Cross-project navigation is an interwiki link, not a redirect record.
        if self.title.trim().is_empty()
            || self.from == self.to
            || self.from.project != self.to.project
        {
            return Err(ModelError::InvalidRedirect);
        }
        Ok(())
    }
}

impl PageRecord {
    pub fn validate(&self) -> Result<(), ModelError> {
        match self {
            Self::Article(article) => article.validate(),
            Self::Redirect(redirect) => redirect.validate(),
        }
    }
}

impl Article {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.schema_version != ARTICLE_SCHEMA_VERSION {
            return Err(ModelError::UnsupportedSchema(self.schema_version));
        }
        self.key.validate()?;
        if self.revision.revision_id == 0 || self.revision.timestamp.trim().is_empty() {
            return Err(ModelError::InvalidRevision);
        }
        if !valid_hash(&self.revision.content_sha256) {
            return Err(ModelError::InvalidHash);
        }
        if self.title.trim().is_empty()
            || self.display_title.trim().is_empty()
            || self.language.trim().is_empty()
        {
            return Err(ModelError::MissingTitle);
        }
        for link in &self.links {
            link.target.validate()?;
        }
        let mut refs = HashSet::new();
        for reference in &self.references {
            if reference.label.trim().is_empty() {
                return Err(ModelError::InvalidReferenceLabel);
            }
            if reference.id.is_empty() || !refs.insert(&reference.id) {
                return Err(ModelError::DuplicateReference(reference.id.clone()));
            }
        }
        for (index, media) in self.media.iter().enumerate() {
            if media.source_url.trim().is_empty()
                || media.mime_type.trim().is_empty()
                || media.license.trim().is_empty()
                || media.attribution.trim().is_empty()
            {
                return Err(ModelError::InvalidMediaMetadata(index));
            }
            if let Some(hash) = &media.sha256 {
                if !valid_hash(hash) {
                    return Err(ModelError::InvalidHash);
                }
            }
        }
        check_blocks(&self.lead, self.media.len())?;
        check_sections(&self.sections, self.media.len())?;
        Ok(())
    }
}

fn check_blocks(blocks: &[Block], media_len: usize) -> Result<(), ModelError> {
    let mut ordinals = HashSet::new();
    for block in blocks {
        if !ordinals.insert(block.ordinal) {
            return Err(ModelError::DuplicateOrdinal);
        }
        if let BlockContent::Media { media_index } = &block.content {
            if *media_index >= media_len {
                return Err(ModelError::InvalidMediaIndex(*media_index));
            }
        }
    }
    Ok(())
}

fn check_sections(sections: &[Section], media_len: usize) -> Result<(), ModelError> {
    let mut ordinals = HashSet::new();
    for section in sections {
        if section.heading.trim().is_empty() {
            return Err(ModelError::InvalidSectionHeading);
        }
        if !ordinals.insert(section.ordinal) {
            return Err(ModelError::DuplicateOrdinal);
        }
        check_blocks(&section.blocks, media_len)?;
        check_sections(&section.subsections, media_len)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn article() -> Article {
        Article {
            schema_version: ARTICLE_SCHEMA_VERSION,
            key: ArticleKey {
                project: "enwiki".into(),
                page_id: 42,
            },
            revision: Revision {
                revision_id: 7,
                timestamp: "2026-10-09T00:00:00Z".into(),
                content_sha256: "a".repeat(64),
            },
            title: "Gravity".into(),
            display_title: "Gravity".into(),
            language: "en".into(),
            wikidata_id: Some("Q1140".into()),
            lead: vec![Block {
                ordinal: 0,
                content: BlockContent::Paragraph("Gravité 🌍".into()),
            }],
            sections: vec![Section {
                ordinal: 1,
                heading: "Physics".into(),
                blocks: vec![Block {
                    ordinal: 0,
                    content: BlockContent::Math {
                        source: "F = ma".into(),
                        html: "<math>F = ma</math>".into(),
                    },
                }],
                subsections: vec![],
            }],
            references: vec![Reference {
                id: "ref1".into(),
                label: "Source".into(),
                source_url: None,
            }],
            links: vec![],
            media: vec![],
            rendered_html: "<article>Gravité 🌍</article>".into(),
            is_disambiguation: false,
        }
    }

    #[test]
    fn accepts_unicode_and_sections() {
        assert!(article().validate().is_ok());
    }

    #[test]
    fn block_ids_change_with_revision() {
        let key = article().key;
        let a = key.block_id(7, &[2, 3], 0);
        assert_ne!(a, key.block_id(8, &[2, 3], 0));
        assert_ne!(a, key.block_id(7, &[2, 4], 0));
        assert_eq!(a, "wkb:enwiki:42:7:2:3:b0");
    }

    #[test]
    fn reference_ids_are_revision_scoped_and_unambiguous() {
        let key = article().key;
        let original = key.reference_id(7, "cite:é");
        assert!(original.starts_with("wkr:enwiki:42:7:"));
        assert_ne!(original, key.reference_id(8, "cite:é"));
        assert_ne!(original, key.reference_id(7, "cite-é"));
        assert_ne!(original, key.reference_id(7, "cite:É"));
        assert_ne!(original, key.block_id(7, &[], 0));
        assert_eq!(original, key.reference_id(7, "cite:é"));
    }

    #[test]
    fn duplicate_ordinals_are_invalid() {
        let mut a = article();
        a.lead.push(a.lead[0].clone());
        assert_eq!(a.validate(), Err(ModelError::DuplicateOrdinal));
    }

    #[test]
    fn invalid_revision_and_media_refs_are_rejected() {
        let mut a = article();
        a.revision.content_sha256 = "not a sha".into();
        assert_eq!(a.validate(), Err(ModelError::InvalidHash));
        a.revision.content_sha256 = "a".repeat(64);
        a.lead[0].content = BlockContent::Media { media_index: 9 };
        assert_eq!(a.validate(), Err(ModelError::InvalidMediaIndex(9)));
    }

    #[test]
    fn media_requires_source_license_and_attribution() {
        let mut a = article();
        a.media.push(MediaAsset {
            source_url: "https://upload.wikimedia.org/wikipedia/commons/example.svg".into(),
            mime_type: "image/svg+xml".into(),
            sha256: Some("b".repeat(64)),
            license: "CC BY-SA 4.0".into(),
            creator: "Example contributor".into(),
            attribution: "Example contributor, CC BY-SA 4.0".into(),
            is_av_preview: false,
        });
        a.lead[0].content = BlockContent::Media { media_index: 0 };
        assert_eq!(a.validate(), Ok(()));
        a.media[0].license = " ".into();
        assert_eq!(a.validate(), Err(ModelError::InvalidMediaMetadata(0)));
        a.media[0].license = "CC BY-SA 4.0".into();
        a.media[0].source_url.clear();
        assert_eq!(a.validate(), Err(ModelError::InvalidMediaMetadata(0)));
        a.media[0].source_url = "https://upload.wikimedia.org/example.svg".into();
        a.media[0].attribution.clear();
        assert_eq!(a.validate(), Err(ModelError::InvalidMediaMetadata(0)));
    }

    #[test]
    fn rejects_blank_reference_labels_and_nested_headings() {
        let mut a = article();
        a.references[0].label = "  ".into();
        assert_eq!(a.validate(), Err(ModelError::InvalidReferenceLabel));
        a.references[0].label = "Source".into();
        a.sections[0].heading = "\t".into();
        assert_eq!(a.validate(), Err(ModelError::InvalidSectionHeading));
        a.sections[0].heading = "Physics".into();
        a.sections[0].subsections.push(Section {
            ordinal: 2,
            heading: "  ".into(),
            blocks: vec![],
            subsections: vec![],
        });
        assert_eq!(a.validate(), Err(ModelError::InvalidSectionHeading));
    }

    #[test]
    fn rejects_blank_revision_and_display_title() {
        let mut a = article();
        a.revision.timestamp = " ".into();
        assert_eq!(a.validate(), Err(ModelError::InvalidRevision));
        a.revision.timestamp = "2026-10-09T00:00:00Z".into();
        a.display_title.clear();
        assert_eq!(a.validate(), Err(ModelError::MissingTitle));
    }

    #[test]
    fn article_identity_must_be_unambiguous() {
        let mut a = article();
        a.key.project = "en:wiki".into();
        assert_eq!(a.validate(), Err(ModelError::InvalidIdentity));
        a.key.project = "enwiki".into();
        a.key.page_id = 0;
        assert_eq!(a.validate(), Err(ModelError::InvalidIdentity));
    }
}

#[cfg(test)]
mod record_contract_tests {
    use super::*;

    fn key(page_id: u64) -> ArticleKey {
        ArticleKey {
            project: "enwiki".into(),
            page_id,
        }
    }

    #[test]
    fn redirect_record_rejects_self_redirect_and_missing_title() {
        let mut redirect = Redirect {
            from: key(10),
            title: "Old title".into(),
            to: key(11),
        };
        assert_eq!(PageRecord::Redirect(redirect.clone()).validate(), Ok(()));
        redirect.to = key(10);
        assert_eq!(redirect.validate(), Err(ModelError::InvalidRedirect));
        redirect.to = key(11);
        redirect.title = " ".into();
        assert_eq!(redirect.validate(), Err(ModelError::InvalidRedirect));
    }

    #[test]
    fn redirect_record_rejects_cross_project_targets() {
        let redirect = Redirect {
            from: key(10),
            title: "Old title".into(),
            to: ArticleKey {
                project: "frwiki".into(),
                page_id: 11,
            },
        };
        assert_eq!(redirect.validate(), Err(ModelError::InvalidRedirect));
    }

    #[test]
    fn redirect_record_rejects_invalid_target_identity() {
        let redirect = Redirect {
            from: key(10),
            title: "Old title".into(),
            to: key(0),
        };
        assert_eq!(redirect.validate(), Err(ModelError::InvalidIdentity));
    }
}
