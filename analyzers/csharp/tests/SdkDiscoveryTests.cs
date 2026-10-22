namespace CodeGraph.CSharp.Engine;

/// <summary>保留旧根平台工厂的真实发现断言，只编入非交付测试驱动。</summary>
internal static class SdkDiscoveryTests
{
    /// <summary>最高稳定补丁、未来 TFM 和仅运行时安装都必须遵守原发现政策。</summary>
    internal static int Run()
    {
        var root = Path.Combine(Path.GetTempPath(), $"codegraph sdk discovery {Guid.NewGuid()}");
        Directory.CreateDirectory(root);
        try
        {
            foreach (var version in new[] { "8.0.0", "8.0.17", "9.0.20", "42.0.3", "42.0.99-preview", "99.0.1" })
            {
                var tfm = $"net{version.Split('.')[0]}.0";
                var references = Path.Combine(root, "packs", "Microsoft.NETCore.App.Ref", version, "ref", tfm);
                Directory.CreateDirectory(references);
                File.WriteAllText(Path.Combine(references, "System.Runtime.dll"), "fixture");
                if (!version.StartsWith("99.")) File.WriteAllText(Path.Combine(references, "mscorlib.dll"), "fixture");
            }
            var platforms = SdkDiscovery.InRoots(new[] { root, "relative" });
            Require(platforms.Length == 3, "only complete stable TFM roots are admitted");
            var net8 = platforms.Single(platform => platform.Metadata.Tfm == "net8.0");
            Require(net8.Version == "8.0.17", "highest stable patch");
            Require(platforms.Single(platform => platform.Metadata.Tfm == "net42.0").Version == "42.0.3", "no target-version allowlist");
            Require(platforms.All(platform => platform.Root.StartsWith(PublicRequest.CanonicalDirectory(root), StringComparison.Ordinal)), "exact reference roots");
            var runtimeOnly = Path.Combine(root, "runtime-only");
            Directory.CreateDirectory(Path.Combine(runtimeOnly, "shared", "Microsoft.NETCore.App", "42.0.3"));
            var rejected = false;
            try { _ = SdkDiscovery.InRoots(new[] { runtimeOnly }); }
            catch (HelperException error) { rejected = error.Code == "target_unavailable"; }
            Require(rejected, "runtime alone cannot provide target references");
            Console.WriteLine("SDK discovery assertions passed");
            return 0;
        }
        finally { Directory.Delete(root, true); }
    }

    /// <summary>断言直接使驱动失败，不能依赖运行时调试开关。</summary>
    private static void Require(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }
}
