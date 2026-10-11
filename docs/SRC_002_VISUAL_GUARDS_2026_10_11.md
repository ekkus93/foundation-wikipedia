# SRC-002 / PACK-003 — Fail-closed unresolved visual markup (partial)

This is partial implementation evidence, not full acceptance or publisher trust.

- Exact qualified implementation `6e309bdd65a3283555d39802f1587dbed3c29a36` passed Rust workspace [38113551191](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38113551191) and Platform shells [38113551112](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38113551112).
- `wiki-source::enterprise_normalize_article` rejects unresolved image, figure, image-map, embedded SVG/math, A/V, stylesheet and other rich visual tags instead of allowing the HTML sanitizer to silently strip required offline content. Regression fixtures cover representative previously missed tags.
- Follow-up exact `4b48ee71b09059586b0f6bd29423f3c5e596b44a` passed Rust workspace [38113851053](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38113851053) and Platform shells [38113851029](https://github.com/ekkus93/foundation-wikipedia/actions/runs/38113851029). Visual tag detection now respects tag-name boundaries, avoiding false positives for unrelated tag prefixes such as `<imgur>` and `<mathematics>`.

**Remaining:** This is a conservative rejection gate, not media discovery, content-addressed visual installation, HTML/CSS/MathML/SVG transformation, or offline readiness. Mandatory image/math/diagram and font/style completeness, attribution, source generation fidelity and real-device airplane-mode acceptance remain unqualified. SRC-002 and PACK-003 stay unchecked. The large canonical TODO was previously rejected by the Ralph Bridge write safety gate, so this note preserves the evidence separately.
