# Design — bounding-box filter example

## Context

`rusmium`'s public API already exposes everything a reference-based spatial
extract needs:

| Object   | What the bbox test needs      | Exposed by `rusmium` today          |
|----------|-------------------------------|-------------------------------------|
| Node     | its own lon/lat               | `Object::location() -> Option<Location>` |
| Way      | locations of its nodes        | node **refs** via `Body::Way { nodes }`  |
| Relation | geometry of its members       | member **refs** via `Body::Relation { members }` |

Ways and relations do **not** carry coordinates (geometry assembly is an explicit
v1 non-goal), but a bbox extract does not need geometry — it needs set
membership on ids, which the refs provide. `Reader::open` is re-callable, so the
algorithm can make multiple ordered passes over the file. No library reading /
writing / FFI changes are required; the only new library surface is the `Bbox`
predicate type.

## Goals / Non-goals

**Goals**
- Ship a reusable `Bbox` primitive in the library.
- Ship a runnable, documented example performing a *complete-ways* extract.
- Produce a **self-consistent** output: every kept way has all of its nodes.

**Non-goals**
- Geometry *clipping* — a way that crosses the boundary is kept whole, not cut at
  the box edge. (Reference extract, not geometric intersection.)
- Recursive relation completion — we do not chase missing relation members back
  into the output (matches osmium's `complete_ways`, not `smart`).
- Streaming in a single pass — correctness requires knowing all kept ways before
  writing nodes, which the file's node→way→relation ordering precludes.

## Decision 1 — `Bbox` type shape

Reuse the existing `Location` type for the corners rather than four bare floats:

```rust
/// An axis-aligned WGS84 bounding box. Bounds are inclusive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bbox {
    pub min: Location, // SW corner (min_lon, min_lat)
    pub max: Location, // NE corner (max_lon, max_lat)
}

impl Bbox {
    pub fn new(min_lon: f64, min_lat: f64, max_lon: f64, max_lat: f64) -> Bbox;
    pub fn contains(&self, loc: &Location) -> bool;
}
```

- `contains` is **inclusive** on all four edges.
- Latitude test is always `min_lat <= lat <= max_lat` (latitude never wraps).
- Longitude test **handles antimeridian wrap-around**:
  - normal box (`min_lon <= max_lon`): `min_lon <= lon <= max_lon`;
  - wrapping box (`min_lon > max_lon`, i.e. spanning ±180°):
    `lon >= min_lon || lon <= max_lon`.
- `new` stores the corners as given (it does **not** reorder longitudes, so the
  `min_lon > max_lon` wrap case is expressible); latitudes are expected
  `min_lat <= max_lat` and this is documented.

### Decision 1a — `FromStr`

```rust
impl std::str::FromStr for Bbox { type Err = BboxParseError; ... }
```

Parses `"min_lon,min_lat,max_lon,max_lat"` (four comma-separated `f64`). A
dedicated `BboxParseError` (wrong field count / non-numeric field) keeps parsing
errors distinct from the crate's IO `Error`. Both the example and library users
benefit; the example's arg handling becomes `arg.parse::<Bbox>()?`.

## Decision 2 — complete-ways extract algorithm (3 passes)

OSM files are ordered nodes → ways → relations, and an object already streamed
past cannot be re-emitted. So:

```
Pass 1 — read → in_box: HashSet<i64>
         node id inserted when bbox.contains(&node.location()?)

Pass 2 — read → kept_ways, required_nodes, kept_rels
         way:  if any node ref ∈ in_box
                 → kept_ways.insert(way.id)
                 → required_nodes.extend(all of way's node refs)   // "complete"
         (seed required_nodes ⊇ in_box so lone in-box nodes survive)
         relation: if any member (Node ref ∈ in_box) OR (Way ref ∈ kept_ways)
                 → kept_rels.insert(rel.id)
         // ways precede relations in-file, so kept_ways is complete
         // by the time relations are tested — single ordered pass

Pass 3 — read → Writer
         node     → emit if id ∈ required_nodes
         way      → emit if id ∈ kept_ways      (whole)
         relation → emit if id ∈ kept_rels      (whole)
         writer.finish()
```

**Why 3 passes and not 2.** `required_nodes` isn't fully known until every way is
read (pass 2), but nodes must be *written* first (they lead the file). So the
write must be its own pass after analysis. Passes 1 and 2 stay separate because a
way in pass 2 needs the fully-populated `in_box` set from pass 1.

**Memory.** Only `HashSet<i64>` id sets are retained (three of them). Objects are
not buffered; each pass re-reads from disk. Cost scales with the number of kept
ids, not file size in bytes.

### Alternatives considered
- **Node-only single pass** — trivial but yields dangling way refs; rejected as
  not a usable extract.
- **2 passes buffering kept objects in memory** — avoids the third disk pass at
  the cost of holding all kept `Object`s in RAM; rejected for the example because
  the 3-pass form is clearer and bounded in memory.

## Decision 3 — where the code lives

- `Bbox` + `FromStr` + `BboxParseError` in `src/lib.rs`, next to `Location`, with
  `#[cfg(test)]` unit tests.
- Extract logic in `examples/bbox_filter.rs`. The example owns the passes; the
  library owns only the reusable primitive.
- Integration test drives the example's extract against `tests/fixtures/sample.osm`
  (Berlin, ~lon 13 / lat 52). Because the example is a binary, factor the extract
  into a small function the test can call (e.g. a `fn extract(in, out, bbox)` in
  the example, or a tiny module) rather than shelling out.

## Open questions
- Should the extract function eventually move into the library as a real
  `extract`/filter API? Deferred — prove the shape in the example first.
- CLI ergonomics (flags, `--strategy`) — out of scope; positional args only.
