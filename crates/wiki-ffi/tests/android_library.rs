//! MOB-001 host tests of the Kotlin-callable offline library API.

use std::fs;
use std::path::{Path, PathBuf};

use wiki_ffi::android::{LibraryError, OfflineLibrary};
use wiki_model::{
    Article, ArticleKey, Block, BlockContent, PageRecord, Revision, ARTICLE_SCHEMA_VERSION,
};
use wiki_pack_format::export::{custom_manifest_from_tree, write_custom_tree};
use wiki_pack_format::manifest::PackMetadata;
use wiki_store::catalog::SnapshotCatalog;
use wiki_store::pack_install::snapshot_pack_metadata;
use wiki_store::record_codec::append_record_frame;

struct TestDir(PathBuf);

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn test_dir(label: &str) -> TestDir {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "wiki-ffi-android-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    TestDir(dir)
}

fn article(page_id: u64, title: &str, text: &str) -> Article {
    Article {
        schema_version: ARTICLE_SCHEMA_VERSION,
        key: ArticleKey {
            project: "enwiki".into(),
            page_id,
        },
        revision: Revision {
            revision_id: 7000 + page_id,
            timestamp: "2026-11-01T00:00:00Z".into(),
            content_sha256: "a".repeat(64),
        },
        title: title.into(),
        display_title: title.into(),
        language: "en".into(),
        namespace: 0,
        aliases: vec![],
        wikidata_id: None,
        lead: vec![Block {
            ordinal: 0,
            content: BlockContent::Paragraph(text.into()),
        }],
        sections: vec![],
        references: vec![],
        links: vec![],
        media: vec![],
        rendered_html: format!("<article><h1>{title}</h1><p>{text}</p></article>"),
        is_disambiguation: false,
    }
}

/// Build a two-article pack file and return its path.
fn build_pack(dir: &Path, pack_id: &str) -> PathBuf {
    let tree = dir.join(format!("{pack_id}-tree"));
    fs::create_dir_all(&tree).unwrap();
    let mut shard = Vec::new();
    let mut frames = Vec::new();
    for article in [article(1, "Moon", "The Moon orbits the Earth."), {
        // An in-pack article link and an external link for the reader
        // (MOB-002); the text blocks used by search and chat are unchanged.
        let mut tide = article(2, "Tide", "Tides follow the Moon's gravity.");
        tide.rendered_html = concat!(
            "<article><p>Tides follow the <a href=\"./Moon\">Moon</a>'s gravity.</p>",
            "<p><a href=\"https://en.wikipedia.org/wiki/Tide\">Tide on Wikipedia</a></p>",
            "</article>"
        )
        .into();
        tide
    }] {
        let offset = shard.len() as u64;
        append_record_frame(&mut shard, &PageRecord::Article(Box::new(article))).unwrap();
        frames.push((offset, shard.len() as u64 - offset));
    }
    fs::write(tree.join("articles.shard"), &shard).unwrap();
    let mut catalog = SnapshotCatalog::open(&tree.join("catalog.sqlite")).unwrap();
    for (offset, bytes) in frames {
        catalog
            .insert_verified(&tree, "articles.shard", offset, bytes)
            .unwrap();
    }
    drop(catalog);
    let metadata = snapshot_pack_metadata(
        &tree,
        PackMetadata::synthetic_fixture().source,
        &"0".repeat(64),
        "CC-BY-SA-4.0",
    )
    .unwrap();
    let manifest =
        custom_manifest_from_tree(&tree, pack_id, "enwiki", "20261101", "moon", metadata).unwrap();
    let path = dir.join(format!("{pack_id}.wpack"));
    let mut file = fs::File::create(&path).unwrap();
    write_custom_tree(&tree, &manifest, &mut file).unwrap();
    path
}

fn library(dir: &Path) -> std::sync::Arc<OfflineLibrary> {
    OfflineLibrary::new(dir.join("library").to_string_lossy().into_owned()).unwrap()
}

#[test]
fn install_activate_search_and_open_through_the_exported_api() {
    let dir = test_dir("lifecycle");
    let pack = build_pack(&dir.0, "moon-pack");
    let pack_path = pack.to_string_lossy().into_owned();
    let library = library(&dir.0);

    assert_eq!(library.active_snapshot().unwrap(), None);
    assert_eq!(
        library.search("moon".into(), 10),
        Err(LibraryError::NotInstalled)
    );
    assert_eq!(
        library.install_pack(pack_path.clone(), false),
        Err(LibraryError::ConfirmationRequired)
    );
    let installed = library.install_pack(pack_path.clone(), true).unwrap();
    assert_eq!(installed.snapshot_id, "moon-pack");
    assert_eq!(installed.project, "enwiki");
    assert_eq!(installed.trust_label, "Custom — integrity verified");
    assert_eq!(installed.articles, 2);
    assert_eq!(
        library.install_pack(pack_path, true),
        Err(LibraryError::AlreadyInstalled)
    );

    library.activate("moon-pack".into()).unwrap();
    assert_eq!(
        library.active_snapshot().unwrap().as_deref(),
        Some("moon-pack")
    );
    let hits = library.search("gravity".into(), 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!((hits[0].page_id, hits[0].title.as_str()), (2, "Tide"));
    let page = library.open_page("enwiki".into(), 1).unwrap().unwrap();
    assert_eq!(page.title, "Moon");
    assert_eq!(page.revision_id, 7001);
    assert!(page.html.contains("orbits the Earth"));
    assert_eq!(library.open_page("enwiki".into(), 99).unwrap(), None);

    // A new instance (app restart) reopens the active snapshot from disk.
    drop(library);
    let restarted = self::library(&dir.0);
    assert_eq!(
        restarted.active_snapshot().unwrap().as_deref(),
        Some("moon-pack")
    );
    assert_eq!(restarted.search("moon".into(), 10).unwrap().len(), 2);
}

#[test]
fn invalid_input_and_corruption_fail_closed() {
    let dir = test_dir("invalid");
    assert!(matches!(
        OfflineLibrary::new("relative/dir".into()),
        Err(LibraryError::InvalidInput)
    ));
    let library = library(&dir.0);
    assert_eq!(
        library.install_pack("relative.wpack".into(), true),
        Err(LibraryError::InvalidInput)
    );
    assert_eq!(
        library.install_pack(dir.0.join("missing.wpack").to_string_lossy().into(), true),
        Err(LibraryError::InvalidInput)
    );
    let garbage = dir.0.join("garbage.wpack");
    fs::write(&garbage, b"not a pack").unwrap();
    assert_eq!(
        library.install_pack(garbage.to_string_lossy().into(), true),
        Err(LibraryError::VerificationFailed)
    );
    assert_eq!(
        library.activate("absent".into()),
        Err(LibraryError::NotInstalled)
    );
    assert_eq!(
        library.open_page("Not A Project".into(), 1),
        Err(LibraryError::InvalidInput)
    );

    let pack = build_pack(&dir.0, "moon-pack");
    library
        .install_pack(pack.to_string_lossy().into(), true)
        .unwrap();
    library.activate("moon-pack".into()).unwrap();
    for (query, limit) in [("", 5), ("   ", 5), ("moon", 0)] {
        assert_eq!(
            library.search(query.into(), limit),
            Err(LibraryError::InvalidInput)
        );
    }
    assert_eq!(
        library.search("x".repeat(1025), 5),
        Err(LibraryError::InvalidInput)
    );

    // Corrupt the installed shard: a restarted library must refuse it.
    let shard = dir.0.join("library/snapshots/moon-pack/articles.shard");
    let mut bytes = fs::read(&shard).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&shard, bytes).unwrap();
    drop(library);
    let restarted = self::library(&dir.0);
    assert_eq!(
        restarted.search("moon".into(), 5),
        Err(LibraryError::VerificationFailed)
    );
}

fn android_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/wpack/android-sample-v2.wpack")
}

/// The Android instrumented test installs this committed pack. It must stay
/// exactly what the builder above produces; regenerate after an intentional
/// change with `WPACK_WRITE_FIXTURE=1 cargo test -p wiki-ffi --test
/// android_library committed_android_fixture` and update its provenance hash.
#[test]
fn committed_android_fixture_matches_the_builder() {
    let first = test_dir("fixture-a");
    let second = test_dir("fixture-b");
    let bytes = fs::read(build_pack(&first.0, "android-sample")).unwrap();
    assert_eq!(
        bytes,
        fs::read(build_pack(&second.0, "android-sample")).unwrap(),
        "the pack builder must be deterministic"
    );
    if std::env::var_os("WPACK_WRITE_FIXTURE").is_some() {
        fs::write(android_fixture_path(), &bytes).unwrap();
    }
    assert_eq!(fs::read(android_fixture_path()).unwrap(), bytes);
}

mod streamed {
    use super::*;
    use std::sync::{Arc, Mutex};
    use wiki_ffi::android::{InstallReviewer, PackReview, PackSource};

    /// In-memory stand-in for an Android document-picker stream.
    struct BytesSource {
        bytes: Vec<u8>,
        position: Mutex<usize>,
        overrun: bool,
    }

    impl BytesSource {
        fn new(bytes: Vec<u8>) -> Arc<Self> {
            Arc::new(Self {
                bytes,
                position: Mutex::new(0),
                overrun: false,
            })
        }
    }

    impl PackSource for BytesSource {
        fn read(&self, max_bytes: u32) -> Result<Vec<u8>, LibraryError> {
            let mut position = self.position.lock().unwrap();
            let take = if self.overrun {
                max_bytes as usize + 1
            } else {
                max_bytes as usize
            };
            let end = (*position + take).min(self.bytes.len());
            let chunk = self.bytes[*position..end].to_vec();
            *position = end;
            Ok(chunk)
        }
    }

    struct Reviewer {
        approve: bool,
        seen: Mutex<Vec<PackReview>>,
    }

    impl Reviewer {
        fn new(approve: bool) -> Arc<Self> {
            Arc::new(Self {
                approve,
                seen: Mutex::new(Vec::new()),
            })
        }
    }

    impl InstallReviewer for Reviewer {
        fn approve(&self, review: PackReview) -> bool {
            self.seen.lock().unwrap().push(review);
            self.approve
        }
    }

    #[test]
    fn streamed_import_reviews_then_installs_and_dedups() {
        let dir = test_dir("stream");
        let bytes = fs::read(build_pack(&dir.0, "moon-pack")).unwrap();
        let library = library(&dir.0);

        let declined = Reviewer::new(false);
        assert_eq!(
            library.install_pack_stream(BytesSource::new(bytes.clone()), declined.clone(), 1 << 30),
            Err(LibraryError::ConfirmationRequired)
        );
        let review = declined.seen.lock().unwrap()[0].clone();
        assert_eq!(review.snapshot_id, "moon-pack");
        assert_eq!(review.trust_label, "Custom — integrity verified");
        assert_eq!(review.articles, 2);
        assert!(library.list_packs().unwrap().is_empty());

        let approved = Reviewer::new(true);
        let installed = library
            .install_pack_stream(BytesSource::new(bytes.clone()), approved, 1 << 30)
            .unwrap();
        assert_eq!(installed.trust_label, "Custom — integrity verified");
        // Importing the same pack again never duplicates it.
        let again = Reviewer::new(true);
        assert_eq!(
            library.install_pack_stream(BytesSource::new(bytes), again.clone(), 1 << 30),
            Err(LibraryError::AlreadyInstalled)
        );
        assert_eq!(library.list_packs().unwrap().len(), 1);
    }

    #[test]
    fn tampered_truncated_or_overrunning_streams_are_rejected() {
        let dir = test_dir("tamper");
        let bytes = fs::read(build_pack(&dir.0, "moon-pack")).unwrap();
        let library = library(&dir.0);
        let mut tampered = bytes.clone();
        let middle = tampered.len() / 2;
        tampered[middle] ^= 0x01;
        for (label, source) in [
            ("tampered", BytesSource::new(tampered)),
            (
                "truncated",
                BytesSource::new(bytes[..bytes.len() - 7].to_vec()),
            ),
            (
                "overrun",
                Arc::new(BytesSource {
                    bytes: bytes.clone(),
                    position: Mutex::new(0),
                    overrun: true,
                }),
            ),
        ] {
            let reviewer = Reviewer::new(true);
            let result = library.install_pack_stream(source, reviewer.clone(), 1 << 30);
            assert!(
                matches!(
                    result,
                    Err(LibraryError::VerificationFailed | LibraryError::Storage)
                ),
                "{label}: {result:?}"
            );
            assert!(
                reviewer.seen.lock().unwrap().is_empty(),
                "{label}: no review"
            );
        }
        // A spool budget smaller than the pack is refused.
        assert!(library
            .install_pack_stream(BytesSource::new(bytes), Reviewer::new(true), 100)
            .is_err());
        assert_eq!(
            library.install_pack_stream(BytesSource::new(vec![]), Reviewer::new(true), 0),
            Err(LibraryError::InvalidInput)
        );
        assert!(library.list_packs().unwrap().is_empty());
    }

    #[test]
    fn packs_are_listed_and_only_inactive_ones_removed() {
        let dir = test_dir("manage");
        let library = library(&dir.0);
        for id in ["moon-pack", "tide-pack"] {
            let bytes = fs::read(build_pack(&dir.0, id)).unwrap();
            library
                .install_pack_stream(BytesSource::new(bytes), Reviewer::new(true), 1 << 30)
                .unwrap();
        }
        library.activate("moon-pack".into()).unwrap();
        let listed = library.list_packs().unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|p| (p.snapshot_id.as_str(), p.active))
                .collect::<Vec<_>>(),
            [("moon-pack", true), ("tide-pack", false)]
        );
        assert_eq!(
            library.remove_pack("moon-pack".into()),
            Err(LibraryError::SnapshotActive)
        );
        library.remove_pack("tide-pack".into()).unwrap();
        assert_eq!(
            library.remove_pack("tide-pack".into()),
            Err(LibraryError::NotInstalled)
        );
        assert_eq!(library.list_packs().unwrap().len(), 1);
        // The remaining active pack still searches.
        assert_eq!(library.search("moon".into(), 5).unwrap().len(), 2);
    }
}

/// Android app directories live under /data/user/0, a symlink to /data/data.
#[cfg(unix)]
#[test]
fn library_opens_through_a_symlinked_app_directory() {
    let dir = test_dir("symlink");
    let real = dir.0.join("data");
    fs::create_dir_all(real.join("app/files")).unwrap();
    let user0 = dir.0.join("user0");
    std::os::unix::fs::symlink(&real, &user0).unwrap();
    let through_link = user0.join("app/files/offline-library");
    let library = OfflineLibrary::new(through_link.to_string_lossy().into_owned()).unwrap();
    let pack = build_pack(&dir.0, "moon-pack");
    library
        .install_pack(pack.to_string_lossy().into(), true)
        .unwrap();
    library.activate("moon-pack".into()).unwrap();
    assert_eq!(library.search("moon".into(), 5).unwrap().len(), 2);
    assert!(real
        .join("app/files/offline-library/snapshots/moon-pack")
        .is_dir());
}

/// AI-003: hosted article chat over an installed pack, without network I/O.
mod hosted_chat {
    use super::*;
    use std::sync::Arc;
    use wiki_ffi::android::{
        ArticleChat, ChatError, ChatHistory, ChatPassage, ChatTurn, HostedAiProvider,
        HostedChatRequest, SavedAnswerPart,
    };
    use wiki_store::user_state::UserState;

    const KEY: &str = "sk-ant-host-test-secret";
    const TIDE_REVISION: u64 = 7002;

    fn active_library(dir: &Path) -> Arc<OfflineLibrary> {
        let pack = build_pack(dir, "moon-pack");
        let library = library(dir);
        library
            .install_pack(pack.to_string_lossy().into_owned(), true)
            .unwrap();
        library.activate("moon-pack".into()).unwrap();
        library
    }

    fn request(page_id: u64, question: &str, thread_id: Option<i64>) -> HostedChatRequest {
        HostedChatRequest {
            project: "enwiki".into(),
            page_id,
            question: question.into(),
            provider: HostedAiProvider::Anthropic,
            model: "claude-haiku-5-5".into(),
            thread_id,
            selection: None,
        }
    }

    fn prepare(
        library: &OfflineLibrary,
        page_id: u64,
        question: &str,
    ) -> Result<Arc<ArticleChat>, ChatError> {
        library.prepare_hosted_chat(request(page_id, question, None), KEY.into(), None)
    }

    fn history(dir: &Path) -> Arc<ChatHistory> {
        ChatHistory::new(dir.join("chat").to_string_lossy().into_owned()).unwrap()
    }

    /// Save one supported exchange about the Tide article directly in the
    /// store, citing `passage`, and return its thread.
    fn seed_thread(dir: &Path, passage: &ChatPassage) -> i64 {
        let mut state = UserState::open(&dir.join("chat").join("chat-history.sqlite")).unwrap();
        let generation = state.personal_generation().unwrap();
        let key = ArticleKey {
            project: "enwiki".into(),
            page_id: 2,
        };
        let citation = wiki_search::saved_citation::SavedCitation::from_evidence(
            &key,
            &wiki_search::EvidenceHit {
                block_id: passage.block_id.clone(),
                revision_id: TIDE_REVISION,
                heading_path: passage.heading_path.clone(),
                excerpt: passage.excerpt.clone(),
                score: 0,
            },
            Some("moon-pack"),
        )
        .unwrap()
        .to_stored();
        let (thread, _, _) = state
            .save_cited_chat_exchange_for_generation(
                generation,
                &key,
                TIDE_REVISION,
                None,
                "What do tides follow?",
                &format!("The Moon. [[cite:{}]]", passage.block_id),
                &[citation],
            )
            .unwrap()
            .unwrap();
        thread
    }

    #[test]
    fn a_preview_discloses_passages_host_and_model_and_needs_consent() {
        let dir = test_dir("chat-preview");
        let library = active_library(&dir.0);
        let chat = prepare(&library, 2, "What do tides follow?").unwrap();
        assert_eq!(chat.destination(), "api.anthropic.com");
        assert_eq!(chat.model(), "claude-haiku-5-5");
        assert_eq!(chat.article_title(), "Tide");
        assert_eq!(chat.prior_turns(), Vec::<ChatTurn>::new());
        let passages = chat.passages();
        assert!(passages
            .iter()
            .any(|passage| passage.excerpt.contains("Tides follow the Moon's gravity.")));
        assert!(passages
            .iter()
            .all(|passage| passage.block_id.starts_with("wkb:enwiki:2:")));
        assert_eq!(chat.send(false, None), Err(ChatError::ConsentRequired));
    }

    #[test]
    fn cancellation_before_sending_never_connects_and_a_preview_is_single_use() {
        let dir = test_dir("chat-cancel");
        let library = active_library(&dir.0);
        let history = history(&dir.0);
        let chat = library
            .prepare_hosted_chat(
                request(1, "What does the Moon orbit?", None),
                KEY.into(),
                Some(history.clone()),
            )
            .unwrap();
        chat.cancel();
        assert_eq!(chat.send(true, None), Err(ChatError::Cancelled));
        assert_eq!(chat.send(true, None), Err(ChatError::AlreadySent));
        // Nothing was saved for the cancelled question.
        assert_eq!(
            history.threads("enwiki".into(), 1, 7001).unwrap(),
            Vec::<i64>::new()
        );
    }

    #[test]
    fn bad_inputs_and_missing_articles_fail_before_any_request() {
        let dir = test_dir("chat-inputs");
        let inactive = library(&dir.0.join("empty"));
        assert_eq!(
            prepare(&inactive, 1, "What does the Moon orbit?").err(),
            Some(ChatError::NotInstalled)
        );
        let library = active_library(&dir.0);
        assert_eq!(
            prepare(&library, 99, "What does the Moon orbit?").err(),
            Some(ChatError::NotInstalled)
        );
        assert_eq!(
            prepare(&library, 1, "   ").err(),
            Some(ChatError::InvalidInput)
        );
        assert_eq!(
            prepare(&library, 1, &"q".repeat(4096)).err(),
            Some(ChatError::InvalidInput)
        );
        assert_eq!(
            prepare(&library, 1, "zebra xylophone quantum").err(),
            Some(ChatError::NoEvidence)
        );
        let bad_key = library.prepare_hosted_chat(
            request(1, "What does the Moon orbit?", None),
            "bad key".into(),
            None,
        );
        assert_eq!(bad_key.err(), Some(ChatError::InvalidKey));
        let mut bad_model = request(1, "What does the Moon orbit?", None);
        bad_model.model = "two words".into();
        assert_eq!(
            library
                .prepare_hosted_chat(bad_model, KEY.into(), None)
                .err(),
            Some(ChatError::InvalidInput)
        );
        // A thread can only be continued with chat history.
        assert_eq!(
            library
                .prepare_hosted_chat(
                    request(1, "What does the Moon orbit?", Some(1)),
                    KEY.into(),
                    None
                )
                .err(),
            Some(ChatError::InvalidInput)
        );
    }

    #[test]
    fn saved_threads_are_listed_read_with_their_evidence_and_continued() {
        let dir = test_dir("chat-history");
        let library = active_library(&dir.0);
        let history = history(&dir.0);
        let passage = prepare(&library, 2, "What do tides follow?")
            .unwrap()
            .passages()[0]
            .clone();
        let thread = seed_thread(&dir.0, &passage);

        assert_eq!(
            history.threads("enwiki".into(), 2, TIDE_REVISION).unwrap(),
            vec![thread]
        );
        assert_eq!(
            history.threads("enwiki".into(), 2, 1).unwrap(),
            Vec::<i64>::new()
        );
        let exchanges = history
            .exchanges("enwiki".into(), 2, TIDE_REVISION, thread)
            .unwrap();
        assert_eq!(exchanges.len(), 1);
        assert_eq!(exchanges[0].question, "What do tides follow?");
        assert_eq!(
            exchanges[0].answer,
            vec![
                SavedAnswerPart::Text {
                    text: "The Moon. ".into()
                },
                SavedAnswerPart::Citation {
                    block_id: passage.block_id.clone(),
                    passage: Some(passage.clone()),
                },
            ]
        );

        // A follow-up resends the thread's turns, disclosed without markers.
        let follow_up = library
            .prepare_hosted_chat(
                request(2, "Why do tides follow the Moon?", Some(thread)),
                KEY.into(),
                Some(history.clone()),
            )
            .unwrap();
        assert_eq!(
            follow_up.prior_turns(),
            vec![ChatTurn {
                question: "What do tides follow?".into(),
                answer: "The Moon.".into(),
            }]
        );

        // A thread of another article cannot be continued here.
        assert_eq!(
            library
                .prepare_hosted_chat(
                    request(1, "What does the Moon orbit?", Some(thread)),
                    KEY.into(),
                    Some(history.clone()),
                )
                .err(),
            Some(ChatError::InvalidInput)
        );

        // Clearing history makes the prepared follow-up unsaveable, and it
        // is refused before any connection.
        history.clear_all().unwrap();
        assert_eq!(follow_up.send(true, None), Err(ChatError::PreviewChanged));
        assert_eq!(
            history.threads("enwiki".into(), 2, TIDE_REVISION).unwrap(),
            Vec::<i64>::new()
        );
    }

    #[test]
    fn a_thread_can_be_deleted() {
        let dir = test_dir("chat-delete");
        let library = active_library(&dir.0);
        let history = history(&dir.0);
        let passage = prepare(&library, 2, "What do tides follow?")
            .unwrap()
            .passages()[0]
            .clone();
        let thread = seed_thread(&dir.0, &passage);
        assert!(history
            .delete_thread("enwiki".into(), 2, TIDE_REVISION, thread)
            .unwrap());
        assert!(!history
            .delete_thread("enwiki".into(), 2, TIDE_REVISION, thread)
            .unwrap());
        assert_eq!(
            history.threads("enwiki".into(), 2, TIDE_REVISION).unwrap(),
            Vec::<i64>::new()
        );
        assert_eq!(
            ChatHistory::new("relative/path".into()).err(),
            Some(ChatError::InvalidInput)
        );
    }
}

/// MOB-002: reader documents for the article WebView.
mod reader {
    use super::*;

    fn active_library(dir: &Path) -> std::sync::Arc<OfflineLibrary> {
        let pack = build_pack(dir, "moon-pack");
        let library = library(dir);
        library
            .install_pack(pack.to_string_lossy().into_owned(), true)
            .unwrap();
        library.activate("moon-pack".into()).unwrap();
        library
    }

    #[test]
    fn pages_open_by_id_as_locked_down_themed_documents() {
        let dir = test_dir("reader-page");
        let library = active_library(&dir.0);
        let page = library
            .open_reader_page("enwiki".into(), 2, false)
            .unwrap()
            .unwrap();
        assert_eq!(page.title, "Tide");
        assert_eq!(page.revision_id, 7002);
        assert_eq!(page.base_url, "https://foundation.invalid/wiki/Tide");
        assert!(page.document.starts_with("<!doctype html>"));
        assert!(page
            .document
            .contains("<a href=\"https://foundation.invalid/wiki/Moon\" rel=\"noopener noreferrer nofollow\">Moon</a>"));
        assert!(page
            .document
            .contains("href=\"https://en.wikipedia.org/wiki/Tide\""));
        assert!(page.document.contains("default-src 'none'"));
        assert!(!page.document.contains("class=\"dark\""));
        let dark = library
            .open_reader_page("enwiki".into(), 2, true)
            .unwrap()
            .unwrap();
        assert!(dark.document.contains("class=\"dark\""));
        assert_eq!(
            library.open_reader_page("enwiki".into(), 99, false),
            Ok(None)
        );
    }

    #[test]
    fn a_citation_opens_its_exact_revision_with_the_passage_highlighted() {
        let dir = test_dir("reader-citation");
        let library = active_library(&dir.0);
        let key = ArticleKey {
            project: "enwiki".into(),
            page_id: 2,
        };
        let block = key.block_id(7002, &[], 0);
        let cited = library
            .open_reader_citation("enwiki".into(), 2, 7002, block.clone(), false)
            .unwrap()
            .unwrap();
        assert!(cited.located);
        assert_eq!(cited.fragment, "foundation-cited");
        assert_eq!(cited.page.base_url, "https://foundation.invalid/wiki/Tide");
        assert!(cited.page.document.contains(
            "<div id=\"foundation-cited\" class=\"foundation-cited\"><p>Tides follow the"
        ));
        // A different revision, a foreign block or a missing page is refused.
        for (page, revision, block) in [
            (2, 7001, block.clone()),
            (2, 7002, key.block_id(7002, &[3], 0)),
            (99, 7099, block.clone()),
        ] {
            assert_eq!(
                library.open_reader_citation("enwiki".into(), page, revision, block, false),
                Ok(None)
            );
        }
        assert_eq!(
            library.open_reader_citation("enwiki".into(), 2, 7002, String::new(), false),
            Err(LibraryError::InvalidInput)
        );
    }

    #[test]
    fn in_app_links_resolve_titles_within_the_pack() {
        let dir = test_dir("reader-title");
        let library = active_library(&dir.0);
        let moon = library
            .open_reader_title("enwiki".into(), "Moon".into(), false)
            .unwrap()
            .unwrap();
        assert_eq!(moon.page_id, 1);
        // Wikipedia folds only the first letter; links use underscores.
        let tide = library
            .open_reader_title("enwiki".into(), "tide".into(), false)
            .unwrap()
            .unwrap();
        assert_eq!(tide.page_id, 2);
        assert_eq!(
            library.open_reader_title("enwiki".into(), "Not_in_pack".into(), false),
            Ok(None)
        );
        for bad in ["", "   ", "a\u{0}b"] {
            assert_eq!(
                library.open_reader_title("enwiki".into(), bad.into(), false),
                Err(LibraryError::InvalidInput),
                "{bad:?}"
            );
        }
        let inactive = super::library(&dir.0.join("empty"));
        assert_eq!(
            inactive.open_reader_title("enwiki".into(), "Moon".into(), false),
            Err(LibraryError::NotInstalled)
        );
    }
}

/// MOB-003: local (Ollama) article chat with a reader selection and progress.
mod local_chat {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use wiki_ffi::android::{
        discover_local_ai, ChatAnswer, ChatAnswerPart, ChatError, ChatLocality, ChatProgress,
        ChatProgressUpdate, LocalAiError, LocalAiProtocol, LocalChatRequest,
    };

    fn active_library(dir: &Path) -> Arc<OfflineLibrary> {
        let pack = build_pack(dir, "moon-pack");
        let library = library(dir);
        library
            .install_pack(pack.to_string_lossy().into_owned(), true)
            .unwrap();
        library.activate("moon-pack".into()).unwrap();
        library
    }

    /// Read one HTTP request (headers and Content-Length body).
    fn read_request(socket: &mut std::net::TcpStream) -> String {
        let mut data = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = socket.read(&mut buf).unwrap();
            data.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&data).into_owned();
            if let Some(end) = text.find("\r\n\r\n") {
                let length = text[..end]
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .map_or(0, |value| value.trim().parse::<usize>().unwrap());
                if data.len() >= end + 4 + length || n == 0 {
                    return text;
                }
            }
            if n == 0 {
                return text;
            }
        }
    }

    /// A fake Ollama: answers the model list, then one streamed chat reply.
    fn fake_ollama(reply: &'static str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            assert!(read_request(&mut socket).starts_with("GET /api/tags "));
            let body = r#"{"models":[{"name":"qwen2.5:3b"}]}"#;
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            drop(socket);
            let (mut socket, _) = listener.accept().unwrap();
            let request = read_request(&mut socket);
            let mut sse = String::new();
            for piece in reply.split_inclusive(' ') {
                // Test replies contain no quotes or backslashes.
                let content = format!("\"{piece}\"");
                sse.push_str(&format!(
                    "data: {{\"choices\":[{{\"index\":0,\"delta\":{{\"content\":{content}}},\"finish_reason\":null}}]}}\n\n"
                ));
            }
            sse.push_str("data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n");
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{sse}",
                sse.len()
            )
            .unwrap();
            request
        });
        (endpoint, handle)
    }

    #[derive(Default)]
    struct Recorder(Mutex<Vec<ChatProgressUpdate>>);

    impl ChatProgress for Recorder {
        fn on_progress(&self, update: ChatProgressUpdate) {
            self.0.lock().unwrap().push(update);
        }
    }

    #[test]
    fn a_selection_is_sent_to_the_local_model_and_progress_is_reported() {
        let dir = test_dir("local-chat");
        let library = active_library(&dir.0);
        let lead = ArticleKey {
            project: "enwiki".into(),
            page_id: 2,
        }
        .block_id(7002, &[], 0);
        let reply: &'static str =
            Box::leak(format!("Tides follow the Moon's pull. [[cite:{lead}]]").into_boxed_str());
        let (endpoint, server) = fake_ollama(reply);
        let local = discover_local_ai(LocalAiProtocol::Ollama, format!("{endpoint}/")).unwrap();
        assert_eq!(local.models(), vec!["qwen2.5:3b".to_owned()]);
        assert_eq!(local.locality(), ChatLocality::ThisDevice);
        let chat = library
            .prepare_local_chat(
                LocalChatRequest {
                    project: "enwiki".into(),
                    page_id: 2,
                    question: "What does this mean?".into(),
                    server: local.clone(),
                    model: "qwen2.5:3b".into(),
                    thread_id: None,
                    // As rendered and selected in the reader.
                    selection: Some("follow the Moon's\n gravity".into()),
                },
                None,
            )
            .unwrap();
        assert_eq!(chat.destination(), endpoint);
        assert_eq!(chat.locality(), ChatLocality::ThisDevice);
        assert_eq!(chat.selected_passage().unwrap().block_id, lead);
        assert!(chat.prompt_chars() > 50);
        assert_eq!(chat.max_output_tokens(), 1024);
        let progress = Arc::new(Recorder::default());
        let reply = chat.send(true, Some(progress.clone())).unwrap();
        let ChatAnswer::Supported { parts } = reply.answer else {
            panic!("expected a supported answer");
        };
        assert!(parts.iter().any(|part| matches!(
            part,
            ChatAnswerPart::Citation { passage } if passage.block_id == lead
        )));
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /v1/chat/completions "));
        assert!(request.contains(&format!("Evidence ID: {lead}")));
        let updates = progress.0.lock().unwrap().clone();
        assert_eq!(updates.first().unwrap().output_chars, 0);
        assert!(updates.first().unwrap().first_output_ms.is_none());
        let last = updates.last().unwrap();
        assert!(last.finished);
        assert_eq!(last.output_chars, reply_len(&lead));
        assert!(last.first_output_ms.is_some());
        assert!(updates
            .windows(2)
            .all(|pair| pair[0].output_chars <= pair[1].output_chars));
    }

    fn reply_len(lead: &str) -> u64 {
        format!("Tides follow the Moon's pull. [[cite:{lead}]]")
            .chars()
            .count() as u64
    }

    #[test]
    fn a_selection_outside_one_passage_or_an_unlisted_model_is_refused_before_sending() {
        let dir = test_dir("local-chat-refused");
        let library = active_library(&dir.0);
        let (endpoint, _server) = fake_ollama("unused");
        let local = discover_local_ai(LocalAiProtocol::Ollama, endpoint).unwrap();
        let ask = |selection: Option<&str>, model: &str| {
            library.prepare_local_chat(
                LocalChatRequest {
                    project: "enwiki".into(),
                    page_id: 2,
                    question: "Why?".into(),
                    server: local.clone(),
                    model: model.into(),
                    thread_id: None,
                    selection: selection.map(str::to_owned),
                },
                None,
            )
        };
        assert_eq!(
            ask(Some("text that is not in the article"), "qwen2.5:3b").err(),
            Some(ChatError::SelectionNotFound)
        );
        assert_eq!(
            ask(Some("Tides"), "other-model").err(),
            Some(ChatError::InvalidInput)
        );
        assert!(ask(Some("Tides"), "qwen2.5:3b").is_ok());
    }

    /// Live check against a real local server (not run by default):
    /// `FOUNDATION_OLLAMA_URL=http://127.0.0.1:11434 FOUNDATION_OLLAMA_MODEL=qwen2.5:3b
    /// cargo test -p wiki-ffi --test android_library local_smoke -- --ignored --nocapture`
    #[test]
    #[ignore = "needs a running local model server"]
    fn local_smoke() {
        let (Ok(endpoint), Ok(model)) = (
            std::env::var("FOUNDATION_OLLAMA_URL"),
            std::env::var("FOUNDATION_OLLAMA_MODEL"),
        ) else {
            panic!("set FOUNDATION_OLLAMA_URL and FOUNDATION_OLLAMA_MODEL");
        };
        let dir = test_dir("local-smoke");
        let library = active_library(&dir.0);
        let local = discover_local_ai(LocalAiProtocol::Ollama, endpoint).unwrap();
        let chat = library
            .prepare_local_chat(
                LocalChatRequest {
                    project: "enwiki".into(),
                    page_id: 2,
                    question: "What do tides follow?".into(),
                    server: local,
                    model,
                    thread_id: None,
                    selection: Some("Tides follow".into()),
                },
                None,
            )
            .unwrap();
        let progress = Arc::new(Recorder::default());
        let started = std::time::Instant::now();
        let reply = chat.send(true, Some(progress.clone()));
        let updates = progress.0.lock().unwrap().clone();
        println!(
            "took {:?}; prompt {} chars; last progress {:?}; reply {reply:?}",
            started.elapsed(),
            chat.prompt_chars(),
            updates.last()
        );
        assert!(updates.last().is_some_and(|update| update.finished));
        reply.unwrap();
    }

    #[test]
    fn only_private_or_loopback_http_servers_can_be_discovered() {
        for endpoint in [
            "https://127.0.0.1:11434",
            "http://example.com:11434",
            "http://8.8.8.8:11434",
            "ftp://10.0.2.2",
            "",
        ] {
            assert_eq!(
                discover_local_ai(LocalAiProtocol::Ollama, endpoint.into()).err(),
                Some(LocalAiError::InvalidEndpoint),
                "{endpoint}"
            );
        }
    }
}
