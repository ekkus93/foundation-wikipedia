//! PACK-003: an article's interactive (Kartographer) maps, drawn offline.
//!
//! Wikimedia Maps' terms forbid third-party apps and offline downloading,
//! so a pack never uses `maps.wikimedia.org`. Instead the pack builder
//! draws each map from open map data (Natural Earth, OpenStreetMap
//! extracts) plus the article's own markers and shapes, which the Action
//! API serves as GeoJSON (`prop=mapdata`).
//!
//! [`scan_maps`] is the one scanner both the builder and the reader use, so
//! the n-th map drawn is the n-th map shown.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::online_requests::map_group_id;

/// One interactive map as the article's rendered HTML describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct MapFrame {
    /// The stated centre, if any; otherwise the map fits its shapes.
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub zoom: Option<u8>,
    /// Size in CSS pixels as shown in the article.
    pub width: u32,
    pub height: u32,
    /// Content-hashed GeoJSON groups the map draws (`data-overlays`).
    pub groups: Vec<String>,
}

/// Where a map element sits in the HTML.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MapSpan<'a> {
    /// `<a class="mw-kartographer-map" …>…</a>`: the whole element.
    Frame {
        start: usize,
        end: usize,
        tag: &'a str,
    },
    /// An inline map link: the element and its text.
    Link {
        start: usize,
        end: usize,
        text: &'a str,
    },
}

/// The interactive maps and map links of `html`, in document order.
pub(crate) fn scan_maps(html: &str) -> Vec<MapSpan<'_>> {
    let mut spans = Vec::new();
    if !html.contains("mw-kartographer-") {
        return spans;
    }
    let mut at = 0;
    while let Some(found) = html[at..].find("<a ") {
        let start = at + found;
        let Some(tag_len) = html[start..].find('>') else {
            break;
        };
        let tag = &html[start..start + tag_len + 1];
        let class = attribute(tag, "class").unwrap_or_default();
        let classes: Vec<&str> = class.split_ascii_whitespace().collect();
        let frame = classes.contains(&"mw-kartographer-map");
        let link = classes.contains(&"mw-kartographer-maplink");
        if !frame && !link {
            at = start + tag.len();
            continue;
        }
        let Some(close) = html[start..].find("</a>").map(|found| start + found) else {
            break;
        };
        let end = close + "</a>".len();
        spans.push(if frame {
            MapSpan::Frame { start, end, tag }
        } else {
            MapSpan::Link {
                start,
                end,
                text: &html[start + tag.len()..close],
            }
        });
        at = end;
    }
    spans
}

/// Each interactive map of `html`, in document order.
pub fn interactive_maps(html: &str) -> Vec<MapFrame> {
    scan_maps(html)
        .into_iter()
        .filter_map(|span| match span {
            MapSpan::Frame { tag, .. } => Some(map_frame(tag)),
            MapSpan::Link { .. } => None,
        })
        .collect()
}

pub(crate) fn map_frame(tag: &str) -> MapFrame {
    let number = |name: &str, limit: f64| {
        attribute(tag, name)
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && value.abs() <= limit)
    };
    let size = |name: &str| {
        attribute(tag, name)
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|size| (32..=2000).contains(size))
    };
    let groups = attribute(tag, "data-overlays")
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|group| map_group_id(group))
        .collect();
    MapFrame {
        lat: number("data-lat", 85.0),
        lon: number("data-lon", 180.0),
        zoom: attribute(tag, "data-zoom")
            .and_then(|value| value.parse::<u8>().ok())
            .filter(|zoom| *zoom <= 19),
        width: size("data-width").unwrap_or(300),
        height: size("data-height").unwrap_or(300),
        groups,
    }
}

/// A double-quoted attribute's value in one start tag (as MediaWiki
/// writes them), entity-decoded only for `&quot;` and `&amp;`.
pub(crate) fn attribute(tag: &str, name: &str) -> Option<String> {
    let needle = format!(" {name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = start + tag[start..].find('"')?;
    Some(
        tag[start..end]
            .replace("&quot;", "\"")
            .replace("&amp;", "&"),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapDataError {
    InvalidResponse,
    /// The reply is for another page.
    WrongPage,
    /// A requested group is missing (the article's maps changed since the
    /// pinned revision) or an unrequested one is present.
    GroupMismatch(String),
}

/// GeoJSON features of each requested group, from a `prop=mapdata` reply.
/// Exactly `groups` must be present: a group ID is a hash of its content,
/// so a match means the reply describes the pinned revision's maps.
pub fn parse_map_data(
    body: &str,
    page_id: u64,
    groups: &[String],
) -> Result<BTreeMap<String, Vec<Value>>, MapDataError> {
    let invalid = || MapDataError::InvalidResponse;
    let reply: Value = serde_json::from_str(body).map_err(|_| invalid())?;
    let pages = reply
        .pointer("/query/pages")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let [page] = pages.as_slice() else {
        return Err(invalid());
    };
    if page.get("pageid").and_then(Value::as_u64) != Some(page_id) {
        return Err(MapDataError::WrongPage);
    }
    let mut found = BTreeMap::new();
    for chunk in page
        .get("mapdata")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?
    {
        let object: serde_json::Map<String, Value> =
            serde_json::from_str(chunk.as_str().ok_or_else(invalid)?).map_err(|_| invalid())?;
        for (group, features) in object {
            if !groups.contains(&group) {
                return Err(MapDataError::GroupMismatch(group));
            }
            let features = match features {
                Value::Array(features) => features,
                single @ Value::Object(_) => vec![single],
                _ => return Err(invalid()),
            };
            if found.insert(group.clone(), features).is_some() {
                return Err(MapDataError::GroupMismatch(group));
            }
        }
    }
    if let Some(missing) = groups.iter().find(|group| !found.contains_key(*group)) {
        return Err(MapDataError::GroupMismatch(missing.clone()));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EIFFEL: &str = "<p>x</p><a class=\"mw-kartographer-map notheme mw-kartographer-container center\" \
        data-mw-kartographer=\"mapframe\" data-style=\"osm-intl\" data-width=\"250\" data-height=\"200\" \
        data-zoom=\"16\" data-overlays=\"[&quot;_477ba0a3c627b1ed526835dae69bd369beac7de4&quot;]\">\
        <img src=\"https://maps.wikimedia.org/img/osm-intl,16,a,a,250x200.png\"></a>\
        <a class=\"mw-kartographer-maplink\" data-lat=\"1\" data-lon=\"2\">1°N</a>\
        <a class=\"mw-kartographer-map\" data-lat=\"35.68\" data-lon=\"139.41\" data-zoom=\"8\" \
        data-width=\"300\" data-height=\"200\"><img src=\"x\"></a>";

    /// The real simplewiki reply for Eiffel Tower (page 5246), 2026-11-17.
    const EIFFEL_DATA: &str = "{\"batchcomplete\":true,\"query\":{\"pages\":[{\"pageid\":5246,\"ns\":0,\"title\":\"Eiffel Tower\",\"mapdata\":[\"{\\\"_477ba0a3c627b1ed526835dae69bd369beac7de4\\\":[{\\\"type\\\":\\\"Feature\\\",\\\"geometry\\\":{\\\"coordinates\\\":[2.2944999999999998,48.858222222222224],\\\"type\\\":\\\"Point\\\"},\\\"properties\\\":{\\\"title\\\":\\\"Eiffel Tower\\\",\\\"marker-color\\\":\\\"#5E74F3\\\"}}]}\"]}]}}";

    #[test]
    fn maps_are_found_in_document_order_with_their_settings() {
        let maps = interactive_maps(EIFFEL);
        assert_eq!(maps.len(), 2, "map links are not maps");
        assert_eq!(
            maps[0],
            MapFrame {
                lat: None,
                lon: None,
                zoom: Some(16),
                width: 250,
                height: 200,
                groups: vec!["_477ba0a3c627b1ed526835dae69bd369beac7de4".into()],
            }
        );
        assert_eq!(
            (maps[1].lat, maps[1].lon, maps[1].zoom),
            (Some(35.68), Some(139.41), Some(8))
        );
        assert!(maps[1].groups.is_empty());
        assert_eq!(scan_maps(EIFFEL).len(), 3);
        assert!(interactive_maps("<p>no maps</p>").is_empty());
    }

    #[test]
    fn map_data_must_be_exactly_the_requested_groups_of_the_page() {
        let group = "_477ba0a3c627b1ed526835dae69bd369beac7de4".to_owned();
        let data = parse_map_data(EIFFEL_DATA, 5246, std::slice::from_ref(&group)).unwrap();
        let point = &data[&group][0];
        assert_eq!(point["geometry"]["type"], "Point");
        assert_eq!(point["properties"]["marker-color"], "#5E74F3");
        assert_eq!(
            parse_map_data(EIFFEL_DATA, 5247, std::slice::from_ref(&group)),
            Err(MapDataError::WrongPage)
        );
        let other = "_0000000000000000000000000000000000000000".to_owned();
        assert_eq!(
            parse_map_data(EIFFEL_DATA, 5246, &[group.clone(), other.clone()]),
            Err(MapDataError::GroupMismatch(other.clone()))
        );
        assert_eq!(
            parse_map_data(EIFFEL_DATA, 5246, std::slice::from_ref(&other)),
            Err(MapDataError::GroupMismatch(group))
        );
        assert_eq!(
            parse_map_data("{}", 5246, &[]),
            Err(MapDataError::InvalidResponse)
        );
    }
}
