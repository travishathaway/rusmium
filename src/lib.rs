//! # rusmium
//!
//! Idiomatic Rust bindings for [libosmium](https://osmcode.org/libosmium/), the
//! C++ library for reading and writing OpenStreetMap data.
//!
//! The crate is layered: a hand-written C++ shim presents a concrete ABI over
//! osmium's templated, header-only API; the `ffi` module bridges it with the
//! `cxx` crate; and this module builds the safe, idiomatic surface on top.
//!
//! Objects are yielded as **owned** Rust values (copy-out): a value you keep
//! stays valid after the iterator advances.
//!
//! Build and test through pixi so the native toolchain is available:
//!
//! ```text
//! pixi run build
//! pixi run test
//! ```

use std::fmt;
use std::path::Path;

use cxx::UniquePtr;

mod ffi;

/// Returns the version of the libosmium headers this crate was built against
/// (e.g. `"2.23.1"`).
pub fn osmium_version() -> String {
    ffi::osmium_version()
}

/// The three kinds of primary OSM object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Node,
    Way,
    Relation,
}

impl ObjectKind {
    /// Decode libosmium's `item_type` byte (node=1, way=2, relation=3).
    ///
    /// Exposed because the allocation-free bulk accessors — notably
    /// [`ObjectRef::relation_members_into`] — hand back member kinds in that
    /// raw encoding, since a Rust enum cannot cross the C++ bridge directly.
    pub fn from_item_type(v: u8) -> Option<Self> {
        match v {
            1 => Some(ObjectKind::Node),
            2 => Some(ObjectKind::Way),
            3 => Some(ObjectKind::Relation),
            _ => None,
        }
    }

    fn from_raw(v: u8) -> Self {
        Self::from_item_type(v)
            .unwrap_or_else(|| unreachable!("libosmium yielded an unexpected item_type: {v}"))
    }
}

/// A geographic location (WGS84 degrees).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Location {
    pub lon: f64,
    pub lat: f64,
}

/// An axis-aligned WGS84 bounding box with inclusive bounds.
///
/// The corners are stored as given — `new` does **not** reorder longitudes — so
/// a box whose `min.lon` exceeds `max.lon` is meaningful: it denotes a region
/// that crosses the ±180° antimeridian, and [`Bbox::contains`] treats longitude
/// as wrapping for it. Latitudes are expected to satisfy `min.lat <= max.lat`.
///
/// ```
/// use rusmium::{Bbox, Location};
/// let berlin: Bbox = "13.0,52.0,13.5,52.5".parse().unwrap();
/// assert!(berlin.contains(&Location { lon: 13.4, lat: 52.5 })); // inclusive edge
/// assert!(!berlin.contains(&Location { lon: 14.0, lat: 52.5 }));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bbox {
    /// South-west corner (minimum longitude and latitude).
    pub min: Location,
    /// North-east corner (maximum longitude and latitude).
    pub max: Location,
}

impl Bbox {
    /// Build a box from its minimum and maximum corners. Longitudes are stored
    /// as provided (see the type docs on antimeridian-crossing boxes).
    pub fn new(min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) -> Bbox {
        Bbox {
            min: Location {
                lon: min_lon,
                lat: min_lat,
            },
            max: Location {
                lon: max_lon,
                lat: max_lat,
            },
        }
    }

    /// Whether `loc` falls inside the box. Bounds are inclusive on all edges.
    ///
    /// Latitude is tested as `min.lat <= lat <= max.lat`. Longitude is tested as
    /// `min.lon <= lon <= max.lon` for a normal box; for an antimeridian-crossing
    /// box (`min.lon > max.lon`) longitude wraps, matching `lon >= min.lon` OR
    /// `lon <= max.lon`.
    pub fn contains(&self, loc: &Location) -> bool {
        let lat_in = loc.lat >= self.min.lat && loc.lat <= self.max.lat;
        let lon_in = if self.min.lon <= self.max.lon {
            loc.lon >= self.min.lon && loc.lon <= self.max.lon
        } else {
            loc.lon >= self.min.lon || loc.lon <= self.max.lon
        };
        lat_in && lon_in
    }
}

/// Error returned when parsing a [`Bbox`] from its string form fails.
#[derive(Debug, Clone, PartialEq)]
pub struct BboxParseError(String);

impl fmt::Display for BboxParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid bounding box: {}", self.0)
    }
}

impl std::error::Error for BboxParseError {}

impl std::str::FromStr for Bbox {
    type Err = BboxParseError;

    /// Parse `"min_lon,min_lat,max_lon,max_lat"` (four comma-separated numbers).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.len() != 4 {
            return Err(BboxParseError(format!(
                "expected 4 comma-separated numbers (min_lon,min_lat,max_lon,max_lat), got {}",
                parts.len()
            )));
        }
        let mut vals = [0.0f64; 4];
        for (i, part) in parts.iter().enumerate() {
            vals[i] = part.trim().parse::<f64>().map_err(|_| {
                BboxParseError(format!("field {} is not a number: {:?}", i + 1, part))
            })?;
        }
        Ok(Bbox::new(vals[0], vals[1], vals[2], vals[3]))
    }
}

/// A member of a relation.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    /// Kind of the referenced object.
    pub kind: ObjectKind,
    /// OSM id of the referenced object.
    pub ref_id: i64,
    /// Role string (may be empty).
    pub role: String,
}

/// Kind-specific payload of an [`Object`].
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// A node and its location (absent if the file carries no location for it).
    Node { location: Option<Location> },
    /// A way and its ordered node references.
    Way { nodes: Vec<i64> },
    /// A relation and its members.
    Relation { members: Vec<Member> },
}

/// A single OSM object yielded by a [`Reader`].
///
/// Copy-out ownership: all data is owned, so an `Object` remains valid and
/// unchanged after the reader advances or is dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    kind: ObjectKind,
    id: i64,
    version: u32,
    tags: Vec<(String, String)>,
    body: Body,
}

impl Object {
    /// Construct a node.
    pub fn node(
        id: i64,
        version: u32,
        location: Option<Location>,
        tags: Vec<(String, String)>,
    ) -> Object {
        Object {
            kind: ObjectKind::Node,
            id,
            version,
            tags,
            body: Body::Node { location },
        }
    }

    /// Construct a way from an ordered list of node references.
    pub fn way(id: i64, version: u32, nodes: Vec<i64>, tags: Vec<(String, String)>) -> Object {
        Object {
            kind: ObjectKind::Way,
            id,
            version,
            tags,
            body: Body::Way { nodes },
        }
    }

    /// Construct a relation from its members.
    pub fn relation(
        id: i64,
        version: u32,
        members: Vec<Member>,
        tags: Vec<(String, String)>,
    ) -> Object {
        Object {
            kind: ObjectKind::Relation,
            id,
            version,
            tags,
            body: Body::Relation { members },
        }
    }

    /// Whether this object is a node, way, or relation.
    pub fn kind(&self) -> ObjectKind {
        self.kind
    }

    /// The OSM id.
    pub fn id(&self) -> i64 {
        self.id
    }

    /// The object version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Tags as `(key, value)` pairs, in file order.
    pub fn tags(&self) -> &[(String, String)] {
        &self.tags
    }

    /// The kind-specific body (location / node refs / members).
    pub fn body(&self) -> &Body {
        &self.body
    }

    /// Location, if this is a node with a valid location.
    pub fn location(&self) -> Option<Location> {
        match &self.body {
            Body::Node { location } => *location,
            _ => None,
        }
    }
}

/// A set of object kinds, used to tell a [`Reader`] which ones to decode.
///
/// Combine with `|`:
///
/// ```
/// use rusmium::Entities;
/// let nw = Entities::NODE | Entities::WAY;
/// assert!(nw.contains(Entities::WAY));
/// assert!(!nw.contains(Entities::RELATION));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entities(u8);

impl Entities {
    // Values are libosmium's `osm_entity_bits` and must stay in sync with it.
    pub const NODE: Entities = Entities(0x01);
    pub const WAY: Entities = Entities(0x02);
    pub const RELATION: Entities = Entities(0x04);
    /// Nodes, ways, and relations — the default.
    pub const ALL: Entities = Entities(0x07);

    /// Whether every kind in `other` is present in `self`.
    pub const fn contains(self, other: Entities) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for Entities {
    type Output = Entities;
    fn bitor(self, rhs: Entities) -> Entities {
        Entities(self.0 | rhs.0)
    }
}

/// How much of each object a [`Reader`] should decode.
///
/// Decoding an OSM file is dominated by the per-object work, so a pass that
/// only needs ids and geometry can be markedly faster if it says so. Defaults
/// are full fidelity; each opt-out trades information for speed.
///
/// ```no_run
/// # fn main() -> Result<(), rusmium::Error> {
/// use rusmium::{Entities, ReadOptions, Reader};
/// // A pass that only locates nodes: no metadata, no tags, no ways/relations.
/// let opts = ReadOptions::default()
///     .metadata(false)
///     .tags(false)
///     .entities(Entities::NODE);
/// for node in Reader::open_with("map.osm.pbf", opts)? {
///     let _ = node.location();
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadOptions {
    metadata: bool,
    tags: bool,
    entities: Entities,
}

impl Default for ReadOptions {
    fn default() -> Self {
        ReadOptions {
            metadata: true,
            tags: true,
            entities: Entities::ALL,
        }
    }
}

impl ReadOptions {
    /// Whether to decode object metadata. With `false`, the parser skips the
    /// user name, uid, timestamp and changeset of every object, and
    /// [`Object::version`] reads as `0`.
    ///
    /// Honored by the **PBF** reader only — libosmium's XML parser always
    /// reads metadata, so this is a no-op on `.osm` input.
    pub fn metadata(mut self, yes: bool) -> Self {
        self.metadata = yes;
        self
    }

    /// Whether to copy out tags. With `false`, [`Object::tags`] is always
    /// empty and no per-object tag allocation happens.
    pub fn tags(mut self, yes: bool) -> Self {
        self.tags = yes;
        self
    }

    /// Which kinds of object to decode. Kinds left out are never yielded.
    pub fn entities(mut self, entities: Entities) -> Self {
        self.entities = entities;
        self
    }
}

/// An error from opening or reading an OSM file.
#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<cxx::Exception> for Error {
    fn from(e: cxx::Exception) -> Self {
        Error(e.what().to_string())
    }
}

/// Reads OSM objects from a file as an [`Iterator`].
///
/// ```no_run
/// # fn main() -> Result<(), rusmium::Error> {
/// use rusmium::{Reader, ObjectKind};
/// let mut nodes = 0;
/// for obj in Reader::open("map.osm.pbf")? {
///     if obj.kind() == ObjectKind::Node {
///         nodes += 1;
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub struct Reader {
    // Field order is load-bearing: `cursor` borrows into `reader`'s C++ object,
    // so it must be dropped first. Rust drops fields in declaration order.
    cursor: UniquePtr<ffi::Cursor>,
    #[allow(dead_code)]
    reader: UniquePtr<ffi::OsmReader>,
    read_tags: bool,
}

impl Reader {
    /// Open an OSM file for reading. The format is detected from the path
    /// (e.g. `.osm.pbf`, `.osm`). Returns an [`Error`] if the file cannot be
    /// opened or is not valid OSM data.
    ///
    /// Everything is decoded. Use [`Reader::open_with`] to skip parts you do
    /// not need.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        Self::open_with(path, ReadOptions::default())
    }

    /// Open an OSM file, decoding only what `options` asks for. See
    /// [`ReadOptions`].
    pub fn open_with(path: impl AsRef<Path>, options: ReadOptions) -> Result<Self, Error> {
        let path = path
            .as_ref()
            .to_str()
            .ok_or_else(|| Error("path is not valid UTF-8".to_string()))?;
        let mut reader = ffi::open_reader(path, options.entities.0, options.metadata)?;
        let cursor = ffi::make_cursor(reader.pin_mut());
        Ok(Reader {
            cursor,
            reader,
            read_tags: options.tags,
        })
    }

    /// Advance to the next object and borrow it in place, copying nothing out
    /// of the decode buffer.
    ///
    /// This is the fast path for filtering and counting: accessors on the
    /// returned [`ObjectRef`] fetch on demand, so a caller that only looks at
    /// ids pays only for ids. Pair it with [`Writer::copy`] to move objects
    /// through without ever materializing them.
    ///
    /// The view borrows the reader, so it cannot be held across the next call
    /// or stored — the borrow checker enforces that. Call
    /// [`ObjectRef::to_owned`] to lift one out.
    ///
    /// Note this shares its position with the [`Iterator`] impl: mixing
    /// `next_ref` and `next` on one reader skips objects.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), rusmium::Error> {
    /// use rusmium::{ObjectKind, Reader};
    /// let mut reader = Reader::open("map.osm.pbf")?;
    /// let mut nodes = 0;
    /// while let Some(obj) = reader.next_ref() {
    ///     if obj.kind() == ObjectKind::Node {
    ///         nodes += 1;
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn next_ref(&mut self) -> Option<ObjectRef<'_>> {
        if !ffi::advance(self.cursor.pin_mut()) {
            return None;
        }
        Some(ObjectRef {
            cursor: self.cursor.as_ref().expect("cursor is never null"),
            read_tags: self.read_tags,
        })
    }
}

/// An OSM object borrowed in place from a [`Reader`]'s decode buffer.
///
/// Nothing is copied until you ask for it. Each accessor crosses into C++ on
/// demand, so the cost of an object is the cost of the fields you actually
/// read — unlike [`Object`], which materializes everything up front.
///
/// Yielded by [`Reader::next_ref`], and valid only until the reader advances.
pub struct ObjectRef<'a> {
    cursor: &'a ffi::Cursor,
    read_tags: bool,
}

impl ObjectRef<'_> {
    /// Whether this object is a node, way, or relation.
    pub fn kind(&self) -> ObjectKind {
        ObjectKind::from_raw(ffi::object_kind(self.cursor))
    }

    /// The OSM id.
    pub fn id(&self) -> i64 {
        ffi::object_id(self.cursor)
    }

    /// The object version.
    pub fn version(&self) -> u32 {
        ffi::object_version(self.cursor)
    }

    /// Location, if this is a node with a valid location.
    ///
    /// `None` for ways and relations. The kind check is load-bearing, not a
    /// convenience: the underlying accessors downcast to `osmium::Node`, which
    /// is only valid once the object is known to be one.
    pub fn location(&self) -> Option<Location> {
        if self.kind() != ObjectKind::Node || !ffi::node_location_valid(self.cursor) {
            return None;
        }
        Some(Location {
            lon: ffi::node_lon(self.cursor),
            lat: ffi::node_lat(self.cursor),
        })
    }

    /// How many tags this object carries.
    pub fn tag_count(&self) -> usize {
        ffi::tag_count(self.cursor)
    }

    /// Last-edit time as seconds since the Unix epoch, or `None` if unset.
    ///
    /// Metadata is only present when the reader was opened with
    /// [`ReadOptions::metadata`] left on (the default). [`Object`] carries no
    /// metadata at all, so this view is the only way to read it — and
    /// [`Writer::copy`] the only way to preserve it.
    pub fn timestamp(&self) -> Option<i64> {
        match ffi::object_timestamp(self.cursor) {
            0 => None,
            secs => Some(secs),
        }
    }

    /// Id of the user who last edited this object (`0` when unset).
    pub fn uid(&self) -> u32 {
        ffi::object_uid(self.cursor)
    }

    /// Name of the user who last edited this object (empty when unset).
    pub fn user(&self) -> String {
        ffi::object_user(self.cursor)
    }

    /// Changeset this object's last edit belonged to (`0` when unset).
    pub fn changeset(&self) -> i64 {
        ffi::object_changeset(self.cursor)
    }

    /// Replace `out` with this way's ordered node references.
    ///
    /// Reusing one buffer across a pass keeps the loop allocation-free. Empty
    /// for objects that are not ways.
    pub fn way_node_refs_into(&self, out: &mut Vec<i64>) {
        if self.kind() != ObjectKind::Way {
            out.clear();
            return;
        }
        ffi::way_node_refs_into(self.cursor, out);
    }

    /// Replace `kinds` and `refs` with this relation's member kinds and
    /// referenced ids, as parallel vectors. Roles are not fetched — use
    /// [`ObjectRef::to_owned`] when you need them. Empty for non-relations.
    pub fn relation_members_into(&self, kinds: &mut Vec<u8>, refs: &mut Vec<i64>) {
        if self.kind() != ObjectKind::Relation {
            kinds.clear();
            refs.clear();
            return;
        }
        ffi::relation_members_into(self.cursor, kinds, refs);
    }

    /// Copy this object out into a self-contained [`Object`], the same value
    /// the [`Iterator`] impl on [`Reader`] yields.
    pub fn to_owned(&self) -> Object {
        let c = self.cursor;
        let kind = ObjectKind::from_raw(ffi::object_kind(c));
        // Fetching tags costs two vector-returning FFI calls and a heap string
        // per tag. Most nodes carry none, so check the count first.
        let tags = if self.read_tags && ffi::tag_count(c) > 0 {
            ffi::tag_keys(c)
                .into_iter()
                .zip(ffi::tag_values(c))
                .collect()
        } else {
            Vec::new()
        };
        let body = match kind {
            ObjectKind::Node => Body::Node {
                location: self.location(),
            },
            ObjectKind::Way => Body::Way {
                nodes: ffi::way_node_refs(c),
            },
            ObjectKind::Relation => {
                let kinds = ffi::relation_member_kinds(c);
                let refs = ffi::relation_member_refs(c);
                let roles = ffi::relation_member_roles(c);
                let members = kinds
                    .into_iter()
                    .zip(refs)
                    .zip(roles)
                    .map(|((k, ref_id), role)| Member {
                        kind: ObjectKind::from_raw(k),
                        ref_id,
                        role,
                    })
                    .collect();
                Body::Relation { members }
            }
        };
        Object {
            kind,
            id: ffi::object_id(c),
            version: ffi::object_version(c),
            tags,
            body,
        }
    }
}

impl Iterator for Reader {
    type Item = Object;

    fn next(&mut self) -> Option<Object> {
        self.next_ref().map(|obj| obj.to_owned())
    }
}

/// Writes OSM objects to a file.
///
/// The output format is detected from the path (e.g. `.osm.pbf`, `.osm`).
/// Append objects with [`Writer::add`], then call [`Writer::finish`] to flush
/// and close — dropping a `Writer` without calling `finish` does not guarantee
/// a complete file.
///
/// ```no_run
/// # fn main() -> Result<(), rusmium::Error> {
/// use rusmium::{Object, Location, Writer};
/// let mut w = Writer::create("out.osm.pbf")?;
/// w.add(&Object::node(1, 1, Some(Location { lon: 13.0, lat: 52.0 }), vec![]))?;
/// w.finish()?;
/// # Ok(())
/// # }
/// ```
pub struct Writer {
    inner: UniquePtr<ffi::OsmWriter>,
}

impl Writer {
    /// Create an OSM file for writing, overwriting any existing file. Returns
    /// an [`Error`] if the file cannot be created.
    pub fn create(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path
            .as_ref()
            .to_str()
            .ok_or_else(|| Error("path is not valid UTF-8".to_string()))?;
        Ok(Writer {
            inner: ffi::create_writer(path)?,
        })
    }

    /// Append an object to the output.
    pub fn add(&mut self, obj: &Object) -> Result<(), Error> {
        let (keys, values) = split_tags(obj.tags());
        match obj.body() {
            Body::Node { location } => {
                let (has, lon, lat) = match location {
                    Some(l) => (true, l.lon, l.lat),
                    None => (false, 0.0, 0.0),
                };
                ffi::writer_add_node(
                    self.inner.pin_mut(),
                    obj.id(),
                    obj.version(),
                    has,
                    lon,
                    lat,
                    &keys,
                    &values,
                )?;
            }
            Body::Way { nodes } => {
                ffi::writer_add_way(
                    self.inner.pin_mut(),
                    obj.id(),
                    obj.version(),
                    nodes,
                    &keys,
                    &values,
                )?;
            }
            Body::Relation { members } => {
                let kinds: Vec<u8> = members.iter().map(|m| kind_to_raw(m.kind)).collect();
                let refs: Vec<i64> = members.iter().map(|m| m.ref_id).collect();
                let roles: Vec<String> = members.iter().map(|m| m.role.clone()).collect();
                ffi::writer_add_relation(
                    self.inner.pin_mut(),
                    obj.id(),
                    obj.version(),
                    &kinds,
                    &refs,
                    &roles,
                    &keys,
                    &values,
                )?;
            }
        }
        Ok(())
    }

    /// Copy an object straight from a reader to the output.
    ///
    /// Much faster than [`Writer::add`] for filter-style jobs: the object is
    /// copied as raw bytes out of the reader's decode buffer instead of being
    /// materialized into an [`Object`] and rebuilt field by field.
    ///
    /// Being a verbatim copy, it also **preserves metadata that [`Writer::add`]
    /// drops** — timestamp, uid, changeset and user name, none of which
    /// [`Object`] can carry. If the source reader was opened with
    /// [`ReadOptions::metadata`] set to `false` that metadata was never decoded,
    /// and the copy writes it empty.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), rusmium::Error> {
    /// use rusmium::{ObjectKind, Reader, Writer};
    /// let mut reader = Reader::open("map.osm.pbf")?;
    /// let mut writer = Writer::create("ways.osm.pbf")?;
    /// while let Some(obj) = reader.next_ref() {
    ///     if obj.kind() == ObjectKind::Way {
    ///         writer.copy(&obj)?;
    ///     }
    /// }
    /// writer.finish()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn copy(&mut self, obj: &ObjectRef<'_>) -> Result<(), Error> {
        ffi::writer_copy(self.inner.pin_mut(), obj.cursor)?;
        Ok(())
    }

    /// Flush all appended objects and close the file. Consumes the writer so
    /// it cannot be used afterwards.
    pub fn finish(mut self) -> Result<(), Error> {
        ffi::finish_writer(self.inner.pin_mut())?;
        Ok(())
    }
}

fn kind_to_raw(kind: ObjectKind) -> u8 {
    match kind {
        ObjectKind::Node => 1,
        ObjectKind::Way => 2,
        ObjectKind::Relation => 3,
    }
}

fn split_tags(tags: &[(String, String)]) -> (Vec<String>, Vec<String>) {
    let keys = tags.iter().map(|(k, _)| k.clone()).collect();
    let values = tags.iter().map(|(_, v)| v.clone()).collect();
    (keys, values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_libosmium_version() {
        let v = osmium_version();
        assert!(
            v.split('.').count() == 3 && v.starts_with('2'),
            "unexpected libosmium version string: {v:?}"
        );
    }

    fn loc(lon: f64, lat: f64) -> Location {
        Location { lon, lat }
    }

    #[test]
    fn bbox_contains_inside_and_outside() {
        let b = Bbox::new(13.0, 52.0, 14.0, 53.0);
        assert!(b.contains(&loc(13.5, 52.5)));
        assert!(!b.contains(&loc(12.9, 52.5)), "west of box");
        assert!(!b.contains(&loc(14.1, 52.5)), "east of box");
        assert!(!b.contains(&loc(13.5, 51.9)), "south of box");
        assert!(!b.contains(&loc(13.5, 53.1)), "north of box");
    }

    #[test]
    fn bbox_bounds_are_inclusive() {
        let b = Bbox::new(13.0, 52.0, 14.0, 53.0);
        assert!(b.contains(&loc(13.0, 52.0)), "SW corner");
        assert!(b.contains(&loc(14.0, 53.0)), "NE corner");
        assert!(b.contains(&loc(13.0, 52.5)), "west edge");
        assert!(b.contains(&loc(13.5, 53.0)), "north edge");
    }

    #[test]
    fn bbox_wraps_across_antimeridian() {
        // A box from +170° east to -170° west spans the seam through ±180°.
        let b = Bbox::new(170.0, -10.0, -170.0, 10.0);
        assert!(b.contains(&loc(175.0, 0.0)), "just east of min");
        assert!(b.contains(&loc(-175.0, 0.0)), "just west of max");
        assert!(b.contains(&loc(180.0, 0.0)), "on the seam");
        assert!(!b.contains(&loc(0.0, 0.0)), "excluded middle");
        assert!(!b.contains(&loc(160.0, 0.0)), "west of min, not wrapped");
        // Latitude never wraps, even for an antimeridian box.
        assert!(!b.contains(&loc(175.0, 20.0)), "north of box");
    }

    #[test]
    fn bbox_parses_from_string() {
        let b: Bbox = "13.0,52.0,13.5,52.5".parse().unwrap();
        assert_eq!(b, Bbox::new(13.0, 52.0, 13.5, 52.5));
        // Surrounding whitespace on fields is tolerated.
        let b2: Bbox = " -1.5, 2.0 ,3.0,4.25 ".parse().unwrap();
        assert_eq!(b2, Bbox::new(-1.5, 2.0, 3.0, 4.25));
    }

    #[test]
    fn bbox_parse_rejects_malformed() {
        assert!("13.0,52.0,13.5".parse::<Bbox>().is_err(), "too few fields");
        assert!(
            "13.0,52.0,13.5,52.5,1".parse::<Bbox>().is_err(),
            "too many fields"
        );
        assert!("13.0,52.0,x,52.5".parse::<Bbox>().is_err(), "non-numeric");
        assert!("".parse::<Bbox>().is_err(), "empty");
    }
}
