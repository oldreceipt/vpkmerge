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
