## Why

`rusmium` can read and write OSM files, but it ships no worked example of a real
task and no spatial primitive. The most common first thing a user reaches for is
"clip this `.osm.pbf` to a bounding box." That task is achievable today with the
existing `Reader`/`Writer` API — nodes carry locations, ways carry node refs, and
relations carry member refs, which is exactly enough for a *reference-based*
extract — but it requires knowing the multi-pass trick, and there is no
`Bbox`/containment type to express the predicate. This change adds that missing
primitive to the library and a runnable example that demonstrates the idiom
end to end.

## What Changes

- Add a `Bbox` value type to the `rusmium` library (pure Rust, no FFI): an
  axis-aligned WGS84 bounding box built from `Location` corners, with an
  inclusive `contains(&Location)` test that **handles antimeridian wrap-around**
  (a box whose `min_lon > max_lon` spans the ±180° seam) and a `FromStr` impl
  parsing `"min_lon,min_lat,max_lon,max_lat"`.
- Add `examples/bbox_filter.rs`: a CLI that reads an input OSM file, extracts the
  objects intersecting a bounding box using the **complete-ways strategy**, and
  writes a self-consistent output OSM file.
- Define the extract semantics precisely: nodes in the box are kept; ways with
  any node in the box are kept **whole**, pulling their out-of-box endpoint nodes
  back into the output so boundary geometry stays complete; relations referencing
  a kept node or way are kept (non-recursive).
- Add a `pixi` task and README/example docs so it runs through the pinned
  toolchain (`pixi run …`), and an integration test asserting the extract's
  correctness properties against the committed fixture.

## Capabilities

### New Capabilities
- `bounding-box`: A geographic bounding-box value type with inclusive
  containment testing (including antimeridian wrap-around) and string parsing.
- `bbox-extract`: A complete-ways bounding-box extract of an OSM file,
  demonstrated by a runnable example, producing an output where every kept way
  retains all of its nodes.

### Modified Capabilities
<!-- None. `osm-file-reading` and `osm-file-writing` are consumed unchanged. -->

## Impact

- **Library API surface**: new public `Bbox` type in `src/lib.rs` (and its
  `FromStr` error). No change to `Reader`/`Writer`/`Object`; no FFI, shim, or
  `build.rs` changes.
- **New example**: `examples/bbox_filter.rs`, runnable via
  `pixi run cargo run --example bbox_filter -- <in> <out> <min_lon,min_lat,max_lon,max_lat>`;
  optional `pixi` task alias.
- **Tests**: unit tests for `Bbox` (containment, inclusivity, wrap-around,
  parsing) and an integration test exercising the example's extract logic on the
  existing `tests/fixtures/sample.osm`.
- **Docs**: README gains a short "Filtering by bounding box" section pointing at
  the example.
- **No new dependencies**: argument parsing uses `std::env::args()`; the crate
  keeps its single non-FFI dependency (`cxx`).
