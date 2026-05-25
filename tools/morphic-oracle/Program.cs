// morphic-oracle: golden-output generator for the morphic Rust decoder.
//
// Dev-time tool. Wraps ValveResourceFormat to produce .png and .meta.json
// siblings for each .vtex_c fixture, which the Rust tests then diff against.
//
// Subcommands:
//   generate --fixtures DIR [--force]
//   extract  --vpk PATH --entry NAME --out DIR
//   survey   --vpk PATH --out CSV
//   model    --vpk PATH --entry NAME [--base PATH] --out GLB   (golden glTF)
//   kv3-dump --vpk PATH --entry NAME --block FOURCC --out JSON (M1 KV3 golden)

using System.Globalization;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using System.Threading;
using SteamDatabase.ValvePak;
using ValveKeyValue;
using ValveResourceFormat;
using ValveResourceFormat.Blocks;
using ValveResourceFormat.IO;
using ValveResourceFormat.ResourceTypes;
using ValveResourceFormat.Serialization.KeyValues;

namespace MorphicOracle;

internal static class Program
{
    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        WriteIndented = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    };

    public static int Main(string[] args)
    {
        if (args.Length == 0)
        {
            PrintUsage();
            return 2;
        }

        try
        {
            return args[0] switch
            {
                "generate" => Generate(args[1..]),
                "extract"  => Extract(args[1..]),
                "survey"   => Survey(args[1..]),
                "model"    => ModelExport(args[1..]),
                "kv3-dump" => Kv3Dump(args[1..]),
                "mesh-buffers" => MeshBuffers(args[1..]),
                "--help" or "-h" => PrintUsage(),
                _ => Fail($"unknown subcommand: {args[0]}"),
            };
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"error: {ex.GetType().Name}: {ex.Message}");
            return 1;
        }
    }

    // ---------- generate ----------

    private static int Generate(string[] args)
    {
        string? fixturesDir = null;
        var force = false;
        for (var i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--fixtures":
                    fixturesDir = args[++i];
                    break;
                case "--force":
                    force = true;
                    break;
                default:
                    return Fail($"generate: unknown flag {args[i]}");
            }
        }
        if (fixturesDir is null)
        {
            return Fail("generate: --fixtures DIR required");
        }
        if (!Directory.Exists(fixturesDir))
        {
            return Fail($"generate: fixtures dir does not exist: {fixturesDir}");
        }

        var vtexFiles = Directory.EnumerateFiles(fixturesDir, "*.vtex_c", SearchOption.AllDirectories)
            .OrderBy(p => p, StringComparer.Ordinal)
            .ToList();

        var made = 0;
        var skipped = 0;
        foreach (var vtexPath in vtexFiles)
        {
            var pngPath = Path.ChangeExtension(vtexPath, ".png");
            var metaPath = Path.ChangeExtension(vtexPath, ".meta.json");
            var srcHash = Sha256Hex(File.ReadAllBytes(vtexPath));

            if (!force && File.Exists(pngPath) && File.Exists(metaPath))
            {
                try
                {
                    using var doc = JsonDocument.Parse(File.ReadAllBytes(metaPath));
                    if (doc.RootElement.TryGetProperty("source_sha256", out var sha)
                        && sha.GetString() == srcHash)
                    {
                        skipped++;
                        continue;
                    }
                }
                catch (JsonException) { /* fall through and regenerate */ }
            }

            GenerateOne(vtexPath, pngPath, metaPath, srcHash);
            made++;
            Console.WriteLine($"generated {Path.GetRelativePath(fixturesDir, vtexPath)}");
        }

        Console.WriteLine($"done: {made} generated, {skipped} up-to-date");
        return 0;
    }

    private static void GenerateOne(string vtexPath, string pngPath, string metaPath, string srcHash)
    {
        using var resource = new Resource();
        resource.Read(vtexPath);
        if (resource.DataBlock is not Texture texture)
        {
            throw new InvalidOperationException($"{vtexPath}: not a texture resource");
        }

        using var bitmap = texture.GenerateBitmap();
        var png = TextureExtract.ToPngImage(bitmap);
        File.WriteAllBytes(pngPath, png);

        // For HDR formats, the .png is tone-mapped and useless for bit-level
        // comparison. Dump the raw RgbaF32 bitmap bytes as a sibling .f32 so
        // the Rust harness can diff in float space. Bitmap dims are already
        // ActualWidth x ActualHeight (VRF crops NonPow2 textures), so the
        // file is exactly Width * Height * 16 bytes, row-major LE.
        var f32Path = Path.ChangeExtension(vtexPath, ".f32");
        if (texture.IsHighDynamicRange && bitmap.ColorType == SkiaSharp.SKColorType.RgbaF32)
        {
            File.WriteAllBytes(f32Path, bitmap.GetPixelSpan().ToArray());
        }
        else if (File.Exists(f32Path))
        {
            // A previous run dumped a .f32 (e.g. format changed during dev).
            // Remove it so the Rust harness doesn't load stale data.
            File.Delete(f32Path);
        }

        var meta = new Meta
        {
            Format = texture.Format.ToString(),
            Width = texture.Width,
            Height = texture.Height,
            ActualWidth = texture.ActualWidth,
            ActualHeight = texture.ActualHeight,
            Depth = texture.Depth,
            MipCount = texture.NumMipLevels,
            Flags = texture.Flags.ToString().Split(", ", StringSplitOptions.RemoveEmptyEntries),
            SourceSha256 = srcHash,
            VrfVersion = typeof(Resource).Assembly.GetName().Version?.ToString() ?? "unknown",
            Tolerance = ToleranceFor(texture.Format),
        };
        File.WriteAllText(metaPath, JsonSerializer.Serialize(meta, JsonOpts) + "\n");
    }

    // ---------- extract ----------

    private static int Extract(string[] args)
    {
        string? vpk = null, entry = null, outDir = null;
        for (var i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--vpk":   vpk    = args[++i]; break;
                case "--entry": entry  = args[++i]; break;
                case "--out":   outDir = args[++i]; break;
                default: return Fail($"extract: unknown flag {args[i]}");
            }
        }
        if (vpk is null || entry is null || outDir is null)
        {
            return Fail("extract: --vpk, --entry, and --out are required");
        }

        Directory.CreateDirectory(outDir);
        using var pak = new Package();
        pak.Read(vpk);
        var packageEntry = pak.FindEntry(entry)
            ?? throw new FileNotFoundException($"entry not found in VPK: {entry}");
        pak.ReadEntry(packageEntry, out var data);

        var basename = SanitizeBasename(entry);
        var outPath = Path.Combine(outDir, basename);
        File.WriteAllBytes(outPath, data);
        Console.WriteLine($"extracted {entry} -> {outPath} ({data.Length} bytes)");
        return 0;
    }

    // ---------- survey ----------

    private static int Survey(string[] args)
    {
        string? vpk = null, outCsv = null;
        for (var i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--vpk": vpk    = args[++i]; break;
                case "--out": outCsv = args[++i]; break;
                default: return Fail($"survey: unknown flag {args[i]}");
            }
        }
        if (vpk is null || outCsv is null)
        {
            return Fail("survey: --vpk and --out are required");
        }

        using var pak = new Package();
        pak.Read(vpk);
        var entries = pak.Entries;
        List<PackageEntry> vtex = (entries is not null
            && entries.TryGetValue("vtex_c", out var list)
            && list is not null)
            ? list
            : new List<PackageEntry>();

        Console.WriteLine($"surveying {vtex.Count} vtex_c entries in {vpk}");
        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(outCsv))!);
        using var w = new StreamWriter(outCsv, false, Encoding.UTF8);
        w.WriteLine("entry,format,width,height,mips,flags,kv3_version_hex,bytes");

        var formatCounts = new SortedDictionary<string, int>(StringComparer.Ordinal);
        foreach (var e in vtex)
        {
            try
            {
                pak.ReadEntry(e, out var bytes);
                using var resource = new Resource();
                using var ms = new MemoryStream(bytes);
                resource.Read(ms);
                if (resource.DataBlock is not Texture t)
                {
                    continue;
                }
                var kv3Hex = TryReadKv3Magic(bytes, resource);
                w.WriteLine(string.Join(",",
                    Csv(EntryPath(e)),
                    t.Format,
                    t.Width.ToString(CultureInfo.InvariantCulture),
                    t.Height.ToString(CultureInfo.InvariantCulture),
                    t.NumMipLevels.ToString(CultureInfo.InvariantCulture),
                    Csv(t.Flags.ToString()),
                    kv3Hex,
                    bytes.Length.ToString(CultureInfo.InvariantCulture)));
                var key = t.Format.ToString();
                formatCounts[key] = formatCounts.TryGetValue(key, out var c) ? c + 1 : 1;
            }
            catch (Exception ex)
            {
                w.WriteLine(string.Join(",",
                    Csv(EntryPath(e)), "ERROR", "0", "0", "0", "", "",
                    Csv(ex.GetType().Name)));
            }
        }

        Console.WriteLine("format counts:");
        foreach (var (k, v) in formatCounts)
        {
            Console.WriteLine($"  {k,-22} {v}");
        }
        Console.WriteLine($"wrote {outCsv}");
        return 0;
    }

    // ---------- model (golden .glb) ----------

    // Produces the golden glTF the Rust exporter is diffed against. Mirrors the
    // arg shape of `vpkmerge model export`: --vpk is where the .vmdl_c lives,
    // --base is the package external refs (materials/textures/skeleton) resolve
    // against. For a base hero model the two are the same pak01_dir.vpk.
    private static int ModelExport(string[] args)
    {
        string? vpk = null, entry = null, basePak = null, outPath = null;
        var noMaterials = false;
        var noAnimations = false;
        for (var i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--vpk":           vpk          = args[++i]; break;
                case "--entry":         entry        = args[++i]; break;
                case "--base":          basePak      = args[++i]; break;
                case "--out":           outPath      = args[++i]; break;
                case "--no-materials":  noMaterials  = true; break;
                case "--no-animations": noAnimations = true; break;
                default: return Fail($"model: unknown flag {args[i]}");
            }
        }
        if (vpk is null || entry is null || outPath is null)
        {
            return Fail("model: --vpk, --entry, and --out are required");
        }
        basePak ??= vpk;

        using var pak = new Package();
        pak.Read(basePak);
        var loader = new GameFileLoader(pak, basePak);

        // If the model lives in a separate (skin) VPK, search it first so its
        // overrides win over the base pak.
        Package? skinPak = null;
        if (!string.Equals(Path.GetFullPath(vpk), Path.GetFullPath(basePak), StringComparison.Ordinal))
        {
            skinPak = new Package();
            skinPak.Read(vpk);
            loader.AddPackageToSearch(skinPak);
        }

        var sourcePak = skinPak ?? pak;
        var packageEntry = sourcePak.FindEntry(entry)
            ?? throw new FileNotFoundException($"entry not found in VPK: {entry}");
        sourcePak.ReadEntry(packageEntry, out var data);

        using var resource = new Resource();
        using var ms = new MemoryStream(data);
        resource.Read(ms);
        resource.FileName = entry;

        var exporter = new GltfModelExporter(loader)
        {
            ProgressReporter = new Progress<string>(s => Console.Error.WriteLine($"  {s}")),
            ExportMaterials = !noMaterials,
            // Keep animations on so the golden carries the full skeleton + skin
            // (joints, inverse-bind matrices, bone names). morphic only needs to
            // match the skin; it emits no animation samplers, but M3 validates
            // joint count + bone-name set against this golden's skin, so the
            // skeleton must be present. With animations off VRF drops it.
            ExportAnimations = !noAnimations,
        };

        var outFull = Path.GetFullPath(outPath);
        Directory.CreateDirectory(Path.GetDirectoryName(outFull)!);
        exporter.Export(resource, outFull, CancellationToken.None);
        Console.WriteLine($"exported {entry} -> {outFull}");
        skinPak?.Dispose();
        return 0;
    }

    // ---------- kv3-dump (M1 validation) ----------

    // Dumps a single KV3 block (DATA, MDAT, CTRL, AGRP, ...) of a resource as
    // canonical JSON for diffing against morphic's kv3 parser, and optionally
    // writes the raw block bytes (a self-contained KV3 document) for committing
    // as a morphic fixture.
    private static int Kv3Dump(string[] args)
    {
        string? vpk = null, entry = null, blockName = null, outJson = null, rawOut = null;
        var nth = 0;
        for (var i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--vpk":   vpk       = args[++i]; break;
                case "--entry": entry     = args[++i]; break;
                case "--block": blockName = args[++i]; break;
                case "--nth":   nth       = int.Parse(args[++i], CultureInfo.InvariantCulture); break;
                case "--out":   outJson   = args[++i]; break;
                case "--raw":   rawOut    = args[++i]; break;
                default: return Fail($"kv3-dump: unknown flag {args[i]}");
            }
        }
        if (vpk is null || entry is null || blockName is null || outJson is null)
        {
            return Fail("kv3-dump: --vpk, --entry, --block, and --out are required");
        }
        if (!Enum.TryParse<BlockType>(blockName, ignoreCase: false, out var blockType))
        {
            return Fail($"kv3-dump: unknown block type {blockName}");
        }

        using var pak = new Package();
        pak.Read(vpk);
        var packageEntry = pak.FindEntry(entry)
            ?? throw new FileNotFoundException($"entry not found in VPK: {entry}");
        pak.ReadEntry(packageEntry, out var data);

        using var resource = new Resource();
        using var ms = new MemoryStream(data);
        resource.Read(ms);

        var matches = resource.Blocks.Where(b => b.Type == blockType).ToList();
        if (matches.Count == 0)
        {
            return Fail($"kv3-dump: no {blockName} block in {entry}");
        }
        if (nth < 0 || nth >= matches.Count)
        {
            return Fail($"kv3-dump: --nth {nth} out of range ({matches.Count} {blockName} blocks)");
        }

        var block = matches[nth];
        // VRF wraps a KV3 block either as a raw BinaryKV3 or, for blocks it
        // recognizes (DATA -> Model, MDAT -> Mesh, ...), as a KeyValuesOrNTRO
        // subclass. Both expose the parsed tree as a KVObject.
        var kvData = block switch
        {
            BinaryKV3 b => b.Data,
            KeyValuesOrNTRO knv => knv.Data,
            _ => null,
        };
        if (kvData is null)
        {
            return Fail($"kv3-dump: {blockName}[{nth}] is not KV3 (got {block.GetType().Name})");
        }

        var json = KvToJson(kvData);
        var outFull = Path.GetFullPath(outJson);
        Directory.CreateDirectory(Path.GetDirectoryName(outFull)!);
        File.WriteAllText(outFull, (json?.ToJsonString(JsonOpts) ?? "null") + "\n");
        Console.WriteLine($"dumped {blockName}[{nth}] of {entry} -> {outFull}");

        if (rawOut is not null)
        {
            // Offset/Size are absolute within the resource file (same convention
            // the survey path already relies on). The slice is a complete KV3
            // document: magic + header + (LZ4) payload.
            var raw = new byte[block.Size];
            Array.Copy(data, (int)block.Offset, raw, 0, (int)block.Size);
            var rawFull = Path.GetFullPath(rawOut);
            Directory.CreateDirectory(Path.GetDirectoryName(rawFull)!);
            File.WriteAllBytes(rawFull, raw);
            Console.WriteLine($"  raw block -> {rawFull} ({raw.Length} bytes)");
        }
        return 0;
    }

    // Canonical JSON encoding of a KV3 tree. The encoding is chosen so morphic's
    // Value tree maps to the exact same JSON, and the Rust comparator can match
    // it without float-formatting ambiguity:
    //   - ints / uints  -> JSON number
    //   - floats (f32 widened, f64) -> {"$f64":"0xHEXBITS"} (IEEE-754 bit pattern)
    //   - binary blobs  -> {"$bin":{"len":N,"sha256":"..."}}
    //   - collections   -> JSON object (compared by key, order-insensitive)
    private static JsonNode? KvToJson(KVObject o)
    {
        switch (o.ValueType)
        {
            case KVValueType.Null:
                return null;
            case KVValueType.Boolean:
                return JsonValue.Create((bool)o);
            case KVValueType.Int16:
            case KVValueType.Int32:
            case KVValueType.Int64:
                return JsonValue.Create((long)o);
            case KVValueType.UInt16:
            case KVValueType.UInt32:
            case KVValueType.UInt64:
                return JsonValue.Create((ulong)o);
            case KVValueType.FloatingPoint:
            case KVValueType.FloatingPoint64:
                return FloatNode((double)o);
            case KVValueType.String:
                return JsonValue.Create((string)o);
            case KVValueType.BinaryBlob:
                return BlobNode((byte[])o);
            case KVValueType.Array:
            {
                var arr = new JsonArray();
                for (var i = 0; i < o.Count; i++)
                {
                    arr.Add(KvToJson(o[i]));
                }
                return arr;
            }
            case KVValueType.Collection:
            {
                var obj = new JsonObject();
                foreach (var key in o.Keys)
                {
                    obj[key] = KvToJson(o[key]);
                }
                return obj;
            }
            default:
                throw new NotSupportedException($"kv3-dump: KV3 value type {o.ValueType} not handled");
        }
    }

    private static JsonObject FloatNode(double d) =>
        new() { ["$f64"] = $"0x{BitConverter.DoubleToUInt64Bits(d):X16}" };

    private static JsonObject BlobNode(byte[] blob) =>
        new() { ["$bin"] = new JsonObject { ["len"] = blob.Length, ["sha256"] = Sha256Hex(blob) } };

    // ---------- mesh-buffers (M2 meshopt validation) ----------

    // For each embedded mesh in a .vmdl_c, reconstructs the VBIB the way VRF
    // does (so MVTX/MIDX blocks get meshopt-decoded) and writes, per buffer:
    //   <mesh>_v{j}.meshopt / _i{j}.meshopt  -> raw compressed block bytes
    //   <mesh>_v{j}.meshopt.json / ...        -> decoded-buffer golden record
    // morphic decodes the raw block and matches the golden's length + SHA-256.
    private static int MeshBuffers(string[] args)
    {
        string? vpk = null, entry = null, outDir = null;
        for (var i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--vpk":     vpk    = args[++i]; break;
                case "--entry":   entry  = args[++i]; break;
                case "--out-dir": outDir = args[++i]; break;
                default: return Fail($"mesh-buffers: unknown flag {args[i]}");
            }
        }
        if (vpk is null || entry is null || outDir is null)
        {
            return Fail("mesh-buffers: --vpk, --entry, and --out-dir are required");
        }

        using var pak = new Package();
        pak.Read(vpk);
        var packageEntry = pak.FindEntry(entry)
            ?? throw new FileNotFoundException($"entry not found in VPK: {entry}");
        pak.ReadEntry(packageEntry, out var data);

        using var resource = new Resource { FileName = entry };
        using var ms = new MemoryStream(data);
        resource.Read(ms);

        KVObject? root = resource.GetBlockByType(BlockType.CTRL) switch
        {
            BinaryKV3 b => b.Data.Root,
            KeyValuesOrNTRO k => k.Data,
            _ => null,
        };
        if (root is null)
        {
            return Fail("mesh-buffers: no CTRL block (embedded mesh control)");
        }

        Directory.CreateDirectory(Path.GetFullPath(outDir));
        var records = new List<MeshBufferRecord>();

        foreach (var em in root.GetArray("embedded_meshes"))
        {
            var name = em.GetStringProperty("m_Name");
            var vbib = new VBIB(resource, em) { Resource = resource };

            EmitBuffers(data, resource, outDir, records, name, "vertex",
                em.GetArray("m_vertexBuffers"), vbib.VertexBuffers);
            EmitBuffers(data, resource, outDir, records, name, "index",
                em.GetArray("m_indexBuffers"), vbib.IndexBuffers);
        }

        File.WriteAllText(Path.Combine(Path.GetFullPath(outDir), "manifest.json"),
            JsonSerializer.Serialize(records, JsonOpts) + "\n");
        Console.WriteLine($"wrote {records.Count} mesh buffers -> {outDir}");
        return 0;
    }

    private static void EmitBuffers(
        byte[] data, Resource resource, string outDir, List<MeshBufferRecord> records,
        string mesh, string kind, IReadOnlyList<KVObject> descriptors, List<VBIB.OnDiskBufferData> buffers)
    {
        for (var j = 0; j < buffers.Count; j++)
        {
            var desc = descriptors[j];
            var blockIndex = desc.GetInt32Property("m_nBlockIndex");
            var block = resource.GetBlockByIndex(blockIndex);
            var raw = new byte[block.Size];
            Array.Copy(data, (int)block.Offset, raw, 0, (int)block.Size);

            var prefix = kind == "vertex" ? "v" : "i";
            var baseName = $"{SanitizeBasename(mesh)}_{prefix}{j}";
            File.WriteAllBytes(Path.Combine(Path.GetFullPath(outDir), baseName + ".meshopt"), raw);

            var rec = new MeshBufferRecord
            {
                Mesh = mesh,
                Kind = kind,
                BufferIndex = j,
                BlockIndex = blockIndex,
                ElementCount = buffers[j].ElementCount,
                ElementSize = buffers[j].ElementSizeInBytes,
                Meshopt = desc.GetByteProperty("m_bMeshoptCompressed") == 1,
                Zstd = desc.GetByteProperty("m_bCompressedZSTD") == 1,
                CompressedSize = (int)block.Size,
                DecodedLen = buffers[j].Data.Length,
                Sha256 = Sha256Hex(buffers[j].Data),
            };
            records.Add(rec);
            File.WriteAllText(
                Path.Combine(Path.GetFullPath(outDir), baseName + ".meshopt.json"),
                JsonSerializer.Serialize(rec, JsonOpts) + "\n");
        }
    }

    // ---------- helpers ----------

    private static string TryReadKv3Magic(byte[] resourceBytes, Resource resource)
    {
        // DATA block magic is KV3 if it starts with 'VKV\x03' or one of the
        // later magic bytes. Read the first 4 bytes of the DATA block.
        var data = resource.DataBlock;
        if (data is null) return "";
        var ofs = data.Offset;
        if (ofs + 4 > (ulong)resourceBytes.LongLength) return "";
        var magic = BitConverter.ToUInt32(resourceBytes, (int)ofs);
        return $"0x{magic:X8}";
    }

    private static string EntryPath(PackageEntry e)
    {
        return string.IsNullOrEmpty(e.DirectoryName)
            ? $"{e.FileName}.{e.TypeName}"
            : $"{e.DirectoryName}/{e.FileName}.{e.TypeName}";
    }

    private static string Csv(string s)
    {
        if (s.Contains(',') || s.Contains('"') || s.Contains('\n'))
        {
            return $"\"{s.Replace("\"", "\"\"")}\"";
        }
        return s;
    }

    private static string SanitizeBasename(string entry)
    {
        var name = Path.GetFileName(entry);
        return string.IsNullOrEmpty(name) ? "extracted.vtex_c" : name;
    }

    private static string Sha256Hex(byte[] data)
    {
        var hash = SHA256.HashData(data);
        var sb = new StringBuilder(hash.Length * 2);
        foreach (var b in hash) sb.Append(b.ToString("x2", CultureInfo.InvariantCulture));
        return sb.ToString();
    }

    private static Tolerance ToleranceFor(VTexFormat fmt)
    {
        // Default tolerances per format family. Hand-edit a fixture's meta to
        // override if a specific decoder needs tighter or looser bounds.
        return fmt switch
        {
            VTexFormat.RGBA8888 => new Tolerance { Kind = "byte_exact" },
            VTexFormat.RGBA16161616F => new Tolerance { Kind = "hdr_eps", Abs = 0.000977, Rel = 0.005 },
            VTexFormat.BC6H => new Tolerance { Kind = "hdr_eps", Abs = 0.000977, Rel = 0.005 },
            VTexFormat.BC7 => new Tolerance { Kind = "mae_u8", Epsilon = 3.0 },
            _ => new Tolerance { Kind = "mae_u8", Epsilon = 2.0 },
        };
    }

    private static int PrintUsage()
    {
        Console.WriteLine("usage:");
        Console.WriteLine("  morphic-oracle generate --fixtures DIR [--force]");
        Console.WriteLine("  morphic-oracle extract  --vpk PATH --entry NAME --out DIR");
        Console.WriteLine("  morphic-oracle survey   --vpk PATH --out CSV");
        Console.WriteLine("  morphic-oracle model    --vpk PATH --entry NAME [--base PATH] --out GLB [--no-materials]");
        Console.WriteLine("  morphic-oracle kv3-dump --vpk PATH --entry NAME --block FOURCC [--nth N] --out JSON [--raw KV3BIN]");
        Console.WriteLine("  morphic-oracle mesh-buffers --vpk PATH --entry NAME --out-dir DIR");
        return 0;
    }

    private static int Fail(string msg)
    {
        Console.Error.WriteLine($"error: {msg}");
        return 2;
    }

    // ---------- JSON shapes ----------

    private sealed class Meta
    {
        [JsonPropertyName("format")]         public string Format { get; init; } = "";
        [JsonPropertyName("width")]          public ushort Width { get; init; }
        [JsonPropertyName("height")]         public ushort Height { get; init; }
        // VRF crops the rendered PNG to ActualWidth/Height when the texture
        // carries NonPow2 metadata; the Rust test crops morphic's full decode
        // to these dims before diffing against the PNG.
        [JsonPropertyName("actual_width")]   public ushort ActualWidth { get; init; }
        [JsonPropertyName("actual_height")]  public ushort ActualHeight { get; init; }
        [JsonPropertyName("depth")]          public ushort Depth { get; init; }
        [JsonPropertyName("mip_count")]      public byte MipCount { get; init; }
        [JsonPropertyName("flags")]          public string[] Flags { get; init; } = Array.Empty<string>();
        [JsonPropertyName("source_sha256")]  public string SourceSha256 { get; init; } = "";
        [JsonPropertyName("vrf_version")]    public string VrfVersion { get; init; } = "";
        [JsonPropertyName("tolerance")]      public Tolerance Tolerance { get; init; } = new();
    }

    private sealed class Tolerance
    {
        [JsonPropertyName("kind")]    public string Kind { get; init; } = "byte_exact";
        [JsonPropertyName("epsilon")] public double? Epsilon { get; init; }
        [JsonPropertyName("abs")]     public double? Abs { get; init; }
        [JsonPropertyName("rel")]     public double? Rel { get; init; }
    }

    private sealed class MeshBufferRecord
    {
        [JsonPropertyName("mesh")]            public string Mesh { get; init; } = "";
        [JsonPropertyName("kind")]            public string Kind { get; init; } = "";
        [JsonPropertyName("buffer_index")]    public int BufferIndex { get; init; }
        [JsonPropertyName("block_index")]     public int BlockIndex { get; init; }
        [JsonPropertyName("element_count")]   public uint ElementCount { get; init; }
        [JsonPropertyName("element_size")]    public uint ElementSize { get; init; }
        [JsonPropertyName("meshopt")]         public bool Meshopt { get; init; }
        [JsonPropertyName("zstd")]            public bool Zstd { get; init; }
        [JsonPropertyName("compressed_size")] public int CompressedSize { get; init; }
        [JsonPropertyName("decoded_len")]     public int DecodedLen { get; init; }
        [JsonPropertyName("sha256")]          public string Sha256 { get; init; } = "";
    }
}
