//! The round-trip integration test — rusmium's definition of "done" for v1.
//!
//! Read fixture A (XML) → write B (PBF) → read B back → assert the two are
//! *semantically equal*. This exercises every layer (reader, cursor, iterator,
//! object model, builders, writer) and both file formats in one assertion.
//!
//! "Semantically equal" is defined here as a **normalized object stream**: each
//! object is canonicalized by (kind, id, version, fixed-point location, ordered
//! node refs / members, sorted tags), and the sorted set of canonical forms
//! must match. This is robust to buffer- and format-level reordering (e.g. PBF
//! grouping objects by type) while still asserting full content fidelity.

use std::path::PathBuf;

use rusmium::{Body, Object, Reader, Writer};

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

/// Fixed-point encoding of a coordinate at osmium's PBF precision (1e-7 deg).
fn fixed(v: f64) -> i64 {
    (v * 1e7).round() as i64
}

/// A canonical, order-insensitive, human-diffable form of one object.
fn canon(o: &Object) -> String {
    let mut tags: Vec<(String, String)> = o.tags().to_vec();
    tags.sort();

    let body = match o.body() {
        Body::Node { location } => match location {
            Some(l) => format!("node@{},{}", fixed(l.lon), fixed(l.lat)),
            None => "node@none".to_string(),
        },
        Body::Way { nodes } => format!("way nodes={nodes:?}"),
        Body::Relation { members } => {
            let m: Vec<String> = members
                .iter()
                .map(|m| format!("{:?}:{}:{}", m.kind, m.ref_id, m.role))
                .collect();
            format!("rel members={m:?}")
        }
    };

    format!(
        "kind={:?} id={} v={} {body} tags={tags:?}",
        o.kind(),
        o.id(),
        o.version()
    )
}

fn normalized_stream(objs: &[Object]) -> Vec<String> {
    let mut s: Vec<String> = objs.iter().map(canon).collect();
    s.sort();
    s
}

#[test]
fn read_write_read_is_semantically_equal() {
    let src: Vec<Object> = Reader::open(fixture("sample.osm"))
        .expect("open source")
        .collect();
    assert!(!src.is_empty(), "fixture should contain objects");

    let dst_path = out_path("rusmium-roundtrip.osm.pbf");
    let _ = std::fs::remove_file(&dst_path);

    let mut w = Writer::create(&dst_path).expect("create writer");
    for obj in &src {
        w.add(obj).expect("append object");
    }
    w.finish().expect("finish writer");

    let round: Vec<Object> = Reader::open(&dst_path).expect("reopen output").collect();

    assert_eq!(
        normalized_stream(&src),
        normalized_stream(&round),
        "round-tripped objects differ from the source"
    );

    let _ = std::fs::remove_file(&dst_path);
}
