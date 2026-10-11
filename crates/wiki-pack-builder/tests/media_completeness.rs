use std::collections::BTreeMap;
use wiki_pack_builder::media::{check_media_completeness, MediaError, Resource, ResourceKind};

fn required(id: &str, kind: ResourceKind) -> Resource {
    Resource {
        id: id.into(),
        kind,
        sha256: Some("a".repeat(64)),
        bytes: Some(128),
        preview_id: None,
        creator: "Wikimedia contributor".into(),
        license: "CC BY-SA 4.0".into(),
        attribution: "Contributor / CC BY-SA 4.0".into(),
    }
}

fn verified() -> BTreeMap<String, u64> {
    BTreeMap::from([("a".repeat(64), 128)])
}

#[test]
fn every_declared_visual_requires_verified_local_bytes() {
    for kind in [
        ResourceKind::Image,
        ResourceKind::Diagram,
        ResourceKind::Plot,
        ResourceKind::Map,
        ResourceKind::Math,
        ResourceKind::Style,
        ResourceKind::Font,
        ResourceKind::AudioVideoPreview,
    ] {
        let item = required("article-image", kind);
        assert_eq!(
            check_media_completeness(&[item.clone()], &BTreeMap::new()),
            Err(MediaError::MissingVerifiedObject(item.id.clone()))
        );
        assert_eq!(
            check_media_completeness(&[item], &verified())
                .unwrap()
                .unique_verified_bytes,
            128
        );
    }
}

#[test]
fn corrupt_or_unattributed_objects_fail_closed() {
    let mut item = required("figure", ResourceKind::Diagram);
    item.sha256 = Some("bad".into());
    assert_eq!(
        check_media_completeness(&[item.clone()], &verified()),
        Err(MediaError::InvalidDigest("figure".into()))
    );
    item.sha256 = Some("a".repeat(64));
    item.attribution.clear();
    assert_eq!(
        check_media_completeness(&[item.clone()], &verified()),
        Err(MediaError::MissingMetadata("figure".into()))
    );
    item.attribution = "Contributor / CC BY-SA 4.0".into();
    item.bytes = Some(129);
    assert_eq!(
        check_media_completeness(&[item], &verified()),
        Err(MediaError::ByteLengthMismatch("figure".into()))
    );
}

#[test]
fn av_stream_bytes_are_not_bundled_but_verified_preview_is_required() {
    let mut stream = required("recording", ResourceKind::AudioVideoStream);
    stream.sha256 = None;
    stream.bytes = None;
    stream.preview_id = Some("recording-thumb".into());
    let preview = required("recording-thumb", ResourceKind::AudioVideoPreview);
    assert_eq!(
        check_media_completeness(&[stream.clone()], &verified()),
        Err(MediaError::MissingPreview("recording".into()))
    );
    let complete =
        check_media_completeness(&[stream.clone(), preview.clone()], &verified()).unwrap();
    assert_eq!(complete.on_demand_streams, 1);
    assert_eq!(complete.required_objects, 1);
    stream.sha256 = Some("b".repeat(64));
    assert_eq!(
        check_media_completeness(&[stream, preview], &verified()),
        Err(MediaError::InvalidStream("recording".into()))
    );
}

#[test]
fn shared_objects_dedupe_and_duplicate_logical_ids_are_rejected() {
    let a = required("image-1", ResourceKind::Image);
    let mut b = a.clone();
    b.id = "image-2".into();
    let result = check_media_completeness(&[a.clone(), b], &verified()).unwrap();
    assert_eq!(result.required_objects, 1);
    assert_eq!(result.unique_verified_bytes, 128);
    assert_eq!(
        check_media_completeness(&[a.clone(), a], &verified()),
        Err(MediaError::DuplicateId("image-1".into()))
    );
}

#[test]
fn uppercase_digest_aliases_deduplicate_against_canonical_object_keys() {
    let first = required("figure-a", ResourceKind::Image);
    let mut second = required("figure-b", ResourceKind::Diagram);
    second.sha256 = Some("A".repeat(64));
    let complete = check_media_completeness(&[first, second], &verified()).unwrap();
    assert_eq!(complete.required_objects, 1);
    assert_eq!(complete.unique_verified_bytes, 128);
}
