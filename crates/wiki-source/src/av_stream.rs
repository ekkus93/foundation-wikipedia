//! MOBPACK-003: where Wikimedia serves an audio or video file. Packs hold no
//! audio or video, only the "Play online" link to the file's Commons page;
//! the addresses to stream are worked out from the file name, the way
//! Wikimedia lays out its media store (two directories from the MD5 of the
//! name). Mirrors `AvStream.kt` on Android. Pure: no network.

use md5::{Digest, Md5};

/// The only host a stream address is ever on.
pub const AV_HOST: &str = "upload.wikimedia.org";
const STORE: &str = "https://upload.wikimedia.org/wikipedia/commons";
const PAGE_PREFIX: &str = "https://commons.wikimedia.org/wiki/File:";
const MAX_NAME_LENGTH: usize = 255;
const VIDEO: [&str; 5] = ["webm", "ogv", "mpg", "mpeg", "mp4"];
const AUDIO: [&str; 7] = ["ogg", "oga", "opus", "mp3", "wav", "flac", "mid"];

/// An audio or video file on Wikimedia Commons, by its file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvFile {
    /// File name with underscores for spaces.
    pub name: String,
    pub video: bool,
}

impl AvFile {
    /// The name as a reader sees it.
    pub fn title(&self) -> String {
        self.name.replace('_', " ")
    }
}

/// Which player the addresses are ordered for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamOrder {
    /// Phones: the 360p MPEG-4 rendering first, then VP9 WebM.
    Android,
    /// Desktop webviews: VP9 WebM first (they may not decode MPEG-4 Part 2).
    Desktop,
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return None;
            }
            out.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The file a "Play online" link names, or `None` for any other link.
pub fn from_commons_page(url: &str) -> Option<AvFile> {
    // Exactly `https://commons.wikimedia.org/wiki/File:`: no other scheme,
    // host case, port or userinfo can match this prefix.
    let rest = url.strip_prefix(PAGE_PREFIX)?;
    // A fragment is never sent to a server; a query is not accepted.
    let rest = rest.split('#').next().unwrap_or_default();
    if rest.contains('?') || rest.bytes().any(|b| b == b' ' || b.is_ascii_control()) {
        return None;
    }
    let name = percent_decode(rest)?.replace(' ', "_");
    let valid = !name.is_empty()
        && name.encode_utf16().count() <= MAX_NAME_LENGTH
        && !name
            .chars()
            .any(|c| c.is_control() || "/#|<>\"?".contains(c));
    if !valid {
        return None;
    }
    let extension = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default();
    if VIDEO.contains(&extension.as_str()) {
        Some(AvFile { name, video: true })
    } else if AUDIO.contains(&extension.as_str()) {
        Some(AvFile { name, video: false })
    } else {
        None
    }
}

fn encode(name: &str) -> String {
    let mut out = String::new();
    for &b in name.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Addresses to try, in order: renderings first, then the file itself. All
/// are on [`AV_HOST`].
pub fn candidates(file: &AvFile, order: StreamOrder) -> Vec<String> {
    let digest = Md5::digest(file.name.as_bytes());
    let directory = format!("{:x}/{:02x}", digest[0] >> 4, digest[0]);
    let name = encode(&file.name);
    let original = format!("{STORE}/{directory}/{name}");
    let renderings: &[&str] = if file.video {
        match order {
            StreamOrder::Android => &[
                "360p.mpeg4.mov",
                "480p.vp9.webm",
                "360p.vp9.webm",
                "240p.vp9.webm",
            ],
            StreamOrder::Desktop => &[
                "480p.vp9.webm",
                "360p.vp9.webm",
                "240p.vp9.webm",
                "360p.mpeg4.mov",
            ],
        }
    } else if file.name.to_lowercase().ends_with(".mp3") {
        &[]
    } else {
        &["mp3"]
    };
    renderings
        .iter()
        .map(|r| format!("{STORE}/transcoded/{directory}/{name}/{name}.{r}"))
        .chain(std::iter::once(original))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, video: bool) -> AvFile {
        AvFile {
            name: name.to_owned(),
            video,
        }
    }

    #[test]
    fn a_play_online_link_names_its_file_and_kind() {
        assert_eq!(
            from_commons_page(
                "https://commons.wikimedia.org/wiki/File:Night_of_the_Living_Dead_(1968).webm"
            ),
            Some(file("Night_of_the_Living_Dead_(1968).webm", true))
        );
        let tom =
            from_commons_page("https://commons.wikimedia.org/wiki/File:Tom_&_Jerry_100%25.OGG");
        assert_eq!(tom, Some(file("Tom_&_Jerry_100%.OGG", false)));
        assert_eq!(tom.unwrap().title(), "Tom & Jerry 100%.OGG");
        assert_eq!(
            from_commons_page("https://commons.wikimedia.org/wiki/File:%C3%9Cn%20x.opus"),
            Some(file("Ün_x.opus", false))
        );
    }

    #[test]
    fn other_links_and_hostile_names_are_rejected() {
        for other in [
            "https://commons.wikimedia.org/wiki/File:Moon.jpg",
            "https://commons.wikimedia.org/wiki/File:NoExtension",
            "https://commons.wikimedia.org/wiki/File:",
            "https://commons.wikimedia.org/wiki/Category:Films.webm",
            "https://commons.wikimedia.org/wiki/File:a%2Fb.webm",
            "https://commons.wikimedia.org/wiki/File:a%2fb.webm",
            "https://commons.wikimedia.org/wiki/File:..%2F..%2Fetc.ogg",
            "https://commons.wikimedia.org/wiki/File:a%00b.webm",
            "https://commons.wikimedia.org/wiki/File:a%0Ab.webm",
            "https://commons.wikimedia.org/wiki/File:a%3Fb.webm",
            "https://commons.wikimedia.org/wiki/File:a%23b.webm",
            "https://commons.wikimedia.org/wiki/File:a%7Cb.webm",
            "https://commons.wikimedia.org/wiki/File:a%22b.webm",
            "https://commons.wikimedia.org/wiki/File:a%3Cb.webm",
            "https://commons.wikimedia.org/wiki/File:a%FFb.webm",
            "https://commons.wikimedia.org/wiki/File:a%zzb.webm",
            "https://commons.wikimedia.org/wiki/File:a%2",
            "https://commons.wikimedia.org/wiki/File:Film.webm?action=raw",
            "https://commons.wikimedia.org:8443/wiki/File:Film.webm",
            "https://user@commons.wikimedia.org/wiki/File:Film.webm",
            "https://commons.wikimedia.org@evil.example/wiki/File:Film.webm",
            "http://commons.wikimedia.org/wiki/File:Film.webm",
            "https://example.org/wiki/File:Film.webm",
            "https://COMMONS.wikimedia.org/wiki/File:Film.webm",
            "https://commons.wikimedia.org.evil.example/wiki/File:Film.webm",
            "not a url",
            "",
        ] {
            assert_eq!(from_commons_page(other), None, "{other}");
        }
        let long = format!(
            "https://commons.wikimedia.org/wiki/File:{}.webm",
            "a".repeat(251)
        );
        assert_eq!(from_commons_page(&long), None);
        let fits = format!(
            "https://commons.wikimedia.org/wiki/File:{}.webm",
            "a".repeat(250)
        );
        assert!(from_commons_page(&fits).is_some());
    }

    /// Checked against Wikimedia on 2026-11-26 (the MPEG-4 one on 2026-11-27).
    #[test]
    fn android_order_is_mpeg4_first_then_the_file_itself() {
        let film = "Night_of_the_Living_Dead_%281968%29.webm";
        assert_eq!(
            candidates(
                &file("Night_of_the_Living_Dead_(1968).webm", true),
                StreamOrder::Android
            ),
            vec![
                format!("{STORE}/transcoded/2/24/{film}/{film}.360p.mpeg4.mov"),
                format!("{STORE}/transcoded/2/24/{film}/{film}.480p.vp9.webm"),
                format!("{STORE}/transcoded/2/24/{film}/{film}.360p.vp9.webm"),
                format!("{STORE}/transcoded/2/24/{film}/{film}.240p.vp9.webm"),
                format!("{STORE}/2/24/{film}"),
            ]
        );
    }

    #[test]
    fn desktop_order_puts_webm_first() {
        let film = "Night_of_the_Living_Dead_%281968%29.webm";
        assert_eq!(
            candidates(
                &file("Night_of_the_Living_Dead_(1968).webm", true),
                StreamOrder::Desktop
            ),
            vec![
                format!("{STORE}/transcoded/2/24/{film}/{film}.480p.vp9.webm"),
                format!("{STORE}/transcoded/2/24/{film}/{film}.360p.vp9.webm"),
                format!("{STORE}/transcoded/2/24/{film}/{film}.240p.vp9.webm"),
                format!("{STORE}/transcoded/2/24/{film}/{film}.360p.mpeg4.mov"),
                format!("{STORE}/2/24/{film}"),
            ]
        );
    }

    #[test]
    fn audio_prefers_an_mp3_rendering_unless_it_is_one() {
        let speech = "Pronunciation_of_Latin_%28Text%29.ogg";
        for order in [StreamOrder::Android, StreamOrder::Desktop] {
            assert_eq!(
                candidates(&file("Pronunciation_of_Latin_(Text).ogg", false), order),
                vec![
                    format!("{STORE}/transcoded/1/12/{speech}/{speech}.mp3"),
                    format!("{STORE}/1/12/{speech}"),
                ]
            );
            assert_eq!(candidates(&file("Song.mp3", false), order).len(), 1);
            assert_eq!(candidates(&file("Song.MP3", false), order).len(), 1);
        }
    }

    #[test]
    fn every_stream_is_on_wikimedias_media_host_whatever_the_name() {
        for name in [
            "Ünï cödé.webm",
            "a b&c=d?e#f.ogg",
            "x\"y<z>.webm",
            "..%2F..%2Fetc.ogg",
        ] {
            for order in [StreamOrder::Android, StreamOrder::Desktop] {
                for url in candidates(&file(name, name.ends_with("webm")), order) {
                    let path = url
                        .strip_prefix(&format!("https://{AV_HOST}/wikipedia/commons/"))
                        .unwrap_or_else(|| panic!("{url}"));
                    assert!(
                        path.chars()
                            .all(|c| c.is_ascii_alphanumeric() || "-_.~%/".contains(c)),
                        "{url}"
                    );
                    assert!(!path.contains("../"), "{url}");
                }
            }
        }
    }
}
