//! Preflight the combined HTML and caller-supplied CSS resource graph.
//!
//! This discovers *candidates*, not verified offline objects. In particular,
//! source authenticity, media bytes, licensing, CSS sanitization, dynamically
//! generated resources, and local URL rewriting remain separate requirements.
//! Neither this result nor a matching candidate count certifies offline readiness.

use std::collections::{BTreeMap, BTreeSet};

use crate::visual_discovery::{
    inventory, inventory_supplied_css_graph, Candidate, CandidateKind, DiscoveryError,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderDependencyError {
    Discovery(DiscoveryError),
    CombinedReferenceBudget,
}

impl From<DiscoveryError> for RenderDependencyError {
    fn from(error: DiscoveryError) -> Self {
        Self::Discovery(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderDependencyGraph {
    /// Sorted and deduplicated by (candidate kind, absolute URL).
    pub candidates: Vec<Candidate>,
    pub html_references: usize,
    pub css_references: usize,
    pub stylesheets_inspected: usize,
    pub unresolved_dynamic_surfaces: usize,
    pub unsupported_schemes: usize,
    pub missing_sources: usize,
    /// Every referenced external stylesheet and CSS import had supplied text.
    /// This says nothing about its source authenticity or byte integrity.
    pub all_referenced_css_supplied: bool,
    /// Always false until CSS/font objects and safe rendering are verified.
    pub css_and_font_dependencies_verified: bool,
    /// Always false until every mandatory object and its attribution is verified.
    pub required_media_verified: bool,
}

/// Combine HTML references (including inline CSS) with the transitive graph of
/// external CSS sources supplied by the caller. A missing stylesheet or import
/// fails closed. No network access or file reads occur.
///
/// `max_references` bounds the *combined* number of raw HTML/CSS references,
/// not merely the unique URL count; duplicates still consume the budget.
/// The result is a necessary discovery input to PACK-003, never an install
/// certificate or an assertion that a Wikimedia snapshot is complete.
pub fn inventory_rendered_dependencies(
    markup: &str,
    document_url: &str,
    stylesheets: &BTreeMap<String, String>,
    max_references: usize,
) -> Result<RenderDependencyGraph, RenderDependencyError> {
    let html = inventory(markup, document_url, max_references)?;
    let roots: Vec<String> = html
        .candidates
        .iter()
        .filter(|candidate| {
            matches!(
                candidate.kind,
                CandidateKind::Stylesheet | CandidateKind::PreloadStyle | CandidateKind::CssImport
            )
        })
        .map(|candidate| candidate.url.clone())
        .collect();
    let css = if roots.is_empty() {
        None
    } else {
        Some(inventory_supplied_css_graph(
            &roots,
            stylesheets,
            max_references,
        )?)
    };
    let css_references = css.as_ref().map_or(0, |graph| graph.observed_references);
    let total_references = html
        .observed_references
        .checked_add(css_references)
        .ok_or(RenderDependencyError::CombinedReferenceBudget)?;
    if total_references > max_references {
        return Err(RenderDependencyError::CombinedReferenceBudget);
    }
    let mut candidates: BTreeSet<Candidate> = html.candidates.into_iter().collect();
    let stylesheets_inspected = if let Some(css) = css {
        let count = css.stylesheets_inspected;
        candidates.extend(css.candidates);
        count
    } else {
        0
    };
    if candidates.len() > max_references {
        return Err(RenderDependencyError::CombinedReferenceBudget);
    }
    Ok(RenderDependencyGraph {
        candidates: candidates.into_iter().collect(),
        html_references: html.observed_references,
        css_references,
        stylesheets_inspected,
        unresolved_dynamic_surfaces: html.unresolved_dynamic_surfaces,
        unsupported_schemes: html.unsupported_schemes,
        missing_sources: html.missing_sources,
        all_referenced_css_supplied: true,
        css_and_font_dependencies_verified: false,
        required_media_verified: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "https://en.wikipedia.org/wiki/Physics";

    #[test]
    fn follows_external_and_inline_imports_to_fonts_and_images() {
        let stylesheets = BTreeMap::from([
            (
                "https://en.wikipedia.org/styles/main.css".into(),
                "@import \"nested.css\"; .x{background:url(../img/bg.png)}".into(),
            ),
            (
                "https://en.wikipedia.org/styles/nested.css".into(),
                "@font-face{src:url(../fonts/math.woff2)}".into(),
            ),
            (
                "https://en.wikipedia.org/styles/inline.css".into(),
                ".y{background:url(../img/inline.png)}".into(),
            ),
        ]);
        let html = concat!(
            "<link rel=\"stylesheet\" href=\"/styles/main.css\">",
            "<style>@import \"/styles/inline.css\";",
            ".a{background:url(/img/direct.png)}</style>",
            "<img src=\"/img/a.png\">"
        );
        let graph = inventory_rendered_dependencies(html, DOC, &stylesheets, 20).unwrap();
        assert_eq!(graph.stylesheets_inspected, 3);
        assert_eq!(graph.html_references, 4);
        assert_eq!(graph.css_references, 4);
        let urls: BTreeSet<_> = graph.candidates.iter().map(|c| c.url.as_str()).collect();
        for url in [
            "https://en.wikipedia.org/img/a.png",
            "https://en.wikipedia.org/img/bg.png",
            "https://en.wikipedia.org/img/inline.png",
            "https://en.wikipedia.org/img/direct.png",
            "https://en.wikipedia.org/fonts/math.woff2",
            "https://en.wikipedia.org/styles/main.css",
            "https://en.wikipedia.org/styles/inline.css",
            "https://en.wikipedia.org/styles/nested.css",
        ] {
            assert!(urls.contains(url), "missing {url}");
        }
        assert!(graph.all_referenced_css_supplied);
        assert!(!graph.css_and_font_dependencies_verified);
        assert!(!graph.required_media_verified);
    }

    #[test]
    fn missing_stylesheet_or_transitive_import_fails_closed() {
        let html = "<link rel=\"stylesheet\" href=\"/styles/main.css\">";
        assert_eq!(
            inventory_rendered_dependencies(html, DOC, &BTreeMap::new(), 20),
            Err(RenderDependencyError::Discovery(
                DiscoveryError::MissingSuppliedStylesheet
            ))
        );
        let stylesheets = BTreeMap::from([(
            "https://en.wikipedia.org/styles/main.css".into(),
            "@import \"missing.css\";".into(),
        )]);
        assert_eq!(
            inventory_rendered_dependencies(html, DOC, &stylesheets, 20),
            Err(RenderDependencyError::Discovery(
                DiscoveryError::MissingSuppliedStylesheet
            ))
        );
    }

    #[test]
    fn enforces_aggregate_reference_budget_not_only_unique_urls() {
        let html = "<img src=\"/a.png\"><link rel=\"stylesheet\" href=\"/s.css\">";
        let css = BTreeMap::from([(
            "https://en.wikipedia.org/s.css".into(),
            ".x{background:url(/a.png)}".into(),
        )]);
        assert_eq!(
            inventory_rendered_dependencies(html, DOC, &css, 2),
            Err(RenderDependencyError::CombinedReferenceBudget)
        );
    }

    #[test]
    fn no_stylesheets_is_valid_but_dynamic_surfaces_remain_unverified() {
        let graph = inventory_rendered_dependencies(
            "<img src=\"/a.png\"><canvas></canvas>",
            DOC,
            &BTreeMap::new(),
            10,
        )
        .unwrap();
        assert_eq!(graph.stylesheets_inspected, 0);
        assert_eq!(graph.css_references, 0);
        assert_eq!(graph.unresolved_dynamic_surfaces, 1);
        assert!(!graph.required_media_verified);
    }
}
