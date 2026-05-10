@0xddc8dbb7e478d532;
# Cap'n Proto schema for Nautilus base types
# These types are used across all schemas to ensure consistency
#
# WARNING: This schema is not yet stable and may change without notice
# between releases. Do not depend on wire compatibility across versions.

# UUID version 4 (RFC 4122)
struct UUID4 {
    value @0 :Data;  # 16 bytes
}

# Unix timestamp in nanoseconds since epoch
struct UnixNanos {
    value @0 :UInt64;
}

# String-to-string map for metadata and tags
struct StringMap {
    entries @0 :List(Entry);

    struct Entry {
        key @0 :Text;
        value @1 :Text;
    }
}
