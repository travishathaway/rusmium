## Purpose

Provides a geographic bounding-box value type for expressing spatial predicates
over OSM locations: constructing a box from WGS84 corners, testing whether a
location falls inside it (including boxes that cross the ±180° antimeridian), and
parsing a box from a compact string form.

## ADDED Requirements

### Requirement: Construct a bounding box from corners

The library SHALL provide a public `Bbox` value type constructed from a minimum
and maximum WGS84 corner (min longitude, min latitude, max longitude, max
latitude). The type SHALL be an owned, copyable Rust value independent of any
reader or file. Construction SHALL preserve the given longitudes without
reordering them, so that a box whose minimum longitude is greater than its
maximum longitude (an antimeridian-crossing box) is expressible.

#### Scenario: Build a box from four coordinates

- **WHEN** the caller constructs a `Bbox` from `min_lon`, `min_lat`, `max_lon`, `max_lat`
- **THEN** a `Bbox` is returned whose corners reflect the given coordinates as provided

### Requirement: Test whether a location is inside the box

`Bbox` SHALL expose a containment test against a `Location` that is inclusive on
all four edges. The latitude test SHALL be `min_lat <= lat <= max_lat`. For a
normal box (`min_lon <= max_lon`) the longitude test SHALL be
`min_lon <= lon <= max_lon`. For an antimeridian-crossing box (`min_lon > max_lon`)
the longitude test SHALL treat longitude as wrapping, matching when
`lon >= min_lon` OR `lon <= max_lon`.

#### Scenario: Location inside a normal box

- **WHEN** a location's longitude and latitude both fall within an inclusive normal box
- **THEN** the containment test returns true

#### Scenario: Location on an edge

- **WHEN** a location lies exactly on a box edge or corner
- **THEN** the containment test returns true (bounds are inclusive)

#### Scenario: Location outside the box

- **WHEN** a location's longitude or latitude falls outside the box
- **THEN** the containment test returns false

#### Scenario: Antimeridian-crossing box

- **WHEN** the box has `min_lon > max_lon` and a location's longitude is `>= min_lon` or `<= max_lon` (with latitude in range)
- **THEN** the containment test returns true, and a longitude in the excluded middle returns false

### Requirement: Parse a bounding box from a string

`Bbox` SHALL implement parsing from the string form
`"min_lon,min_lat,max_lon,max_lat"` (four comma-separated decimal numbers) via
the standard `FromStr` trait. Malformed input — the wrong number of fields or a
non-numeric field — SHALL surface as a distinct, recoverable parse error rather
than a panic.

#### Scenario: Parse a well-formed box string

- **WHEN** the caller parses `"13.0,52.0,13.5,52.5"` as a `Bbox`
- **THEN** a `Bbox` with the corresponding corners is returned

#### Scenario: Reject a malformed box string

- **WHEN** the caller parses a string with the wrong field count or a non-numeric field
- **THEN** an `Err` describing the parse failure is returned and the process does not panic
