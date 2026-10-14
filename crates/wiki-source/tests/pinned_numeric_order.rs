use sha2::{Digest, Sha256};
use wiki_source::public_dump::declared_member_page_range;
use wiki_source::public_dump_pinned::parse_pinned_inventory;

#[test]
fn legacy_shard_ten_follows_shard_two() {
    use std::fmt::Write;
    let mut raw = String::new();
    for i in (1..=10_u64).rev() {
        writeln!(
            &mut raw,
            "{}  enwiki-20261001-pages-meta-current{i}.xml-p{}p{}.bz2",
            "a".repeat(64),
            (i - 1) * 100 + 1,
            i * 100
        )
        .unwrap();
    }
    let digest = format!("{:x}", Sha256::digest(raw.as_bytes()));
    let inventory =
        parse_pinned_inventory(raw.as_bytes(), "enwiki", "2026-10-01", &digest).unwrap();
    let starts: Vec<_> = inventory
        .members()
        .iter()
        .map(|m| {
            declared_member_page_range(m, "enwiki", "2026-10-01")
                .unwrap()
                .0
        })
        .collect();
    assert_eq!(starts, (0..10_u64).map(|i| i * 100 + 1).collect::<Vec<_>>());
}
