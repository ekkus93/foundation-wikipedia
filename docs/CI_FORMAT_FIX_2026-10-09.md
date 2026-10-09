# CI formatting issue

Rust CI at master `668b5cb1026b1014a6d4f29cdd24e6708e5b4b17` reports a rustfmt-only failure in `crates/wiki-pack-builder/tests/selection_contract.rs`: the first assertion in `invalid_page_limits_and_missing_categories_fail_closed` must be formatted as a single line. Platform shells CI passed. The selection contract tests remain unqualified until Rust CI passes on the exact corrected commit.
