use std::path::PathBuf;

fn main() {
    let prefix = resolve_prefix();
    let include = prefix.join("include");
    let lib = prefix.join("lib");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());

    // Compile the cxx-generated glue together with our C++ shim.
    // `.include(&manifest)` lets the bridge resolve `include!("cpp/shim.h")`;
    // `.include(&include)` exposes the libosmium and protozero headers.
    cxx_build::bridge("src/ffi.rs")
        .file("cpp/shim.cc")
        .include(&manifest)
        .include(&include)
        .std("c++17")
        // libosmium and protozero are header-only, so the entire PBF codec is
        // compiled *here* rather than linked from a prebuilt library. `cc`
        // otherwise forwards cargo's OPT_LEVEL, which means a `cargo build`
        // without --release ships a decoder built at -O0 with osmium's
        // asserts live — measurably an order of magnitude slower. Pin the
        // native side to optimized, the way cargo's
        // `[profile.dev.package."*"]` treats third-party Rust crates.
        .opt_level(2)
        .define("NDEBUG", None)
        .flag_if_supported("-Wno-unused-parameter")
        .compile("rusmium_shim");

    // Link the native libraries libosmium depends on (PBF/XML/compression).
    println!("cargo:rustc-link-search=native={}", lib.display());
    for l in ["z", "expat", "bz2", "lz4", "pthread"] {
        println!("cargo:rustc-link-lib=dylib={l}");
    }
    // Bake an rpath so built binaries/tests locate the conda .so files at
    // run time without an activated environment.
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());

    println!("cargo:rerun-if-changed=cpp/shim.cc");
    println!("cargo:rerun-if-changed=cpp/shim.h");
    println!("cargo:rerun-if-changed=src/ffi.rs");
    println!("cargo:rerun-if-env-changed=CONDA_PREFIX");
}

/// Locate the environment prefix that holds the libosmium headers and native
/// libraries. Prefer an active conda/pixi env; fall back to this project's
/// default pixi env; otherwise fail with actionable guidance.
fn resolve_prefix() -> PathBuf {
    if let Ok(p) = std::env::var("CONDA_PREFIX") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(root) = std::env::var("PIXI_PROJECT_ROOT") {
        let candidate = PathBuf::from(root).join(".pixi/envs/default");
        if candidate.join("include/osmium").is_dir() {
            return candidate;
        }
    }
    // Last resort: this crate's own default pixi env, if it has been installed.
    let candidate =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join(".pixi/envs/default");
    if candidate.join("include/osmium").is_dir() {
        return candidate;
    }

    panic!(
        "rusmium: could not locate the native dependency prefix (libosmium et al.).\n\
         CONDA_PREFIX is unset and no provisioned pixi environment was found.\n\
         Build through pixi so the native toolchain is on hand, e.g.:\n\
         \n    pixi install   # once\n    pixi run build\n    pixi run test\n\
         \nSee README.md for setup details."
    );
}
