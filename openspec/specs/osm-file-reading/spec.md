## Purpose

Provides a safe, idiomatic Rust way to open an OpenStreetMap data file and iterate over the objects it contains (nodes, ways, relations) with access to their core attributes and tags.

## Requirements

### Requirement: Open an OSM file for reading

The library SHALL open an OSM data file for reading given a filesystem path, detecting the format from the file. It SHALL support at minimum the PBF format and SHALL surface any failure to open or parse the file as a recoverable Rust error rather than a panic or process abort.

#### Scenario: Open a valid PBF file

- **WHEN** the caller opens an existing, well-formed `.osm.pbf` file
- **THEN** a reader is returned successfully and is ready to yield objects

#### Scenario: Open a nonexistent or malformed file

- **WHEN** the caller opens a path that does not exist or is not a valid OSM file
- **THEN** the library returns an `Err` describing the failure and does not panic or abort the process

### Requirement: Iterate objects as a Rust iterator

The reader SHALL expose OSM objects through a pull-based Rust `Iterator`, yielding each node, way, and relation in the file exactly once. Iteration SHALL terminate cleanly at end of input. The caller SHALL be able to distinguish the kind (node, way, or relation) of each yielded object.

#### Scenario: Iterate every object

- **WHEN** the caller iterates a reader to completion
- **THEN** every node, way, and relation in the file is yielded exactly once and the iterator then terminates

#### Scenario: Discriminate object kind

- **WHEN** the caller inspects a yielded object
- **THEN** the caller can determine whether it is a node, a way, or a relation

### Requirement: Access core object attributes

For each yielded object the library SHALL expose its OSM id and version. For nodes it SHALL additionally expose the geographic location (longitude and latitude). Objects SHALL be owned Rust values whose validity does not depend on the reader's internal buffer state (copy-out semantics).

#### Scenario: Read a node's id and location

- **WHEN** the caller inspects a yielded node
- **THEN** the node's id, version, longitude, and latitude are available as owned Rust values

#### Scenario: Object outlives the next iteration step

- **WHEN** the caller retains a yielded object and then advances the iterator
- **THEN** the retained object's attributes remain valid and unchanged

### Requirement: Access object tags

For each yielded object the library SHALL expose its tags as key/value string pairs, preserving the tags present on the object (including the empty set). Tag keys and values SHALL be exposed as owned Rust strings.

#### Scenario: Read tags of a tagged object

- **WHEN** the caller inspects a yielded object that carries tags
- **THEN** each tag's key and value are available as owned Rust strings

#### Scenario: Read tags of an untagged object

- **WHEN** the caller inspects a yielded object with no tags
- **THEN** an empty tag set is reported and no error occurs
