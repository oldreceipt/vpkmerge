using SteamDatabase.ValvePak;

return Run(args);

static int Run(string[] args)
{
    var strict = false;
    var verbose = false;
    var positional = new List<string>();

    foreach (var a in args)
    {
        switch (a)
        {
            case "--strict": strict = true; break;
            case "--verbose" or "-v": verbose = true; break;
            case "--help" or "-h": PrintUsage(); return 0;
            default:
                if (a.StartsWith("--"))
                {
                    Console.Error.WriteLine($"unknown option: {a}");
                    return 2;
                }
                positional.Add(a);
                break;
        }
    }

    if (positional.Count < 3)
    {
        PrintUsage();
        return 2;
    }

    var outputPath = positional[0];
    var inputPaths = positional.Skip(1).ToArray();

    foreach (var p in inputPaths)
    {
        if (!File.Exists(p))
        {
            Console.Error.WriteLine($"input not found: {p}");
            return 1;
        }
    }

    using var outPkg = new Package();
    var collisions = 0;
    var totalEntries = 0;

    foreach (var inPath in inputPaths)
    {
        Console.WriteLine($"reading {inPath}");
        using var inPkg = new Package();
        inPkg.Read(inPath);

        foreach (var group in inPkg.Entries)
        {
            if (group.Value is null) continue;
            foreach (var entry in group.Value)
            {
                var fullPath = entry.GetFullPath();
                inPkg.ReadEntry(entry, out var data);

                var existing = outPkg.FindEntry(fullPath);
                if (existing != null)
                {
                    collisions++;
                    if (strict)
                    {
                        Console.Error.WriteLine($"collision (strict mode): {fullPath} (in {inPath})");
                        return 1;
                    }
                    if (verbose)
                    {
                        Console.WriteLine($"override: {fullPath} <- {inPath}");
                    }
                    outPkg.RemoveFile(existing);
                    totalEntries--;
                }

                outPkg.AddFile(fullPath, data);
                totalEntries++;
            }
        }
    }

    var outDir = Path.GetDirectoryName(Path.GetFullPath(outputPath));
    if (!string.IsNullOrEmpty(outDir))
    {
        Directory.CreateDirectory(outDir);
    }

    outPkg.Write(outputPath);
    Console.WriteLine($"wrote {outputPath}: {totalEntries} entries, {collisions} overrides from {inputPaths.Length} inputs");
    return 0;
}

static void PrintUsage()
{
    Console.WriteLine("""
        vpkmerge — combine multiple VPK files into one

        usage: vpkmerge <output.vpk> <input1.vpk> <input2.vpk> [more.vpk...] [options]

        options:
          --strict       error out on any path collision (default: later input wins)
          --verbose, -v  print each overridden file
          --help, -h     show this message

        for chunked inputs, pass the *_dir.vpk file; chunk files (*_000.vpk, *_001.vpk, ...)
        are read automatically when they sit alongside it.
        """);
}
