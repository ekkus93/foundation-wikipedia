use std::io::Cursor;
use std::path::Path;

use wiki_pack_format::container::ContainerError;
use wiki_pack_format::stage::stage_container;

#[test]
fn stage_rejects_unsafe_name_before_reading_input() {
    let mut empty = Cursor::new(Vec::<u8>::new());
    assert!(matches!(
        stage_container(&mut empty, Path::new("."), "../outside"),
        Err(ContainerError::UnsafeStagingPath)
    ));
}
