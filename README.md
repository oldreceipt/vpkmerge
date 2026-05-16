# vpkmerge

A small CLI that combines multiple Valve Pak (`.vpk`) files into a single VPK.

Built for **Deadlock** modding: the game caps mounted mod VPKs at roughly 100, so pre-merging several mods into one VPK lets players run more mods than the engine would otherwise allow.

## Requirements

- [.NET 8 SDK](https://dotnet.microsoft.com/download) or newer
- Uses the [ValvePak](https://github.com/SteamDatabase/ValvePak) library under the hood

## Build

```bash
git clone https://github.com/Slush97/vpkmerge
cd vpkmerge
dotnet build -c Release
```

The compiled output lands in `bin/Release/net8.0/vpkmerge.dll`.

## Usage

```bash
dotnet run -- <output_dir.vpk> <input1_dir.vpk> <input2_dir.vpk> [more.vpk...] [options]
```

Or run the built dll directly:

```bash
dotnet bin/Release/net8.0/vpkmerge.dll out_dir.vpk mod1_dir.vpk mod2_dir.vpk
```

### Options

| Flag | Description |
|------|-------------|
| `--strict` | Error out on any path collision instead of silently overriding |
| `--verbose`, `-v` | Print each file that gets overridden |
| `--help`, `-h` | Show usage |

### Collision policy

By default, **later inputs win**: if two VPKs contain the same path, the version from the VPK passed later on the command line is kept. Use `--strict` if you want the tool to refuse to merge on any collision.

### Chunked inputs

For VPKs split across `*_dir.vpk` + `*_000.vpk`, `*_001.vpk`, ... pass only the `_dir.vpk` file. The chunk files are read automatically when they sit alongside it.

## Example

Merge two Deadlock mod VPKs into one:

```bash
dotnet run -- combined_dir.vpk \
  ~/.steam/steam/steamapps/common/Deadlock/game/citadel/addons/pak24_dir.vpk \
  ~/.steam/steam/steamapps/common/Deadlock/game/citadel/addons/pak75_dir.vpk \
  --verbose
```

Drop the resulting `combined_dir.vpk` into `citadel/addons/` to mount it as a single mod slot.
