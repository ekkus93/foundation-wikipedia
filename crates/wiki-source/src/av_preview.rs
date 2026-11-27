//! MOBPACK-003: audio and video in articles are never fetched or played by
//! the readers. Each player MediaWiki renders (`<video>`/`<audio>` naming
//! its file in `data-mwtitle`) becomes a short note with the kind, length
//! and a link to the file's page on Wikimedia Commons, where it can be
//! played when there is a connection; a video also keeps its still image,
//! which a pack holds like any other image (an A/V preview asset).
//!
//! Runs on untrusted HTML before sanitising, and what it writes is
//! sanitised too. A player without a usable file name is left for the
//! sanitiser, which removes it.

use crate::map_data::attribute;
use crate::media_download::media_file_name;

pub(crate) const AV_NOTE_CLASS: &str = "foundation-av-note";
const MAX_TITLE_BYTES: usize = 255;
/// Longest stated length shown (100 hours); anything else is not shown.
const MAX_SECONDS: u64 = 360_000;

fn tag_end(html: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (at, c) in html[start..].char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(start + at + 1),
            _ => {}
        }
    }
    None
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `95` → `1:35`, `5753` → `1:35:53`.
fn length(seconds: u64) -> String {
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// A Wikimedia file name as `data-mwtitle` gives it (`Some_film.webm`).
fn file_name(tag: &str) -> Option<String> {
    let name = attribute(tag, "data-mwtitle")?;
    let name = name.trim().replace(' ', "_");
    let valid = !name.is_empty()
        && name.len() <= MAX_TITLE_BYTES
        && name.contains('.')
        && !name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '#' | '|' | '<' | '>' | '"' | '?'));
    valid.then_some(name)
}

/// The Commons page of `name`, percent-encoded.
fn commons_page(name: &str) -> Option<String> {
    let mut url = url::Url::parse("https://commons.wikimedia.org/wiki/").ok()?;
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .push(&format!("File:{name}"));
    Some(url.to_string())
}

/// The still image a video player names, if it is this file's own
/// Wikimedia rendering.
pub(crate) fn poster(tag: &str, name: &str) -> Option<String> {
    let poster = attribute(tag, "poster")?;
    let absolute = match poster.strip_prefix("//") {
        Some(rest) => format!("https://{rest}"),
        None => poster,
    };
    // The query (`utm_*` tracking) is not part of the file.
    let absolute = absolute.split(['?', '#']).next()?.to_owned();
    (media_file_name(&absolute).as_deref() == Some(name)).then_some(absolute)
}

fn preview(tag: &str, video: bool) -> Option<String> {
    let name = file_name(tag)?;
    // A file kept on the wiki itself (not Commons) has no page the reader
    // can name here: it gets the note without a link.
    let page = attribute(tag, "data-mwprovider")
        .is_none_or(|provider| provider == "wikimediacommons")
        .then(|| commons_page(&name))
        .flatten();
    let mut out = String::new();
    if video {
        if let Some(still) = poster(tag, &name) {
            out.push_str(&format!(
                "<img src=\"{}\" alt=\"Video still\">",
                escape(&still)
            ));
        }
    }
    out.push_str(&format!(
        "<span class=\"{AV_NOTE_CLASS}\">{}",
        if video { "Video" } else { "Audio" }
    ));
    if let Some(seconds) = attribute(tag, "data-durationhint")
        .and_then(|hint| hint.trim().parse::<f64>().ok())
        .filter(|seconds| seconds.is_finite() && *seconds >= 1.0)
        .map(|seconds| seconds.round() as u64)
        .filter(|seconds| *seconds <= MAX_SECONDS)
    {
        out.push_str(&format!(" · {}", length(seconds)));
    }
    match page {
        Some(page) => out.push_str(&format!(
            " · <a href=\"{}\">Play online (needs Internet)</a></span>",
            escape(&page)
        )),
        None => out.push_str(" · online only</span>"),
    }
    Some(out)
}

/// The start of each `<video`/`<audio` tag in `lower` (ASCII-lowercased
/// HTML, so offsets match the original), in order.
fn players(lower: &str) -> Vec<(usize, bool)> {
    let mut found = Vec::new();
    for (needle, video) in [("<video", true), ("<audio", false)] {
        let mut at = 0;
        while let Some(start) = lower[at..].find(needle).map(|i| at + i) {
            at = start + needle.len();
            // `<videos>` is not a player.
            if lower[at..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_whitespace() || c == '>' || c == '/')
            {
                found.push((start, video));
            }
        }
    }
    found.sort_unstable();
    found
}

/// Replace each audio or video player that names its file with its preview.
pub(crate) fn with_av_previews(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    if !lower.contains("<video") && !lower.contains("<audio") {
        return html.to_owned();
    }
    let mut out = String::with_capacity(html.len());
    let mut at = 0;
    for (start, video) in players(&lower) {
        if start < at {
            continue; // inside a player already replaced
        }
        let Some(open_end) = tag_end(html, start) else {
            break;
        };
        let Some(preview) = preview(&html[start..open_end], video) else {
            continue;
        };
        let close = if video { "</video>" } else { "</audio>" };
        let end = lower[open_end..]
            .find(close)
            .map_or(open_end, |i| open_end + i + close.len());
        out.push_str(&html[at..start]);
        out.push_str(&preview);
        at = end;
    }
    out.push_str(&html[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO: &str = concat!(
        "<figure typeof=\"mw:File/Thumb\"><span><video id=\"mwe_player_0\" ",
        "poster=\"//thumb.wikimedia.org/wikipedia/commons/thumb/2/24/Night_of_the_Living_Dead_%281968%29.webm/250px-Night_of_the_Living_Dead_%281968%29.webm.jpg?utm_source=simple.wikipedia.org&amp;utm_campaign=parser\" ",
        "controls=\"\" preload=\"none\" class=\"mw-file-element\" width=\"250\" height=\"188\" ",
        "data-durationhint=\"5753\" data-mwtitle=\"Night_of_the_Living_Dead_(1968).webm\" ",
        "resource=\"/wiki/File:Night_of_the_Living_Dead_(1968).webm\">",
        "<source src=\"//upload.wikimedia.org/wikipedia/commons/transcoded/2/24/x.webm/x.webm.240p.vp9.webm\" type=\"video/webm\" />",
        "</video></span><figcaption>A whole film</figcaption></figure>",
    );
    const AUDIO: &str = concat!(
        "<figure><span><audio id=\"mwe_player_1\" controls=\"\" preload=\"none\" class=\"mw-file-element\" ",
        "data-durationhint=\"88\" data-mwtitle=\"Pronunciation_of_Latin_(Text).ogg\">",
        "<source src=\"//upload.wikimedia.org/wikipedia/commons/transcoded/1/12/x.ogg/x.ogg.mp3\" type=\"audio/mpeg\" />",
        "</audio></span><figcaption>Spoken Latin</figcaption></figure>",
    );

    #[test]
    fn a_video_becomes_its_still_and_a_note_with_length_and_commons_link() {
        let html = with_av_previews(VIDEO);
        assert_eq!(
            html,
            concat!(
                "<figure typeof=\"mw:File/Thumb\"><span>",
                "<img src=\"https://thumb.wikimedia.org/wikipedia/commons/thumb/2/24/Night_of_the_Living_Dead_%281968%29.webm/250px-Night_of_the_Living_Dead_%281968%29.webm.jpg\" alt=\"Video still\">",
                "<span class=\"foundation-av-note\">Video · 1:35:53 · ",
                "<a href=\"https://commons.wikimedia.org/wiki/File:Night_of_the_Living_Dead_(1968).webm\">Play online (needs Internet)</a></span>",
                "</span><figcaption>A whole film</figcaption></figure>",
            )
        );
    }

    #[test]
    fn audio_becomes_a_note_only() {
        let html = with_av_previews(AUDIO);
        assert!(!html.contains("<img"), "{html}");
        assert!(html.contains(concat!(
            "<span class=\"foundation-av-note\">Audio · 1:28 · ",
            "<a href=\"https://commons.wikimedia.org/wiki/File:Pronunciation_of_Latin_(Text).ogg\">",
        )));
        for gone in ["<audio", "<source", ".mp3", "preload"] {
            assert!(!html.contains(gone), "{gone}: {html}");
        }
        assert!(html.contains("<figcaption>Spoken Latin</figcaption>"));
    }

    #[test]
    fn a_player_without_a_usable_file_name_is_left_for_the_sanitiser() {
        for html in [
            "<video src=\"https://upload.wikimedia.org/v.webm\"></video>",
            "<audio data-mwtitle=\"\"></audio>",
            "<video data-mwtitle=\"a/b.webm\"></video>",
            "<video data-mwtitle=\"x&quot;&gt;&lt;script&gt;.webm\"></video>",
            "<videos data-mwtitle=\"x.webm\"></videos>",
        ] {
            assert_eq!(with_av_previews(html), html);
        }
    }

    #[test]
    fn a_poster_of_another_file_or_host_is_not_shown_and_odd_lengths_are_not_stated() {
        for poster in [
            "https://example.org/wikipedia/commons/thumb/a/ab/Film.webm/250px-Film.webm.jpg",
            "//thumb.wikimedia.org/wikipedia/commons/thumb/a/ab/Other.webm/250px-Other.webm.jpg",
            "javascript:alert(1)",
        ] {
            let html = with_av_previews(&format!(
                "<video poster=\"{poster}\" data-durationhint=\"-4\" data-mwtitle=\"Film.webm\"></video>"
            ));
            assert_eq!(
                html,
                "<span class=\"foundation-av-note\">Video · <a href=\"https://commons.wikimedia.org/wiki/File:Film.webm\">Play online (needs Internet)</a></span>"
            );
        }
        for hint in ["NaN", "1e99", "0", "abc"] {
            let html = with_av_previews(&format!(
                "<audio data-durationhint=\"{hint}\" data-mwtitle=\"A.ogg\"></audio>"
            ));
            assert!(html.contains(">Audio · <a "), "{hint}: {html}");
        }
    }

    #[test]
    fn a_file_kept_on_the_wiki_itself_gets_a_note_without_a_link() {
        let html = with_av_previews(
            "<audio data-durationhint=\"30\" data-mwtitle=\"Clip.ogg\" data-mwprovider=\"local\"></audio>",
        );
        assert_eq!(
            html,
            "<span class=\"foundation-av-note\">Audio · 0:30 · online only</span>"
        );
    }

    #[test]
    fn names_are_escaped_and_encoded_and_several_players_keep_their_order() {
        let html = with_av_previews(concat!(
            "<AUDIO data-mwtitle=\"Tom &amp; Jerry 100%.ogg\"></AUDIO><p>between</p>",
            "<video data-mwtitle=\"Ünï.webm\">never closed",
        ));
        assert!(html.starts_with("<span class=\"foundation-av-note\">Audio · <a href=\"https://commons.wikimedia.org/wiki/File:Tom_&amp;_Jerry_100%25.ogg\">"), "{html}");
        assert!(html.contains("<p>between</p><span class=\"foundation-av-note\">Video · <a href=\"https://commons.wikimedia.org/wiki/File:%C3%9Cn%C3%AF.webm\">"), "{html}");
        assert!(html.ends_with("never closed"));
    }
}
