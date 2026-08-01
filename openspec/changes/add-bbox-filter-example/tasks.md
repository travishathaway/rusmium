## 1. `Bbox` library type (bounding-box)

- [x] 1.1 Add public `Bbox { min: Location, max: Location }` to `src/lib.rs` with `new(min_lon, min_lat, max_lon, max_lat)` preserving longitudes as given (no reorder); document that latitudes are expected `min_lat <= max_lat`
- [x] 1.2 Implement `contains(&self, loc: &Location) -> bool`: inclusive latitude test; longitude test with antimeridian wrap-around (`min_lon <= max_lon` → `min_lon <= lon <= max_lon`; else `lon >= min_lon || lon <= max_lon`)
- [x] 1.3 Add `BboxParseError` and `impl FromStr for Bbox` parsing `"min_lon,min_lat,max_lon,max_lat"`; reject wrong field count and non-numeric fields
- [x] 1.4 Unit tests: inside/outside/edge (inclusive) containment; wrap-around box matches both seam sides and excludes the middle; `FromStr` round-trips a valid string and errors cleanly on malformed input
- [x] 1.5 Export `Bbox`/`BboxParseError` from the crate root and add a short rustdoc example

## 2. `examples/bbox_filter.rs` — complete-ways extract (bbox-extract)

- [x] 2.1 Scaffold `examples/bbox_filter.rs`: parse positional args `<in> <out> <min_lon,min_lat,max_lon,max_lat>` via `std::env::args()` and `arg.parse::<Bbox>()`; on bad args print usage to stderr and exit non-zero
- [x] 2.2 Factor the extract into a callable `fn extract(input: &Path, output: &Path, bbox: &Bbox) -> Result<(), rusmium::Error>` so tests can drive it without spawning a process
- [x] 2.3 Pass 1: read input, build `in_box: HashSet<i64>` of node ids where `bbox.contains(&node.location()?)`
- [x] 2.4 Pass 2: read input, build `kept_ways`, `required_nodes` (seeded from `in_box`, extended with all node refs of each kept way), and `kept_rels` (relations referencing an in-box node or a kept way)
- [x] 2.5 Pass 3: read input and write with `Writer::create`: emit nodes in `required_nodes`, ways in `kept_ways` (whole), relations in `kept_rels` (whole); call `finish()`
- [x] 2.6 Header doc comment stating the semantics: reference extract (not geometric clipping), complete-ways, non-recursive relations

## 3. Wiring, tests, docs

- [x] 3.1 Add a `pixi` task (e.g. `example-bbox = "cargo run --example bbox_filter --"`) and confirm `pixi run build` compiles the example
- [x] 3.2 Integration test (`tests/bbox_filter.rs`): run `extract` on `tests/fixtures/sample.osm` with a box covering the in-box nodes; assert kept nodes/ways/relations and, critically, that every node referenced by a kept way is present (no dangling refs)
- [x] 3.3 Integration test: a box excluding everything yields an empty (but valid) output; a way straddling the boundary keeps its out-of-box node
- [x] 3.4 README: add a "Filtering by bounding box" section with the run command and a one-line note on complete-ways semantics
- [x] 3.5 `pixi run fmt` and `pixi run lint` clean; `pixi run test` green
