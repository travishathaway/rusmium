#pragma once

// `rust/cxx.h` is provided on the include path by cxx-build.
#include "rust/cxx.h"

#include <cstdint>
#include <memory>
#include <string>

#include <osmium/io/any_input.hpp>
#include <osmium/io/any_output.hpp>
#include <osmium/io/file_format.hpp>
#include <osmium/io/input_iterator.hpp>
#include <osmium/io/reader.hpp>
#include <osmium/io/writer.hpp>
#include <osmium/memory/buffer.hpp>
#include <osmium/osm/entity_bits.hpp>
#include <osmium/osm/item_type.hpp>
#include <osmium/osm/node.hpp>
#include <osmium/osm/object.hpp>
#include <osmium/osm/relation.hpp>
#include <osmium/osm/way.hpp>

namespace rusmium {

// Owns an osmium reader. Heap-allocated behind a UniquePtr on the Rust side so
// its address is stable — a Cursor keeps a reference into it.
//
// `entity_bits` and `read_metadata` are handed straight to osmium and let the
// PBF parser skip work the caller will never look at: whole entity classes, and
// the per-object user/uid/timestamp/changeset metadata. Both are significant on
// a multi-pass job where early passes only need ids and locations.
class OsmReader {
public:
    OsmReader(const std::string& path, std::uint8_t entity_bits,
              bool read_metadata)
        : reader_(path,
                  static_cast<osmium::osm_entity_bits::type>(entity_bits),
                  read_metadata ? osmium::io::read_meta::yes
                                : osmium::io::read_meta::no) {}
    osmium::io::Reader& inner() noexcept { return reader_; }

private:
    osmium::io::Reader reader_;
};

// A pull-position over an OsmReader. osmium's InputIterator hides all buffer
// management and yields node/way/relation objects one at a time.
class Cursor {
public:
    explicit Cursor(osmium::io::Reader& reader) : it_(reader) {}

    // Move to the next object. On the first call, expose the object the
    // iterator was constructed pointing at; afterwards, pre-increment.
    bool advance() {
        if (!started_) {
            started_ = true;
        } else if (it_ != end_) {
            ++it_;
        }
        return it_ != end_;
    }

    const osmium::OSMObject& current() const { return *it_; }

private:
    osmium::io::InputIterator<osmium::io::Reader, osmium::OSMObject> it_;
    osmium::io::InputIterator<osmium::io::Reader, osmium::OSMObject> end_{};
    bool started_ = false;
};

rust::String osmium_version();

std::unique_ptr<OsmReader> open_reader(rust::Str path, std::uint8_t entity_bits,
                                       bool read_metadata);
std::unique_ptr<Cursor> make_cursor(OsmReader& reader);
bool advance(Cursor& cursor);

std::uint8_t object_kind(const Cursor& cursor);
std::int64_t object_id(const Cursor& cursor);
std::uint32_t object_version(const Cursor& cursor);

// Object metadata. Only populated when the reader was opened with metadata
// enabled; otherwise these read as 0 / empty. Timestamp is seconds since the
// epoch, 0 when unset.
std::int64_t object_timestamp(const Cursor& cursor);
std::uint32_t object_uid(const Cursor& cursor);
rust::String object_user(const Cursor& cursor);
std::int64_t object_changeset(const Cursor& cursor);

bool node_location_valid(const Cursor& cursor);
double node_lon(const Cursor& cursor);
double node_lat(const Cursor& cursor);

// Number of tags on the current object. Lets the Rust side skip the two
// vector-returning calls below entirely for untagged objects — which is most
// nodes in a typical extract.
std::size_t tag_count(const Cursor& cursor);
rust::Vec<rust::String> tag_keys(const Cursor& cursor);
rust::Vec<rust::String> tag_values(const Cursor& cursor);

rust::Vec<std::int64_t> way_node_refs(const Cursor& cursor);

// Fill a caller-owned vector with the current way's node refs, replacing its
// contents. Lets a caller reuse one buffer for a whole pass instead of
// allocating a fresh vector per way.
void way_node_refs_into(const Cursor& cursor, rust::Vec<std::int64_t>& out);

rust::Vec<std::uint8_t> relation_member_kinds(const Cursor& cursor);
rust::Vec<std::int64_t> relation_member_refs(const Cursor& cursor);
rust::Vec<rust::String> relation_member_roles(const Cursor& cursor);

// As way_node_refs_into, for relation member kinds and refs. Roles are left
// out deliberately: they are the only part that needs a string allocation, and
// filters rarely look at them. Use relation_member_roles when they are needed.
void relation_members_into(const Cursor& cursor, rust::Vec<std::uint8_t>& kinds,
                           rust::Vec<std::int64_t>& refs);

// Owns a writer and an accumulation buffer. Objects are built into the buffer
// via osmium's builders and handed to the writer once the buffer fills, rather
// than accumulating the whole output and flushing once at finish(). That keeps
// memory bounded and — because osmium::io::Writer compresses each buffer on a
// pool thread — overlaps encoding with the caller's reading.
//
// Heap-allocated behind a UniquePtr and never moved (osmium::io::Writer is
// non-movable).
class OsmWriter {
public:
    explicit OsmWriter(const std::string& path)
        : writer_(path, osmium::io::overwrite::allow),
          buffer_(buffer_size, osmium::memory::Buffer::auto_grow::yes) {}

    void add_node(std::int64_t id, std::uint32_t version, bool has_location,
                  double lon, double lat,
                  const rust::Vec<rust::String>& keys,
                  const rust::Vec<rust::String>& values);
    void add_way(std::int64_t id, std::uint32_t version,
                 const rust::Vec<std::int64_t>& node_refs,
                 const rust::Vec<rust::String>& keys,
                 const rust::Vec<rust::String>& values);
    void add_relation(std::int64_t id, std::uint32_t version,
                      const rust::Vec<std::uint8_t>& member_kinds,
                      const rust::Vec<std::int64_t>& member_refs,
                      const rust::Vec<rust::String>& member_roles,
                      const rust::Vec<rust::String>& keys,
                      const rust::Vec<rust::String>& values);

    // Copy an already-decoded object verbatim into the output buffer. This is
    // a memcpy of item.padded_size() bytes: no rebuild, and every field —
    // including the metadata the add_* paths cannot express — survives.
    void copy_item(const osmium::memory::Item& item);

    void finish();

private:
    // Matches osmium-tool's Extract::buffer_size. Large enough that the
    // per-flush handoff is negligible, small enough to bound memory.
    static constexpr std::size_t buffer_size = 10UL * 1024UL * 1024UL;

    // Hand the buffer to the writer once it is full and start a fresh one.
    // Called after each committed object.
    void flush_if_full();

    osmium::io::Writer writer_;
    osmium::memory::Buffer buffer_;
};

std::unique_ptr<OsmWriter> create_writer(rust::Str path);
void writer_add_node(OsmWriter& writer, std::int64_t id, std::uint32_t version,
                     bool has_location, double lon, double lat,
                     const rust::Vec<rust::String>& keys,
                     const rust::Vec<rust::String>& values);
void writer_add_way(OsmWriter& writer, std::int64_t id, std::uint32_t version,
                    const rust::Vec<std::int64_t>& node_refs,
                    const rust::Vec<rust::String>& keys,
                    const rust::Vec<rust::String>& values);
void writer_add_relation(OsmWriter& writer, std::int64_t id,
                         std::uint32_t version,
                         const rust::Vec<std::uint8_t>& member_kinds,
                         const rust::Vec<std::int64_t>& member_refs,
                         const rust::Vec<rust::String>& member_roles,
                         const rust::Vec<rust::String>& keys,
                         const rust::Vec<rust::String>& values);
// Copy the cursor's current object straight into the writer's buffer.
void writer_copy(OsmWriter& writer, const Cursor& cursor);
void finish_writer(OsmWriter& writer);

}  // namespace rusmium
