use sha2::{Digest, Sha256};
use wiki_source::public_dump::declared_member_page_range;
use wiki_source::public_dump_pinned::parse_pinned_inventory;
use wiki_source::public_dump_stream::StreamShardAudit;

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
    let mut audit = StreamShardAudit::new(&inventory);
    for (member, page_id) in inventory.members().iter().zip(starts) {
        audit.begin_member(&member.relative_path).unwrap();
        audit.push_page_id(page_id).unwrap();
        audit.end_member().unwrap();
    }
    assert_eq!(audit.finish(), Ok(10));
}

#[test]
fn modern_page_range_shards_are_sorted_numerically() {
    let raw = [
        "enwiki-2026-10-01-p201p300.xml.bz2",
        "enwiki-2026-10-01-p1001p1100.xml.bz2",
        "enwiki-2026-10-01-p1p100.xml.bz2",
        "enwiki-2026-10-01-p101p200.xml.bz2",
    ]
    .iter()
    .map(|name| format!("{}  {name}\\n", "a".repeat(64)))
    .collect::<String>();
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
    assert_eq!(starts, vec![1, 101, 201, 1001]);
}
