//! Bounded revision and page-identity reconstruction from a validated
//! streaming XML parser's semantic events.
//!
//! Not an XML tokenizer/decompressor. A caller MUST only emit these events
//! from structurally correct MediaWiki XML, distinguishing `page/id` from
//! `revision/id` and ignoring contributor IDs. Do not use this collector to
//! claim authenticated snapshots, rendered text, or complete offline media.

const MAX_TITLE_BYTES: usize = 4096;
const MAX_TIMESTAMP_BYTES: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Outside,
    Page,
    Revision,
    AfterRevision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageEvent<'a> {
    BeginPage,
    Title(&'a str),
    Namespace(i32),
    PageId(u64),
    RedirectTarget(&'a str),
    BeginRevision,
    RevisionId(u64),
    Timestamp(&'a str),
    EndRevision,
    EndPage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicDumpPage {
    pub page_id: u64,
    pub title: String,
    pub namespace: i32,
    pub revision_id: u64,
    pub timestamp: String,
    pub redirect_target: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageMetadataError {
    InvalidState,
    DuplicateField,
    MissingField,
    InvalidField,
}

pub struct PageMetadataCollector {
    state: State,
    failed: bool,
    title: Option<String>,
    namespace: Option<i32>,
    page_id: Option<u64>,
    redirect: Option<String>,
    revision_id: Option<u64>,
    timestamp: Option<String>,
}

impl Default for PageMetadataCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl PageMetadataCollector {
    pub fn new() -> Self {
        Self {
            state: State::Outside,
            failed: false,
            title: None,
            namespace: None,
            page_id: None,
            redirect: None,
            revision_id: None,
            timestamp: None,
        }
    }

    fn invalid(
        &mut self,
        error: PageMetadataError,
    ) -> Result<Option<PublicDumpPage>, PageMetadataError> {
        self.failed = true;
        Err(error)
    }

    /// Exactly one revision per current-content page, including redirects.
    /// Any rejected event permanently invalidates this collector instance.
    pub fn push(
        &mut self,
        event: PageEvent<'_>,
    ) -> Result<Option<PublicDumpPage>, PageMetadataError> {
        if self.failed {
            return Err(PageMetadataError::InvalidState);
        }
        match (self.state, event) {
            (State::Outside, PageEvent::BeginPage) => {
                self.title = None;
                self.namespace = None;
                self.page_id = None;
                self.redirect = None;
                self.revision_id = None;
                self.timestamp = None;
                self.state = State::Page;
            }
            (State::Page, PageEvent::Title(value)) => {
                if self.title.is_some() {
                    return self.invalid(PageMetadataError::DuplicateField);
                }
                if !valid_text(value, MAX_TITLE_BYTES) {
                    return self.invalid(PageMetadataError::InvalidField);
                }
                self.title = Some(value.to_owned());
            }
            (State::Page, PageEvent::Namespace(value)) => {
                if self.namespace.is_some() {
                    return self.invalid(PageMetadataError::DuplicateField);
                }
                if value < 0 {
                    return self.invalid(PageMetadataError::InvalidField);
                }
                self.namespace = Some(value);
            }
            (State::Page, PageEvent::PageId(value)) => {
                if self.page_id.is_some() {
                    return self.invalid(PageMetadataError::DuplicateField);
                }
                if value == 0 || value > i64::MAX as u64 {
                    return self.invalid(PageMetadataError::InvalidField);
                }
                self.page_id = Some(value);
            }
            (State::Page, PageEvent::RedirectTarget(value)) => {
                if self.redirect.is_some() {
                    return self.invalid(PageMetadataError::DuplicateField);
                }
                if !valid_text(value, MAX_TITLE_BYTES) {
                    return self.invalid(PageMetadataError::InvalidField);
                }
                self.redirect = Some(value.to_owned());
            }
            (State::Page, PageEvent::BeginRevision) => {
                if self.page_id.is_none() || self.title.is_none() || self.namespace.is_none() {
                    return self.invalid(PageMetadataError::MissingField);
                }
                self.state = State::Revision;
            }
            (State::Revision, PageEvent::RevisionId(value)) => {
                if self.revision_id.is_some() {
                    return self.invalid(PageMetadataError::DuplicateField);
                }
                if value == 0 || value > i64::MAX as u64 {
                    return self.invalid(PageMetadataError::InvalidField);
                }
                self.revision_id = Some(value);
            }
            (State::Revision, PageEvent::Timestamp(value)) => {
                if self.timestamp.is_some() {
                    return self.invalid(PageMetadataError::DuplicateField);
                }
                if value.len() > MAX_TIMESTAMP_BYTES
                    || !crate::online_article::valid_utc_timestamp(value)
                {
                    return self.invalid(PageMetadataError::InvalidField);
                }
                self.timestamp = Some(value.to_owned());
            }
            (State::Revision, PageEvent::EndRevision) => {
                if self.revision_id.is_none() || self.timestamp.is_none() {
                    return self.invalid(PageMetadataError::MissingField);
                }
                self.state = State::AfterRevision;
            }
            (State::AfterRevision, PageEvent::EndPage) => {
                let result = PublicDumpPage {
                    page_id: self.page_id.take().ok_or(PageMetadataError::MissingField)?,
                    title: self.title.take().ok_or(PageMetadataError::MissingField)?,
                    namespace: self
                        .namespace
                        .take()
                        .ok_or(PageMetadataError::MissingField)?,
                    revision_id: self
                        .revision_id
                        .take()
                        .ok_or(PageMetadataError::MissingField)?,
                    timestamp: self
                        .timestamp
                        .take()
                        .ok_or(PageMetadataError::MissingField)?,
                    redirect_target: self.redirect.take(),
                };
                self.state = State::Outside;
                return Ok(Some(result));
            }
            _ => return self.invalid(PageMetadataError::InvalidState),
        }
        Ok(None)
    }

    /// Called at EOF of a decoder's well-formed XML document.
    pub fn finish(self) -> Result<(), PageMetadataError> {
        if self.failed || self.state != State::Outside {
            Err(PageMetadataError::InvalidState)
        } else {
            Ok(())
        }
    }
}

fn valid_text(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty()
        && value == value.trim()
        && value.len() <= max_bytes
        && !value.chars().any(|ch| {
            let code = ch as u32;
            ch.is_control()
                || (0xfdd0..=0xfdef).contains(&code)
                || code & 0xffff == 0xfffe
                || code & 0xffff == 0xffff
                || matches!(
                    ch,
                    '\u{061c}'
                        | '\u{200b}'..='\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2060}'
                        | '\u{2066}'..='\u{206f}'
                        | '\u{feff}'
                )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_page(collector: &mut PageMetadataCollector) -> PublicDumpPage {
        assert_eq!(collector.push(PageEvent::BeginPage), Ok(None));
        assert_eq!(collector.push(PageEvent::Title("München")), Ok(None));
        assert_eq!(collector.push(PageEvent::Namespace(0)), Ok(None));
        assert_eq!(collector.push(PageEvent::PageId(42)), Ok(None));
        assert_eq!(collector.push(PageEvent::BeginRevision), Ok(None));
        assert_eq!(collector.push(PageEvent::RevisionId(333)), Ok(None));
        assert_eq!(
            collector.push(PageEvent::Timestamp("2026-10-13T12:00:00Z")),
            Ok(None)
        );
        assert_eq!(collector.push(PageEvent::EndRevision), Ok(None));
        collector.push(PageEvent::EndPage).unwrap().unwrap()
    }

    #[test]
    fn reconstructs_unicode_and_exact_revision_without_confusing_ids() {
        let mut collector = PageMetadataCollector::new();
        let page = valid_page(&mut collector);
        assert_eq!(page.page_id, 42);
        assert_eq!(page.revision_id, 333);
        assert_eq!(page.title, "München");
        assert_eq!(page.timestamp, "2026-10-13T12:00:00Z");
        assert_eq!(page.redirect_target, None);
        assert_eq!(collector.push(PageEvent::BeginPage), Ok(None));
        assert_eq!(collector.push(PageEvent::Title("Gravity")), Ok(None));
        assert_eq!(collector.push(PageEvent::Namespace(0)), Ok(None));
        assert_eq!(collector.push(PageEvent::PageId(43)), Ok(None));
        assert_eq!(
            collector.push(PageEvent::RedirectTarget("Physics")),
            Ok(None)
        );
        assert_eq!(collector.push(PageEvent::BeginRevision), Ok(None));
        assert_eq!(collector.push(PageEvent::RevisionId(334)), Ok(None));
        assert_eq!(
            collector.push(PageEvent::Timestamp("2026-10-13T12:01:00Z")),
            Ok(None)
        );
        assert_eq!(collector.push(PageEvent::EndRevision), Ok(None));
        assert_eq!(
            collector
                .push(PageEvent::EndPage)
                .unwrap()
                .unwrap()
                .redirect_target,
            Some("Physics".to_owned())
        );
        assert_eq!(collector.finish(), Ok(()));
    }

    #[test]
    fn rejects_partial_page_and_duplicate_outer_id_permanently() {
        let mut collector = PageMetadataCollector::new();
        collector.push(PageEvent::BeginPage).unwrap();
        collector.push(PageEvent::Title("Physics")).unwrap();
        collector.push(PageEvent::Namespace(0)).unwrap();
        collector.push(PageEvent::PageId(42)).unwrap();
        assert_eq!(
            collector.push(PageEvent::PageId(43)),
            Err(PageMetadataError::DuplicateField)
        );
        assert_eq!(
            collector.push(PageEvent::BeginRevision),
            Err(PageMetadataError::InvalidState)
        );
        assert_eq!(collector.finish(), Err(PageMetadataError::InvalidState));

        let mut collector = PageMetadataCollector::new();
        collector.push(PageEvent::BeginPage).unwrap();
        assert_eq!(collector.finish(), Err(PageMetadataError::InvalidState));
    }

    #[test]
    fn rejects_revision_id_instead_of_page_id_and_missing_revision() {
        let mut collector = PageMetadataCollector::new();
        collector.push(PageEvent::BeginPage).unwrap();
        collector.push(PageEvent::Title("Physics")).unwrap();
        collector.push(PageEvent::Namespace(0)).unwrap();
        assert_eq!(
            collector.push(PageEvent::BeginRevision),
            Err(PageMetadataError::MissingField)
        );
        assert_eq!(collector.finish(), Err(PageMetadataError::InvalidState));

        let mut collector = PageMetadataCollector::new();
        collector.push(PageEvent::BeginPage).unwrap();
        collector.push(PageEvent::Title("Physics")).unwrap();
        collector.push(PageEvent::Namespace(0)).unwrap();
        collector.push(PageEvent::PageId(42)).unwrap();
        collector.push(PageEvent::BeginRevision).unwrap();
        assert_eq!(
            collector.push(PageEvent::EndRevision),
            Err(PageMetadataError::MissingField)
        );
    }

    #[test]
    fn rejects_oversized_untrusted_fields_and_invalid_event_order() {
        for event in [
            PageEvent::Title(""),
            PageEvent::Title(" spaced "),
            PageEvent::Title("bad\ncontrol"),
            PageEvent::Title("\u{FFFE}"),
            PageEvent::RedirectTarget("\u{FFFF}"),
            PageEvent::Title(&"X".repeat(MAX_TITLE_BYTES + 1)),
            PageEvent::PageId(0),
            PageEvent::Namespace(-1),
            PageEvent::RedirectTarget(""),
        ] {
            let mut collector = PageMetadataCollector::new();
            collector.push(PageEvent::BeginPage).unwrap();
            assert_eq!(collector.push(event), Err(PageMetadataError::InvalidField));
            assert_eq!(collector.finish(), Err(PageMetadataError::InvalidState));
        }

        let mut collector = PageMetadataCollector::new();
        assert_eq!(
            collector.push(PageEvent::RevisionId(1)),
            Err(PageMetadataError::InvalidState)
        );
        assert_eq!(collector.finish(), Err(PageMetadataError::InvalidState));
    }

    #[test]
    fn rejects_noncanonical_or_impossible_revision_timestamps() {
        for timestamp in [
            "2026-02-29T12:00:00Z",
            "2026-10-13T24:00:00Z",
            "2026-10-13T12:60:00Z",
            "2026-10-13",
            "2026-10-13T12:00:00+00:00",
        ] {
            let mut collector = PageMetadataCollector::new();
            collector.push(PageEvent::BeginPage).unwrap();
            collector.push(PageEvent::Title("Example")).unwrap();
            collector.push(PageEvent::Namespace(0)).unwrap();
            collector.push(PageEvent::PageId(1)).unwrap();
            collector.push(PageEvent::BeginRevision).unwrap();
            collector.push(PageEvent::RevisionId(2)).unwrap();
            assert_eq!(
                collector.push(PageEvent::Timestamp(timestamp)),
                Err(PageMetadataError::InvalidField)
            );
            assert_eq!(collector.finish(), Err(PageMetadataError::InvalidState));
        }
    }

    #[test]
    fn rejects_second_revision_of_a_current_content_page() {
        let mut collector = PageMetadataCollector::new();
        collector.push(PageEvent::BeginPage).unwrap();
        collector.push(PageEvent::Title("Physics")).unwrap();
        collector.push(PageEvent::Namespace(0)).unwrap();
        collector.push(PageEvent::PageId(42)).unwrap();
        collector.push(PageEvent::BeginRevision).unwrap();
        collector.push(PageEvent::RevisionId(100)).unwrap();
        collector
            .push(PageEvent::Timestamp("2026-10-13T00:00:00Z"))
            .unwrap();
        collector.push(PageEvent::EndRevision).unwrap();
        assert_eq!(
            collector.push(PageEvent::BeginRevision),
            Err(PageMetadataError::InvalidState)
        );
        assert_eq!(collector.finish(), Err(PageMetadataError::InvalidState));
    }

    #[test]
    fn rejects_zero_width_spoofing_and_unicode_noncharacters_in_dump_labels() {
        for ch in [
            '\u{200b}',
            '\u{200c}',
            '\u{200d}',
            '\u{2060}',
            '\u{206a}',
            '\u{feff}',
            '\u{fdd0}',
            '\u{fdef}',
            '\u{1fffe}',
            '\u{10ffff}',
        ] {
            let spoofed = format!("Physics{ch}history");
            for redirect in [false, true] {
                let mut collector = PageMetadataCollector::new();
                collector.push(PageEvent::BeginPage).unwrap();
                let event = if redirect {
                    PageEvent::RedirectTarget(&spoofed)
                } else {
                    PageEvent::Title(&spoofed)
                };
                assert_eq!(collector.push(event), Err(PageMetadataError::InvalidField));
                assert_eq!(collector.finish(), Err(PageMetadataError::InvalidState));
            }
        }
        // Ordinary Unicode scripts must still work for international dumps.
        let mut collector = PageMetadataCollector::new();
        collector.push(PageEvent::BeginPage).unwrap();
        assert_eq!(collector.push(PageEvent::Title("رياضيات 日本語")), Ok(None));
        assert_eq!(
            collector.push(PageEvent::RedirectTarget("פיזיקה")),
            Ok(None)
        );
    }

    #[test]
    fn rejects_directional_format_controls_in_dump_titles_and_redirects() {
        for ch in [
            '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}',
            '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        ] {
            let spoofed = format!("Physics{ch}History");
            for redirect in [false, true] {
                let mut collector = PageMetadataCollector::new();
                collector.push(PageEvent::BeginPage).unwrap();
                let event = if redirect {
                    PageEvent::RedirectTarget(&spoofed)
                } else {
                    PageEvent::Title(&spoofed)
                };
                assert_eq!(collector.push(event), Err(PageMetadataError::InvalidField));
                assert_eq!(collector.finish(), Err(PageMetadataError::InvalidState));
            }
        }

        let mut collector = PageMetadataCollector::new();
        collector.push(PageEvent::BeginPage).unwrap();
        assert_eq!(collector.push(PageEvent::Title("رياضيات")), Ok(None));
        assert_eq!(collector.push(PageEvent::RedirectTarget("עברית")), Ok(None));
    }
}
