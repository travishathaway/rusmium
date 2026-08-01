## 1. M0 — Toolchain & build plumbing (native-build-integration)

- [x] 1.1 Add `pixi.toml` declaring platforms (linux-64, osx-arm64, osx-64) and conda-forge deps: libosmium, protozero, zlib, expat, bzip2, lz4, cxx-compiler, rust
- [x] 1.2 Run `pixi install`, commit the resulting `pixi.lock`; verify headers exist under `$CONDA_PREFIX/include/osmium` and `.../protozero`
- [x] 1.3 Add pixi `[tasks]`: `build = "cargo build"`, `test = "cargo test"`, `roundtrip = "cargo test --test roundtrip"`
- [x] 1.4 Scaffold the `rusmium` crate: `Cargo.toml` (add `cxx`, dev-deps), `src/lib.rs`, `cpp/` dir, `build.rs`
- [x] 1.5 Implement `build.rs`: resolve prefix from `CONDA_PREFIX` (fallback `PIXI_PROJECT_ROOT/.pixi/envs/default`), add include path, emit link-search/link-lib (z, expat, bz2, lz4, pthread) and `-Wl,-rpath,<prefix>/lib`; fail fast with a clear message when no prefix is found
- [x] 1.6 Prove the gate: compile a trivial C++ TU that `#include`s an osmium header and links the deps; `pixi run build` is green
- [x] 1.7 Verify the fail-clearly path: a bare `cargo build` (no `CONDA_PREFIX`) errors with the guidance message, not a raw missing-header/symbol error

## 2. M1 — Walking skeleton: cxx bridge + read cursor (osm-file-reading)

- [x] 2.1 Define the `#[cxx::bridge]` module: opaque `OsmReader`/`Cursor`, shared `Location` and `TagPair` structs, `ObjectKind` enum
- [x] 2.2 Implement shim `OsmReader` wrapping `osmium::io::Reader`; `open_reader(path) -> Result<UniquePtr<OsmReader>>` (cxx maps C++ exceptions to `Err`)
- [x] 2.3 Implement shim `Cursor` that owns the current `osmium::memory::Buffer` + position; `cursor(&mut OsmReader)` and `advance(&mut Cursor) -> bool`
- [x] 2.4 Add cursor accessor `kind(&Cursor) -> ObjectKind`
- [x] 2.5 Integration test: count nodes/ways/relations in a small committed `.osm.pbf` fixture and assert expected totals (`pixi run test`)

## 3. M2 — Read model: idiomatic iterator with attributes & tags (osm-file-reading)

- [x] 3.1 Add cursor accessors `id`, `version`, `location`; copy values out (copy-out ownership per design D4)
- [x] 3.2 Add `tags(&Cursor, out: &mut Vec<TagPair>)`; copy keys/values into owned strings
- [x] 3.3 Define the owned Rust `Object` model (Node/Way/Relation) and a public `Reader` that returns `impl Iterator<Item = Object>`
- [x] 3.4 Test: iterate to completion yields each object exactly once and terminates; kinds discriminate correctly
- [x] 3.5 Test: a retained object stays valid/unchanged after advancing the iterator (copy-out contract)
- [x] 3.6 Test: tagged object exposes owned key/value strings; untagged object yields an empty tag set

## 4. M3 — Write model: builders + writer (osm-file-writing)

- [x] 4.1 Extend the bridge with opaque `OsmWriter`; `create_writer(path) -> Result<UniquePtr<OsmWriter>>`
- [x] 4.2 Implement shim `add_node(id, version, Location, tags)` using `osmium::builder::NodeBuilder` into an internal buffer
- [x] 4.3 Implement shim `add_way(id, version, node_refs, tags)` and `add_relation(...)` builders
- [x] 4.4 Implement `finish(&mut OsmWriter) -> Result<()>` to flush the buffer and close the file
- [x] 4.5 Public Rust `Writer` API wrapping the above; errors surfaced as `Result`
- [x] 4.6 Test: create fails clearly on an unwritable path; a written tagged node/way reads back with matching attributes and tags

## 5. M4 — Round-trip definition of done (osm-file-writing)

- [x] 5.1 Decide and document the "semantically equal" comparison (normalized object stream vs bytes) per design Open Question
- [x] 5.2 Write `tests/roundtrip.rs`: read fixture A → append every object to writer → finish B → assert B is semantically equal to A
- [x] 5.3 Wire `pixi run roundtrip`; ensure it passes in the provisioned env
- [x] 5.4 Confirm the built test executable resolves native `.so`s at runtime via the rpath (native-build-integration runtime scenario)

## 6. Documentation & wrap-up

- [x] 6.1 README: prerequisites (install pixi), `pixi install`, and the hard rule to drive cargo via `pixi run …`
- [x] 6.2 Document the layered architecture (shim → cxx → safe API), copy-out semantics, and v1 non-goals (zero-copy, geometry resolution, visitor API)
- [x] 6.3 Update `openspec/config.yaml` project `context` with the tech stack for future changes
