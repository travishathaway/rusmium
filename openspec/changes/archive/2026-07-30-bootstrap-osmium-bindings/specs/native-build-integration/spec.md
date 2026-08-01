## Purpose

Ensures the C++ shim and libosmium native dependencies are provisioned reproducibly and compiled/linked automatically as part of the Rust build, so contributors and CI get a consistent, working build across supported platforms.

## ADDED Requirements

### Requirement: Reproducible native dependency provisioning

The project SHALL declare its native dependencies (libosmium, protozero, zlib, expat, bzip2, lz4, a C++ compiler, and the Rust toolchain) in a pinned, lockfile-backed manifest resolved from conda-forge, such that provisioning the environment yields identical dependency versions for every contributor on each supported platform (linux-64, osx-arm64, osx-64).

#### Scenario: Fresh environment resolves to locked versions

- **WHEN** a contributor provisions the environment from a clean checkout
- **THEN** the exact locked dependency versions are installed without manual dependency setup

### Requirement: Automated shim compilation and linking

The Rust build SHALL compile the C++ shim and link it against libosmium and its native dependencies as part of a normal build, discovering headers and libraries from the provisioned environment prefix. The build SHALL NOT require the contributor to hand-edit paths or compiler flags.

#### Scenario: Build within the provisioned environment succeeds

- **WHEN** the build runs inside the provisioned environment
- **THEN** the shim compiles, links against the native dependencies, and the crate builds successfully

#### Scenario: Build outside the provisioned environment fails clearly

- **WHEN** the build runs without the provisioned environment available (dependency prefix unset)
- **THEN** the build fails with a clear message indicating the environment must be provisioned/activated, rather than a confusing missing-header or missing-symbol error

### Requirement: Runtime resolution of native libraries

A binary or test built by the project SHALL locate the native shared libraries it depends on at run time without the contributor manually configuring the dynamic loader path for each invocation.

#### Scenario: Built test executable runs and loads native libraries

- **WHEN** a test executable produced by the build is run through the project's provisioned toolchain
- **THEN** it loads the required native shared libraries and executes without a loader error
