## Purpose

Provides a safe, idiomatic Rust way to construct OpenStreetMap objects and serialize them to a valid OSM data file, enabling programs to produce or transform OSM data.

## ADDED Requirements

### Requirement: Create an OSM file for writing

The library SHALL create an OSM data file for writing given a filesystem path, selecting the output format (at minimum PBF). It SHALL surface any failure to create or encode the file as a recoverable Rust error rather than a panic or process abort.

#### Scenario: Create a writable PBF target

- **WHEN** the caller creates a writer for a `.osm.pbf` path in a writable location
- **THEN** a writer is returned successfully and is ready to accept objects

#### Scenario: Create fails on an unwritable target

- **WHEN** the caller creates a writer for a path that cannot be written
- **THEN** the library returns an `Err` describing the failure and does not panic or abort the process

### Requirement: Construct and append OSM objects

The writer SHALL let the caller construct nodes, ways, and relations with their id, version, and tags — including for nodes a geographic location and for ways an ordered list of node references — and append them to the output. Attributes and tags supplied by the caller SHALL be preserved in the written object.

#### Scenario: Write a tagged node

- **WHEN** the caller appends a node with an id, a location, and one or more tags
- **THEN** the node is recorded for output with exactly those attributes and tags

#### Scenario: Write a way with node references

- **WHEN** the caller appends a way with an ordered list of node references and tags
- **THEN** the way is recorded for output preserving the reference order and tags

### Requirement: Finalize the output file

The writer SHALL provide an explicit completion step that flushes all appended objects and closes the file, producing a valid OSM file readable by conformant OSM tooling. Errors during finalization SHALL be reported to the caller.

#### Scenario: Finalize produces a valid file

- **WHEN** the caller finalizes a writer after appending objects
- **THEN** all appended objects are flushed, the file is closed, and the result is a valid OSM file

#### Scenario: Round-trip preserves object content

- **WHEN** the caller reads objects from a source file, appends each to a writer, and finalizes it
- **THEN** reading the produced file back yields the same objects — matching ids, kinds, locations, references, and tags — as the source
