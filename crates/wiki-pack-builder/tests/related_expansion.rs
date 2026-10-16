use std::collections::{BTreeMap, BTreeSet};

use wiki_pack_builder::related::expand_related_links;

#[test]
fn public_related_api_is_deterministic_and_excludes_unwanted_pages() {
    let graph = BTreeMap::from([(1, vec![3, 2]), (2, vec![1, 4]), (3, vec![4])]);
    let excluded = BTreeSet::from([4]);
    assert_eq!(
        expand_related_links(&[1], &graph, &excluded, 2, 10, 10),
        Ok(vec![1, 2, 3])
    );
}
