//! PACK-003: draw an article's interactive map as SVG from open map data,
//! so packs need neither Wikimedia Maps nor any network to show it.
//!
//! * **Base map:** Natural Earth (public domain) vector layers, pinned by
//!   SHA-256 ([`NATURAL_EARTH_FILES`]): land, lakes, rivers, country and
//!   state borders, and place names. The 1:50m layers serve zoom 0–4, the
//!   1:10m layers zoom 5 and closer.
//! * **The article's map data:** its markers, lines and shapes (GeoJSON
//!   from the Action API, `wiki_source::map_data`), with their simplestyle
//!   colours. Shapes Kartographer loads from Wikimedia Maps (`ExternalData`)
//!   are not drawn and are counted in [`DrawnMap::omitted_shapes`].
//!
//! The map uses Web Mercator at the article's centre, zoom and size, as
//! Kartographer does, so it shows the same area. Geometry is clipped to the
//! frame and thinned below half a pixel. The SVG then goes through the pack
//! SVG sanitiser like any other SVG.

use std::fmt::Write;
use std::fs;
use std::path::Path;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use crate::osm_extract::{FeatureKind, OsmFeature, OsmPlace, OsmShape};
use wiki_source::map_data::MapFrame;

/// The Natural Earth v5.1.2 GeoJSON files the base map uses, with their
/// SHA-256, from <https://github.com/nvkelso/natural-earth-vector/tree/v5.1.2/geojson>.
pub const NATURAL_EARTH_FILES: [(&str, &str); 12] = [
    (
        "ne_50m_land.geojson",
        "e874b27a51d146452be360cafb3cc50c86001074a67d534113e6534682f9826b",
    ),
    (
        "ne_50m_lakes.geojson",
        "d350b75978b26fe839b797c2c529b2fb8f47fb3983c03f4964e36d5df9378a52",
    ),
    (
        "ne_50m_rivers_lake_centerlines.geojson",
        "f286e0ce978fde999ca2d7a78c764be08542e19b63cded52b05c12d5173ccc51",
    ),
    (
        "ne_50m_admin_0_boundary_lines_land.geojson",
        "2faac4f6b34386f3d21b6e018cf151f241f00e5c936d44dd17d7d9bfb147fa48",
    ),
    (
        "ne_50m_admin_1_states_provinces_lines.geojson",
        "72cca93c850d412628a5da4bc5ebfe21ba4d376eb34611bde6b623ee73f0fdcf",
    ),
    (
        "ne_50m_populated_places_simple.geojson",
        "8e70756b39fae9bcdc1e332bfc510c024c5edd3a13203ffd20092ee37b61d978",
    ),
    (
        "ne_10m_land.geojson",
        "1ac90796408bc6ad6911d69448485d3c4dbf2190370080368a09976e1c9f7416",
    ),
    (
        "ne_10m_lakes.geojson",
        "2d036f53dedec578001c5c30c2959ee7d4eebc1306900fa4367c49929ec8f2d9",
    ),
    (
        "ne_10m_rivers_lake_centerlines.geojson",
        "bb854a900ecbd3b408df46d5e16e3e0f974ba55993f9d8b5c26e855273c0905a",
    ),
    (
        "ne_10m_admin_0_boundary_lines_land.geojson",
        "74d9c16229c095fde65943a9919e337682f044bcebccb120764f38edf3b70f4a",
    ),
    (
        "ne_10m_admin_1_states_provinces_lines.geojson",
        "1a1f30ccaaf4cc9c4bde34266f0b8cbb955d3a4cf254b756912255f2ec7c75b6",
    ),
    (
        "ne_10m_populated_places_simple.geojson",
        "fd3fa867a320cbd5c5b6bb5bc550afeec2939fb2cef688e508007282a55ac42f",
    ),
];

/// Licence and credit of a map drawn from Natural Earth and article data.
pub const MAP_LICENSE: &str =
    "Base map: Natural Earth (public domain). Markers and shapes: the article's map data (CC BY-SA 4.0)";
pub const MAP_CREATOR: &str = "Natural Earth; Wikipedia contributors";
/// The same, for a map that also draws OpenStreetMap data (ODbL 1.0).
pub const MAP_LICENSE_OSM: &str =
    "Map data © OpenStreetMap contributors (ODbL 1.0, openstreetmap.org/copyright). Base map: Natural Earth (public domain). Markers and shapes: the article's map data (CC BY-SA 4.0)";
pub const MAP_CREATOR_OSM: &str =
    "OpenStreetMap contributors; Natural Earth; Wikipedia contributors";

const WATER: &str = "#aad3df";
const LAND: &str = "#f2efe9";

/// Lines or rings in longitude/latitude, with their bounding box.
#[derive(Debug)]
struct Shape {
    bounds: [f64; 4],
    parts: Vec<Vec<[f64; 2]>>,
}

#[derive(Debug)]
struct Place {
    at: [f64; 2],
    name: String,
    min_zoom: f64,
}

#[derive(Debug, Default)]
struct Layers {
    land: Vec<Shape>,
    lakes: Vec<Shape>,
    rivers: Vec<Shape>,
    borders: Vec<Shape>,
    states: Vec<Shape>,
    places: Vec<Place>,
}

/// The pinned Natural Earth base map, loaded once per build.
#[derive(Debug)]
pub struct NaturalEarth {
    coarse: Layers,
    fine: Layers,
}

impl NaturalEarth {
    /// Load and verify [`NATURAL_EARTH_FILES`] from `dir`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let mut layers: Vec<Value> = Vec::with_capacity(NATURAL_EARTH_FILES.len());
        for (name, pin) in NATURAL_EARTH_FILES {
            let bytes = fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
            let digest = format!("{:x}", Sha256::digest(&bytes));
            if digest != pin {
                return Err(format!("{name}: SHA-256 {digest} is not the pinned {pin}"));
            }
            layers.push(serde_json::from_slice(&bytes).map_err(|e| format!("{name}: {e}"))?);
        }
        let set = |offset: usize| -> Result<Layers, String> {
            Ok(Layers {
                land: shapes(&layers[offset])?,
                lakes: shapes(&layers[offset + 1])?,
                rivers: shapes(&layers[offset + 2])?,
                borders: shapes(&layers[offset + 3])?,
                states: shapes(&layers[offset + 4])?,
                places: places(&layers[offset + 5])?,
            })
        };
        Ok(Self {
            coarse: set(0)?,
            fine: set(6)?,
        })
    }
}

/// Coordinates as `[lon, lat]`, finite and in range.
fn position(value: &Value) -> Option<[f64; 2]> {
    let pair = value.as_array()?;
    let lon = pair.first()?.as_f64()?;
    let lat = pair.get(1)?.as_f64()?;
    (lon.is_finite() && lat.is_finite() && lon.abs() <= 180.0 && lat.abs() <= 90.0)
        .then_some([lon, lat])
}

fn line(value: &Value) -> Option<Vec<[f64; 2]>> {
    value.as_array()?.iter().map(position).collect()
}

/// The lines or rings of a geometry; points are ignored.
fn geometry_parts(geometry: &Value) -> Option<Vec<Vec<[f64; 2]>>> {
    let coordinates = geometry.get("coordinates")?;
    let lines = |value: &Value| -> Option<Vec<Vec<[f64; 2]>>> {
        value.as_array()?.iter().map(line).collect()
    };
    match geometry.get("type")?.as_str()? {
        "LineString" => Some(vec![line(coordinates)?]),
        "MultiLineString" | "Polygon" => lines(coordinates),
        "MultiPolygon" => {
            let mut parts = Vec::new();
            for polygon in coordinates.as_array()? {
                parts.extend(lines(polygon)?);
            }
            Some(parts)
        }
        _ => Some(Vec::new()),
    }
}

fn bounds(parts: &[Vec<[f64; 2]>]) -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for point in parts.iter().flatten() {
        b = [
            b[0].min(point[0]),
            b[1].min(point[1]),
            b[2].max(point[0]),
            b[3].max(point[1]),
        ];
    }
    b
}

fn shapes(collection: &Value) -> Result<Vec<Shape>, String> {
    let features = collection
        .get("features")
        .and_then(Value::as_array)
        .ok_or("not a FeatureCollection")?;
    Ok(features
        .iter()
        .filter_map(|feature| geometry_parts(feature.get("geometry")?))
        .filter(|parts| !parts.is_empty())
        .map(|parts| Shape {
            bounds: bounds(&parts),
            parts,
        })
        .collect())
}

fn places(collection: &Value) -> Result<Vec<Place>, String> {
    let features = collection
        .get("features")
        .and_then(Value::as_array)
        .ok_or("not a FeatureCollection")?;
    Ok(features
        .iter()
        .filter_map(|feature| {
            let properties = feature.get("properties")?;
            Some(Place {
                at: position(feature.pointer("/geometry/coordinates")?)?,
                name: properties.get("name")?.as_str()?.to_owned(),
                min_zoom: properties
                    .get("min_zoom")
                    .and_then(Value::as_f64)
                    .unwrap_or(10.0),
            })
        })
        .collect())
}

/// Web Mercator at one zoom, in world pixels (256 per tile).
#[derive(Clone, Copy, Debug)]
struct View {
    scale: f64,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    /// Simplification tolerance in pixels: finer when zoomed in.
    tolerance: f64,
}

const MAX_LAT: f64 = 85.051_128_78;

impl View {
    fn new(lon: f64, lat: f64, zoom: f64, width: u32, height: u32) -> Self {
        let scale = 256.0 * 2f64.powf(zoom);
        let (x, y) = Self::world(lon, lat, scale);
        Self {
            scale,
            left: x - f64::from(width) / 2.0,
            top: y - f64::from(height) / 2.0,
            width: f64::from(width),
            height: f64::from(height),
            tolerance: if zoom >= 13.0 { 0.35 } else { 0.8 },
        }
    }

    fn world(lon: f64, lat: f64, scale: f64) -> (f64, f64) {
        let lat = lat.clamp(-MAX_LAT, MAX_LAT).to_radians();
        let x = (lon + 180.0) / 360.0 * scale;
        let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * scale;
        (x, y)
    }

    fn project(&self, point: [f64; 2]) -> [f64; 2] {
        let (x, y) = Self::world(point[0], point[1], self.scale);
        [x - self.left, y - self.top]
    }

    /// The frame's longitude/latitude bounds, with a margin.
    fn geo_bounds(&self) -> [f64; 4] {
        let unproject = |x: f64, y: f64| {
            let lon = x / self.scale * 360.0 - 180.0;
            let n = std::f64::consts::PI * (1.0 - 2.0 * y / self.scale);
            (lon, n.sinh().atan().to_degrees())
        };
        let margin = 16.0;
        let (west, north) = unproject(self.left - margin, self.top - margin);
        let (east, south) = unproject(
            self.left + self.width + margin,
            self.top + self.height + margin,
        );
        [west, south, east, north]
    }
}

fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// Clip a closed ring to the frame (plus a margin): Sutherland–Hodgman.
fn clip_ring(ring: &[[f64; 2]], view: &View) -> Vec<[f64; 2]> {
    let m = 4.0;
    let edges: [(usize, f64, bool); 4] = [
        (0, -m, true),
        (0, view.width + m, false),
        (1, -m, true),
        (1, view.height + m, false),
    ];
    let mut points = ring.to_vec();
    for (axis, limit, lower) in edges {
        if points.is_empty() {
            break;
        }
        let inside = |p: &[f64; 2]| {
            if lower {
                p[axis] >= limit
            } else {
                p[axis] <= limit
            }
        };
        let mut out = Vec::with_capacity(points.len());
        for i in 0..points.len() {
            let current = points[i];
            let previous = points[(i + points.len() - 1) % points.len()];
            let cross = |a: [f64; 2], b: [f64; 2]| {
                let t = (limit - a[axis]) / (b[axis] - a[axis]);
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
            };
            match (inside(&previous), inside(&current)) {
                (true, true) => out.push(current),
                (true, false) => out.push(cross(previous, current)),
                (false, true) => {
                    out.push(cross(previous, current));
                    out.push(current);
                }
                (false, false) => {}
            }
        }
        points = out;
    }
    points
}

/// Clip a polyline to the frame (plus a margin) into visible runs.
fn clip_line(line: &[[f64; 2]], view: &View) -> Vec<Vec<[f64; 2]>> {
    let m = 4.0;
    let (x0, y0, x1, y1) = (-m, -m, view.width + m, view.height + m);
    let mut runs: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut run: Vec<[f64; 2]> = Vec::new();
    for pair in line.windows(2) {
        // Liang–Barsky.
        let (a, b) = (pair[0], pair[1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let mut visible = true;
        for (p, q) in [
            (-dx, a[0] - x0),
            (dx, x1 - a[0]),
            (-dy, a[1] - y0),
            (dy, y1 - a[1]),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    visible = false;
                }
            } else {
                let t = q / p;
                if p < 0.0 {
                    t0 = t0.max(t);
                } else {
                    t1 = t1.min(t);
                }
            }
        }
        if !visible || t0 > t1 {
            if run.len() > 1 {
                runs.push(std::mem::take(&mut run));
            }
            run.clear();
            continue;
        }
        let start = [a[0] + dx * t0, a[1] + dy * t0];
        let end = [a[0] + dx * t1, a[1] + dy * t1];
        if run.is_empty() || t0 > 0.0 {
            if run.len() > 1 {
                runs.push(std::mem::take(&mut run));
            }
            run = vec![start];
        }
        run.push(end);
        if t1 < 1.0 {
            runs.push(std::mem::take(&mut run));
        }
    }
    if run.len() > 1 {
        runs.push(run);
    }
    runs
}

/// SVG path data for projected parts, thinned below half a pixel.
/// The points of `line` that keep it within `tolerance` pixels
/// (Douglas–Peucker), first and last always kept.
fn simplify(line: &[[f64; 2]], tolerance: f64) -> Vec<[f64; 2]> {
    if line.len() < 3 {
        return line.to_vec();
    }
    let mut keep = vec![false; line.len()];
    keep[0] = true;
    keep[line.len() - 1] = true;
    let mut stack = vec![(0, line.len() - 1)];
    while let Some((first, last)) = stack.pop() {
        let (a, b) = (line[first], line[last]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = (dx * dx + dy * dy).sqrt();
        let mut farthest = (0.0, first);
        for (index, p) in line.iter().enumerate().take(last).skip(first + 1) {
            let distance = if length == 0.0 {
                ((p[0] - a[0]).powi(2) + (p[1] - a[1]).powi(2)).sqrt()
            } else {
                ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / length
            };
            if distance > farthest.0 {
                farthest = (distance, index);
            }
        }
        if farthest.0 > tolerance {
            keep[farthest.1] = true;
            stack.push((first, farthest.1));
            stack.push((farthest.1, last));
        }
    }
    line.iter()
        .zip(keep)
        .filter_map(|(point, kept)| kept.then_some(*point))
        .collect()
}

/// SVG path data for projected parts, simplified to `tolerance` pixels.
fn path_data(parts: &[Vec<[f64; 2]>], closed: bool, tolerance: f64, out: &mut String) {
    for part in parts {
        let points = simplify(part, tolerance);
        if points.len() < 2 || (closed && points.len() < 3) {
            continue;
        }
        let mut last = [f64::NAN; 2];
        let start = out.len();
        let mut count = 0;
        for point in &points {
            let rounded = [
                (point[0] * 10.0).round() / 10.0,
                (point[1] * 10.0).round() / 10.0,
            ];
            if rounded == last {
                continue;
            }
            let _ = write!(
                out,
                "{}{:.1} {:.1}",
                if count == 0 { "M" } else { "L" },
                rounded[0],
                rounded[1]
            );
            last = rounded;
            count += 1;
        }
        if count < 2 || (closed && count < 3) {
            out.truncate(start);
        } else if closed {
            out.push('Z');
        }
    }
}

fn layer_path(shapes: &[Shape], view: &View, closed: bool) -> String {
    let frame = view.geo_bounds();
    let mut out = String::new();
    for shape in shapes.iter().filter(|shape| overlaps(shape.bounds, frame)) {
        let projected: Vec<Vec<[f64; 2]>> = shape
            .parts
            .iter()
            .map(|part| part.iter().map(|p| view.project(*p)).collect())
            .collect();
        let clipped: Vec<Vec<[f64; 2]>> = if closed {
            projected
                .iter()
                .map(|ring| clip_ring(ring, view))
                .filter(|ring| ring.len() > 2)
                .collect()
        } else {
            projected
                .iter()
                .flat_map(|line| clip_line(line, view))
                .collect()
        };
        path_data(&clipped, closed, view.tolerance, &mut out);
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A simplestyle colour, `#rgb` or `#rrggbb`.
fn colour(value: Option<&Value>, default: &str) -> String {
    value
        .and_then(Value::as_str)
        .filter(|c| {
            (c.len() == 4 || c.len() == 7)
                && c.starts_with('#')
                && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
        })
        .unwrap_or(default)
        .to_ascii_lowercase()
}

fn number(value: Option<&Value>, default: f64, max: f64) -> f64 {
    value
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0)
        .map_or(default, |n| n.min(max))
}

/// A Kartographer `ExternalData` reference: the Wikidata IDs whose
/// outline (`geoshape`) or line (`geoline`) Wikimedia Maps would load.
struct External<'a> {
    ids: Vec<String>,
    properties: Option<&'a Value>,
}

fn external(value: &Value) -> Option<External<'_>> {
    let service = value.get("service").and_then(Value::as_str)?;
    if service != "geoshape" && service != "geoline" {
        return None;
    }
    let url = url::Url::parse(value.get("url").and_then(Value::as_str)?).ok()?;
    let ids: Vec<String> = url
        .query_pairs()
        .find(|(key, _)| key == "ids")?
        .1
        .split(',')
        .map(str::trim)
        .filter(|id| {
            id.len() > 1 && id.starts_with('Q') && id[1..].bytes().all(|b| b.is_ascii_digit())
        })
        .map(str::to_owned)
        .collect();
    (!ids.is_empty()).then_some(External {
        ids,
        properties: value.get("properties"),
    })
}

/// Each drawable GeoJSON feature in `value` (Feature, FeatureCollection or
/// bare geometry), with its properties; `ExternalData` goes to `externals`.
fn features<'a>(
    value: &'a Value,
    out: &mut Vec<(&'a Value, Option<&'a Value>)>,
    externals: &mut Vec<Option<External<'a>>>,
) {
    match value.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => {
            for feature in value
                .get("features")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                features(feature, out, externals);
            }
        }
        Some("Feature") => {
            if let Some(geometry) = value.get("geometry").filter(|g| !g.is_null()) {
                out.push((geometry, value.get("properties")));
            }
        }
        Some("ExternalData") => externals.push(external(value)),
        Some(_) => out.push((value, None)),
        None => {}
    }
}

fn points(geometry: &Value) -> Vec<[f64; 2]> {
    let coordinates = geometry.get("coordinates");
    match geometry.get("type").and_then(Value::as_str) {
        Some("Point") => coordinates.and_then(position).into_iter().collect(),
        Some("MultiPoint") => coordinates
            .and_then(Value::as_array)
            .map(|all| all.iter().filter_map(position).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Street-level detail for one map, from an OpenStreetMap extract.
#[derive(Clone, Copy, Debug)]
pub struct MapDetail<'a> {
    /// Features in the map's area (`None` if the extract does not cover it).
    pub features: Option<&'a [OsmFeature]>,
    pub places: &'a [OsmPlace],
    /// Outlines by Wikidata ID.
    pub shapes: &'a HashMap<String, OsmShape>,
}

/// A map drawn for a pack.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawnMap {
    /// The SVG, already in the pack sanitiser's exact form.
    pub svg: Vec<u8>,
    /// Outlines the article loads from Wikimedia Maps that could not be
    /// drawn (no OpenStreetMap extract holds them).
    pub omitted_shapes: usize,
    /// Whether OpenStreetMap data was drawn (it must then be credited).
    pub uses_openstreetmap: bool,
    /// The centre drawn, for the map's link: `(lat, lon)`.
    pub centre: (f64, f64),
    pub zoom: u8,
}

/// A map's planned centre, zoom and Wikidata outlines, before any
/// OpenStreetMap data is read.
#[derive(Clone, Debug, PartialEq)]
pub struct MapPlan {
    /// `(lat, lon, zoom)`, or `None` until outlines are known.
    pub view: Option<(f64, f64, u8)>,
    /// `[west, south, east, north]` of the planned view.
    pub bounds: Option<[f64; 4]>,
    pub shapes: Vec<String>,
}

/// Plan `frame` from its stated centre or its drawable shapes.
pub fn plan_view(frame: &MapFrame, data: &[Value]) -> MapPlan {
    let mut drawable = Vec::new();
    let mut externals = Vec::new();
    for value in data {
        features(value, &mut drawable, &mut externals);
    }
    let shapes = externals
        .into_iter()
        .flatten()
        .flat_map(|external| external.ids)
        .collect();
    let points: Vec<[f64; 2]> = drawable
        .iter()
        .flat_map(|(geometry, _)| {
            let mut all = points(geometry);
            if let Some(parts) = geometry_parts(geometry) {
                all.extend(parts.into_iter().flatten());
            }
            all
        })
        .collect();
    let view = fit(frame, &points);
    MapPlan {
        bounds: view.map(|(lat, lon, zoom)| {
            View::new(lon, lat, f64::from(zoom), frame.width, frame.height).geo_bounds()
        }),
        view,
        shapes,
    }
}

/// The centre and zoom: as stated, or fitted to `points`.
fn fit(frame: &MapFrame, points: &[[f64; 2]]) -> Option<(f64, f64, u8)> {
    let (width, height) = (frame.width, frame.height);
    if let (Some(lat), Some(lon)) = (frame.lat, frame.lon) {
        return Some((lat, lon, frame.zoom.unwrap_or(12)));
    }
    if points.is_empty() {
        return None;
    }
    let b = bounds(&[points.to_vec()]);
    let (lon, lat) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
    let zoom = frame.zoom.unwrap_or_else(|| {
        // The closest zoom at which the shapes fit, up to 18.
        (0..=18u8)
            .rev()
            .find(|z| {
                let v = View::new(lon, lat, f64::from(*z), width, height);
                let (sw, ne) = (v.project([b[0], b[1]]), v.project([b[2], b[3]]));
                sw[0] >= 8.0 && ne[0] <= v.width - 8.0 && ne[1] >= 8.0 && sw[1] <= v.height - 8.0
            })
            .unwrap_or(0)
    });
    Some((lat, lon, zoom))
}

/// Road widths at zoom 16 by class (motorway … footpath), in pixels.
const ROAD_WIDTH: [f64; 7] = [11.0, 10.0, 9.0, 8.0, 6.0, 4.0, 1.6];
const ROAD_FILL: [&str; 7] = [
    "#e892a2", "#fcd6a4", "#f7fabf", "#ffffff", "#ffffff", "#ffffff", "#fa8072",
];

fn features_path(features: &[&OsmFeature], view: &View, closed: bool) -> String {
    let shapes: Vec<Shape> = features
        .iter()
        .map(|feature| Shape {
            bounds: bounds(&feature.parts),
            parts: feature.parts.clone(),
        })
        .collect();
    layer_path(&shapes, view, closed)
}

/// Draw `frame` with the article's map `data` (the features of each of its
/// groups) and, if given, OpenStreetMap `detail`. `None` if it has no centre
/// and no drawable shapes to fit.
pub fn draw_map(
    base: &NaturalEarth,
    frame: &MapFrame,
    data: &[Value],
    detail: Option<&MapDetail<'_>>,
) -> Option<DrawnMap> {
    let mut drawable = Vec::new();
    let mut externals = Vec::new();
    for value in data {
        features(value, &mut drawable, &mut externals);
    }
    // Outlines the extract holds, with the article's styles.
    let mut outlines: Vec<(&OsmShape, Option<&Value>)> = Vec::new();
    let mut omitted = 0;
    for external in &externals {
        let found: Vec<&OsmShape> = external
            .iter()
            .flat_map(|external| external.ids.iter())
            .filter_map(|id| detail.and_then(|detail| detail.shapes.get(id)))
            .collect();
        match external {
            Some(external) if found.len() == external.ids.len() => {
                outlines.extend(found.into_iter().map(|shape| (shape, external.properties)));
            }
            _ => omitted += 1,
        }
    }
    let mut every_point: Vec<[f64; 2]> = Vec::new();
    for (geometry, _) in &drawable {
        every_point.extend(points(geometry));
        if let Some(parts) = geometry_parts(geometry) {
            every_point.extend(parts.into_iter().flatten());
        }
    }
    for (shape, _) in &outlines {
        every_point.extend(shape.rings.iter().chain(&shape.lines).flatten());
    }
    let (lat, lon, zoom) = fit(frame, &every_point)?;
    let (width, height) = (frame.width, frame.height);
    let view = View::new(lon, lat, f64::from(zoom), width, height);
    let layers = if zoom <= 4 { &base.coarse } else { &base.fine };
    let osm = detail.and_then(|detail| detail.features);
    let mut uses_openstreetmap = !outlines.is_empty();

    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\"><rect width=\"{width}\" height=\"{height}\" fill=\"{WATER}\"/>"
    );
    let mut draw = |d: String, style: &str| {
        if !d.is_empty() {
            let _ = write!(svg, "<path d=\"{d}\" {style}/>");
        }
    };
    draw(
        layer_path(&layers.land, &view, true),
        &format!("fill=\"{LAND}\" fill-rule=\"evenodd\""),
    );
    draw(
        layer_path(&layers.lakes, &view, true),
        &format!("fill=\"{WATER}\" fill-rule=\"evenodd\""),
    );
    if osm.is_none() {
        draw(
            layer_path(&layers.rivers, &view, false),
            &format!("fill=\"none\" stroke=\"{WATER}\" stroke-width=\"1.2\""),
        );
    }
    if let Some(osm) = osm {
        uses_openstreetmap |= !osm.is_empty();
        let of = |kind: fn(FeatureKind) -> bool| -> Vec<&OsmFeature> {
            osm.iter().filter(|feature| kind(feature.kind)).collect()
        };
        let scale = 2f64.powf(f64::from(zoom) - 16.0);
        draw(
            features_path(&of(|k| k == FeatureKind::Green), &view, true),
            "fill=\"#c8facc\" fill-rule=\"evenodd\"",
        );
        draw(
            features_path(&of(|k| k == FeatureKind::Water), &view, true),
            &format!("fill=\"{WATER}\" fill-rule=\"evenodd\""),
        );
        draw(
            features_path(&of(|k| k == FeatureKind::River), &view, false),
            &format!(
                "fill=\"none\" stroke=\"{WATER}\" stroke-width=\"{:.1}\" stroke-linecap=\"round\"",
                (6.0 * scale).clamp(1.0, 8.0)
            ),
        );
        draw(
            features_path(&of(|k| k == FeatureKind::Coastline), &view, false),
            "fill=\"none\" stroke=\"#6a9bb0\" stroke-width=\"0.8\"",
        );
        draw(
            features_path(&of(|k| k == FeatureKind::Building), &view, true),
            "fill=\"#d9d0c9\" stroke=\"#c4b6ab\" stroke-width=\"0.5\"",
        );
        // Roads: casings, then fills, minor first so major roads are on top.
        for class in (0..7u8).rev() {
            let roads = of_class(osm, class);
            let width = (ROAD_WIDTH[usize::from(class)] * scale).max(0.6);
            if class < 6 && zoom >= 13 {
                draw(
                    features_path(&roads, &view, false),
                    &format!(
                        "fill=\"none\" stroke=\"#a89d93\" stroke-width=\"{:.1}\" \
                         stroke-linecap=\"round\" stroke-linejoin=\"round\"",
                        width + 1.2
                    ),
                );
            }
        }
        for class in (0..7u8).rev() {
            let roads = of_class(osm, class);
            let width = (ROAD_WIDTH[usize::from(class)] * scale).max(0.6);
            let dash = if class == 6 {
                " stroke-dasharray=\"2 2\""
            } else {
                ""
            };
            draw(
                features_path(&roads, &view, false),
                &format!(
                    "fill=\"none\" stroke=\"{}\" stroke-width=\"{width:.1}\" \
                     stroke-linecap=\"round\" stroke-linejoin=\"round\"{dash}",
                    ROAD_FILL[usize::from(class)]
                ),
            );
        }
        draw(
            features_path(&of(|k| k == FeatureKind::Rail), &view, false),
            "fill=\"none\" stroke=\"#707070\" stroke-width=\"1.4\" stroke-dasharray=\"6 3\"",
        );
    }
    if zoom >= 3 {
        draw(
            layer_path(&layers.states, &view, false),
            "fill=\"none\" stroke=\"#b9a8b9\" stroke-width=\"0.6\" stroke-dasharray=\"3 2\"",
        );
    }
    draw(
        layer_path(&layers.borders, &view, false),
        "fill=\"none\" stroke=\"#9e8c9e\" stroke-width=\"1\"",
    );

    // Place names for this zoom, most important first and never
    // overlapping: OpenStreetMap's close up (zoom 11+, where it has
    // detail), Natural Earth's curated places further out.
    let mut places: Vec<([f64; 2], &str, f64)> = match (osm, detail) {
        (Some(_), Some(detail)) if zoom >= 11 && !detail.places.is_empty() => detail
            .places
            .iter()
            .map(|place| (place.at, place.name.as_str(), f64::from(place.min_zoom)))
            .collect(),
        _ => layers
            .places
            .iter()
            .filter(|place| place.min_zoom <= f64::from(zoom) + 0.5)
            .map(|place| (place.at, place.name.as_str(), place.min_zoom))
            .collect(),
    };
    places.sort_by(|a, b| a.2.total_cmp(&b.2).then_with(|| a.1.cmp(b.1)));
    let mut taken: Vec<[f64; 4]> = Vec::new();
    let mut labels = String::new();
    for (at, name, _) in places {
        let [x, y] = view.project(at);
        // Wide (CJK) characters take about a full em, others ~0.6 em.
        let width: f64 = name
            .chars()
            .map(|c| if (c as u32) >= 0x2E80 { 11.0 } else { 6.6 })
            .sum();
        let label = [x - 3.0, y - 14.0, x + 5.0 + width, y + 3.0];
        if label[0] < 0.0
            || label[1] < 0.0
            || label[2] > view.width
            || label[3] > view.height - 12.0
        {
            continue;
        }
        if taken
            .iter()
            .any(|t| label[0] < t[2] && t[0] < label[2] && label[1] < t[3] && t[1] < label[3])
        {
            continue;
        }
        taken.push(label);
        let _ = write!(
            labels,
            "<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"2\" fill=\"#333\"/>\
             <text x=\"{:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"11\" fill=\"#333\" \
             stroke=\"#fff\" stroke-width=\"3\" paint-order=\"stroke\">{}</text>",
            x + 4.0,
            y - 3.0,
            escape(name)
        );
    }

    // The article's outlines, shapes, lines and markers, on top.
    let mut overlay = String::new();
    let mut shape_path = |parts: Vec<Vec<[f64; 2]>>, closed: bool, properties: Option<&Value>| {
        let style = |key: &str| properties.and_then(|p| p.get(key));
        if parts.is_empty() {
            return;
        }
        let shape = Shape {
            bounds: bounds(&parts),
            parts,
        };
        let d = layer_path(std::slice::from_ref(&shape), &view, closed);
        if d.is_empty() {
            return;
        }
        let stroke = colour(style("stroke"), "#555555");
        let stroke_width = number(style("stroke-width"), 2.0, 20.0);
        let stroke_opacity = number(style("stroke-opacity"), 1.0, 1.0);
        let fill = if closed {
            format!(
                "fill=\"{}\" fill-opacity=\"{}\" fill-rule=\"evenodd\"",
                colour(style("fill"), "#555555"),
                number(style("fill-opacity"), 0.3, 1.0)
            )
        } else {
            "fill=\"none\"".to_owned()
        };
        let _ = write!(
            overlay,
            "<path d=\"{d}\" {fill} stroke=\"{stroke}\" stroke-width=\"{stroke_width}\" \
             stroke-opacity=\"{stroke_opacity}\" stroke-linejoin=\"round\"/>"
        );
    };
    for (shape, properties) in &outlines {
        shape_path(shape.rings.clone(), true, *properties);
        shape_path(shape.lines.clone(), false, *properties);
    }
    for (geometry, properties) in &drawable {
        if let Some(parts) = geometry_parts(geometry) {
            let closed = matches!(
                geometry.get("type").and_then(Value::as_str),
                Some("Polygon" | "MultiPolygon")
            );
            shape_path(parts, closed, *properties);
        }
    }
    for (geometry, properties) in &drawable {
        let style = |key: &str| properties.and_then(|p| p.get(key));
        for point in points(geometry) {
            let [x, y] = view.project(point);
            if !(0.0..=view.width).contains(&x) || !(0.0..=view.height).contains(&y) {
                continue;
            }
            let fill = colour(style("marker-color"), "#7e7e7e");
            // A map pin: a circle on a point, outlined to show on any base.
            let _ = write!(
                overlay,
                "<path d=\"M{x:.1} {y:.1}l-6 -10a7 7 0 1 1 12 0z\" fill=\"{fill}\" \
                 stroke=\"#fff\" stroke-width=\"1.5\"/><circle cx=\"{x:.1}\" cy=\"{:.1}\" r=\"2.5\" fill=\"#fff\"/>",
                y - 13.0
            );
        }
    }
    svg.push_str(&labels);
    svg.push_str(&overlay);
    let credit = if uses_openstreetmap {
        "© OpenStreetMap contributors · Natural Earth"
    } else {
        "Natural Earth"
    };
    let _ = write!(
        svg,
        "<text x=\"{}\" y=\"{}\" text-anchor=\"end\" font-family=\"sans-serif\" font-size=\"9\" \
         fill=\"#555\" stroke=\"#fff\" stroke-width=\"2\" paint-order=\"stroke\">{credit}</text></svg>",
        width - 3,
        height - 3
    );
    let svg = wiki_store::pack_svg::sanitize_svg(svg.as_bytes()).ok()?;
    Some(DrawnMap {
        svg,
        omitted_shapes: omitted,
        uses_openstreetmap,
        centre: (lat, lon),
        zoom,
    })
}

fn of_class(osm: &[OsmFeature], class: u8) -> Vec<&OsmFeature> {
    osm.iter()
        .filter(|feature| feature.kind == FeatureKind::Road(class))
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tiny base map: one square island around (0, 0), a lake inside it,
    /// a river across it, and two places.
    pub(crate) fn base() -> NaturalEarth {
        let square = |r: f64| Shape {
            bounds: [-r, -r, r, r],
            parts: vec![vec![[-r, -r], [r, -r], [r, r], [-r, r], [-r, -r]]],
        };
        let layers = || Layers {
            land: vec![square(1.0)],
            lakes: vec![square(0.1)],
            rivers: vec![Shape {
                bounds: [-5.0, 0.5, 5.0, 0.5],
                parts: vec![vec![[-5.0, 0.5], [5.0, 0.5]]],
            }],
            borders: Vec::new(),
            states: Vec::new(),
            places: vec![
                Place {
                    at: [0.2, 0.2],
                    name: "Port <&> Town".into(),
                    min_zoom: 3.0,
                },
                Place {
                    at: [0.3, -0.3],
                    name: "Village".into(),
                    min_zoom: 9.0,
                },
            ],
        };
        NaturalEarth {
            coarse: layers(),
            fine: layers(),
        }
    }

    fn frame(lat: Option<f64>, lon: Option<f64>, zoom: Option<u8>) -> MapFrame {
        MapFrame {
            lat,
            lon,
            zoom,
            width: 300,
            height: 200,
            groups: Vec::new(),
        }
    }

    #[test]
    fn a_map_is_drawn_at_the_articles_centre_with_its_markers() {
        let marker: Value = serde_json::json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [0.0, 0.0]},
            "properties": {"marker-color": "#5E74F3", "title": "Here"}
        });
        let map = draw_map(
            &base(),
            &frame(Some(0.0), Some(0.0), Some(6)),
            &[marker],
            None,
        )
        .unwrap();
        let svg = String::from_utf8(map.svg.clone()).unwrap();
        assert!(wiki_store::pack_svg::is_clean_svg(&map.svg));
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"300\" height=\"200\" viewBox=\"0 0 300 200\">"));
        // Land, lake and river, clipped to the frame.
        assert_eq!(svg.matches("fill=\"#f2efe9\"").count(), 1, "{svg}");
        assert!(svg.contains("stroke=\"#aad3df\""), "{svg}");
        // At zoom 6 the island (2° wide) fills the frame: the land path is
        // clipped to just outside it.
        let land = svg.split("fill=\"#f2efe9\"").next().unwrap();
        assert!(!land.contains("M-") || land.contains("M-4.0"), "{svg}");
        // The marker is at the centre (150, 100), in its colour.
        assert!(
            svg.contains("d=\"M150.0 100.0l-6 -10a7 7 0 1 1 12 0z\" fill=\"#5e74f3\""),
            "{svg}"
        );
        // Only places for this zoom, escaped.
        assert!(svg.contains(">Port &lt;&amp;&gt; Town</text>"), "{svg}");
        assert!(!svg.contains("Village"), "{svg}");
        assert!(svg.contains(">Natural Earth</text>"), "{svg}");
        assert_eq!(
            (map.centre, map.zoom, map.omitted_shapes),
            ((0.0, 0.0), 6, 0)
        );
    }

    #[test]
    fn a_map_without_a_centre_fits_its_shapes_and_external_shapes_are_counted() {
        let data: Value = serde_json::json!([
            {"type": "ExternalData", "service": "geoshape", "url": "https://maps.wikimedia.org/geoshape?ids=Q1"},
            {"type": "FeatureCollection", "features": [{
                "type": "Feature",
                "geometry": {"type": "LineString", "coordinates": [[0.0, 0.0], [0.5, 0.5]]},
                "properties": {"stroke": "#ff0000", "stroke-width": 99}
            }]}
        ]);
        let data = data.as_array().unwrap().clone();
        let map = draw_map(&base(), &frame(None, None, None), &data, None).unwrap();
        let svg = String::from_utf8(map.svg).unwrap();
        assert_eq!(map.omitted_shapes, 1);
        assert_eq!(map.centre, (0.25, 0.25));
        // Half a degree is about 182 px at zoom 9: the closest zoom that fits.
        assert_eq!(map.zoom, 9);
        assert!(
            svg.contains("stroke=\"#ff0000\" stroke-width=\"20\""),
            "capped: {svg}"
        );
        // No centre and nothing drawable: no map.
        assert_eq!(
            draw_map(&base(), &frame(None, None, Some(5)), &data[..1], None),
            None
        );
    }

    #[test]
    fn openstreetmap_detail_and_wikidata_outlines_are_drawn_and_credited() {
        use crate::osm_extract::{FeatureKind, OsmFeature, OsmPlace, OsmShape};
        let road = OsmFeature {
            kind: FeatureKind::Road(1),
            parts: vec![vec![[-0.01, 0.0], [0.01, 0.0]]],
        };
        let building = OsmFeature {
            kind: FeatureKind::Building,
            parts: vec![vec![
                [0.001, 0.001],
                [0.002, 0.001],
                [0.002, 0.002],
                [0.001, 0.001],
            ]],
        };
        let places = [OsmPlace {
            at: [0.0005, 0.0005],
            name: "Old Town".into(),
            min_zoom: 12,
        }];
        let shapes = HashMap::from([(
            "Q42".to_owned(),
            OsmShape {
                rings: vec![vec![
                    [-0.003, -0.003],
                    [0.003, -0.003],
                    [0.003, 0.003],
                    [-0.003, -0.003],
                ]],
                lines: vec![],
            },
        )]);
        let features = [road, building];
        let detail = MapDetail {
            features: Some(&features),
            places: &places,
            shapes: &shapes,
        };
        let data: Value = serde_json::json!({
            "type": "ExternalData", "service": "geoshape",
            "url": "https://maps.wikimedia.org/geoshape?getgeojson=1&ids=Q42",
            "properties": {"stroke": "#ff0000", "fill": "#00ff00"}
        });
        let map = draw_map(
            &base(),
            &frame(Some(0.0), Some(0.0), Some(16)),
            &[data.clone()],
            Some(&detail),
        )
        .unwrap();
        let svg = String::from_utf8(map.svg).unwrap();
        assert_eq!(map.omitted_shapes, 0);
        assert!(map.uses_openstreetmap);
        assert!(svg.contains("fill=\"#d9d0c9\""), "building: {svg}");
        assert!(svg.contains("stroke=\"#fcd6a4\""), "primary road: {svg}");
        assert!(
            svg.contains("fill=\"#00ff00\"") && svg.contains("stroke=\"#ff0000\""),
            "outline: {svg}"
        );
        assert!(svg.contains(">Old Town</text>"), "OSM place: {svg}");
        assert!(!svg.contains("Port &lt;"), "base places replaced: {svg}");
        assert!(
            svg.contains("© OpenStreetMap contributors · Natural Earth</text>"),
            "{svg}"
        );
        // An outline the extract lacks is counted, not drawn.
        let other = HashMap::new();
        let without = MapDetail {
            features: Some(&features),
            places: &places,
            shapes: &other,
        };
        let map = draw_map(
            &base(),
            &frame(Some(0.0), Some(0.0), Some(16)),
            &[data.clone()],
            Some(&without),
        )
        .unwrap();
        assert_eq!(map.omitted_shapes, 1);
        // A planned view names the outlines to read, and fits nothing yet.
        let plan = plan_view(&frame(None, None, None), &[data]);
        assert_eq!(plan.shapes, ["Q42"]);
        assert_eq!(plan.view, None);
        let plan = plan_view(&frame(Some(0.0), Some(0.0), Some(16)), &[]);
        let b = plan.bounds.unwrap();
        assert!(
            b[0] < 0.0 && b[2] > 0.0 && b[1] < 0.0 && b[3] > 0.0,
            "{b:?}"
        );
    }

    #[test]
    fn hostile_map_data_cannot_escape_into_the_svg() {
        let data: Value = serde_json::json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [0.0, 0.0]},
            "properties": {"marker-color": "\"/><script>alert(1)</script>", "stroke": "url(http://x/)"}
        });
        let map = draw_map(
            &base(),
            &frame(Some(0.0), Some(0.0), Some(5)),
            &[data],
            None,
        )
        .unwrap();
        let svg = String::from_utf8(map.svg).unwrap();
        assert!(
            !svg.contains("script") && !svg.contains("http://x"),
            "{svg}"
        );
        assert!(svg.contains("fill=\"#7e7e7e\""), "default colour: {svg}");
    }

    #[test]
    fn lines_are_simplified_within_the_tolerance() {
        // A wiggle smaller than the tolerance is dropped, a corner is kept.
        let line = [[0.0, 0.0], [5.0, 0.2], [10.0, 0.0], [10.0, 10.0]];
        assert_eq!(
            simplify(&line, 0.5),
            vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]
        );
        assert_eq!(simplify(&line, 0.1), line.to_vec());
        assert_eq!(simplify(&line[..2], 5.0), line[..2].to_vec());
    }

    #[test]
    fn place_names_never_overlap_and_the_most_important_wins() {
        let mut base = base();
        base.fine.places = vec![
            Place {
                at: [0.0, 0.0],
                name: "Minor".into(),
                min_zoom: 6.0,
            },
            Place {
                at: [0.0, 0.0],
                name: "Capital".into(),
                min_zoom: 2.0,
            },
            Place {
                at: [0.3, -0.3],
                name: "Elsewhere".into(),
                min_zoom: 6.0,
            },
        ];
        let map = draw_map(&base, &frame(Some(0.0), Some(0.0), Some(7)), &[], None).unwrap();
        let svg = String::from_utf8(map.svg).unwrap();
        assert!(svg.contains(">Capital</text>"), "{svg}");
        assert!(
            !svg.contains(">Minor</text>"),
            "same spot, less important: {svg}"
        );
        assert!(svg.contains(">Elsewhere</text>"), "{svg}");
    }

    #[test]
    fn clipping_keeps_only_what_is_in_the_frame() {
        let view = View {
            scale: 256.0,
            left: 0.0,
            top: 0.0,
            width: 100.0,
            height: 100.0,
            tolerance: 0.5,
        };
        let ring = clip_ring(
            &[
                [-50.0, -50.0],
                [150.0, -50.0],
                [150.0, 150.0],
                [-50.0, 150.0],
            ],
            &view,
        );
        assert!(ring
            .iter()
            .all(|p| (-4.0..=104.0).contains(&p[0]) && (-4.0..=104.0).contains(&p[1])));
        assert_eq!(ring.len(), 4);
        let runs = clip_line(&[[-50.0, 50.0], [150.0, 50.0], [150.0, 60.0]], &view);
        assert_eq!(runs, vec![vec![[-4.0, 50.0], [104.0, 50.0]]]);
        assert!(clip_line(&[[200.0, 200.0], [300.0, 300.0]], &view).is_empty());
    }
}

/// Opt-in: draw real maps from a Natural Earth directory, for review.
/// `NATURAL_EARTH_DIR=<dir> MAP_OUT=<dir> cargo test -p wiki-pack-builder
/// --lib real_natural_earth -- --ignored`
#[cfg(test)]
mod real_data {
    use super::*;

    #[test]
    #[ignore = "needs the pinned Natural Earth files"]
    fn real_natural_earth_maps() {
        let dir = std::env::var_os("NATURAL_EARTH_DIR").expect("set NATURAL_EARTH_DIR");
        let out = std::path::PathBuf::from(std::env::var_os("MAP_OUT").expect("set MAP_OUT"));
        let started = std::time::Instant::now();
        let base = NaturalEarth::load(Path::new(&dir)).unwrap();
        println!("loaded in {:?}", started.elapsed());
        let eiffel: Value = serde_json::json!({"type": "Feature",
            "geometry": {"type": "Point", "coordinates": [2.2945, 48.858222]},
            "properties": {"title": "Eiffel Tower", "marker-color": "#5E74F3"}});
        let cases = [
            (
                "tokyo-z8",
                MapFrame {
                    lat: Some(35.68),
                    lon: Some(139.41),
                    zoom: Some(8),
                    width: 300,
                    height: 200,
                    groups: vec![],
                },
                vec![],
            ),
            (
                "eiffel-z16",
                MapFrame {
                    lat: None,
                    lon: None,
                    zoom: Some(16),
                    width: 250,
                    height: 200,
                    groups: vec![],
                },
                vec![eiffel.clone()],
            ),
            (
                "paris-z5",
                MapFrame {
                    lat: Some(48.86),
                    lon: Some(2.29),
                    zoom: Some(5),
                    width: 300,
                    height: 300,
                    groups: vec![],
                },
                vec![eiffel],
            ),
            (
                "world-z1",
                MapFrame {
                    lat: Some(20.0),
                    lon: Some(0.0),
                    zoom: Some(1),
                    width: 400,
                    height: 300,
                    groups: vec![],
                },
                vec![],
            ),
        ];
        for (name, frame, data) in cases {
            let started = std::time::Instant::now();
            let map = draw_map(&base, &frame, &data, None).unwrap();
            println!("{name}: {} bytes in {:?}", map.svg.len(), started.elapsed());
            fs::write(out.join(format!("{name}.svg")), &map.svg).unwrap();
        }
    }
}
