//! Extract the part of an OSM file that intersects a bounding box.
//!
//! This is a **reference-based** extract, not geometric clipping: a way that
//! crosses the box boundary is kept *whole* (not cut at the edge), and its
//! out-of-box nodes are pulled back into the output so the way keeps a complete
//! geometry. This is the "complete ways" strategy:
//!
//! - a node is kept if its location is inside the box;
//! - a way is kept if it references at least one in-box node, and *all* of its
//!   nodes are then included (even the ones outside the box);
//! - a relation is kept if it references a kept node or a kept way. Relations are
//!   emitted whole and membership is **not** completed recursively, so a kept
//!   relation may reference objects that are not in the extract.
//!
//! Run it through pixi so the native toolchain is available:
//!
//! ```text
//! pixi run cargo run --example bbox_filter -- in.osm.pbf out.osm.pbf 13.0,52.0,13.5,52.5
//! ```
//!
//! OSM files are ordered nodes → ways → relations and an object already streamed
//! past cannot be re-emitted, so the extract makes three ordered passes: one to
//! find the in-box nodes, one to decide the kept ways/relations and the full set
//! of required nodes, and one to write the result.

use std::collections::HashSet;
use std::path::Path;
use std::process::ExitCode;

use rusmium::{Bbox, Body, ObjectKind, Reader, Writer};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!(
            "usage: {} <input.osm.pbf> <output.osm.pbf> <min_lon,min_lat,max_lon,max_lat>",
            args.first().map(String::as_str).unwrap_or("bbox_filter")
        );
        return ExitCode::FAILURE;
    }

    let input = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    let bbox: Bbox = match args[3].parse() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    match extract(input, output, &bbox) {
        Ok(stats) => {
            println!(
                "wrote {} nodes, {} ways, {} relations to {}",
                stats.nodes,
                stats.ways,
                stats.relations,
                output.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Counts of what the extract wrote, for reporting and testing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub nodes: usize,
    pub ways: usize,
    pub relations: usize,
}

/// Extract `bbox` from `input` into `output` using the complete-ways strategy.
///
/// Factored out of `main` so integration tests can drive it directly instead of
/// spawning a process.
pub fn extract(input: &Path, output: &Path, bbox: &Bbox) -> Result<Stats, rusmium::Error> {
    // Pass 1: node ids whose location falls inside the box.
    let mut in_box: HashSet<i64> = HashSet::new();
    for obj in Reader::open(input)? {
        if obj.kind() == ObjectKind::Node {
            if let Some(loc) = obj.location() {
                if bbox.contains(&loc) {
                    in_box.insert(obj.id());
                }
            }
        }
    }

    // Pass 2: decide which ways and relations to keep, and the full set of nodes
    // that must be present. `required_nodes` starts from the in-box nodes so that
    // lone in-box nodes survive even when no kept way references them, then grows
    // to cover every node of every kept way (the "complete" part). Ways precede
    // relations in the file, so `kept_ways` is fully populated before any
    // relation is tested.
    let mut kept_ways: HashSet<i64> = HashSet::new();
    let mut kept_rels: HashSet<i64> = HashSet::new();
    let mut required_nodes: HashSet<i64> = in_box.clone();
    for obj in Reader::open(input)? {
        match obj.body() {
            Body::Way { nodes } => {
                if nodes.iter().any(|n| in_box.contains(n)) {
                    kept_ways.insert(obj.id());
                    required_nodes.extend(nodes.iter().copied());
                }
            }
            Body::Relation { members } => {
                let referenced_kept = members.iter().any(|m| match m.kind {
                    ObjectKind::Node => in_box.contains(&m.ref_id),
                    ObjectKind::Way => kept_ways.contains(&m.ref_id),
                    ObjectKind::Relation => false,
                });
                if referenced_kept {
                    kept_rels.insert(obj.id());
                }
            }
            Body::Node { .. } => {}
        }
    }

    // Pass 3: write the extract.
    let mut writer = Writer::create(output)?;
    let mut stats = Stats::default();
    for obj in Reader::open(input)? {
        let keep = match obj.kind() {
            ObjectKind::Node => required_nodes.contains(&obj.id()),
            ObjectKind::Way => kept_ways.contains(&obj.id()),
            ObjectKind::Relation => kept_rels.contains(&obj.id()),
        };
        if keep {
            writer.add(&obj)?;
            match obj.kind() {
                ObjectKind::Node => stats.nodes += 1,
                ObjectKind::Way => stats.ways += 1,
                ObjectKind::Relation => stats.relations += 1,
            }
        }
    }
    writer.finish()?;
    Ok(stats)
}
