//! Read-side integration tests.
//!
//! These use an XML `.osm` fixture (human-readable, git-friendly, and readable
//! by osmium via expat). The PBF read path is exercised by the round-trip test
//! once the writer exists, since generating a `.osm.pbf` fixture requires a
//! writer in the first place.

use std::path::PathBuf;

use rusmium::{Body, ObjectKind, Reader};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn read_all(name: &str) -> Vec<rusmium::Object> {
    Reader::open(fixture(name)).expect("open fixture").collect()
}

#[test]
fn counts_nodes_ways_relations() {
    let (mut nodes, mut ways, mut relations) = (0, 0, 0);
    for obj in Reader::open(fixture("sample.osm")).expect("open fixture") {
        match obj.kind() {
            ObjectKind::Node => nodes += 1,
            ObjectKind::Way => ways += 1,
            ObjectKind::Relation => relations += 1,
        }
    }
    assert_eq!((nodes, ways, relations), (3, 1, 1));
}

#[test]
fn open_nonexistent_file_errors_cleanly() {
    assert!(
        Reader::open(fixture("does-not-exist.osm.pbf")).is_err(),
        "opening a missing file must return Err, not panic"
    );
}

#[test]
fn reads_node_id_version_and_location() {
    let objs = read_all("sample.osm");
    let node = objs
        .iter()
        .find(|o| o.kind() == ObjectKind::Node && o.id() == 1)
        .expect("node 1");

    assert_eq!(node.version(), 1);
    let loc = node.location().expect("node 1 has a location");
    assert!((loc.lat - 52.0).abs() < 1e-6, "lat was {}", loc.lat);
    assert!((loc.lon - 13.0).abs() < 1e-6, "lon was {}", loc.lon);
}

#[test]
fn reads_tags_of_tagged_object() {
    let objs = read_all("sample.osm");
    let node = objs.iter().find(|o| o.id() == 1).unwrap();

    let tags: Vec<(&str, &str)> = node
        .tags()
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert!(tags.contains(&("amenity", "cafe")));
    assert!(tags.contains(&("name", "Café Kranzler")));
}

#[test]
fn untagged_object_has_empty_tag_set() {
    let objs = read_all("sample.osm");
    let node = objs
        .iter()
        .find(|o| o.kind() == ObjectKind::Node && o.id() == 2)
        .unwrap();
    assert!(node.tags().is_empty());
}

#[test]
fn reads_way_node_refs_and_relation_members() {
    let objs = read_all("sample.osm");

    let way = objs.iter().find(|o| o.kind() == ObjectKind::Way).unwrap();
    match way.body() {
        Body::Way { nodes } => assert_eq!(nodes, &[1, 2, 3]),
        other => panic!("expected way body, got {other:?}"),
    }

    let rel = objs
        .iter()
        .find(|o| o.kind() == ObjectKind::Relation)
        .unwrap();
    match rel.body() {
        Body::Relation { members } => {
            assert_eq!(members.len(), 2);
            assert_eq!(members[0].kind, ObjectKind::Way);
            assert_eq!(members[0].ref_id, 10);
            assert_eq!(members[0].role, "outer");
        }
        other => panic!("expected relation body, got {other:?}"),
    }
}

#[test]
fn retained_object_survives_iteration() {
    // Copy-out contract: collecting drains the reader (dropping its buffers),
    // yet every retained Object stays valid and unchanged.
    let objs = read_all("sample.osm");
    let node1 = objs.iter().find(|o| o.id() == 1).unwrap().clone();

    // Re-read independently and confirm the retained copy still matches.
    let objs2 = read_all("sample.osm");
    let node1_again = objs2.iter().find(|o| o.id() == 1).unwrap();
    assert_eq!(&node1, node1_again);
}
