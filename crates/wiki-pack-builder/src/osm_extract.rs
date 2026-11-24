//! PACK-003: map detail from an OpenStreetMap extract (ODbL), so maps
//! drawn into packs show streets, buildings, water and parks, and the
//! outlines Kartographer loads from Wikimedia Maps by Wikidata ID.
//!
//! The publisher downloads a regional `.osm.pbf` extract once (e.g. from
//! Geofabrik); it is read in four streaming passes, so a country-sized file
//! is never held in memory:
//!
//! 1. relations: the Wikidata-tagged relations the articles' shapes name,
//!    and water/green multipolygons, with their member ways;
//! 2. nodes inside any map's area (and named places there);
//! 3. ways: drawable features touching a map's area, and every member of
//!    a relevant relation;
//! 4. nodes those ways still need.
//!
//! Tiles or other Wikimedia / OpenStreetMap services are never used.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A relation member's type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberKind {
    Node,
    Way,
    Relation,
}

type Tags<'a, 'b> = &'b mut dyn Iterator<Item = (&'a str, &'a str)>;

/// Receives the elements of one pass over an extract.
pub trait OsmVisitor {
    fn node(&mut self, _id: i64, _at: [f64; 2], _tags: Tags<'_, '_>) {}
    fn way(&mut self, _id: i64, _refs: &mut dyn Iterator<Item = i64>, _tags: Tags<'_, '_>) {}
    fn relation(
        &mut self,
        _id: i64,
        _members: &mut dyn Iterator<Item = (i64, MemberKind, &str)>,
        _tags: Tags<'_, '_>,
    ) {
    }
}

/// An OpenStreetMap extract that can be read in passes.
pub trait OsmSource {
    /// Visit every element, in file order (nodes, then ways, then relations).
    fn visit(&self, visitor: &mut dyn OsmVisitor) -> Result<(), String>;
    /// The area the extract covers, `[west, south, east, north]`, if stated.
    fn bounds(&self) -> Option<[f64; 4]>;
}

/// An `.osm.pbf` file.
#[derive(Clone, Debug)]
pub struct PbfFile {
    path: PathBuf,
    bounds: Option<[f64; 4]>,
}

impl PbfFile {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut blobs =
            osmpbf::BlobReader::from_path(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let header = blobs
            .next()
            .ok_or_else(|| format!("{}: empty extract", path.display()))?
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let header = header
            .to_headerblock()
            .map_err(|e| format!("{}: not an OSM PBF file: {e}", path.display()))?;
        // osmpbf's docs swap top/bottom; take the extremes.
        let bounds = header.bbox().map(|b| {
            [
                b.left.min(b.right),
                b.top.min(b.bottom),
                b.left.max(b.right),
                b.top.max(b.bottom),
            ]
        });
        Ok(Self {
            path: path.to_owned(),
            bounds,
        })
    }
}

impl OsmSource for PbfFile {
    fn visit(&self, visitor: &mut dyn OsmVisitor) -> Result<(), String> {
        let reader = osmpbf::ElementReader::from_path(&self.path)
            .map_err(|e| format!("{}: {e}", self.path.display()))?;
        reader
            .for_each(|element| match element {
                osmpbf::Element::Node(node) => {
                    visitor.node(node.id(), [node.lon(), node.lat()], &mut node.tags())
                }
                osmpbf::Element::DenseNode(node) => {
                    visitor.node(node.id(), [node.lon(), node.lat()], &mut node.tags())
                }
                osmpbf::Element::Way(way) => {
                    visitor.way(way.id(), &mut way.refs(), &mut way.tags())
                }
                osmpbf::Element::Relation(relation) => {
                    let mut members = relation.members().map(|member| {
                        let kind = match member.member_type {
                            osmpbf::RelMemberType::Node => MemberKind::Node,
                            osmpbf::RelMemberType::Way => MemberKind::Way,
                            osmpbf::RelMemberType::Relation => MemberKind::Relation,
                        };
                        (member.member_id, kind, member.role().unwrap_or(""))
                    });
                    visitor.relation(relation.id(), &mut members, &mut relation.tags())
                }
            })
            .map_err(|e| format!("{}: {e}", self.path.display()))
    }

    fn bounds(&self) -> Option<[f64; 4]> {
        self.bounds
    }
}

/// What a feature is, for drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeatureKind {
    Green,
    Water,
    River,
    Building,
    Rail,
    /// Road class, 0 (motorway) to 6 (footpath).
    Road(u8),
    Coastline,
}

impl FeatureKind {
    /// Filled areas rather than lines.
    pub fn is_area(self) -> bool {
        matches!(self, Self::Green | Self::Water | Self::Building)
    }
}

/// A feature's kind and the closest zoom it is drawn from.
fn classify<'a>(tags: impl Iterator<Item = (&'a str, &'a str)>) -> Option<(FeatureKind, u8)> {
    let mut found: Option<(FeatureKind, u8)> = None;
    let mut tunnel = false;
    for (key, value) in tags {
        let kind = match (key, value) {
            ("highway", class) => match class.trim_end_matches("_link") {
                "motorway" | "trunk" => Some((FeatureKind::Road(0), 5)),
                "primary" => Some((FeatureKind::Road(1), 8)),
                "secondary" => Some((FeatureKind::Road(2), 9)),
                "tertiary" => Some((FeatureKind::Road(3), 11)),
                "residential" | "unclassified" | "living_street" | "road" => {
                    Some((FeatureKind::Road(4), 13))
                }
                "service" | "pedestrian" => Some((FeatureKind::Road(5), 14)),
                "footway" | "path" | "cycleway" | "steps" | "track" | "bridleway" => {
                    Some((FeatureKind::Road(6), 15))
                }
                _ => None,
            },
            ("railway", "rail" | "light_rail" | "narrow_gauge") => Some((FeatureKind::Rail, 10)),
            ("building", value) if value != "no" => Some((FeatureKind::Building, 14)),
            ("natural", "water") | ("waterway", "riverbank") => Some((FeatureKind::Water, 9)),
            ("landuse", "reservoir" | "basin") => Some((FeatureKind::Water, 9)),
            ("waterway", "river") => Some((FeatureKind::River, 8)),
            ("waterway", "canal") => Some((FeatureKind::River, 10)),
            ("waterway", "stream") => Some((FeatureKind::River, 13)),
            ("natural", "coastline") => Some((FeatureKind::Coastline, 5)),
            ("leisure", "park" | "garden" | "nature_reserve")
            | ("landuse", "grass" | "forest" | "meadow" | "recreation_ground" | "cemetery")
            | ("landuse", "village_green")
            | ("natural", "wood" | "scrub" | "grassland") => Some((FeatureKind::Green, 10)),
            ("tunnel", value) if value != "no" => {
                tunnel = true;
                None
            }
            _ => None,
        };
        // A road or railway wins over an area tag on the same way.
        if let Some(kind) = kind {
            found = match found {
                Some(existing) if existing.0 > kind.0 => Some(existing),
                _ => Some(kind),
            };
        }
    }
    if tunnel && matches!(found, Some((FeatureKind::Road(_) | FeatureKind::Rail, _))) {
        return None;
    }
    found
}

/// One map's area and zoom, and the Wikidata shapes it draws.
#[derive(Clone, Debug, PartialEq)]
pub struct OsmRequest {
    /// `[west, south, east, north]`; `None` when only shapes are wanted.
    pub bounds: Option<[f64; 4]>,
    pub zoom: u8,
    pub shapes: Vec<String>,
}

/// A drawable feature, as longitude/latitude lines or rings.
#[derive(Clone, Debug, PartialEq)]
pub struct OsmFeature {
    pub kind: FeatureKind,
    pub parts: Vec<Vec<[f64; 2]>>,
}

/// A named place (city, suburb, neighbourhood…).
#[derive(Clone, Debug, PartialEq)]
pub struct OsmPlace {
    pub at: [f64; 2],
    pub name: String,
    pub min_zoom: u8,
}

/// The outline of a Wikidata item: closed rings (filled even-odd) and any
/// lines that do not close (a geoline, or a ring the extract cuts off).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OsmShape {
    pub rings: Vec<Vec<[f64; 2]>>,
    pub lines: Vec<Vec<[f64; 2]>>,
}

/// What an extract holds for each request, in request order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OsmData {
    pub features: Vec<Vec<OsmFeature>>,
    pub places: Vec<Vec<OsmPlace>>,
    /// Whether the extract covers the request's whole area.
    pub covers: Vec<bool>,
    /// Outlines by Wikidata ID (`Q…`).
    pub shapes: HashMap<String, OsmShape>,
}

fn place_min_zoom(kind: &str) -> Option<u8> {
    match kind {
        "city" => Some(6),
        "town" => Some(9),
        "suburb" | "borough" => Some(12),
        "village" | "quarter" => Some(13),
        "neighbourhood" | "hamlet" => Some(15),
        _ => None,
    }
}

fn contains(bounds: [f64; 4], at: [f64; 2]) -> bool {
    at[0] >= bounds[0] && at[0] <= bounds[2] && at[1] >= bounds[1] && at[1] <= bounds[3]
}

/// A multipolygon area: its kind, the zoom it is drawn from, member ways.
type Area = (FeatureKind, u8, Vec<i64>);

/// Pass 1: relations.
struct Relations<'w> {
    wanted: &'w HashSet<String>,
    /// Wikidata ID → member ways (id, outer?).
    shapes: HashMap<String, Vec<(i64, bool)>>,
    /// Multipolygon id → (kind, min zoom, member ways).
    areas: HashMap<i64, Area>,
}

impl OsmVisitor for Relations<'_> {
    fn relation(
        &mut self,
        id: i64,
        members: &mut dyn Iterator<Item = (i64, MemberKind, &str)>,
        tags: Tags<'_, '_>,
    ) {
        let tags: Vec<(&str, &str)> = tags.collect();
        let wikidata = tags
            .iter()
            .find(|(key, _)| *key == "wikidata")
            .map(|(_, value)| *value)
            .filter(|qid| self.wanted.contains(*qid));
        let multipolygon = tags
            .iter()
            .any(|tag| *tag == ("type", "multipolygon"))
            .then(|| classify(tags.iter().copied()))
            .flatten()
            .filter(|(kind, _)| kind.is_area());
        if wikidata.is_none() && multipolygon.is_none() {
            return;
        }
        let ways: Vec<(i64, bool)> = members
            .filter(|(_, kind, _)| *kind == MemberKind::Way)
            .map(|(way, _, role)| (way, role != "inner"))
            .collect();
        if let Some(qid) = wikidata {
            self.shapes
                .entry(qid.to_owned())
                .or_default()
                .extend(ways.iter().copied());
        }
        if let Some((kind, zoom)) = multipolygon {
            self.areas.insert(
                id,
                (kind, zoom, ways.into_iter().map(|(way, _)| way).collect()),
            );
        }
    }
}

/// Pass 2 and 4: node coordinates.
struct Nodes<'r> {
    areas: &'r [[f64; 4]],
    /// Ids whose coordinates are needed wherever they are (pass 4).
    needed: Option<&'r HashSet<i64>>,
    found: HashMap<i64, [f64; 2]>,
    places: Vec<OsmPlace>,
}

impl OsmVisitor for Nodes<'_> {
    fn node(&mut self, id: i64, at: [f64; 2], tags: Tags<'_, '_>) {
        if let Some(needed) = self.needed {
            if needed.contains(&id) {
                self.found.insert(id, at);
            }
            return;
        }
        if !self.areas.iter().any(|area| contains(*area, at)) {
            return;
        }
        self.found.insert(id, at);
        let (mut name, mut zoom) = (None, None);
        for (key, value) in tags {
            match key {
                "name" => name = Some(value.to_owned()),
                "place" => zoom = place_min_zoom(value),
                _ => {}
            }
        }
        if let (Some(name), Some(min_zoom)) = (name, zoom) {
            self.places.push(OsmPlace { at, name, min_zoom });
        }
    }
}

/// Pass 3: ways.
struct Ways<'p> {
    near: &'p HashMap<i64, [f64; 2]>,
    wanted: &'p HashSet<String>,
    relation_ways: &'p HashSet<i64>,
    features: Vec<(FeatureKind, u8, Vec<i64>)>,
    /// Members of relations (and Wikidata-tagged ways), by way id.
    members: HashMap<i64, Vec<i64>>,
    tagged_lines: HashMap<String, Vec<i64>>,
    /// Multipolygon member ways that touch a map's area.
    touching: HashSet<i64>,
}

impl OsmVisitor for Ways<'_> {
    fn way(&mut self, id: i64, refs: &mut dyn Iterator<Item = i64>, tags: Tags<'_, '_>) {
        let tags: Vec<(&str, &str)> = tags.collect();
        let member = self.relation_ways.contains(&id);
        let wikidata = tags
            .iter()
            .find(|(key, _)| *key == "wikidata")
            .map(|(_, value)| *value)
            .filter(|qid| self.wanted.contains(*qid));
        let feature = classify(tags.iter().copied());
        if !member && wikidata.is_none() && feature.is_none() {
            return;
        }
        let refs: Vec<i64> = refs.collect();
        let near = refs.iter().any(|node| self.near.contains_key(node));
        if member {
            if near {
                self.touching.insert(id);
            }
            self.members.insert(id, refs.clone());
        }
        if let Some(qid) = wikidata {
            self.tagged_lines.insert(qid.to_owned(), refs.clone());
        }
        if let Some((kind, zoom)) = feature {
            if near {
                self.features.push((kind, zoom, refs));
            }
        }
    }
}

/// Join ways into the longest chains their shared end nodes allow (a
/// closed ring stops growing). Ends are indexed, so tens of thousands of
/// short ways (a motorway network) join in near-linear time.
fn join(ways: Vec<Vec<i64>>) -> Vec<Vec<i64>> {
    let mut ways: Vec<Option<Vec<i64>>> = ways
        .into_iter()
        .filter(|way| way.len() > 1)
        .map(Some)
        .collect();
    let mut ends: HashMap<i64, Vec<usize>> = HashMap::new();
    for (index, way) in ways.iter().enumerate() {
        let way = way.as_ref().expect("all present");
        ends.entry(way[0]).or_default().push(index);
        ends.entry(way[way.len() - 1]).or_default().push(index);
    }
    let mut chains = Vec::new();
    for start in 0..ways.len() {
        let Some(mut chain) = ways[start].take() else {
            continue;
        };
        // Grow at the tail, then at the head.
        for at_tail in [true, false] {
            loop {
                if chain.len() > 3 && chain.first() == chain.last() {
                    break;
                }
                let end = if at_tail {
                    chain[chain.len() - 1]
                } else {
                    chain[0]
                };
                let Some(next) = ends
                    .get(&end)
                    .and_then(|candidates| candidates.iter().copied().find(|i| ways[*i].is_some()))
                else {
                    break;
                };
                let mut way = ways[next].take().expect("checked");
                if at_tail {
                    if way[0] != end {
                        way.reverse();
                    }
                    chain.extend_from_slice(&way[1..]);
                } else {
                    if way[way.len() - 1] != end {
                        way.reverse();
                    }
                    way.extend_from_slice(&chain[1..]);
                    chain = way;
                }
            }
        }
        chains.push(chain);
    }
    chains
}

/// Node ids to coordinates, splitting where a node is missing.
fn locate(refs: &[i64], nodes: &HashMap<i64, [f64; 2]>) -> Vec<Vec<[f64; 2]>> {
    let mut runs = vec![Vec::new()];
    for node in refs {
        match nodes.get(node) {
            Some(at) => runs.last_mut().expect("one run").push(*at),
            None => runs.push(Vec::new()),
        }
    }
    runs.into_iter().filter(|run| run.len() > 1).collect()
}

fn bounds_of(parts: &[Vec<[f64; 2]>]) -> [f64; 4] {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for at in parts.iter().flatten() {
        b = [
            b[0].min(at[0]),
            b[1].min(at[1]),
            b[2].max(at[0]),
            b[3].max(at[1]),
        ];
    }
    b
}

fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// Read what `requests` need from `source`.
pub fn extract(source: &dyn OsmSource, requests: &[OsmRequest]) -> Result<OsmData, String> {
    let wanted: HashSet<String> = requests
        .iter()
        .flat_map(|request| request.shapes.iter().cloned())
        .collect();
    let areas: Vec<[f64; 4]> = requests.iter().filter_map(|r| r.bounds).collect();

    let mut relations = Relations {
        wanted: &wanted,
        shapes: HashMap::new(),
        areas: HashMap::new(),
    };
    source.visit(&mut relations)?;
    let relation_ways: HashSet<i64> = relations
        .shapes
        .values()
        .flatten()
        .map(|(way, _)| *way)
        .chain(
            relations
                .areas
                .values()
                .flat_map(|(_, _, ways)| ways.iter().copied()),
        )
        .collect();

    let mut nodes = Nodes {
        areas: &areas,
        needed: None,
        found: HashMap::new(),
        places: Vec::new(),
    };
    source.visit(&mut nodes)?;
    let near = nodes.found;
    let places = nodes.places;

    let mut ways = Ways {
        near: &near,
        wanted: &wanted,
        relation_ways: &relation_ways,
        features: Vec::new(),
        members: HashMap::new(),
        tagged_lines: HashMap::new(),
        touching: HashSet::new(),
    };
    source.visit(&mut ways)?;
    let Ways {
        features: way_features,
        members: way_members,
        tagged_lines,
        touching,
        ..
    } = ways;

    // Areas drawn: multipolygons with a member touching a map's area.
    let mut touched: Vec<(i64, Area)> = relations
        .areas
        .into_iter()
        .filter(|(_, (_, _, members))| members.iter().any(|way| touching.contains(way)))
        .collect();
    // Ordered by relation id, so the same extract always draws the same map.
    touched.sort_by_key(|(id, _)| *id);
    let touched_areas: Vec<Area> = touched.into_iter().map(|(_, area)| area).collect();
    let mut needed: HashSet<i64> = HashSet::new();
    let mut need = |refs: &[i64]| {
        needed.extend(refs.iter().filter(|node| !near.contains_key(node)).copied());
    };
    for (_, _, refs) in &way_features {
        need(refs);
    }
    for (_, _, members) in &touched_areas {
        for way in members {
            if let Some(refs) = way_members.get(way) {
                need(refs);
            }
        }
    }
    for members in relations.shapes.values() {
        for (way, _) in members {
            if let Some(refs) = way_members.get(way) {
                need(refs);
            }
        }
    }
    for refs in tagged_lines.values() {
        need(refs);
    }
    let mut far = Nodes {
        areas: &[],
        needed: Some(&needed),
        found: HashMap::new(),
        places: Vec::new(),
    };
    if !needed.is_empty() {
        source.visit(&mut far)?;
    }
    let mut all = near;
    all.extend(far.found);

    // Every feature, located, with its bounds.
    let mut located: Vec<(OsmFeature, u8, [f64; 4])> = Vec::new();
    // Lines of one kind and zoom are merged into chains (one OSM road is
    // often hundreds of short ways); areas stay as they are.
    // Ordered, so the same extract always draws the same map.
    let mut lines: std::collections::BTreeMap<(FeatureKind, u8), Vec<Vec<i64>>> =
        std::collections::BTreeMap::new();
    for (kind, zoom, refs) in way_features {
        if kind.is_area() {
            let parts = locate(&refs, &all);
            if !parts.is_empty() {
                let bounds = bounds_of(&parts);
                located.push((OsmFeature { kind, parts }, zoom, bounds));
            }
        } else {
            lines.entry((kind, zoom)).or_default().push(refs);
        }
    }
    for ((kind, zoom), ways) in lines {
        for chain in join(ways) {
            let parts = locate(&chain, &all);
            if !parts.is_empty() {
                let bounds = bounds_of(&parts);
                located.push((OsmFeature { kind, parts }, zoom, bounds));
            }
        }
    }
    for (kind, zoom, members) in touched_areas {
        let chains = join(
            members
                .iter()
                .filter_map(|way| way_members.get(way).cloned())
                .collect(),
        );
        let rings: Vec<Vec<[f64; 2]>> = chains
            .iter()
            .filter(|chain| chain.first() == chain.last())
            .flat_map(|chain| locate(chain, &all))
            .collect();
        if !rings.is_empty() {
            let bounds = bounds_of(&rings);
            located.push((OsmFeature { kind, parts: rings }, zoom, bounds));
        }
    }

    let mut shapes: HashMap<String, OsmShape> = HashMap::new();
    for (qid, members) in relations.shapes {
        let chains = join(
            members
                .iter()
                .filter_map(|(way, _)| way_members.get(way).cloned())
                .collect(),
        );
        let shape = shapes.entry(qid).or_default();
        for chain in chains {
            let closed = chain.len() > 3 && chain.first() == chain.last();
            let parts = locate(&chain, &all);
            // A ring the extract cuts off is drawn as lines.
            if closed && parts.len() == 1 && parts[0].len() == chain.len() {
                shape.rings.extend(parts);
            } else {
                shape.lines.extend(parts);
            }
        }
    }
    for (qid, refs) in tagged_lines {
        let parts = locate(&refs, &all);
        shapes.entry(qid).or_default().lines.extend(parts);
    }
    shapes.retain(|_, shape| !shape.rings.is_empty() || !shape.lines.is_empty());

    let mut data = OsmData {
        shapes,
        ..OsmData::default()
    };
    for request in requests {
        let Some(area) = request.bounds else {
            data.features.push(Vec::new());
            data.places.push(Vec::new());
            data.covers.push(false);
            continue;
        };
        data.covers.push(source.bounds().is_none_or(|b| {
            b[0] <= area[0] && b[1] <= area[1] && b[2] >= area[2] && b[3] >= area[3]
        }));
        data.features.push(
            located
                .iter()
                .filter(|(_, zoom, bounds)| *zoom <= request.zoom && overlaps(*bounds, area))
                .map(|(feature, _, _)| feature.clone())
                .collect(),
        );
        data.places.push(
            places
                .iter()
                .filter(|place| place.min_zoom <= request.zoom && contains(area, place.at))
                .cloned()
                .collect(),
        );
    }
    Ok(data)
}

/// An in-memory extract, for tests.
#[cfg(test)]
pub(crate) mod memory {
    use super::*;

    pub(crate) enum Element {
        Node(i64, [f64; 2], Vec<(&'static str, &'static str)>),
        Way(i64, Vec<i64>, Vec<(&'static str, &'static str)>),
        Relation(
            i64,
            Vec<(i64, MemberKind, &'static str)>,
            Vec<(&'static str, &'static str)>,
        ),
    }

    pub(crate) struct Memory {
        pub(crate) elements: Vec<Element>,
        pub(crate) bounds: Option<[f64; 4]>,
        pub(crate) passes: std::cell::Cell<usize>,
    }

    impl OsmSource for Memory {
        fn visit(&self, visitor: &mut dyn OsmVisitor) -> Result<(), String> {
            self.passes.set(self.passes.get() + 1);
            for element in &self.elements {
                match element {
                    Element::Node(id, at, tags) => {
                        visitor.node(*id, *at, &mut tags.iter().copied())
                    }
                    Element::Way(id, refs, tags) => {
                        visitor.way(*id, &mut refs.iter().copied(), &mut tags.iter().copied())
                    }
                    Element::Relation(id, members, tags) => visitor.relation(
                        *id,
                        &mut members.iter().copied(),
                        &mut tags.iter().copied(),
                    ),
                }
            }
            Ok(())
        }

        fn bounds(&self) -> Option<[f64; 4]> {
            self.bounds
        }
    }
}

#[cfg(test)]
mod tests {
    use super::memory::{Element, Memory};
    use super::*;

    /// A small town at (0, 0): a primary road and a footpath through it, a
    /// building, a pond, a park multipolygon from two ways, a far-away road,
    /// a tunnel, a place, and a district outline (Wikidata Q42) of two ways
    /// with nodes outside the map's area.
    fn town() -> Memory {
        use Element::*;
        let node = |id, x: f64, y: f64| Node(id, [x, y], vec![]);
        let mut elements = vec![
            node(1, -0.01, 0.0),
            node(2, 0.01, 0.0),
            node(3, 0.0, -0.01),
            node(4, 0.0, 0.01),
            node(5, 0.001, 0.001),
            node(6, 0.002, 0.001),
            node(7, 0.002, 0.002),
            node(8, 0.003, -0.003),
            node(9, 0.004, -0.003),
            node(10, 0.004, -0.004),
            node(11, -0.003, 0.003),
            node(12, -0.002, 0.003),
            node(13, -0.002, 0.004),
            node(20, 5.0, 5.0),
            node(21, 5.1, 5.0),
            // Outline corners far outside the map's area, and one inside.
            node(30, -1.0, -1.0),
            node(31, 1.0, -1.0),
            node(32, 1.0, 1.0),
            node(33, -1.0, 1.0),
            Node(
                40,
                [0.0005, 0.0005],
                vec![("place", "suburb"), ("name", "Old Town")],
            ),
            Node(
                41,
                [0.0006, 0.0006],
                vec![("place", "neighbourhood"), ("name", "Tiny")],
            ),
        ];
        elements.extend([
            Way(100, vec![1, 2], vec![("highway", "primary")]),
            Way(101, vec![3, 4], vec![("highway", "footway")]),
            Way(102, vec![5, 6, 7, 5], vec![("building", "yes")]),
            Way(103, vec![8, 9, 10, 8], vec![("natural", "water")]),
            Way(104, vec![11, 12], vec![]),
            Way(105, vec![12, 13, 11], vec![]),
            Way(106, vec![20, 21], vec![("highway", "motorway")]),
            Way(
                107,
                vec![1, 4],
                vec![("highway", "primary"), ("tunnel", "yes")],
            ),
            Way(110, vec![30, 31, 32], vec![]),
            Way(111, vec![32, 33, 30], vec![]),
            Relation(
                200,
                vec![
                    (104, MemberKind::Way, "outer"),
                    (105, MemberKind::Way, "outer"),
                ],
                vec![("type", "multipolygon"), ("leisure", "park")],
            ),
            Relation(
                201,
                vec![
                    (110, MemberKind::Way, "outer"),
                    (111, MemberKind::Way, "outer"),
                    (40, MemberKind::Node, "label"),
                ],
                vec![("type", "boundary"), ("wikidata", "Q42")],
            ),
        ]);
        Memory {
            elements,
            bounds: Some([-2.0, -2.0, 2.0, 2.0]),
            passes: std::cell::Cell::new(0),
        }
    }

    fn request(zoom: u8) -> OsmRequest {
        OsmRequest {
            bounds: Some([-0.02, -0.02, 0.02, 0.02]),
            zoom,
            shapes: vec!["Q42".into()],
        }
    }

    fn kinds(features: &[OsmFeature]) -> Vec<FeatureKind> {
        let mut kinds: Vec<FeatureKind> = features.iter().map(|f| f.kind).collect();
        kinds.sort();
        kinds
    }

    #[test]
    fn features_in_the_maps_area_are_read_for_its_zoom() {
        let source = town();
        let data = extract(&source, &[request(16), request(12)]).unwrap();
        assert_eq!(source.passes.get(), 4, "relations, nodes, ways, far nodes");
        // Close up: everything in the area, but no tunnel or far-away road.
        assert_eq!(
            kinds(&data.features[0]),
            vec![
                FeatureKind::Green,
                FeatureKind::Water,
                FeatureKind::Building,
                FeatureKind::Road(1),
                FeatureKind::Road(6),
            ]
        );
        // Further out: no buildings or footpaths.
        assert_eq!(
            kinds(&data.features[1]),
            vec![FeatureKind::Green, FeatureKind::Water, FeatureKind::Road(1)]
        );
        // The park multipolygon is assembled into one closed ring.
        let park = data.features[0]
            .iter()
            .find(|f| f.kind == FeatureKind::Green)
            .unwrap();
        assert_eq!(park.parts.len(), 1);
        assert_eq!(park.parts[0].first(), park.parts[0].last());
        assert_eq!(park.parts[0].len(), 4);
        assert_eq!(
            data.places[0]
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["Old Town", "Tiny"]
        );
        assert_eq!(data.places[1].len(), 1, "neighbourhoods only close up");
        assert_eq!(data.covers, [true, true]);
    }

    #[test]
    fn a_wikidata_outline_is_assembled_from_its_relation_wherever_its_nodes_are() {
        let data = extract(&town(), &[request(12)]).unwrap();
        let outline = &data.shapes["Q42"];
        assert_eq!(outline.rings.len(), 1);
        assert!(outline.lines.is_empty());
        assert_eq!(outline.rings[0].len(), 5);
        assert!(outline.rings[0].contains(&[1.0, 1.0]));
        // An outline the extract cuts off (a member way missing) is lines.
        let mut cut = town();
        cut.elements.retain(|e| !matches!(e, Element::Way(111, ..)));
        let data = extract(&cut, &[request(12)]).unwrap();
        assert!(data.shapes["Q42"].rings.is_empty());
        assert_eq!(data.shapes["Q42"].lines.len(), 1);
        // A shape only (no area) still reads the outline.
        let data = extract(
            &town(),
            &[OsmRequest {
                bounds: None,
                zoom: 9,
                shapes: vec!["Q42".into()],
            }],
        )
        .unwrap();
        assert_eq!(data.shapes["Q42"].rings.len(), 1);
        assert_eq!(data.covers, [false]);
    }

    #[test]
    fn an_area_outside_the_extract_is_not_covered() {
        let data = extract(
            &town(),
            &[OsmRequest {
                bounds: Some([1.5, 1.5, 2.5, 2.5]),
                zoom: 12,
                shapes: vec![],
            }],
        )
        .unwrap();
        assert_eq!(data.covers, [false]);
    }

    #[test]
    fn consecutive_ways_of_one_road_class_are_drawn_as_one_line() {
        use Element::*;
        let mut source = town();
        // The primary road continues east in two more ways.
        source.elements.splice(
            0..0,
            [
                Node(50, [0.012, 0.0], vec![]),
                Node(51, [0.014, 0.0], vec![]),
            ],
        );
        let at = source
            .elements
            .iter()
            .position(|e| matches!(e, Way(..)))
            .unwrap();
        source.elements.splice(
            at..at,
            [
                Way(150, vec![50, 2], vec![("highway", "primary")]),
                Way(151, vec![50, 51], vec![("highway", "primary")]),
            ],
        );
        let data = extract(&source, &[request(12)]).unwrap();
        let primary: Vec<&OsmFeature> = data.features[0]
            .iter()
            .filter(|f| f.kind == FeatureKind::Road(1))
            .collect();
        assert_eq!(primary.len(), 1, "{primary:?}");
        let mut line = primary[0].parts[0].clone();
        if line[0][0] > 0.0 {
            line.reverse();
        }
        assert_eq!(primary[0].parts.len(), 1);
        assert_eq!(
            line,
            vec![[-0.01, 0.0], [0.01, 0.0], [0.012, 0.0], [0.014, 0.0]]
        );
    }

    #[test]
    fn ways_join_into_chains_in_either_direction() {
        assert_eq!(
            join(vec![vec![1, 2], vec![3, 2], vec![3, 1]]),
            // The ring 1-2-3-1, joined in either direction.
            vec![vec![1, 2, 3, 1]]
        );
        let mut open = join(vec![vec![1, 2], vec![2, 3], vec![7, 8]]);
        open.sort();
        assert_eq!(open, vec![vec![1, 2, 3], vec![7, 8]]);
        assert_eq!(
            classify([("highway", "primary"), ("tunnel", "yes")].into_iter()),
            None
        );
        assert_eq!(
            classify([("building", "yes"), ("highway", "footway")].into_iter()),
            Some((FeatureKind::Road(6), 15))
        );
    }
}
