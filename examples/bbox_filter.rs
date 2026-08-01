//! Extract the part of an OSM file that intersects a bounding box.
//!
//! This is a **reference-based** extract, not geometric clipping: a way that
//! crosses the box boundary is kept *whole* (not cut at the edge), and its
//! out-of-box nodes are pulled back into the output so the way keeps a complete
//! geometry. This is the "complete ways" strategy:
//!
//! - a node is kept if its location is inside the box;
//! - a way is kept if it references at least one in-box node, and *all* of its
//!   nodes are then included (even the ones outside the box);
//! - a relation is kept if it references a kept node or a kept way. Relations are
//!   emitted whole and membership is **not** completed recursively, so a kept
//!   relation may reference objects that are not in the extract.
//!
//! Run it through pixi so the native toolchain is available:
//!
//! ```text
//! pixi run example-bbox in.osm.pbf out.osm.pbf 13.0,52.0,13.5,52.5
//! ```
//!
//! The shape follows osmium-tool's `complete_ways` strategy (its `Pass1` and
//! `Pass2`). OSM files are ordered nodes → ways → relations and an object
//! already streamed past cannot be re-emitted, so two ordered passes are needed:
//! one to decide what to keep, one to write it. The first pass gets everything
//! it needs from a single read *because* of that ordering — every node is seen
//! before any way, and every way before any relation.
//!
//! The first pass reads no metadata and no tags and never looks at a full
//! object's contents, so it asks the reader to skip both.

use std::path::Path;
use std::process::ExitCode;

use rusmium::{Bbox, ObjectKind, ReadOptions, Reader, Writer};

pub use idset::Backend;
use idset::IdSet;

fn main() -> ExitCode {
    let mut backend = Backend::Auto;
    let mut positional: Vec<String> = Vec::new();
    let mut argv = std::env::args();
    let program = argv.next().unwrap_or_else(|| "bbox_filter".to_string());

    for arg in argv {
        match arg.strip_prefix("--idset=") {
            Some(value) => match Backend::parse(value) {
                Some(b) => backend = b,
                None => {
                    eprintln!("error: unknown --idset value {value:?} (auto, sorted, dense)");
                    return ExitCode::FAILURE;
                }
            },
            None => positional.push(arg),
        }
    }

    if positional.len() != 3 {
        eprintln!(
            "usage: {program} [--idset=auto|sorted|dense] \
             <input.osm.pbf> <output.osm.pbf> <min_lon,min_lat,max_lon,max_lat>"
        );
        return ExitCode::FAILURE;
    }

    let input = Path::new(&positional[0]);
    let output = Path::new(&positional[1]);
    let bbox: Bbox = match positional[2].parse() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    match extract_with(input, output, &bbox, backend) {
        Ok(stats) => {
            println!(
                "wrote {} nodes, {} ways, {} relations to {}",
                stats.nodes,
                stats.ways,
                stats.relations,
                output.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Counts of what the extract wrote, for reporting and testing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub nodes: usize,
    pub ways: usize,
    pub relations: usize,
}

/// The four id sets that decide the contents of the extract.
///
/// `in_box` and `extra_nodes` are deliberately kept apart rather than merged
/// into one "required nodes" set: a way qualifies only by touching an *in-box*
/// node, so pulling an out-of-box node in for one way must not make a later way
/// qualify through it.
struct Selection {
    in_box: IdSet,
    extra_nodes: IdSet,
    ways: IdSet,
    relations: IdSet,
}

impl Selection {
    fn new(backend: Backend) -> Selection {
        Selection {
            in_box: IdSet::new(backend),
            extra_nodes: IdSet::new(backend),
            ways: IdSet::new(backend),
            relations: IdSet::new(backend),
        }
    }
}

/// Extract `bbox` from `input` into `output` using the complete-ways strategy,
/// letting the id sets pick their own representation.
///
/// Factored out of `main` so integration tests can drive it directly instead of
/// spawning a process.
pub fn extract(input: &Path, output: &Path, bbox: &Bbox) -> Result<Stats, rusmium::Error> {
    extract_with(input, output, bbox, Backend::Auto)
}

/// As [`extract`], with an explicit id-set representation. All backends produce
/// identical output; they differ only in memory and lookup cost.
pub fn extract_with(
    input: &Path,
    output: &Path,
    bbox: &Bbox,
    backend: Backend,
) -> Result<Stats, rusmium::Error> {
    let selection = select(input, bbox, backend)?;
    write(input, output, &selection)
}

/// Pass 1: decide what the extract contains. Relies on the nodes → ways →
/// relations ordering so a single read suffices.
///
/// That ordering also gives every set a **freeze point**: the moment it stops
/// being written, which is always before it starts being read. `in_box` is
/// complete once the first way appears, `ways` once the first relation does,
/// and the rest at end of pass. Freezing is what lets a sorted set sort once
/// and then answer by binary search — and it is where an `Auto` set decides
/// which representation to keep, knowing its final size and id range.
fn select(input: &Path, bbox: &Bbox, backend: Backend) -> Result<Selection, rusmium::Error> {
    let mut sel = Selection::new(backend);
    let mut phase = ObjectKind::Node;

    // Ids, node locations, way node refs and relation members only — no
    // metadata, no tags.
    let opts = ReadOptions::default().metadata(false).tags(false);
    let mut reader = Reader::open_with(input, opts)?;

    // Reused across the whole pass so the loop allocates nothing.
    let mut refs: Vec<i64> = Vec::new();
    let mut kinds: Vec<u8> = Vec::new();

    while let Some(obj) = reader.next_ref() {
        let kind = obj.kind();
        if kind != phase {
            // A kind we have not seen before means every earlier kind is done.
            // Freezing is idempotent, so covering both here also handles files
            // that skip a kind entirely (e.g. no ways at all).
            match kind {
                ObjectKind::Node => {}
                ObjectKind::Way => sel.in_box.freeze(),
                ObjectKind::Relation => {
                    sel.in_box.freeze();
                    sel.ways.freeze();
                }
            }
            phase = kind;
        }

        match kind {
            ObjectKind::Node => {
                if let Some(loc) = obj.location() {
                    if bbox.contains(&loc) {
                        sel.in_box.insert(obj.id());
                    }
                }
            }
            ObjectKind::Way => {
                obj.way_node_refs_into(&mut refs);
                if refs.iter().any(|n| sel.in_box.contains(*n)) {
                    sel.ways.insert(obj.id());
                    // Keep the way whole: every node it references must be in
                    // the output, in-box or not.
                    for n in &refs {
                        sel.extra_nodes.insert(*n);
                    }
                }
            }
            ObjectKind::Relation => {
                obj.relation_members_into(&mut kinds, &mut refs);
                let refs_kept = kinds.iter().zip(&refs).any(|(kind, id)| {
                    match ObjectKind::from_item_type(*kind) {
                        Some(ObjectKind::Node) => sel.in_box.contains(*id),
                        Some(ObjectKind::Way) => sel.ways.contains(*id),
                        _ => false,
                    }
                });
                if refs_kept {
                    sel.relations.insert(obj.id());
                }
            }
        }
    }

    // Everything not already frozen by a phase transition is done now.
    sel.in_box.freeze();
    sel.ways.freeze();
    sel.extra_nodes.freeze();
    sel.relations.freeze();

    Ok(sel)
}

/// Pass 2: copy the selected objects to the output, metadata and tags included.
fn write(input: &Path, output: &Path, sel: &Selection) -> Result<Stats, rusmium::Error> {
    let mut writer = Writer::create(output)?;
    let mut reader = Reader::open(input)?;
    let mut stats = Stats::default();

    // Nothing here materialises an object: the keep test reads two fields, and
    // a kept object is memcpy'd from the decode buffer to the output buffer.
    // Tags, node refs and members are never touched.
    while let Some(obj) = reader.next_ref() {
        let kind = obj.kind();
        let id = obj.id();
        let keep = match kind {
            ObjectKind::Node => sel.in_box.contains(id) || sel.extra_nodes.contains(id),
            ObjectKind::Way => sel.ways.contains(id),
            ObjectKind::Relation => sel.relations.contains(id),
        };
        if keep {
            writer.copy(&obj)?;
            match kind {
                ObjectKind::Node => stats.nodes += 1,
                ObjectKind::Way => stats.ways += 1,
                ObjectKind::Relation => stats.relations += 1,
            }
        }
    }

    writer.finish()?;
    Ok(stats)
}

/// Sets of OSM object ids, in two shapes with very different cost curves.
///
/// The two mirror libosmium's own pair (`osmium/index/id_set.hpp`):
///
/// - [`Dense`](Backend::Dense) ports `IdSetDense` — a bitmap in chunks allocated
///   on first touch. Membership is a shift and a mask, but the memory is
///   **O(id range), not O(ids stored)**: about `max_id / 8` bytes whether the
///   set holds a hundred ids or a hundred million.
/// - [`Sorted`](Backend::Sorted) ports `IdSetSmall` — a sorted `Vec` answering
///   by binary search, costing 8 bytes per id and nothing for the gaps.
///
/// Sorted is smaller until `8 · count > max_id / 8`, i.e. `count > max_id / 64`.
/// Against today's OSM node ids (~1.4e10) that crossover is around 220M ids, so
/// any regional extract is far better served by sorted — a Brandenburg-sized
/// job holds ~4.3M ids, where dense allocates 3.4 GB and sorted needs ~72 MB.
/// Dense wins only at continent or planet scale, where its cost is a *ceiling*
/// while sorted keeps growing.
///
/// [`Backend::Auto`] applies that comparison per set at its freeze point, when
/// the final count and id range are both known.
mod idset {
    /// Which representation an [`IdSet`] should use.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum Backend {
        /// Start sorted, and switch to dense only for sets where dense would
        /// genuinely be smaller. The right choice unless you are measuring.
        #[default]
        Auto,
        /// Always sorted: least memory below the crossover.
        Sorted,
        /// Always dense: fastest lookups, and a fixed ceiling on memory.
        Dense,
    }

    impl Backend {
        /// Parse the `--idset` argument.
        pub fn parse(s: &str) -> Option<Backend> {
            match s {
                "auto" => Some(Backend::Auto),
                "sorted" => Some(Backend::Sorted),
                "dense" => Some(Backend::Dense),
                _ => None,
            }
        }
    }

    /// A set of OSM ids that fills, freezes, then answers lookups.
    pub struct IdSet {
        repr: Repr,
        backend: Backend,
    }

    enum Repr {
        Sorted(SortedIdSet),
        Dense(DenseIdSet),
    }

    /// Ids are keyed by magnitude, matching osmium's `positive_id()`, so the
    /// negative ids some editors use for not-yet-uploaded objects do not alias
    /// onto a different slot.
    fn key(id: i64) -> u64 {
        id.unsigned_abs()
    }

    impl IdSet {
        pub fn new(backend: Backend) -> IdSet {
            let repr = match backend {
                Backend::Dense => Repr::Dense(DenseIdSet::default()),
                Backend::Auto | Backend::Sorted => Repr::Sorted(SortedIdSet::default()),
            };
            IdSet { repr, backend }
        }

        pub fn insert(&mut self, id: i64) {
            match &mut self.repr {
                Repr::Sorted(s) => s.insert(key(id)),
                Repr::Dense(d) => d.insert(key(id)),
            }
        }

        /// Mark the set complete: sort and deduplicate a sorted set, and — for
        /// [`Backend::Auto`] — promote to dense if that is now the smaller
        /// representation. Idempotent, so callers can freeze defensively.
        pub fn freeze(&mut self) {
            let Repr::Sorted(sorted) = &mut self.repr else {
                return;
            };
            if sorted.frozen {
                return;
            }
            sorted.freeze();

            if self.backend == Backend::Auto && sorted.should_promote() {
                let sorted = std::mem::take(sorted);
                self.repr = Repr::Dense(sorted.into_dense());
            }
        }

        pub fn contains(&self, id: i64) -> bool {
            match &self.repr {
                Repr::Sorted(s) => s.contains(key(id)),
                Repr::Dense(d) => d.contains(key(id)),
            }
        }

        /// Which representation is in use, so tests can assert what `Auto`
        /// decided rather than inferring it from timings.
        #[cfg(test)]
        pub fn repr(&self) -> &'static str {
            match self.repr {
                Repr::Sorted(_) => "sorted",
                Repr::Dense(_) => "dense",
            }
        }
    }

    /// A sorted, deduplicated `Vec` of ids — libosmium's `IdSetSmall`.
    ///
    /// Inserts append; duplicates and disorder are resolved by [`Self::freeze`].
    /// Ids that arrive already ascending (object ids do, within a kind) make
    /// that sort nearly free.
    struct SortedIdSet {
        ids: Vec<u64>,
        frozen: bool,
        /// Length at which to compact early, so a set fed many duplicates —
        /// way node refs, where adjacent ways share endpoints — cannot grow
        /// without bound before it is frozen.
        compact_at: usize,
    }

    impl Default for SortedIdSet {
        fn default() -> Self {
            SortedIdSet {
                ids: Vec::new(),
                frozen: false,
                compact_at: INITIAL_COMPACT_AT,
            }
        }
    }

    /// Small enough to bound duplicate growth, large enough that short-lived
    /// sets never pay for a compaction.
    const INITIAL_COMPACT_AT: usize = 1 << 16;

    impl SortedIdSet {
        fn insert(&mut self, id: u64) {
            debug_assert!(!self.frozen, "insert() after freeze()");
            self.ids.push(id);
            if self.ids.len() >= self.compact_at {
                self.compact();
                // Compact again only once the set has doubled, keeping the
                // amortized cost of deduplication constant per insert.
                self.compact_at = (self.ids.len() * 2).max(INITIAL_COMPACT_AT);
            }
        }

        fn compact(&mut self) {
            self.ids.sort_unstable();
            self.ids.dedup();
        }

        fn freeze(&mut self) {
            self.compact();
            self.ids.shrink_to_fit();
            self.frozen = true;
        }

        fn contains(&self, id: u64) -> bool {
            debug_assert!(
                self.frozen,
                "contains() before freeze(): the set is unsorted, so lookups would be wrong"
            );
            self.ids.binary_search(&id).is_ok()
        }

        /// Whether a dense bitmap would now be the smaller representation.
        /// Sorted costs 8 bytes per id; dense costs about `max_id / 8` bytes
        /// regardless of how many ids it holds.
        fn should_promote(&self) -> bool {
            debug_assert!(self.frozen, "should_promote() needs the final contents");
            let max_id = self.ids.last().copied().unwrap_or(0);
            self.ids.len() as u64 > max_id / 64
        }

        /// Convert to a bitmap, releasing the vector as it goes rather than
        /// holding both representations whole. At the crossover the two are the
        /// same size by construction, so the transient would otherwise double
        /// exactly when memory is already the binding constraint.
        fn into_dense(mut self) -> DenseIdSet {
            let mut dense = DenseIdSet::default();
            const BATCH: usize = 1 << 20;
            while !self.ids.is_empty() {
                let cut = self.ids.len().saturating_sub(BATCH);
                for id in self.ids.drain(cut..) {
                    dense.insert(id);
                }
                // Halving whenever the vector is mostly empty keeps the total
                // copying linear rather than quadratic.
                if self.ids.len() * 2 <= self.ids.capacity() {
                    self.ids.shrink_to_fit();
                }
            }
            dense
        }
    }

    /// A bitmap of ids in lazily allocated chunks — libosmium's `IdSetDense`.
    #[derive(Default)]
    struct DenseIdSet {
        chunks: Vec<Option<Box<[u8]>>>,
    }

    /// Bytes per chunk. Each chunk therefore covers `CHUNK_BYTES * 8` ids.
    /// libosmium's default, and a reasonable compromise between allocation
    /// count and waste on sparse sets.
    const CHUNK_BYTES: usize = 1 << 22;

    impl DenseIdSet {
        fn chunk_index(id: u64) -> usize {
            (id >> (CHUNK_BYTES.trailing_zeros() + 3)) as usize
        }

        fn byte_offset(id: u64) -> usize {
            (id >> 3) as usize & (CHUNK_BYTES - 1)
        }

        fn bit(id: u64) -> u8 {
            1u8 << (id & 0b111)
        }

        fn insert(&mut self, id: u64) {
            let index = Self::chunk_index(id);
            if index >= self.chunks.len() {
                self.chunks.resize_with(index + 1, || None);
            }
            let chunk =
                self.chunks[index].get_or_insert_with(|| vec![0u8; CHUNK_BYTES].into_boxed_slice());
            chunk[Self::byte_offset(id)] |= Self::bit(id);
        }

        fn contains(&self, id: u64) -> bool {
            match self
                .chunks
                .get(Self::chunk_index(id))
                .and_then(Option::as_ref)
            {
                Some(chunk) => chunk[Self::byte_offset(id)] & Self::bit(id) != 0,
                None => false,
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{Backend, IdSet};

        /// Every backend must satisfy the same contract, so the bodies below
        /// run against all of them rather than testing one representation.
        const BACKENDS: [Backend; 3] = [Backend::Auto, Backend::Sorted, Backend::Dense];

        fn build(backend: Backend, ids: &[i64]) -> IdSet {
            let mut set = IdSet::new(backend);
            for id in ids {
                set.insert(*id);
            }
            set.freeze();
            set
        }

        #[test]
        fn stores_and_recalls_ids_across_chunks() {
            // Spans several dense chunks, including the first and eighth bit of
            // a byte, and is deliberately given out of order with a duplicate.
            let ids = [12_345_678_901, 7, 1 << 25, 1, 4242, 8, (1 << 25) + 1, 4242];
            for backend in BACKENDS {
                let set = build(backend, &ids);
                for id in ids {
                    assert!(set.contains(id), "{backend:?}: id {id} should be present");
                }
                for id in [0, 2, 6, 9, 4241, (1 << 25) + 2, 12_345_678_900] {
                    assert!(!set.contains(id), "{backend:?}: id {id} should be absent");
                }
            }
        }

        #[test]
        fn empty_set_contains_nothing() {
            for backend in BACKENDS {
                let set = build(backend, &[]);
                assert!(!set.contains(0), "{backend:?}");
                assert!(!set.contains(999_999_999), "{backend:?}");
            }
        }

        #[test]
        fn negative_ids_are_keyed_by_magnitude() {
            for backend in BACKENDS {
                let set = build(backend, &[-42]);
                assert!(set.contains(-42), "{backend:?}");
                assert!(set.contains(42), "{backend:?}");
            }
        }

        #[test]
        fn survives_more_inserts_than_the_compaction_threshold() {
            // Crosses the early-compaction path several times, with every id
            // inserted three times so deduplication actually has work to do.
            let ids: Vec<i64> = (0..3)
                .flat_map(|_| (0..200_000i64).map(|i| i * 7))
                .collect();
            for backend in BACKENDS {
                let set = build(backend, &ids);
                for i in [0i64, 1, 99_999, 199_999] {
                    assert!(set.contains(i * 7), "{backend:?}: {i}");
                    assert!(!set.contains(i * 7 + 1), "{backend:?}: {i} + 1");
                }
            }
        }

        #[test]
        fn freeze_is_idempotent() {
            let mut set = build(Backend::Auto, &[5, 9]);
            set.freeze();
            set.freeze();
            assert!(set.contains(5) && set.contains(9));
        }

        #[test]
        fn auto_keeps_a_sparse_set_sorted_and_promotes_a_dense_one() {
            // Sparse: a few ids spread over a huge range — dense would cost
            // max_id/8 bytes to hold almost nothing, so stay sorted.
            let sparse = build(Backend::Auto, &[1, 1_000_000_000, 13_000_000_000]);
            assert_eq!(sparse.repr(), "sorted", "sparse set should stay sorted");

            // Contiguous: more than max_id/64 ids present, so the bitmap is the
            // smaller of the two and Auto should switch to it.
            let contiguous: Vec<i64> = (0..5000).collect();
            let dense = build(Backend::Auto, &contiguous);
            assert_eq!(dense.repr(), "dense", "contiguous set should promote");
            for id in [0i64, 2500, 4999] {
                assert!(dense.contains(id));
            }
            assert!(!dense.contains(5000));
        }

        #[test]
        #[should_panic(expected = "before freeze()")]
        #[cfg(debug_assertions)]
        fn querying_before_freeze_is_caught() {
            let mut set = IdSet::new(Backend::Sorted);
            set.insert(1);
            // Unsorted contents would make this silently return the wrong
            // answer, so it must fail loudly instead.
            set.contains(1);
        }
    }
}
