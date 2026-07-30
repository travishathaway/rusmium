//! Write-side integration tests.

use std::path::PathBuf;

use rusmium::{Body, Location, Object, ObjectKind, Reader, Writer};

fn out_path(name: &str) -> PathBuf {
    let dir = std::env::var("CARGO_TARGET_TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    dir.join(name)
}

#[test]
fn create_on_unwritable_path_errors_cleanly() {
    // A directory that does not exist -> writer creation must fail, not panic.
    let bad = PathBuf::from("/nonexistent-dir-xyz/out.osm.pbf");
    assert!(Writer::create(bad).is_err());
}

#[test]
fn written_objects_read_back_with_matching_content() {
    let path = out_path("rusmium-write-test.osm.pbf");
    let _ = std::fs::remove_file(&path);

    let node = Object::node(
        42,
        7,
        Some(Location {
            lon: 13.5,
            lat: 52.5,
        }),
        vec![
            ("amenity".to_string(), "bar".to_string()),
            ("name".to_string(), "Zur Küche".to_string()),
        ],
    );
    let way = Object::way(
        100,
        2,
        vec![42, 43, 44],
        vec![("highway".to_string(), "path".to_string())],
    );

    let mut w = Writer::create(&path).expect("create writer");
    w.add(&node).expect("add node");
    w.add(&way).expect("add way");
    w.finish().expect("finish");

    let objs: Vec<Object> = Reader::open(&path).expect("reopen").collect();

    let rt_node = objs.iter().find(|o| o.id() == 42).expect("node 42");
    assert_eq!(rt_node.kind(), ObjectKind::Node);
    assert_eq!(rt_node.version(), 7);
    let loc = rt_node.location().expect("location");
    assert!((loc.lon - 13.5).abs() < 1e-6);
    assert!((loc.lat - 52.5).abs() < 1e-6);
    assert!(rt_node
        .tags()
        .iter()
        .any(|(k, v)| k == "name" && v == "Zur Küche"));

    let rt_way = objs.iter().find(|o| o.id() == 100).expect("way 100");
    match rt_way.body() {
        Body::Way { nodes } => assert_eq!(nodes, &[42, 43, 44]),
        other => panic!("expected way, got {other:?}"),
    }

    let _ = std::fs::remove_file(&path);
}
