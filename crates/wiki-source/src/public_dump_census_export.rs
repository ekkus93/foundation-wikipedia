//! Fail-closed, bounded-memory observed page-ID census export.
//!
//! Feed only pages returned by a successfully validated PinnedPageIngest.
//! The output must remain in staging until BOTH the upstream member audit and
//! this writer finish successfully. It is not an independent expected census,
//! publisher authentication, XML parser, or proof of source completeness.

use std::io::{self, Write};

use crate::public_dump_page_meta::PublicDumpPage;

const MAX_PAGES: usize = 100_000_000;

#[derive(Debug)]
pub enum CensusExportError {
    InvalidState,
    InvalidNamespace,
    InvalidPageId(u64),
    UnsortedOrDuplicate(u64),
    TooManyPages,
    EmptyCensus,
    Write(io::Error),
}

/// Stream one namespace's observed IDs as strict LF-terminated decimal lines.
///
/// The caller owns staging, verified source decoding, and atomic publication.
/// Once an error occurs, no further writes or successful finish are possible.
pub struct ObservedCensusWriter<W> {
    output: W,
    namespace: i32,
    previous_id: u64,
    count: usize,
    failed: bool,
}

impl<W: Write> ObservedCensusWriter<W> {
    pub fn new(output: W, namespace: i32) -> Result<Self, CensusExportError> {
        if namespace < 0 {
            return Err(CensusExportError::InvalidNamespace);
        }
        Ok(Self {
            output,
            namespace,
            previous_id: 0,
            count: 0,
            failed: false,
        })
    }

    /// Consume the next validated outer page in source order. All namespaces
    /// participate in ordering checks; only the selected namespace is written.
    pub fn push(&mut self, page: &PublicDumpPage) -> Result<(), CensusExportError> {
        if self.failed {
            return Err(CensusExportError::InvalidState);
        }
        if page.namespace < 0 {
            self.failed = true;
            return Err(CensusExportError::InvalidNamespace);
        }
        if page.page_id == 0 || page.page_id > i64::MAX as u64 {
            self.failed = true;
            return Err(CensusExportError::InvalidPageId(page.page_id));
        }
        if page.page_id <= self.previous_id {
            self.failed = true;
            return Err(CensusExportError::UnsortedOrDuplicate(page.page_id));
        }
        self.previous_id = page.page_id;
        if page.namespace == self.namespace {
            if self.count >= MAX_PAGES {
                self.failed = true;
                return Err(CensusExportError::TooManyPages);
            }
            if let Err(error) = writeln!(self.output, "{}", page.page_id) {
                self.failed = true;
                return Err(CensusExportError::Write(error));
            }
            self.count += 1;
        }
        Ok(())
    }

    /// Flush and return the staged bytes only after at least one matching page.
    /// A successful finish does NOT replace independent expected-census audit.
    pub fn finish(mut self) -> Result<(W, usize), CensusExportError> {
        if self.failed {
            return Err(CensusExportError::InvalidState);
        }
        if self.count == 0 {
            return Err(CensusExportError::EmptyCensus);
        }
        self.output.flush().map_err(CensusExportError::Write)?;
        Ok((self.output, self.count))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::public_dump_census_stream::CensusScope;
    use crate::public_dump_census_text::audit_page_coverage_text;
    use std::io::Cursor;

    fn page(id: u64, namespace: i32) -> PublicDumpPage {
        PublicDumpPage {
            page_id: id,
            title: format!("Page {id}"),
            namespace,
            revision_id: id + 100,
            timestamp: "2026-10-14T00:00:00Z".into(),
            redirect_target: None,
        }
    }

    #[test]
    fn observed_namespace_stream_roundtrips_through_independent_auditor() {
        let mut writer = ObservedCensusWriter::new(Vec::new(), 0).unwrap();
        writer.push(&page(1, 0)).unwrap();
        writer.push(&page(2, 1)).unwrap();
        writer.push(&page(3, 0)).unwrap();
        let (bytes, count) = writer.finish().unwrap();
        assert_eq!(count, 2);
        assert_eq!(bytes, b"1\n3\n");
        let scope = CensusScope {
            project: "enwiki",
            generation: "2026-10-14",
            namespace: 0,
        };
        let delta = audit_page_coverage_text(
            scope,
            Cursor::new(b"1\n3\n"),
            scope,
            Cursor::new(bytes),
        )
        .unwrap();
        assert_eq!(delta.expected_count, 2);
        assert_eq!(delta.observed_count, 2);
    }

    #[test]
    fn invalid_or_unsorted_input_poisoned_permanently() {
        for bad in [page(0, 0), page(i64::MAX as u64 + 1, 0), page(2, -1)] {
            let mut writer = ObservedCensusWriter::new(Vec::new(), 0).unwrap();
            assert!(writer.push(&bad).is_err());
            assert!(matches!(writer.push(&page(3, 0)), Err(CensusExportError::InvalidState)));
            assert!(matches!(writer.finish(), Err(CensusExportError::InvalidState)));
        }
        let mut writer = ObservedCensusWriter::new(Vec::new(), 0).unwrap();
        writer.push(&page(2, 1)).unwrap();
        assert!(matches!(writer.push(&page(1, 0)), Err(CensusExportError::UnsortedOrDuplicate(1))));
        assert!(matches!(writer.finish(), Err(CensusExportError::InvalidState)));
        let mut writer = ObservedCensusWriter::new(Vec::new(), 0).unwrap();
        writer.push(&page(1, 0)).unwrap();
        assert!(matches!(writer.push(&page(1, 1)), Err(CensusExportError::UnsortedOrDuplicate(1))));
        assert!(matches!(writer.finish(), Err(CensusExportError::InvalidState)));
    }

    #[test]
    fn empty_namespace_and_invalid_configuration_fail_closed() {
        assert!(matches!(
            ObservedCensusWriter::new(Vec::new(), -1),
            Err(CensusExportError::InvalidNamespace)
        ));
        let mut writer = ObservedCensusWriter::new(Vec::new(), 0).unwrap();
        writer.push(&page(1, 1)).unwrap();
        assert!(matches!(writer.finish(), Err(CensusExportError::EmptyCensus)));
    }

    struct BrokenOutput;
    impl Write for BrokenOutput {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "simulated write failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn write_failure_poisoning_prevents_publication() {
        let mut writer = ObservedCensusWriter::new(BrokenOutput, 0).unwrap();
        assert!(matches!(writer.push(&page(1, 0)), Err(CensusExportError::Write(_))));
        assert!(matches!(writer.push(&page(2, 0)), Err(CensusExportError::InvalidState)));
        assert!(matches!(writer.finish(), Err(CensusExportError::InvalidState)));
    }
}
