//! Tests for the zero-copy read path: `Reader::next_ref`, `ObjectRef`, and the
//! pass-through `Writer::copy`.
//!
//! The borrowed view and the owned `Object` must agree on everything they both
//! expose, and `copy` must additionally preserve the metadata that `add`
//! cannot represent.

use std::path::PathBuf;

use rusmium::{Body, ObjectKind, ReadOptions, Reader, Writer};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn out_path(name: &str) -> PathBuf {
    std::env::var("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join(name)
}

#[test]
fn borrowed_view_agrees_with_owned_iteration() {
    // `Iterator::next` is implemented as `next_ref().to_owned()`, so this is a
    // guard on the scalar accessors reading the same object the view points at
    // — not two independent copy-out implementations.
    let owned: Vec<rusmium::Object> = Reader::open(fixture("sample.osm")).unwrap().collect();

    let mut reader = Reader::open(fixture("sample.osm")).unwrap();
    let mut i = 0;
    while let Some(obj) = reader.next_ref() {
        let expected = &owned[i];
        assert_eq!(obj.kind(), expected.kind(), "kind at index {i}");
        assert_eq!(obj.id(), expected.id(), "id at index {i}");
        assert_eq!(obj.version(), expected.version(), "version at index {i}");
        assert_eq!(obj.location(), expected.location(), "location at index {i}");
        assert_eq!(
            obj.tag_count(),
            expected.tags().len(),
            "tag count at index {i}"
        );
        assert_eq!(&obj.to_owned(), expected, "to_owned at index {i}");
        i += 1;
    }
    assert_eq!(i, owned.len(), "borrowed pass yielded a different count");
}

#[test]
fn bulk_accessors_match_owned_bodies_and_reuse_cleanly() {
    // One buffer serves the whole pass; each fill must fully replace the last,
    // leaving no residue from the previous object.
    let mut refs: Vec<i64> = Vec::new();
    let mut kinds: Vec<u8> = Vec::new();

    let mut reader = Reader::open(fixture("sample.osm")).unwrap();
    let mut saw_way = false;
    let mut saw_relation = false;

    while let Some(obj) = reader.next_ref() {
        let owned = obj.to_owned();
        match owned.body() {
            Body::Way { nodes } => {
                obj.way_node_refs_into(&mut refs);
                assert_eq!(&refs, nodes, "way {} node refs", owned.id());
                saw_way = true;
            }
            Body::Relation { members } => {
                obj.relation_members_into(&mut kinds, &mut refs);
                assert_eq!(kinds.len(), members.len());
                assert_eq!(refs.len(), members.len());
                for (i, m) in members.iter().enumerate() {
                    assert_eq!(ObjectKind::from_item_type(kinds[i]), Some(m.kind));
                    assert_eq!(refs[i], m.ref_id);
                }
                saw_relation = true;
            }
            Body::Node { .. } => {
                // A node is neither, so both fills must clear rather than keep
                // whatever the previous way left behind.
                obj.way_node_refs_into(&mut refs);
                assert!(refs.is_empty(), "node left stale way refs behind");
                obj.relation_members_into(&mut kinds, &mut refs);
                assert!(kinds.is_empty() && refs.is_empty());
            }
        }
    }

    assert!(saw_way && saw_relation, "fixture should cover both kinds");
}

#[test]
fn copy_and_add_produce_the_same_objects() {
    let via_copy = out_path("borrowed-via-copy.osm.pbf");
    let via_add = out_path("borrowed-via-add.osm.pbf");

    let mut w = Writer::create(&via_copy).unwrap();
    let mut reader = Reader::open(fixture("sample.osm")).unwrap();
    while let Some(obj) = reader.next_ref() {
        w.copy(&obj).unwrap();
    }
    w.finish().unwrap();

    let mut w = Writer::create(&via_add).unwrap();
    for obj in Reader::open(fixture("sample.osm")).unwrap() {
        w.add(&obj).unwrap();
    }
    w.finish().unwrap();

    let copied: Vec<_> = Reader::open(&via_copy).unwrap().collect();
    let added: Vec<_> = Reader::open(&via_add).unwrap().collect();
    assert_eq!(
        copied, added,
        "pass-through and rebuild paths must agree on everything Object carries"
    );

    let _ = std::fs::remove_file(&via_copy);
    let _ = std::fs::remove_file(&via_add);
}

#[test]
fn copy_preserves_metadata_that_add_drops() {
    let via_copy = out_path("meta-via-copy.osm.pbf");
    let via_add = out_path("meta-via-add.osm.pbf");

    let mut w = Writer::create(&via_copy).unwrap();
    let mut reader = Reader::open(fixture("sample_meta.osm")).unwrap();
    while let Some(obj) = reader.next_ref() {
        w.copy(&obj).unwrap();
    }
    w.finish().unwrap();

    let mut w = Writer::create(&via_add).unwrap();
    for obj in Reader::open(fixture("sample_meta.osm")).unwrap() {
        w.add(&obj).unwrap();
    }
    w.finish().unwrap();

    // node 1: timestamp 2020-01-02T03:04:05Z, uid 42, user alice, changeset 1001
    const NODE1_TS: i64 = 1_577_934_245;

    let mut reader = Reader::open(&via_copy).unwrap();
    let mut checked = false;
    while let Some(obj) = reader.next_ref() {
        if obj.kind() == ObjectKind::Node && obj.id() == 1 {
            assert_eq!(obj.timestamp(), Some(NODE1_TS), "timestamp survived copy");
            assert_eq!(obj.uid(), 42, "uid survived copy");
            assert_eq!(obj.user(), "alice", "user survived copy");
            assert_eq!(obj.changeset(), 1001, "changeset survived copy");
            checked = true;
        }
    }
    assert!(checked, "node 1 missing from the copied output");

    // The rebuild path has nowhere to put any of it, so it is all lost.
    let mut reader = Reader::open(&via_add).unwrap();
    while let Some(obj) = reader.next_ref() {
        if obj.kind() == ObjectKind::Node && obj.id() == 1 {
            assert_eq!(obj.timestamp(), None, "add() cannot carry a timestamp");
            assert_eq!(obj.uid(), 0);
            assert_eq!(obj.user(), "");
        }
    }

    let _ = std::fs::remove_file(&via_copy);
    let _ = std::fs::remove_file(&via_add);
}

#[test]
fn metadata_is_absent_when_not_requested() {
    // read_meta is consulted by libosmium's PBF decoder only — the XML parser
    // always reads metadata — so this has to go through a .osm.pbf.
    let pbf = out_path("meta-opt-out.osm.pbf");
    let mut w = Writer::create(&pbf).unwrap();
    let mut src = Reader::open(fixture("sample_meta.osm")).unwrap();
    while let Some(obj) = src.next_ref() {
        w.copy(&obj).unwrap();
    }
    w.finish().unwrap();

    // Present by default…
    let mut reader = Reader::open(&pbf).unwrap();
    let first = reader.next_ref().expect("at least one object");
    assert!(
        first.timestamp().is_some(),
        "metadata should be in the file"
    );
    drop(reader);

    // …and skipped on request.
    let opts = ReadOptions::default().metadata(false);
    let mut reader = Reader::open_with(&pbf, opts).unwrap();
    let mut any = false;
    while let Some(obj) = reader.next_ref() {
        assert_eq!(
            obj.timestamp(),
            None,
            "metadata(false) must skip the timestamp"
        );
        assert_eq!(obj.uid(), 0);
        assert_eq!(obj.user(), "");
        any = true;
    }
    assert!(any);

    let _ = std::fs::remove_file(&pbf);
}

#[test]
fn tags_are_skipped_when_not_requested() {
    let opts = ReadOptions::default().tags(false);
    let mut reader = Reader::open_with(fixture("sample.osm"), opts).unwrap();
    let mut any = false;
    while let Some(obj) = reader.next_ref() {
        assert!(
            obj.to_owned().tags().is_empty(),
            "tags(false) must yield no tags"
        );
        any = true;
    }
    assert!(any);
}
