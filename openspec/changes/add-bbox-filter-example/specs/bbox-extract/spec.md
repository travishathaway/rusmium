## Purpose

Demonstrates, via a runnable example, extracting the subset of an OSM file that
intersects a bounding box using the complete-ways strategy: nodes inside the box,
the ways that touch them kept whole (with their out-of-box nodes pulled back in),
and the relations that reference kept objects — producing a self-consistent
output file.

## ADDED Requirements

### Requirement: Command-line bounding-box extract

The project SHALL provide a runnable example that takes an input OSM file path, an
output OSM file path, and a bounding box, and writes the box's extract to the
output file. The bounding box SHALL be accepted in the
`"min_lon,min_lat,max_lon,max_lat"` string form. Invalid arguments (missing
paths, an unparseable box) SHALL produce a clear error and a non-zero exit rather
than a panic or a malformed output file.

#### Scenario: Run the extract end to end

- **WHEN** the example is invoked with a readable input OSM file, a writable output path, and a valid bounding-box string
- **THEN** it writes an output OSM file containing the box's extract and exits successfully

#### Scenario: Reject invalid arguments

- **WHEN** the example is invoked with a missing argument or an unparseable bounding box
- **THEN** it prints an error and exits non-zero without producing a malformed output file

### Requirement: Keep nodes inside the box

The extract SHALL include every node whose location falls inside the bounding box
(inclusive), regardless of whether any way or relation references it.

#### Scenario: In-box node retained

- **WHEN** a node's location is inside the box
- **THEN** that node appears in the output

#### Scenario: Out-of-box lone node dropped

- **WHEN** a node's location is outside the box and no kept way references it
- **THEN** that node does not appear in the output

### Requirement: Keep touching ways complete

The extract SHALL include every way that references at least one in-box node, and
SHALL emit each such way whole. For every kept way, all of its referenced nodes
SHALL be present in the output — including nodes that lie outside the box — so
that no kept way has a dangling node reference.

#### Scenario: Way touching the box is kept whole

- **WHEN** a way references at least one node inside the box
- **THEN** the way appears in the output and every node it references also appears in the output

#### Scenario: Way entirely outside the box is dropped

- **WHEN** a way references no node inside the box
- **THEN** the way does not appear in the output

### Requirement: Keep referencing relations

The extract SHALL include every relation that references at least one kept node or
kept way. Relations SHALL be emitted whole; the extract SHALL NOT recursively
pull missing relation members back into the output (member references may point
outside the extract).

#### Scenario: Relation referencing a kept object is kept

- **WHEN** a relation has a member referencing an in-box node or a kept way
- **THEN** the relation appears in the output with its member list unchanged

#### Scenario: Relation referencing nothing kept is dropped

- **WHEN** a relation references no kept node or kept way
- **THEN** the relation does not appear in the output
