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
    fn from_raw(v: u8) -> Self {
        match v {
            1 => ObjectKind::Node,
            2 => ObjectKind::Way,
            3 => ObjectKind::Relation,
            other => unreachable!("libosmium yielded an unexpected item_type: {other}"),
        }
    }
}

/// A geographic location (WGS84 degrees).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Location {
    pub lon: f64,
    pub lat: f64,
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
}

impl Reader {
    /// Open an OSM file for reading. The format is detected from the path
    /// (e.g. `.osm.pbf`, `.osm`). Returns an [`Error`] if the file cannot be
    /// opened or is not valid OSM data.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path
            .as_ref()
            .to_str()
            .ok_or_else(|| Error("path is not valid UTF-8".to_string()))?;
        let mut reader = ffi::open_reader(path)?;
        let cursor = ffi::make_cursor(reader.pin_mut());
        Ok(Reader { cursor, reader })
    }

    fn read_current(&self) -> Object {
        let c = self.cursor.as_ref().expect("cursor is never null");
        let kind = ObjectKind::from_raw(ffi::object_kind(c));
        let tags = ffi::tag_keys(c)
            .into_iter()
            .zip(ffi::tag_values(c))
            .collect();
        let body = match kind {
            ObjectKind::Node => {
                let location = ffi::node_location_valid(c).then(|| Location {
                    lon: ffi::node_lon(c),
                    lat: ffi::node_lat(c),
                });
                Body::Node { location }
            }
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
        if ffi::advance(self.cursor.pin_mut()) {
            Some(self.read_current())
        } else {
            None
        }
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
}
