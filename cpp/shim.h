#pragma once

// `rust/cxx.h` is provided on the include path by cxx-build.
#include "rust/cxx.h"

#include <cstdint>
#include <memory>
#include <string>

#include <osmium/io/any_input.hpp>
#include <osmium/io/any_output.hpp>
#include <osmium/io/input_iterator.hpp>
#include <osmium/io/reader.hpp>
#include <osmium/io/writer.hpp>
#include <osmium/memory/buffer.hpp>
#include <osmium/osm/item_type.hpp>
#include <osmium/osm/node.hpp>
#include <osmium/osm/object.hpp>
#include <osmium/osm/relation.hpp>
#include <osmium/osm/way.hpp>

namespace rusmium {

// Owns an osmium reader. Heap-allocated behind a UniquePtr on the Rust side so
// its address is stable — a Cursor keeps a reference into it.
class OsmReader {
public:
    explicit OsmReader(const std::string& path) : reader_(path) {}
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

std::unique_ptr<OsmReader> open_reader(rust::Str path);
std::unique_ptr<Cursor> make_cursor(OsmReader& reader);
bool advance(Cursor& cursor);

std::uint8_t object_kind(const Cursor& cursor);
std::int64_t object_id(const Cursor& cursor);
std::uint32_t object_version(const Cursor& cursor);

bool node_location_valid(const Cursor& cursor);
double node_lon(const Cursor& cursor);
double node_lat(const Cursor& cursor);

rust::Vec<rust::String> tag_keys(const Cursor& cursor);
rust::Vec<rust::String> tag_values(const Cursor& cursor);

rust::Vec<std::int64_t> way_node_refs(const Cursor& cursor);

rust::Vec<std::uint8_t> relation_member_kinds(const Cursor& cursor);
rust::Vec<std::int64_t> relation_member_refs(const Cursor& cursor);
rust::Vec<rust::String> relation_member_roles(const Cursor& cursor);

// Owns a writer and an accumulation buffer. Objects are built into the buffer
// via osmium's builders, then flushed to the writer on finish(). Heap-allocated
// behind a UniquePtr and never moved (osmium::io::Writer is non-movable).
class OsmWriter {
public:
    explicit OsmWriter(const std::string& path)
        : writer_(path, osmium::io::overwrite::allow),
          buffer_(1024UL * 1024UL, osmium::memory::Buffer::auto_grow::yes) {}

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
    void finish();

private:
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
void finish_writer(OsmWriter& writer);

}  // namespace rusmium
