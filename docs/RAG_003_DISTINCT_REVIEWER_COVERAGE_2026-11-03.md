# RAG-003 — Two distinct valid reviewers required for qualification coverage

**Date:** 2026-11-03  
**Exact implementation commit:** `44131223d18a92419057a80c1584c4ec2227fd91`  
**Status:** Partial RAG-003 infrastructure only.

## Qualification gap

The preregistered human review protocol in `docs/RAG_003_INDEPENDENT_QUALIFICATION_PROTOCOL_2026-11-03.md` requires **at least two independent reviewers per sampled answer**. Previously, `scripts/validate_rag003_review_manifest.py` counted a case and its article toward project coverage as soon as it found **one** valid review. A duplicated reviewer judgment was rejected, but one unique reviewer alone still sufficed for the coverage calculation.

## Implemented behavior

- Track qualifying reviewer IDs for each exact source-revision- and claim-span-bound case ID.
- Only validated records (registered/acquired before review, no duplicate judgment, valid claim byte span, matching first source and rubric, no duplicate source identity) contribute to the reviewer set.
- Require **at least two distinct qualifying reviewer IDs per case** before counting that case or its article toward the preregistered project coverage floors.
- Explicitly report cases that fall short, while continuing to report their project's insufficient cases and distinct articles.
- Preserve raw review entries and do not pretend a reviewer ID is an authenticated independent person.

The validator rejects a manifest with insufficient case coverage. It does not discard negative review verdicts, which are still legitimate independent judgments; the required reviewers need not agree on whether the generated claim is supported.

## Deterministic regression evidence

`scripts/test_validate_rag003_review_manifest.py` now constructs valid two-reviewer baselines and covers:

1. One reviewer cannot qualify a case or distinct article.
2. A replay with the same reviewer ID cannot act as a second reviewer.
3. An invalid claim span in the second judgment cannot qualify a case.
4. A second case/article with only one reviewer cannot satisfy project floors.
5. Two distinct reviewers with the same revision, claim span and rubric can qualify, while conflicts remain rejected.

The regression suite runs in the Rust workspace workflow's **RAG-003 structural and semantic review manifest regression gate**.

## CI and scope

- [Rust workspace CI 41705609716](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41705609716) — **passed** for source commit `44131223d18a92419057a80c1584c4ec2227fd91`.
- [Platform shells 41705609500](https://github.com/ekkus93/foundation-wikipedia/actions/runs/41705609500) — pending at report preparation.

A string ID is not proof of an actual independent human. Real-world reviewer identities, separateness, source-revision authentication, raw artifact digests, factual-entailment quality, blinded sampling, and desktop/Android physical-device acceptance remain unverified. All fixtures here are synthetic; **RAG-003 stays unchecked** until independent evidence meets the preregistered requirements.
