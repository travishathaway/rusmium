## Context

See `proposal.md` — Why. The binding target is unusual: libosmium is header-only C++14, heavily templated, and built around a compile-time CRTP visitor. bindgen/autocxx cannot consume its public API, and its objects are non-owning views into a `osmium::memory::Buffer` that the reader recycles. Those two facts — "no concrete ABI exists" and "objects are borrowed into recycled memory" — drive every decision below. Requirements are in `specs/` (`osm-file-reading`, `osm-file-writing`, `native-build-integration`).

## Goals / Non-Goals

**Goals:**
- A layered architecture where a hand-written C++ shim presents a concrete, opaque ABI and Rust builds the safe/idiomatic surface on top.
- Contain libosmium's buffer-lifetime hazard entirely inside the shim so no `unsafe` lifetime obligations leak to `rusmium` users in v1.
- A reproducible, one-command native toolchain via pixi so the build is deterministic across linux-64/osx-arm64/osx-64.
- A single round-trip integration test as the executable definition of "done".

**Non-Goals (design-level boundaries):**
- No zero-copy/borrowed-object API in v1 (copy-out only). The borrowed layer is a future addition, deliberately not designed here.
- No native osmium visitor/handler surface exposed to Rust — the iterator is the only read model.
- No resolution of way/relation members into geometry; references are passed through verbatim.
- No `-sys`/safe crate split in v1 (see Decisions).

## Decisions

### D1: cxx bridge over hand-written C shim
Use the `cxx` crate as the FFI boundary. The shim still hand-writes concrete C++ wrappers over osmium's templates, but cxx generates the safe Rust side, models opaque C++ types via `UniquePtr`, and — critically — translates C++ exceptions thrown by shim functions declared `-> Result<T>` into Rust `Err`. osmium throws on IO/parse errors, so this gives idiomatic error handling for free.
- *Alternatives:* raw `extern "C"` + bindgen (more manual `unsafe`, hand-rolled error codes); autocxx (leans on bindgen, falls over on osmium's header-only templates). Rejected for more glue / lower reliability on this library shape.

### D2: Opaque shim types flatten the templates
The shim defines plain, non-template classes that wrap the osmium templates: `OsmReader` (wraps `osmium::io::Reader`), `OsmWriter` (wraps `osmium::io::Writer`), and `Cursor` (owns the current `Buffer` plus a position). cxx sees only these opaque types plus POD shared structs (`Location { lon, lat }`, `TagPair { key, value }`) and an `ObjectKind` enum. No osmium template ever appears in the bridge.

### D3: Pull-cursor at the FFI boundary, `Iterator` in Rust
The shim exposes a cursor protocol: `advance(&mut Cursor) -> bool`, then accessors `kind`, `id`, `version`, `location`, `tags(out: &mut Vec<TagPair>)`. The Rust layer wraps this into `impl Iterator<Item = Object>`. This keeps the FFI a dumb pull-one-at-a-time loop while giving users native `?`/`filter`/`map`. It also aligns with how osmium actually reads (fill a buffer, walk items), so it is a thin mapping rather than an impedance mismatch.
- *Alternative:* callback/visitor across FFI (function pointers or trait objects invoked from C++). Rejected: re-entrancy and lifetime handling across the boundary are harder, and it is less idiomatic for Rust consumers.

### D4: Copy-out ownership contains the buffer-lifetime hazard
An osmium object is valid only until its buffer is refilled. Rather than encode "valid until refill" in Rust lifetimes (easy to get wrong = UB), the shim keeps the current buffer alive as a `Cursor` member and each accessor copies primitives and strings out into owned Rust values. Yielded `Object`s are therefore `'static`-owned and survive further iteration (satisfies `osm-file-reading`'s "object outlives next step" scenario). Cost: an allocation per object/tag — acceptable for v1; the zero-copy layer (D-future) revisits it.

### D5: Writing is a build-into-buffer-then-flush surface
Writing is not the inverse of the read accessors. The shim wraps osmium's builders (`NodeBuilder`, `WayBuilder`, …) behind `OsmWriter::add_node/add_way/add_relation(...)` that build a packed item into an internal buffer, plus `finish()` that flushes and closes. This roughly doubles the shim surface and is the larger half of the work — planned as its own milestone.

### D6: pixi manages the full toolchain, including the compiler and Rust
`pixi.toml` (+ `pixi.lock`) declares libosmium, protozero, zlib, expat, bzip2, lz4, `cxx-compiler`, and `rust` from conda-forge across the three platforms. Pulling `cxx-compiler` into the env means the shim is compiled ABI-compatible (matching `libstdc++`/exception ABI) with the conda-built libs it links — sidestepping a miserable class of link/runtime mismatch by construction. `rust` in-env keeps the toolchain reproducible and ABI-matched.
- *Alternative:* host rustup + system compiler. Rejected for v1: reintroduces the compiler/ABI-match question that pixi was chosen to eliminate. (Latest-stable Rust is not needed here.)

### D7: `build.rs` discovers the prefix and pins an rpath
`build.rs` resolves the environment prefix from `CONDA_PREFIX` (fallback: `PIXI_PROJECT_ROOT/.pixi/envs/default`), adds `<prefix>/include` to the cxx/`cc` include path, emits `rustc-link-search=<prefix>/lib` and `rustc-link-lib` for `z/expat/bz2/lz4/pthread`, and emits `rustc-link-arg=-Wl,-rpath,<prefix>/lib` so built binaries/tests find the conda `.so`s at run time (satisfies `native-build-integration`'s runtime-resolution requirement). If no prefix is found, `build.rs` fails fast with a message telling the contributor to run via pixi.

### D8: Single crate in v1, not a `-sys` split
Because cxx makes the raw layer thin (a `ffi` module inside the bridge), v1 ships one `rusmium` crate with an internal `ffi` module rather than a `rusmium-sys` + `rusmium` pair. The split can happen later if/when the crate is published and the raw layer needs independent versioning.

### D9: PBF-first; XML falls out of osmium's format abstraction
The vertical slice and tests target `.osm.pbf` (needs protozero + zlib). Because osmium selects the encoder/decoder by format behind a common interface, XML support (via expat) is largely incidental and not separately designed here.

## Risks / Trade-offs

- **conda rpath not set → binary links but won't load native libs at runtime** → D7 pins `-Wl,-rpath,<prefix>/lib`; the `native-build-integration` runtime scenario tests exactly this.
- **Compiler/`libstdc++` ABI mismatch between shim and conda libs** → D6 provisions `cxx-compiler` in the same env so the ABI matches by construction.
- **Bare `cargo build` (outside pixi) sees no `CONDA_PREFIX`** → D7 fails fast with a clear message; pixi `[tasks]` make `pixi run …` the natural path; documented as a hard build contract.
- **Copy-out allocates per object/tag → perf cost on large extracts (millions of objects)** → accepted for v1; isolated behind the `Object`/iterator API so a future zero-copy layer can be added without changing the read specs.
- **osmium's heavy templates make shim compilation slow** → keep the shim small and in one translation unit; incremental cargo builds only recompile it on shim changes.
- **cxx cannot express osmium iterators/templates directly** → by design the shim exposes only the flat cursor/builder protocol (D2/D3/D5); the bridge never references a template.

## Resolved Questions

- **Rust toolchain packaging** — the conda-forge `rust` package provides `cargo`, `rustc`, and (usefully) `clippy` and `rustfmt` in one env; no split package was needed. Pinned via `pixi.lock`. Verified at M0 with `rust 1.97.1` and conda gcc 14.4.0.
- **"Semantically equal" in the round-trip test** — resolved to a **normalized object stream**, not a byte comparison. Each object is canonicalized by (kind, id, version, fixed-point location at PBF's 1e-7° precision, ordered node refs / members, and *sorted* tags); the sorted set of canonical forms must match. This is robust to buffer- and format-level reordering (e.g. PBF grouping by type) while still asserting full content fidelity, and prints a human-diffable form on failure.

## Implementation Notes / Deviations

These emerged during implementation and refine, but do not contradict, the decisions above:

- **Structured data crosses as primitives + `Vec`s, not shared cxx structs** (refines D3). The cxx-generated header `#include`s `cpp/shim.h` *before* it defines any shared struct, so referencing shared structs from the shim header is an include-ordering hazard. Tags/members therefore cross as parallel `Vec<String>`/`Vec<i64>`/`Vec<u8>` (e.g. `tag_keys` + `tag_values`) rather than a `Vec<TagPair>`. Kinds cross as `u8` (osmium item_type: node=1/way=2/relation=3).
- **Read model completeness pulled forward** — way node references and relation members are read in the M2 read model (not deferred), since a faithful round-trip requires them on the read side.
- **Test fixture is XML `.osm`, not `.osm.pbf`** (refines D9). A committed PBF fixture is circular to produce before a writer exists, so read tests use a human-readable XML fixture; the PBF write+read path is covered by the M3 write-read test and the M4 round-trip (XML in → PBF out → read back).
- **FFI arity** — writer functions take flat argument lists (osmium exposes no stable ABI struct to pass), so `clippy::too_many_arguments` is explicitly allowed on the bridge module.
