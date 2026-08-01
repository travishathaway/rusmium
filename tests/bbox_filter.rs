//! Integration tests for the `bbox_filter` example's complete-ways extract.
//!
//! The example's `extract` function is pulled in directly as a module (rather
//! than spawning the compiled binary) so these tests exercise the real code path
//! with no process plumbing. The `allow(dead_code)` covers items of the example
//! (e.g. `main`) that are unused in this test context.

#[allow(dead_code)]
#[path = "../examples/bbox_filter.rs"]
mod bbox_filter;

use std::collections::HashSet;
use std::path::PathBuf;

use bbox_filter::extract;
use rusmium::{Bbox, Body, ObjectKind, Reader};

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

/// Fixture layout (see tests/fixtures/sample.osm):
///   node 1 @ (13.0, 52.0)   tagged cafe
///   node 2 @ (13.1, 52.1)
///   node 3 @ (13.2, 52.2)
///   way 10  -> nodes [1, 2, 3]
///   relation 100 -> way 10 (outer), node 1 (admin_centre)
fn run(name: &str, bbox: Bbox) -> Vec<rusmium::Object> {
    let out = out_path(name);
    let _ = std::fs::remove_file(&out);
    extract(&fixture("sample.osm"), &out, &bbox).expect("extract");
    let objs: Vec<_> = Reader::open(&out).expect("reopen output").collect();
    let _ = std::fs::remove_file(&out);
    objs
}

fn ids_of(objs: &[rusmium::Object], kind: ObjectKind) -> HashSet<i64> {
    objs.iter()
        .filter(|o| o.kind() == kind)
        .map(|o| o.id())
        .collect()
}

#[test]
fn keeps_touching_way_whole_with_out_of_box_nodes() {
    // A tight box around node 1 only. Node 1 is in; nodes 2 and 3 are outside.
    let objs = run(
        "bbox-node1-only.osm.pbf",
        Bbox::new(12.95, 51.95, 13.05, 52.05),
    );

    let node_ids = ids_of(&objs, ObjectKind::Node);
    // Way 10 touches node 1, so it is kept whole — every one of its nodes must be
    // present, including the out-of-box nodes 2 and 3 (the "complete" guarantee).
    assert_eq!(
        node_ids,
        HashSet::from([1, 2, 3]),
        "kept way's out-of-box nodes must be pulled into the output"
    );
    assert_eq!(ids_of(&objs, ObjectKind::Way), HashSet::from([10]));
    // Relation 100 references way 10 and node 1, both kept, so it is kept too.
    assert_eq!(ids_of(&objs, ObjectKind::Relation), HashSet::from([100]));

    // No kept way may have a dangling node reference.
    let present: HashSet<i64> = node_ids;
    for o in &objs {
        if let Body::Way { nodes } = o.body() {
            for n in nodes {
                assert!(present.contains(n), "way {} has dangling ref {n}", o.id());
            }
        }
    }
}

#[test]
fn empty_box_yields_valid_empty_output() {
    // A box far from the fixture: nothing is inside it.
    let objs = run("bbox-empty.osm.pbf", Bbox::new(0.0, 0.0, 1.0, 1.0));
    assert!(
        objs.is_empty(),
        "an out-of-range box must produce a valid but empty extract, got {} objects",
        objs.len()
    );
}

#[test]
fn box_covering_everything_keeps_all() {
    let objs = run("bbox-all.osm.pbf", Bbox::new(12.0, 51.0, 14.0, 53.0));
    assert_eq!(ids_of(&objs, ObjectKind::Node), HashSet::from([1, 2, 3]));
    assert_eq!(ids_of(&objs, ObjectKind::Way), HashSet::from([10]));
    assert_eq!(ids_of(&objs, ObjectKind::Relation), HashSet::from([100]));
}

#[test]
fn all_idset_backends_agree() {
    // The three representations are a memory/speed trade only: for any input
    // they must select exactly the same objects.
    use bbox_filter::{extract_with, Backend};

    let bbox = Bbox::new(12.95, 51.95, 13.15, 52.15);
    let mut results = Vec::new();

    for backend in [Backend::Auto, Backend::Sorted, Backend::Dense] {
        let out = out_path(&format!("bbox-backend-{backend:?}.osm.pbf"));
        let _ = std::fs::remove_file(&out);
        let stats = extract_with(&fixture("sample.osm"), &out, &bbox, backend).expect("extract");
        let objs: Vec<_> = Reader::open(&out).expect("reopen output").collect();
        let _ = std::fs::remove_file(&out);
        results.push((backend, stats, objs));
    }

    let (first_backend, first_stats, first_objs) = &results[0];
    for (backend, stats, objs) in &results[1..] {
        assert_eq!(
            stats, first_stats,
            "{backend:?} and {first_backend:?} disagree on counts"
        );
        assert_eq!(
            objs, first_objs,
            "{backend:?} and {first_backend:?} disagree on contents"
        );
    }
    assert!(!first_objs.is_empty(), "fixture should select something");
}

#[test]
fn lone_in_box_node_survives_without_a_way() {
    // Box around node 3 only. Way 10 touches node 3, so this fixture keeps the
    // way — but node 3 must be present regardless via required_nodes seeding.
    let objs = run("bbox-node3.osm.pbf", Bbox::new(13.15, 52.15, 13.25, 52.25));
    assert!(
        ids_of(&objs, ObjectKind::Node).contains(&3),
        "an in-box node must always be emitted"
    );
}
