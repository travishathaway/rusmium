//! The `cxx` bridge: the single concrete ABI between Rust and the C++ shim.
//!
//! Nothing here references a libosmium template directly — the shim
//! (`cpp/shim.{h,cc}`) flattens osmium's templated, header-only API into the
//! opaque types and plain functions declared below.
//!
//! Structured data crosses as primitives and `Vec`s of primitives/`String`
//! (never shared structs): the cxx-generated header includes `cpp/shim.h`
//! before any shared-struct definitions, so keeping the shim header free of
//! them avoids an include-ordering hazard. Object kinds and relation-member
//! kinds cross as a plain `u8` (osmium item_type: node=1, way=2, relation=3).

// FFI shim signatures intentionally take flat argument lists (osmium has no
// stable ABI struct we can pass), so the arity lint does not apply here.
#[allow(clippy::too_many_arguments)]
#[cxx::bridge(namespace = "rusmium")]
pub mod bridge {
    unsafe extern "C++" {
        include!("cpp/shim.h");

        /// Owns an `osmium::io::Reader`.
        type OsmReader;
        /// A pull-position over an `OsmReader`. Valid only while its
        /// `OsmReader` is alive, and its accessors are valid only after
        /// `advance` has returned true.
        type Cursor;

        /// libosmium header version this crate was built against.
        fn osmium_version() -> String;

        /// Open an OSM file for reading, detecting the format from the path.
        /// Errors surface as `Err` (the C++ side throws; cxx converts).
        ///
        /// `entity_bits` is libosmium's `osm_entity_bits` mask (node=1, way=2,
        /// relation=4) and `read_metadata` selects `read_meta::yes`/`no`;
        /// together they let the parser skip decoding work outright.
        fn open_reader(
            path: &str,
            entity_bits: u8,
            read_metadata: bool,
        ) -> Result<UniquePtr<OsmReader>>;

        /// Create a cursor positioned before the first object.
        fn make_cursor(reader: Pin<&mut OsmReader>) -> UniquePtr<Cursor>;

        /// Advance to the next object; returns false at end of input.
        fn advance(cursor: Pin<&mut Cursor>) -> bool;

        // --- current-object accessors (copy-out) ---

        /// Kind of the current object (node=1, way=2, relation=3).
        fn object_kind(cursor: &Cursor) -> u8;
        fn object_id(cursor: &Cursor) -> i64;
        fn object_version(cursor: &Cursor) -> u32;

        /// Object metadata; zero/empty unless the reader was opened with
        /// metadata enabled. Timestamp is seconds since the epoch, 0 if unset.
        fn object_timestamp(cursor: &Cursor) -> i64;
        fn object_uid(cursor: &Cursor) -> u32;
        fn object_user(cursor: &Cursor) -> String;
        fn object_changeset(cursor: &Cursor) -> i64;

        /// Whether the current node has a valid location.
        fn node_location_valid(cursor: &Cursor) -> bool;
        fn node_lon(cursor: &Cursor) -> f64;
        fn node_lat(cursor: &Cursor) -> f64;

        /// Number of tags on the current object; lets callers avoid the two
        /// allocating calls below when there are none.
        fn tag_count(cursor: &Cursor) -> usize;

        /// Tags as parallel key/value vectors (same length, same order).
        fn tag_keys(cursor: &Cursor) -> Vec<String>;
        fn tag_values(cursor: &Cursor) -> Vec<String>;

        /// Ordered node references of the current way (empty otherwise).
        fn way_node_refs(cursor: &Cursor) -> Vec<i64>;

        /// As `way_node_refs`, but into a caller-owned vector (replacing its
        /// contents) so one buffer can serve a whole pass.
        fn way_node_refs_into(cursor: &Cursor, out: &mut Vec<i64>);

        /// Members of the current relation as parallel vectors.
        fn relation_member_kinds(cursor: &Cursor) -> Vec<u8>;
        fn relation_member_refs(cursor: &Cursor) -> Vec<i64>;
        fn relation_member_roles(cursor: &Cursor) -> Vec<String>;

        /// As the two above, into caller-owned vectors. Roles are excluded —
        /// they are the only allocating part and are rarely needed.
        fn relation_members_into(cursor: &Cursor, kinds: &mut Vec<u8>, refs: &mut Vec<i64>);

        // --- writing ---

        /// Owns an `osmium::io::Writer` plus an accumulation buffer.
        type OsmWriter;

        /// Create an OSM file for writing; format detected from the path.
        /// Existing files are overwritten. Errors surface as `Err`.
        fn create_writer(path: &str) -> Result<UniquePtr<OsmWriter>>;

        /// Append a node. `keys`/`values` are parallel and same-length.
        fn writer_add_node(
            writer: Pin<&mut OsmWriter>,
            id: i64,
            version: u32,
            has_location: bool,
            lon: f64,
            lat: f64,
            keys: &Vec<String>,
            values: &Vec<String>,
        ) -> Result<()>;

        /// Append a way with ordered node references.
        fn writer_add_way(
            writer: Pin<&mut OsmWriter>,
            id: i64,
            version: u32,
            node_refs: &Vec<i64>,
            keys: &Vec<String>,
            values: &Vec<String>,
        ) -> Result<()>;

        /// Append a relation. Member fields are parallel and same-length.
        fn writer_add_relation(
            writer: Pin<&mut OsmWriter>,
            id: i64,
            version: u32,
            member_kinds: &Vec<u8>,
            member_refs: &Vec<i64>,
            member_roles: &Vec<String>,
            keys: &Vec<String>,
            values: &Vec<String>,
        ) -> Result<()>;

        /// Copy the cursor's current object straight into the writer's buffer:
        /// one memcpy, with metadata and tags preserved exactly as read.
        fn writer_copy(writer: Pin<&mut OsmWriter>, cursor: &Cursor) -> Result<()>;

        /// Flush all appended objects and close the file.
        fn finish_writer(writer: Pin<&mut OsmWriter>) -> Result<()>;
    }
}

pub(crate) use bridge::*;
