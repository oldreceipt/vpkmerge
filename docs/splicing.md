# VPK splicing — implementation spec

Add **split** (one VPK to many) to `vpkmerge-core` as the symmetric counterpart to **merge** (many VPKs to one). Splicing routes entries from a single input into one or more outputs by path predicate, with anything unmatched optionally collected into a residual bucket.

Driving use case: Grimoire's sound-mods-in-Locker work needs per-ability granularity. A multi-ability sound mod ships one VPK that touches `sounds/abilities/abrams/a2_*` and `sounds/abilities/abrams/a4_*`; splicing produces separate per-slot VPKs the Locker can toggle independently. See `grimoire/docs/hero-sound-codenames.md` for the path conventions that motivate this.

## Scope

- `vpkmerge-core` gains a **generic** split primitive. No game-specific knowledge.
- All Deadlock awareness (codename dictionary, ability slot classification) stays in `grimoire/` (TypeScript). Grimoire builds a plan, hands it to the CLI as JSON, reads the report back.
- `vpkmerge-cli` gains a `split` subcommand. Existing CLI invocation continues to mean merge for backward compatibility.

## Public API (vpkmerge-core)

Add to `vpkmerge-core/src/lib.rs`:

```rust
/// One output bucket: where to write, and the rule that decides which paths
/// from the input belong in it.
#[derive(Clone, Debug)]
pub struct SplitOutput {
    pub path: PathBuf,
    pub predicate: PathPredicate,
}

/// Path matchers. Start with prefix-only (covers ability slots); add more
/// variants if a real use case shows up. Keep this an enum, not a closure,
/// so SplitPlan is serializable from the CLI/JSON layer.
#[derive(Clone, Debug)]
pub enum PathPredicate {
    /// Match if the entry path starts with any of the given prefixes.
    /// Case-sensitive. Empty list matches nothing.
    AnyPrefix(Vec<String>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverlapPolicy {
    /// Each path goes to the FIRST output whose predicate matches it.
    #[default]
    FirstMatch,
    /// Each path goes to EVERY output whose predicate matches it.
    /// Use when you intentionally want the same entry in multiple outputs.
    AllMatches,
    /// Refuse to split if any path matches more than one output.
    Error,
}

#[derive(Clone, Debug, Default)]
pub struct SplitOptions {
    pub overlap_policy: OverlapPolicy,
    /// Optional path for a VPK containing every input entry that no
    /// `SplitOutput` predicate claimed. None = drop unmatched entries silently.
    pub residual_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct SplitOutputReport {
    pub path: PathBuf,
    pub entries: usize,
}

#[derive(Debug, Clone)]
pub struct SplitReport {
    pub input_entries: usize,
    pub outputs: Vec<SplitOutputReport>,
    pub residual: Option<SplitOutputReport>,
    /// Number of input entries that landed in zero outputs (and were either
    /// written to residual or dropped depending on options).
    pub unmatched: usize,
}

/// Route entries from `input` into N output VPKs according to `outputs`.
/// Reads `input` once. Returns a per-output entry count.
pub fn split<I: AsRef<Path>>(
    input: I,
    outputs: &[SplitOutput],
    options: &SplitOptions,
) -> Result<SplitReport>;
```

### Engine notes

Mirror the `merge` strategy exactly so behavior stays consistent:

1. Open input with `valve_pak::open`.
2. Iterate `vpk.file_paths()`. For each path, run predicates; build a `Vec<(path, dst_output_idx_or_residual)>` routing table per `OverlapPolicy`.
3. For each output bucket: create a `tempfile::tempdir`, extract its assigned entries (via `get_file(path).read_all()`), write them with directory creation, then `valve_pak::from_directory(tmp)?.save(output_path)`.
4. Same `reject_output_equals_input` guard the merge path uses, generalized to N outputs.

Empty buckets: if a predicate matches zero entries, still emit a valid empty VPK at its `path` (so the caller's downstream pipeline doesn't have to special-case "the splicer skipped this one"). Note this in the `SplitOutputReport` with `entries: 0`.

### Predicate matching

`AnyPrefix` is intentionally minimal. The ability use case is satisfied by `vec!["sounds/abilities/abrams/a2_".into()]`. Prefixes are exact byte prefixes on the path string, no globbing. Two reasons not to start with regex/glob:

- VPK paths use forward slashes consistently (Source 2 convention). Prefix is unambiguous.
- Predicate must be serializable for the CLI JSON layer. Closure variants can come later if a real need shows up; don't pre-build them.

### Overlap policy choices

- `FirstMatch` (default): clean per-slot split when predicates are disjoint, deterministic when they overlap (caller orders outputs to indicate priority).
- `AllMatches`: lets callers ask for cross-listed entries (a single source byte landing in multiple VPK outputs). Caveat that this multiplies output VPK size if used carelessly.
- `Error`: defensive mode for callers that *think* their predicates are disjoint and want the engine to catch the bug.

## CLI surface (vpkmerge-cli)

The CLI gains a `split` subcommand. Keep the existing top-level merge invocation untouched (`vpkmerge <output> <input1> <input2> ...` still merges) so we don't break existing scripts.

```
vpkmerge split <input.vpk> --plan <plan.json> [--strict] [--all-matches] [--residual <residual.vpk>]
```

Flags:

- `--plan FILE` (required): JSON file describing outputs and predicates. Shape below.
- `--strict`: maps to `OverlapPolicy::Error`.
- `--all-matches`: maps to `OverlapPolicy::AllMatches`. Mutually exclusive with `--strict`.
- `--residual FILE`: overrides any residual path in the plan. Convenience.
- `-v, --verbose`: log each path routed to each output.

Plan JSON shape:

```json
{
  "outputs": [
    {
      "path": "abrams_a2_dir.vpk",
      "prefixes": ["sounds/abilities/abrams/a2_"]
    },
    {
      "path": "abrams_a4_dir.vpk",
      "prefixes": ["sounds/abilities/abrams/a4_"]
    }
  ],
  "residual": "abrams_other_dir.vpk"
}
```

`outputs[].prefixes` becomes `PathPredicate::AnyPrefix(prefixes)`. Top-level `residual` is optional and is overridden by `--residual` if both are present.

The CLI exits non-zero on `OverlapPolicy::Error` violations, missing input, missing plan, or write failures. Prints the `SplitReport` as a small table at the end.

## What lives in Grimoire (not vpkmerge)

The Deadlock-specific layer is one file in `grimoire/electron/main/services/`:

- Reads `heroSoundCodenames.json` (the codename dictionary).
- Calls `parseVpkDirectory(input)` (Node-side VPK reader, already in `vpk.ts`).
- Groups discovered paths into `(hero, slot)` tuples using the dictionary.
- Builds a plan JSON, writes it to a temp file, spawns `vpkmerge split` with `extraResources`-bundled binary, parses the report.

This keeps `vpkmerge-core` reusable for any Source 2 game and keeps the codename map in exactly one place.

## Rollup

"Rollup" (combine split slot VPKs back into one consolidated VPK) is just `merge`. No new core API needed. Document the symmetry in the README:

> `merge` is rollup; `split` is the inverse. Either is reversible: split a VPK by some predicates, then merge the outputs (in the same order, default policy) and you should get the same path set back.

A test (`stable_split_then_merge`) below pins this property.

## Test plan

Add to `vpkmerge-core/src/lib.rs` test module, mirroring existing merge tests:

| Test | What it pins |
|---|---|
| `split_routes_by_prefix` | Two disjoint prefixes, expect two outputs with correct entries. |
| `split_residual_collects_unmatched` | One prefix, plus `residual_path`. Unmatched entries land in residual. |
| `split_no_residual_drops_unmatched` | `residual_path = None` silently drops unmatched. `SplitReport.unmatched` still reports the count. |
| `split_empty_output_still_writes_vpk` | Predicate matching zero entries produces a valid empty VPK at the configured path. |
| `split_first_match_wins` | Two overlapping prefixes, `FirstMatch`, entry goes to the earlier output only. |
| `split_all_matches_duplicates` | Two overlapping prefixes, `AllMatches`, entry appears in both outputs. |
| `split_error_policy_rejects_overlap` | Two overlapping prefixes, `Error`, function returns Err with the offending path in the message. |
| `split_rejects_output_equals_input` | Any output path resolving to the same canonical file as the input errors out. |
| `split_creates_missing_parent_dirs` | Output paths with non-existent parent directories are created (matches merge behavior). |
| `stable_split_then_merge` | Split a VPK by N disjoint prefixes, merge outputs back with `LastWins`, assert path set equals original. |

Use the existing `make_vpk` and `read_entry` helpers; add a `make_abilities_fixture` helper that creates a VPK with paths like `sounds/abilities/abrams/a2_charge/x.vsnd_c`, `sounds/abilities/abrams/a4_leap/y.vsnd_c`, `other/unrelated.txt`.

## Phased delivery

1. **vpkmerge-core**: add `split`, `SplitOutput`, `PathPredicate`, `OverlapPolicy`, `SplitOptions`, `SplitReport`, `SplitOutputReport`. Tests above. No CLI yet.
2. **vpkmerge-cli**: `split` subcommand reading the JSON plan. Hand-test against a real Deadlock sound mod VPK.
3. **Bundle the CLI** into Grimoire's Electron package via `electron-builder.yml` `extraResources`. Add a thin spawn wrapper in `grimoire/electron/main/services/`.
4. **Grimoire ability classifier**: new service file that builds the plan from `heroSoundCodenames.json` + `parseVpkDirectory` output. No UI yet; surface results via a dev-only IPC handler for validation.
5. **Locker UI** (separate spec, not this doc): expose a "Split into per-ability mods" action per multi-ability sound mod.

## Out of scope (for now)

- Chunked VPK outputs (`*_dir.vpk` + `*_NNN.vpk`). Sound mods are small; single-chunk outputs are fine. Add later if needed.
- Closure-based predicates in core. Add when a real use case requires logic the JSON plan can't express.
- Byte-reproducible output. The merge engine already has this limitation noted (`stable_entry_set` test asserts set equality, not byte equality, because `valve_pak::from_directory` walks the filesystem in OS-dependent order). Splice inherits the same property; do not promise byte stability.
- A `vpkmerge-deadlock` crate. Stays a future option if Rust-side ability awareness ever becomes worth the maintenance.
