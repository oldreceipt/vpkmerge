#!/usr/bin/env bash
# Populate morphic/fixtures/_local/ (gitignored) from a local Deadlock install
# and regenerate goldens for each. Used for stress-testing the decoder against
# hundreds of real textures before declaring a milestone done.
#
# Curated entries live in tools/curated-fixture-entries.txt, one per line.
# Lines beginning with # are comments.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/.." && pwd)"
deadlock="${DEADLOCK_DIR:-$HOME/.steam/steam/steamapps/common/Deadlock/game/citadel}"
vpk="$deadlock/pak01_dir.vpk"
entries="$here/curated-fixture-entries.txt"
local_dir="$repo/morphic/fixtures/_local"

if [[ ! -f "$vpk" ]]; then
    echo "bootstrap-fixtures: vpk not found at $vpk" >&2
    echo "set DEADLOCK_DIR to your install root if it lives elsewhere" >&2
    exit 1
fi

if [[ ! -f "$entries" ]]; then
    echo "bootstrap-fixtures: no curated list at $entries (creating empty)"
    mkdir -p "$here"
    cat >"$entries" <<'EOF'
# One Source 2 entry path per line. Comments start with '#'.
# Examples:
# materials/minimap/minimap_circle.vtex_c
# materials/default/default_color_tga_99901565.vtex_c
EOF
fi

mkdir -p "$local_dir"

count=0
while IFS= read -r line; do
    line="${line%%#*}"
    line="${line## }"
    line="${line%% }"
    [[ -z "$line" ]] && continue

    fmt_subdir="misc"
    case "$line" in
        *bc7*|*BC7*)        fmt_subdir="bc7" ;;
        *bc6h*|*BC6H*)      fmt_subdir="bc6h" ;;
        *dxt1*|*DXT1*)      fmt_subdir="dxt1" ;;
        *dxt5*|*DXT5*)      fmt_subdir="dxt5" ;;
        *normal*|*ati*)     fmt_subdir="normal" ;;
        *minimap*)          fmt_subdir="rgba8" ;;
    esac

    dest="$local_dir/$fmt_subdir"
    mkdir -p "$dest"
    (cd "$repo/tools/morphic-oracle" && dotnet run -- extract \
        --vpk "$vpk" \
        --entry "$line" \
        --out "$dest")
    count=$((count + 1))
done <"$entries"

echo "extracted $count entries to $local_dir"

(cd "$repo/tools/morphic-oracle" && dotnet run -- generate --fixtures "$local_dir")
echo "done. run: MORPHIC_FIXTURES=morphic/fixtures/_local cargo test -p morphic --test golden"
