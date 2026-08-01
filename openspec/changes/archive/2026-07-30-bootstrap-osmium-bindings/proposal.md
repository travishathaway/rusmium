## Why

There is no idiomatic Rust binding for [libosmium](https://osmcode.org/libosmium/), the de-facto C++ library for reading and writing OpenStreetMap (OSM) data. Rust projects that need to process OSM extracts today must shell out to `osmium-tool`, reimplement PBF/XML parsing, or hand-roll unsafe FFI. libosmium is header-only, heavily templated, and uses a compile-time CRTP visitor model — none of which cross an FFI boundary directly — so a real binding requires a deliberate shim architecture. This change bootstraps that binding (`rusmium`) with a small, provably-correct vertical slice.

## What Changes

- Introduce the `rusmium` crate: a safe, idiomatic Rust API over libosmium.
- Add a hand-written C++ shim, bridged to Rust with the **cxx** crate, that flattens osmium's templates/CRTP into concrete opaque types (`Reader`, `Writer`, `Cursor`) plus builder functions.
- Provide **reading**: open an OSM file (PBF first, XML falling out of osmium's format abstraction) and iterate nodes/ways/relations as a pull-based Rust `Iterator`, exposing id, version, location, and tags (copy-out ownership — no borrowed-buffer lifetimes exposed in v1).
- Provide **writing**: construct nodes/ways/relations via a builder API and emit a valid OSM file.
- Manage the entire native toolchain (libosmium, protozero, zlib, expat, bzip2, lz4, C++ compiler, and the Rust toolchain) reproducibly with **pixi** over conda-forge; `build.rs` discovers headers/libs from the pixi environment prefix and sets an rpath so binaries run outside an activated env.
- Define "done" for v1 as a **round-trip integration test**: read file A → write file B → assert the two are semantically equal, exercising every layer in one assertion.
- Non-goals for v1: resolving way/relation member coordinates into geometry (refs only), a zero-copy borrowed API, and a native-osmium visitor/handler surface. These are deferred, not precluded.

## Capabilities

### New Capabilities
- `osm-file-reading`: Open OSM files and iterate their objects (nodes, ways, relations) with access to core attributes and tags via an idiomatic Rust iterator.
- `osm-file-writing`: Construct OSM objects and serialize them to a valid OSM file, enabling read → write round-trips.
- `native-build-integration`: Reproducibly provision libosmium and its native dependencies via pixi/conda-forge and compile+link the cxx shim through `build.rs`.

### Modified Capabilities
<!-- None — this is a greenfield library; no existing specs. -->

## Impact

- **New crate/repo layout**: `rusmium` crate root with `src/` (safe API + cxx bridge module), `cpp/` (shim `.cc`/`.h`), and `build.rs`.
- **New toolchain files**: `pixi.toml` + `pixi.lock` pinning the conda-forge dependency set and platforms; `Cargo.toml`.
- **Native dependencies**: libosmium, protozero (PBF), zlib, expat, bzip2, lz4, plus `cxx-compiler` and `rust` from conda-forge.
- **Build contract**: cargo must be driven through pixi (`pixi run …`) so `CONDA_PREFIX` is populated when `build.rs` runs; documented as a hard requirement.
- **CI**: reduces to `pixi run test` across the declared platforms (linux-64, osx-arm64, osx-64).
