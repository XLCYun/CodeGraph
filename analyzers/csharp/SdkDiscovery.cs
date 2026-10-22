namespace CodeGraph.CSharp.Engine;

/// <summary>只读取已安装的引用包目录，运行程序自带的 .NET 不作为项目目标 SDK。</summary>
internal static class SdkDiscovery
{
    /// <summary>枚举常见安装路径，缺失目录不会隐藏其他安装。</summary>
    private static IEnumerable<string> Directories(string root) => Directory.Exists(root)
        ? Directory.GetDirectories(root).Order(StringComparer.Ordinal) : [];

    /// <summary>解释稳定数字版本，preview 等目录不会进入目标目录集合。</summary>
    internal static int[]? Numeric(string text)
    {
        var values = text.Split('.');
        if (values.Length < 2 || values.Any(value => value.Length == 0 || value.Any(c => c is < '0' or > '9'))) return null;
        var result = new List<int>();
        foreach (var value in values) { if (!int.TryParse(value, out var number)) return null; result.Add(number); }
        return result.ToArray();
    }

    /// <summary>每个 netN.0 只保留最高稳定补丁，同版本保留最先发现的安装。</summary>
    internal static Platform[] InRoots(IEnumerable<string> roots)
    {
        var best = new SortedDictionary<string, Platform>(StringComparer.Ordinal);
        foreach (var basePath in roots.Where(Path.IsPathFullyQualified))
        foreach (var package in Directories(Path.Combine(basePath, "packs", "Microsoft.NETCore.App.Ref")))
        {
            var version = Path.GetFileName(package);
            var parts = Numeric(version);
            if (parts is not { Length: 3 } || parts[0] < 5 || parts[1] != 0) continue;
            var tfm = $"net{parts[0]}.0";
            var root = Path.Combine(package, "ref", tfm);
            if (!File.Exists(Path.Combine(root, "System.Runtime.dll")) || !File.Exists(Path.Combine(root, "mscorlib.dll"))) continue;
            if (best.TryGetValue(tfm, out var current) && Numeric(current.Version)![2] >= parts[2]) continue;
            best[tfm] = new Platform($"{tfm}-reference-assemblies", "dotnet-reference-assemblies", version,
                PublicRequest.CanonicalDirectory(root), new PlatformMetadata(tfm, "Microsoft.NETCore.App.Ref", version));
        }
        if (best.Count == 0) throw new HelperException("target_unavailable", "未找到本机 .NET 标准引用程序集，请安装项目目标对应的 .NET SDK；仅安装运行时不足以分析");
        return best.Values.ToArray();
    }

    /// <summary>只使用环境与标准目录提供的安装线索，不运行 dotnet、MSBuild 或项目命令。</summary>
    internal static Platform[] Discover()
    {
        var roots = new List<string>();
        foreach (var key in new[] { "DOTNET_ROOT", "DOTNET_ROOT_ARM64", "DOTNET_ROOT_X64", "DOTNET_ROOT_X86", "DOTNET_ROOT(x86)" })
            if (Environment.GetEnvironmentVariable(key) is { Length: > 0 } value) roots.Add(value);
        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        roots.Add(Path.Combine(home, ".dotnet"));
        if (OperatingSystem.IsWindows())
        {
            foreach (var key in new[] { "ProgramFiles", "ProgramFiles(x86)" })
                if (Environment.GetEnvironmentVariable(key) is { Length: > 0 } value) roots.Add(Path.Combine(value, "dotnet"));
        }
        else roots.AddRange(["/usr/local/share/dotnet", "/usr/local/share/dotnet/x64", "/opt/homebrew/share/dotnet", "/usr/share/dotnet", "/usr/lib/dotnet"]);
        return InRoots(roots);
    }
}
