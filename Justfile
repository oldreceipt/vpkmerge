# Workspace task runner for vpkmerge + morphic.
# Install just: https://just.systems/

# `just` with no args runs the default recipe.
default: goldens test

# Regenerate sibling .png + .meta.json for every committed fixture.
goldens:
    cd tools/morphic-oracle && dotnet run -- generate --fixtures ../../morphic/fixtures

# Regenerate even if hashes match.
goldens-force:
    cd tools/morphic-oracle && dotnet run -- generate --fixtures ../../morphic/fixtures --force

# Run the morphic Rust tests (no dotnet needed).
test:
    cargo test -p morphic

# Run every workspace check that CI runs.
ci:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

# Extract one entry from the local Deadlock install into a fixture subdir,
# then regenerate its golden. Usage: just fixture <entry> <subdir>
fixture entry subdir:
    cd tools/morphic-oracle && dotnet run -- extract \
      --vpk "${DEADLOCK_DIR:-$HOME/.steam/steam/steamapps/common/Deadlock/game/citadel}/pak01_dir.vpk" \
      --entry "{{entry}}" \
      --out "../../morphic/fixtures/{{subdir}}"
    just goldens

# Re-bless the committed KV3 goldens (.kv3.json + raw .kv3bin) from the local
# install. Run after a game update changes the KV3 schema; diff before committing.
kv3-goldens:
    cd tools/morphic-oracle && dotnet run -- kv3-dump \
      --vpk "${DEADLOCK_DIR:-$HOME/.steam/steam/steamapps/common/Deadlock/game/citadel}/pak01_dir.vpk" \
      --entry models/heroes_staging/hornet_v3/hornet.vmdl_c --block DATA \
      --out ../../morphic/fixtures/kv3/hornet_data.kv3.json \
      --raw ../../morphic/fixtures/kv3/hornet_data.kv3bin
    cd tools/morphic-oracle && dotnet run -- kv3-dump \
      --vpk "${DEADLOCK_DIR:-$HOME/.steam/steam/steamapps/common/Deadlock/game/citadel}/pak01_dir.vpk" \
      --entry models/heroes_staging/hornet_v3/hornet.vmdl_c --block MDAT --nth 0 \
      --out ../../morphic/fixtures/kv3/hornet_mdat0.kv3.json \
      --raw ../../morphic/fixtures/kv3/hornet_mdat0.kv3bin
    cd tools/morphic-oracle && dotnet run -- kv3-dump \
      --vpk "${DEADLOCK_DIR:-$HOME/.steam/steam/steamapps/common/Deadlock/game/citadel}/pak01_dir.vpk" \
      --entry models/heroes_staging/hornet_v3/hornet.vmdl_c --block CTRL \
      --out ../../morphic/fixtures/kv3/hornet_ctrl.kv3.json \
      --raw ../../morphic/fixtures/kv3/hornet_ctrl.kv3bin

# Re-bless the compact model-meta golden (sorted bone names, per-LOD0-mesh
# layouts + draw calls + materials, vertex/index totals, source-space bbox) the
# M3 model decoder diffs against. Small JSON; committed. Re-run after a game
# update changes the model schema; diff before committing.
model-meta:
    cd tools/morphic-oracle && dotnet run -- model-meta \
      --vpk "${DEADLOCK_DIR:-$HOME/.steam/steam/steamapps/common/Deadlock/game/citadel}/pak01_dir.vpk" \
      --entry models/heroes_staging/hornet_v3/hornet.vmdl_c \
      --out ../../morphic/fixtures/kv3/hornet_model_meta.json

# Re-dump the meshopt buffer goldens (raw MVTX/MIDX + decoded SHA/metadata) for
# every embedded mesh in hornet. Copy the small ones into morphic/fixtures/meshopt/.
mesh-buffers:
    cd tools/morphic-oracle && dotnet run -- mesh-buffers \
      --vpk "${DEADLOCK_DIR:-$HOME/.steam/steam/steamapps/common/Deadlock/game/citadel}/pak01_dir.vpk" \
      --entry models/heroes_staging/hornet_v3/hornet.vmdl_c \
      --out-dir /tmp/morphic-meshbuf

# Export a golden .glb for one model entry, for the M3+ semantic diff. The GLB
# is large and not committed; regenerate on demand. Usage: just model-golden <entry> <out.glb>
model-golden entry out:
    cd tools/morphic-oracle && dotnet run -- model \
      --vpk "${DEADLOCK_DIR:-$HOME/.steam/steam/steamapps/common/Deadlock/game/citadel}/pak01_dir.vpk" \
      --entry "{{entry}}" \
      --base "${DEADLOCK_DIR:-$HOME/.steam/steam/steamapps/common/Deadlock/game/citadel}/pak01_dir.vpk" \
      --out "{{out}}"

# Resurvey every .vtex_c format in Deadlock pak01. Writes tools/format-counts.csv.
survey:
    cd tools/morphic-oracle && dotnet run -- survey \
      --vpk "${DEADLOCK_DIR:-$HOME/.steam/steam/steamapps/common/Deadlock/game/citadel}/pak01_dir.vpk" \
      --out ../../tools/format-counts.csv

# Populate the gitignored _local/ corpus from the local install (stress runs).
bootstrap:
    ./tools/bootstrap-fixtures.sh

# Test against the gitignored local corpus instead of the canonical one.
stress: bootstrap
    MORPHIC_FIXTURES=morphic/fixtures/_local cargo test -p morphic --test golden -- --nocapture
