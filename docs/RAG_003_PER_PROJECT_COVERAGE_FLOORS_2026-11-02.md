# RAG-003: Per-project benchmark coverage floors (2026-11-02)

## Implemented
The human-reviewed answer-support runner now accepts `--min-project-cases N` and `--min-project-distinct-articles N` (0–1000, default zero). Each observed Wikimedia project is evaluated independently. Revisions and multiple questions for the same project/page identity do not inflate distinct-article coverage. A project below either floor fails the benchmark even when aggregate coverage and answer-quality gates pass. The JSON diagnostic report includes `gate.project_coverage_gate` with observed per-project case/article counts, thresholds and failed projects. Existing behavior is preserved when both floors are zero.

## Evidence
Implementation qualified at exact master SHA `7ffcb6688c421cf24ce11db381bd0e0c55a7db31`. Rust workspace CI 41568323930 (including executable example tests), Platform shells CI 41568324010, and Android Rust ABI CI 41568323851 all passed. Initial implementation failed formatting; the formatted revision exposed an inconsistent test fixture, which was corrected before qualification.

## Limits
These floors enforce structural diversity within projects, not representative sampling, independently reviewed annotations, publisher-authenticated evidence, factual entailment or physical-device acceptance. RAG-003 remains unchecked. Future TODO reconciliation should reference this qualification and retain all unmet acceptance requirements.
