#include "cpp/shim.h"

#include <osmium/builder/osm_object_builder.hpp>
#include <osmium/version.hpp>

namespace rusmium {

rust::String osmium_version() {
    return rust::String(LIBOSMIUM_VERSION_STRING);
}

std::unique_ptr<OsmReader> open_reader(rust::Str path) {
    // osmium::io::Reader opens the file in its constructor and throws on
    // failure; cxx turns any thrown std::exception into a Rust `Err`.
    return std::make_unique<OsmReader>(std::string(path));
}

std::unique_ptr<Cursor> make_cursor(OsmReader& reader) {
    return std::make_unique<Cursor>(reader.inner());
}

bool advance(Cursor& cursor) {
    return cursor.advance();
}

std::uint8_t object_kind(const Cursor& cursor) {
    return static_cast<std::uint8_t>(cursor.current().type());
}

std::int64_t object_id(const Cursor& cursor) {
    return cursor.current().id();
}

std::uint32_t object_version(const Cursor& cursor) {
    return cursor.current().version();
}

bool node_location_valid(const Cursor& cursor) {
    const auto& node = static_cast<const osmium::Node&>(cursor.current());
    return node.location().valid();
}

double node_lon(const Cursor& cursor) {
    return static_cast<const osmium::Node&>(cursor.current()).location().lon();
}

double node_lat(const Cursor& cursor) {
    return static_cast<const osmium::Node&>(cursor.current()).location().lat();
}

rust::Vec<rust::String> tag_keys(const Cursor& cursor) {
    rust::Vec<rust::String> out;
    for (const osmium::Tag& tag : cursor.current().tags()) {
        out.push_back(rust::String(tag.key()));
    }
    return out;
}

rust::Vec<rust::String> tag_values(const Cursor& cursor) {
    rust::Vec<rust::String> out;
    for (const osmium::Tag& tag : cursor.current().tags()) {
        out.push_back(rust::String(tag.value()));
    }
    return out;
}

rust::Vec<std::int64_t> way_node_refs(const Cursor& cursor) {
    rust::Vec<std::int64_t> out;
    const auto& way = static_cast<const osmium::Way&>(cursor.current());
    for (const osmium::NodeRef& nr : way.nodes()) {
        out.push_back(nr.ref());
    }
    return out;
}

rust::Vec<std::uint8_t> relation_member_kinds(const Cursor& cursor) {
    rust::Vec<std::uint8_t> out;
    const auto& rel = static_cast<const osmium::Relation&>(cursor.current());
    for (const osmium::RelationMember& m : rel.members()) {
        out.push_back(static_cast<std::uint8_t>(m.type()));
    }
    return out;
}

rust::Vec<std::int64_t> relation_member_refs(const Cursor& cursor) {
    rust::Vec<std::int64_t> out;
    const auto& rel = static_cast<const osmium::Relation&>(cursor.current());
    for (const osmium::RelationMember& m : rel.members()) {
        out.push_back(m.ref());
    }
    return out;
}

rust::Vec<rust::String> relation_member_roles(const Cursor& cursor) {
    rust::Vec<rust::String> out;
    const auto& rel = static_cast<const osmium::Relation&>(cursor.current());
    for (const osmium::RelationMember& m : rel.members()) {
        out.push_back(rust::String(m.role()));
    }
    return out;
}

// --- writing ---

namespace {

// Attach tags to an object being built. Creating the sub-builder must happen
// after the object's fixed fields and set_user(), and after any earlier
// sub-builder (node/member list) has been finalized.
template <typename Builder>
void add_tags(osmium::memory::Buffer& buffer, Builder& parent,
              const rust::Vec<rust::String>& keys,
              const rust::Vec<rust::String>& values) {
    if (keys.empty()) {
        return;
    }
    osmium::builder::TagListBuilder tl{buffer, &parent};
    for (std::size_t i = 0; i < keys.size(); ++i) {
        tl.add_tag(std::string(keys[i]), std::string(values[i]));
    }
}

}  // namespace

void OsmWriter::add_node(std::int64_t id, std::uint32_t version,
                         bool has_location, double lon, double lat,
                         const rust::Vec<rust::String>& keys,
                         const rust::Vec<rust::String>& values) {
    {
        osmium::builder::NodeBuilder builder{buffer_};
        osmium::Node& node = builder.object();
        node.set_id(id);
        node.set_version(version);
        node.set_visible(true);
        if (has_location) {
            node.set_location(osmium::Location{lon, lat});
        }
        builder.set_user("");
        add_tags(buffer_, builder, keys, values);
    }
    buffer_.commit();
}

void OsmWriter::add_way(std::int64_t id, std::uint32_t version,
                        const rust::Vec<std::int64_t>& node_refs,
                        const rust::Vec<rust::String>& keys,
                        const rust::Vec<rust::String>& values) {
    {
        osmium::builder::WayBuilder builder{buffer_};
        osmium::Way& way = builder.object();
        way.set_id(id);
        way.set_version(version);
        way.set_visible(true);
        builder.set_user("");
        if (!node_refs.empty()) {
            osmium::builder::WayNodeListBuilder wnl{buffer_, &builder};
            for (std::int64_t ref : node_refs) {
                wnl.add_node_ref(ref);
            }
        }
        add_tags(buffer_, builder, keys, values);
    }
    buffer_.commit();
}

void OsmWriter::add_relation(std::int64_t id, std::uint32_t version,
                             const rust::Vec<std::uint8_t>& member_kinds,
                             const rust::Vec<std::int64_t>& member_refs,
                             const rust::Vec<rust::String>& member_roles,
                             const rust::Vec<rust::String>& keys,
                             const rust::Vec<rust::String>& values) {
    {
        osmium::builder::RelationBuilder builder{buffer_};
        osmium::Relation& rel = builder.object();
        rel.set_id(id);
        rel.set_version(version);
        rel.set_visible(true);
        builder.set_user("");
        if (!member_kinds.empty()) {
            osmium::builder::RelationMemberListBuilder rml{buffer_, &builder};
            for (std::size_t i = 0; i < member_kinds.size(); ++i) {
                rml.add_member(static_cast<osmium::item_type>(member_kinds[i]),
                               member_refs[i],
                               std::string(member_roles[i]).c_str());
            }
        }
        add_tags(buffer_, builder, keys, values);
    }
    buffer_.commit();
}

void OsmWriter::finish() {
    writer_(std::move(buffer_));
    writer_.close();
}

std::unique_ptr<OsmWriter> create_writer(rust::Str path) {
    return std::make_unique<OsmWriter>(std::string(path));
}

void writer_add_node(OsmWriter& writer, std::int64_t id, std::uint32_t version,
                     bool has_location, double lon, double lat,
                     const rust::Vec<rust::String>& keys,
                     const rust::Vec<rust::String>& values) {
    writer.add_node(id, version, has_location, lon, lat, keys, values);
}

void writer_add_way(OsmWriter& writer, std::int64_t id, std::uint32_t version,
                    const rust::Vec<std::int64_t>& node_refs,
                    const rust::Vec<rust::String>& keys,
                    const rust::Vec<rust::String>& values) {
    writer.add_way(id, version, node_refs, keys, values);
}

void writer_add_relation(OsmWriter& writer, std::int64_t id,
                         std::uint32_t version,
                         const rust::Vec<std::uint8_t>& member_kinds,
                         const rust::Vec<std::int64_t>& member_refs,
                         const rust::Vec<rust::String>& member_roles,
                         const rust::Vec<rust::String>& keys,
                         const rust::Vec<rust::String>& values) {
    writer.add_relation(id, version, member_kinds, member_refs, member_roles,
                        keys, values);
}

void finish_writer(OsmWriter& writer) {
    writer.finish();
}

}  // namespace rusmium
