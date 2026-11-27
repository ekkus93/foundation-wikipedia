//! Opt-in check that real packs on this machine still install under the
//! current media rules (TRUST-003): set `FOUNDATION_REAL_PACKS` to one or
//! more `.wpack` paths separated by `:` and run with `--ignored`. Nothing
//! here downloads anything; the packs stay where they are. Use absolute
//! paths: the test runs from the crate directory.

mod support;

use support::test_dir;
use wiki_ffi::android::OfflineLibrary;

#[test]
#[ignore = "needs real packs: FOUNDATION_REAL_PACKS=a.wpack:b.wpack"]
fn real_packs_install_and_serve_every_image() {
    let paths = std::env::var("FOUNDATION_REAL_PACKS").expect("FOUNDATION_REAL_PACKS");
    for (n, path) in paths.split(':').filter(|p| !p.is_empty()).enumerate() {
        let dir = test_dir(&format!("real-pack-{n}"));
        let library =
            OfflineLibrary::new(dir.0.join("library").to_string_lossy().into_owned()).unwrap();
        let installed = library
            .install_pack(path.to_owned(), true)
            .unwrap_or_else(|error| panic!("{path}: {error:?}"));
        library.activate(installed.snapshot_id.clone()).unwrap();
        let index = dir
            .0
            .join("library/snapshots")
            .join(&installed.snapshot_id)
            .join("media/index.json");
        let mut types = std::collections::BTreeMap::<String, usize>::new();
        if let Ok(bytes) = std::fs::read(&index) {
            for image in wiki_store::pack_media::parse_media_index(&bytes).unwrap() {
                let digest = image.sha256.clone();
                let served = library.read_image(digest).unwrap().expect("listed image");
                assert_eq!(served.mime, image.mime, "{path}: {}", image.sha256);
                *types.entry(image.mime).or_default() += 1;
            }
        }
        println!(
            "{path}: installed {} ({} articles), images served: {types:?}",
            installed.snapshot_id, installed.articles
        );
    }
}
