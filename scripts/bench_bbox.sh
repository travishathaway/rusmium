#!/usr/bin/env bash
#
# Time the bbox_filter example and report wall clock and peak memory.
#
#   pixi run bench-bbox <input.osm.pbf> [bbox]
#
# The extract is dominated by decoding the input, so run it against a
# real-world file — a small fixture measures startup, not throughput. Compare
# against osmium-tool on the same input and box with:
#
#   osmium extract -b <bbox> -s complete_ways -o /tmp/ref.osm.pbf <input>
#
# Note that osmium-tool's complete_ways additionally pulls in parent relations
# recursively, so its object counts are a superset of this example's.

set -euo pipefail

input=${1:-}
bbox=${2:-13.0,52.0,13.5,52.5}
# Which id-set backends to time. Default runs all three, since the interesting
# result is the memory/speed trade between them.
modes=${3:-auto sorted dense}

if [[ -z $input ]]; then
    echo "usage: $0 <input.osm.pbf> [min_lon,min_lat,max_lon,max_lat] [\"auto sorted dense\"]" >&2
    exit 1
fi

output=$(mktemp -t bbox_bench.XXXXXX).osm.pbf
trap 'rm -f "$output"' EXIT

cargo build --release --example bbox_filter
bin=target/release/examples/bbox_filter

echo "input:  $input ($(du -hL "$input" | cut -f1))"
echo "bbox:   $bbox"

for mode in $modes; do
    echo
    echo "--- --idset=$mode"
    # GNU time reports peak RSS in KB with -v, BSD/macOS time in bytes with -l.
    # Fall back to bash's builtin when neither is present.
    if /usr/bin/time -v true >/dev/null 2>&1; then
        /usr/bin/time -f 'wall %e s   peak RSS %M KB' \
            "$bin" "--idset=$mode" "$input" "$output" "$bbox"
    elif /usr/bin/time -l true >/dev/null 2>&1; then
        /usr/bin/time -l "$bin" "--idset=$mode" "$input" "$output" "$bbox" 2>&1 |
            awk '/real/ {printf "wall %s s\n", $1} /maximum resident/ {printf "peak RSS %.2f GB\n", $1/1073741824}'
    else
        time "$bin" "--idset=$mode" "$input" "$output" "$bbox"
    fi
done

echo "output: $(du -h "$output" | cut -f1)"
