# rusmium

Idiomatic Rust bindings for [libosmium](https://osmcode.org/libosmium/), the C++
library for reading and writing [OpenStreetMap](https://www.openstreetmap.org)
data.

```rust
use rusmium::{Reader, Writer, Object, Location, ObjectKind};

// Read: iterate objects like any Rust iterator.
let mut cafes = 0;
for obj in Reader::open("map.osm.pbf")? {
    if obj.kind() == ObjectKind::Node
        && obj.tags().iter().any(|(k, v)| k == "amenity" && v == "cafe")
    {
        cafes += 1;
    }
}

// Write: construct objects and append them.
let mut w = Writer::create("out.osm.pbf")?;
w.add(&Object::node(1, 1, Some(Location { lon: 13.0, lat: 52.0 }), vec![]))?;
w.finish()?;
# Ok::<(), rusmium::Error>(())
```

## Prerequisites

The native toolchain (libosmium and its dependencies, a C++ compiler, and the
Rust toolchain) is provisioned reproducibly with [pixi](https://pixi.sh) from
conda-forge. You do **not** need a system Rust or C++ compiler.

1. Install pixi (one-time):

   ```bash
   curl -fsSL https://pixi.sh/install.sh | bash
   ```

2. Provision the environment (reads `pixi.toml` / `pixi.lock`):

   ```bash
   pixi install
   ```

## Building and testing

**Always drive cargo through pixi** so the build script can find the native
dependencies:

```bash
pixi run build       # cargo build
pixi run test        # cargo test
pixi run roundtrip   # the read→write→read round-trip test
```

> A bare `cargo build` outside the pixi environment cannot see `CONDA_PREFIX`
> and will fail fast with a message telling you to build through pixi — rather
> than a confusing missing-header or missing-symbol error.

## Contributing

Dependencies are split across two pixi environments. `default` holds only what
is needed to build and run the crate; `dev` adds the contributor tooling, so a
plain `pixi install` stays lean.

```bash
pixi run -e dev hooks   # install the git hooks, once per clone
pixi run -e dev check   # run every hook over the whole tree, as CI does
```

Hooks are managed by [prek](https://prek.j178.dev/), a drop-in `pre-commit`
replacement, and configured in `.pre-commit-config.yaml`:

| stage | hooks |
| --- | --- |
| pre-commit | file hygiene (whitespace, EOF, YAML/TOML, large files), `cargo fmt`, `cargo clippy` |
| pre-push | `cargo test` — it builds the C++ shim, so it is too slow per commit |

CI runs the same hook definitions rather than a parallel list of its own, so the
two cannot drift apart. It also runs the test suite on Linux and macOS.

## Filtering by bounding box

`examples/bbox_filter.rs` extracts the part of an OSM file that intersects a
bounding box, given as `min_lon,min_lat,max_lon,max_lat`:

```bash
pixi run example-bbox in.osm.pbf out.osm.pbf 13.0,52.0,13.5,52.5
```

The id sets that drive the selection pick their representation automatically;
`--idset=sorted|dense` forces one, and `pixi run bench-bbox <input>` times all
three so the memory/speed trade is measurable.

It uses the **complete-ways** strategy: nodes inside the box are kept, any way
touching one is kept *whole* (its out-of-box nodes are pulled back in so geometry
stays complete), and relations referencing a kept object are kept. This is a
reference-based extract, not geometric clipping — a boundary-crossing way is kept
entire, not cut at the edge. The [`Bbox`](src/lib.rs) type it builds on (inclusive
containment, antimeridian wrap-around, `FromStr` parsing) is part of the public
library API.

## Architecture

libosmium is header-only, heavily templated, and built around a compile-time
CRTP visitor — none of which cross an FFI boundary directly. rusmium therefore
uses a layered design:

```
  rusmium            safe, idiomatic Rust API  (Reader/Writer/Object, Iterator)
  ─────────────────────────────────────────────────────────────────────────
  src/ffi.rs         cxx bridge — the single concrete ABI
  ─────────────────────────────────────────────────────────────────────────
  cpp/shim.{h,cc}    hand-written C++ shim: flattens osmium's templates into
                     opaque types (OsmReader/Cursor/OsmWriter) + plain funcs
  ─────────────────────────────────────────────────────────────────────────
  libosmium headers  +  zlib / expat / bzip2 / lz4   (from the pixi env)
```

Key design points:

- **Reading is pull-based.** The shim exposes a cursor (`advance()` + field
  accessors) built on osmium's `InputIterator`; the Rust side wraps it as a
  standard `Iterator<Item = Object>`.
- **Copy-out ownership.** osmium objects are non-owning views into a buffer that
  the reader recycles. To keep that hazard out of the public API, every accessor
  copies data into owned Rust values. An `Object` you retain stays valid and
  unchanged after the iterator advances — at the cost of an allocation per
  object/tag.
- **Writing builds into a buffer.** The shim wraps osmium's builders
  (`NodeBuilder`/`WayBuilder`/`RelationBuilder`) and flushes on `finish()`.
- **Structured data crosses as primitives and `Vec`s** (never shared cxx
  structs), keeping the shim header self-contained and free of include-ordering
  hazards.
- **Reproducible + ABI-matched.** Pulling `cxx-compiler` and `rust` from the
  same conda-forge env means the shim is compiled ABI-compatible with the
  prebuilt native libraries it links. `build.rs` bakes an rpath so binaries and
  tests resolve those libraries at run time without an activated environment.

## Scope (v1)

**Supported:** reading OSM files (PBF and XML) as an iterator with ids,
versions, node locations, tags, way node references, and relation members;
constructing and writing those objects back out; full read→write→read
round-trips.

**Non-goals (deferred, not precluded):**

- **Zero-copy / borrowed objects** — v1 is copy-out only.
- **Geometry assembly** — way/relation members are exposed as references, not
  resolved into coordinates/geometries.
- **A native osmium visitor/handler API** — the iterator is the only read model.

## License

Licensed under either of MIT or Apache-2.0, at your option.
