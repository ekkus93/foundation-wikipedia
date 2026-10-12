use wiki_pack_format::stage::stage_container;

#[test]
fn stage_api_is_public() {
    let _ = stage_container::<std::io::Cursor<Vec<u8>>>;
}
