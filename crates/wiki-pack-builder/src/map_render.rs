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
fn path_data(parts: &[Vec<[f64; 2]>], closed: bool, out: &mut String) {
    for part in parts {
        let mut last: Option<[f64; 2]> = None;
        let mut count = 0;
        let start = out.len();
        for point in part {
            if let Some(previous) = last {
                if (point[0] - previous[0]).abs() < 0.5 && (point[1] - previous[1]).abs() < 0.5 {
                    continue;
                }
            }
            let _ = write!(
                out,
                "{}{:.1} {:.1}",
                if count == 0 { "M" } else { "L" },
                point[0],
                point[1]
            );
            last = Some(*point);
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
        path_data(&clipped, closed, &mut out);
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

/// Each drawable GeoJSON feature in `value` (Feature, FeatureCollection or
/// bare geometry), with its properties. `ExternalData` is counted.
fn features<'a>(
    value: &'a Value,
    out: &mut Vec<(&'a Value, Option<&'a Value>)>,
    omitted: &mut usize,
) {
    match value.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => {
            for feature in value
                .get("features")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                features(feature, out, omitted);
            }
        }
        Some("Feature") => {
            if let Some(geometry) = value.get("geometry").filter(|g| !g.is_null()) {
                out.push((geometry, value.get("properties")));
            }
        }
        Some("ExternalData") => *omitted += 1,
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

/// A map drawn for a pack.
#[derive(Clone, Debug, PartialEq)]
pub struct DrawnMap {
    /// The SVG, already in the pack sanitiser's exact form.
    pub svg: Vec<u8>,
    /// Shapes the article loads from Wikimedia Maps, not drawn.
    pub omitted_shapes: usize,
    /// The centre drawn, for the map's link: `(lat, lon)`.
    pub centre: (f64, f64),
    pub zoom: u8,
}

/// Draw `frame` with the article's map `data` (the features of each of its
/// groups). `None` if it has no centre and no drawable shapes to fit.
pub fn draw_map(base: &NaturalEarth, frame: &MapFrame, data: &[Value]) -> Option<DrawnMap> {
    let mut drawable = Vec::new();
    let mut omitted = 0;
    for value in data {
        features(value, &mut drawable, &mut omitted);
    }
    let mut every_point = Vec::new();
    for (geometry, _) in &drawable {
        every_point.extend(points(geometry));
        if let Some(parts) = geometry_parts(geometry) {
            every_point.extend(parts.into_iter().flatten());
        }
    }
    let (width, height) = (frame.width, frame.height);
    let (lat, lon, zoom) = match (frame.lat, frame.lon) {
        (Some(lat), Some(lon)) => (lat, lon, frame.zoom.unwrap_or(12)),
        _ => {
            if every_point.is_empty() {
                return None;
            }
            let b = bounds(&[every_point.clone()]);
            let (lon, lat) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
            let zoom = frame.zoom.unwrap_or_else(|| {
                // The closest zoom at which the shapes fit, up to 18.
                (0..=18u8)
                    .rev()
                    .find(|z| {
                        let v = View::new(lon, lat, f64::from(*z), width, height);
                        let (sw, ne) = (v.project([b[0], b[1]]), v.project([b[2], b[3]]));
                        sw[0] >= 8.0
                            && ne[0] <= v.width - 8.0
                            && ne[1] >= 8.0
                            && sw[1] <= v.height - 8.0
                    })
                    .unwrap_or(0)
            });
            (lat, lon, zoom)
        }
    };
    let view = View::new(lon, lat, f64::from(zoom), width, height);
    let layers = if zoom <= 4 { &base.coarse } else { &base.fine };

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
    draw(
        layer_path(&layers.rivers, &view, false),
        &format!("fill=\"none\" stroke=\"{WATER}\" stroke-width=\"1.2\""),
    );
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

    // Place names that fit this zoom.
    let mut labels = String::new();
    for place in layers
        .places
        .iter()
        .filter(|place| place.min_zoom <= f64::from(zoom) + 0.5)
    {
        let [x, y] = view.project(place.at);
        if !(6.0..=view.width - 6.0).contains(&x) || !(10.0..=view.height - 4.0).contains(&y) {
            continue;
        }
        let _ = write!(
            labels,
            "<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"2\" fill=\"#333\"/>\
             <text x=\"{:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"11\" fill=\"#333\" \
             stroke=\"#fff\" stroke-width=\"3\" paint-order=\"stroke\">{}</text>",
            x + 4.0,
            y - 3.0,
            escape(&place.name)
        );
    }
    svg.push_str(&labels);

    // The article's own shapes, lines and markers, on top.
    for (geometry, properties) in &drawable {
        let style = |key: &str| properties.and_then(|p| p.get(key));
        let Some(parts) = geometry_parts(geometry) else {
            continue;
        };
        if !parts.is_empty() {
            let closed = matches!(
                geometry.get("type").and_then(Value::as_str),
                Some("Polygon" | "MultiPolygon")
            );
            let shape = Shape {
                bounds: bounds(&parts),
                parts,
            };
            let d = layer_path(std::slice::from_ref(&shape), &view, closed);
            if !d.is_empty() {
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
                    svg,
                    "<path d=\"{d}\" {fill} stroke=\"{stroke}\" stroke-width=\"{stroke_width}\" \
                     stroke-opacity=\"{stroke_opacity}\" stroke-linejoin=\"round\"/>"
                );
            }
        }
        for point in points(geometry) {
            let [x, y] = view.project(point);
            if !(0.0..=view.width).contains(&x) || !(0.0..=view.height).contains(&y) {
                continue;
            }
            let fill = colour(style("marker-color"), "#7e7e7e");
            // A map pin: a circle on a point, outlined to show on any base.
            let _ = write!(
                svg,
                "<path d=\"M{x:.1} {y:.1}l-6 -10a7 7 0 1 1 12 0z\" fill=\"{fill}\" \
                 stroke=\"#fff\" stroke-width=\"1.5\"/><circle cx=\"{x:.1}\" cy=\"{:.1}\" r=\"2.5\" fill=\"#fff\"/>",
                y - 13.0
            );
        }
    }
    let _ = write!(
        svg,
        "<text x=\"{}\" y=\"{}\" text-anchor=\"end\" font-family=\"sans-serif\" font-size=\"9\" \
         fill=\"#555\" stroke=\"#fff\" stroke-width=\"2\" paint-order=\"stroke\">Natural Earth</text></svg>",
        width - 3,
        height - 3
    );
    let svg = wiki_store::pack_svg::sanitize_svg(svg.as_bytes()).ok()?;
    Some(DrawnMap {
        svg,
        omitted_shapes: omitted,
        centre: (lat, lon),
        zoom,
    })
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
        let map = draw_map(&base(), &frame(Some(0.0), Some(0.0), Some(6)), &[marker]).unwrap();
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
        let map = draw_map(&base(), &frame(None, None, None), &data).unwrap();
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
            draw_map(&base(), &frame(None, None, Some(5)), &data[..1]),
            None
        );
    }

    #[test]
    fn hostile_map_data_cannot_escape_into_the_svg() {
        let data: Value = serde_json::json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [0.0, 0.0]},
            "properties": {"marker-color": "\"/><script>alert(1)</script>", "stroke": "url(http://x/)"}
        });
        let map = draw_map(&base(), &frame(Some(0.0), Some(0.0), Some(5)), &[data]).unwrap();
        let svg = String::from_utf8(map.svg).unwrap();
        assert!(
            !svg.contains("script") && !svg.contains("http://x"),
            "{svg}"
        );
        assert!(svg.contains("fill=\"#7e7e7e\""), "default colour: {svg}");
    }

    #[test]
    fn clipping_keeps_only_what_is_in_the_frame() {
        let view = View {
            scale: 256.0,
            left: 0.0,
            top: 0.0,
            width: 100.0,
            height: 100.0,
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
            let map = draw_map(&base, &frame, &data).unwrap();
            println!("{name}: {} bytes in {:?}", map.svg.len(), started.elapsed());
            fs::write(out.join(format!("{name}.svg")), &map.svg).unwrap();
        }
    }
}
