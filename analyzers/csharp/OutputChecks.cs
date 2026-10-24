namespace CodeGraph.CSharp.Engine;

/// <summary>在图构造结束后一次检查来源覆盖，内部证据从不写到公共 stdout。</summary>
internal static class OutputChecks
{
    /// <summary>交叉检查配置、精确选集和节点来源，避免上下文源码扩展完整图。</summary>
    internal static void Sources(AnalyzeRequest request, Evidence evidence)
    {
        var configurations = new HashSet<string>(StringComparer.Ordinal);
        var platforms = request.Environment.Platforms.Select(platform => platform.Id).ToHashSet(StringComparer.Ordinal);
        foreach (var config in evidence.Configurations)
        {
            if (config.Id.Length == 0 || !configurations.Add(config.Id)) Invalid("Duplicate or empty configuration");
            var sources = new HashSet<string>(StringComparer.Ordinal);
            foreach (var source in config.SourcePaths)
            {
                if (!PublicRequest.IsRelative(source) || !sources.Add(source)) Invalid("Invalid configuration source");
                var path = Path.GetFullPath(source.Replace('/', Path.DirectorySeparatorChar), request.Input.RepositoryRoot);
                if (!File.Exists(path)) Invalid("Missing configuration source");
                Configuration.CheckRepositoryPath(request.Input.RepositoryRoot, path);
            }
            if (config.PlatformIds.Distinct(StringComparer.Ordinal).Count() != config.PlatformIds.Length ||
                config.PlatformIds.Any(id => !platforms.Contains(id))) Invalid("Unapproved configuration SDK");
        }
        if (request.Input.Files is null && configurations.Count == 0) Invalid("Discovery lacks actual configuration");
        var requested = request.Input.Files?.ToHashSet(StringComparer.Ordinal);
        var coverage = new Dictionary<string, string>(StringComparer.Ordinal);
        var physical = new HashSet<string>(OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal);
        foreach (var file in evidence.Files)
        {
            if (!coverage.TryAdd(file.Path, file.Disposition) || requested is not null && !requested.Contains(file.Path)) Invalid("Invalid source coverage");
            if (!physical.Add(Configuration.CheckFile(request.Input.RepositoryRoot, file.Path))) Invalid("Physical source alias");
            if (file.Disposition is not "analyzed" and not "excluded_test" and not "inactive" || request.Input.IncludeTests && file.Disposition == "excluded_test") Invalid("Invalid source disposition");
            if (file.ConfigurationIds.Length == 0 || file.ConfigurationIds.Distinct(StringComparer.Ordinal).Count() != file.ConfigurationIds.Length ||
                file.ConfigurationIds.Any(id => !configurations.Contains(id))) Invalid("Source lacks actual configuration");
        }
        if (coverage.Count > Protocol.MaxFiles || requested is not null && coverage.Count != requested.Count) Invalid("Selection coverage mismatch");
        var origins = new HashSet<string>(StringComparer.Ordinal);
        foreach (var origin in evidence.Origins)
        {
            if (!origins.Add(origin.NodeId) || origin.SelectedPaths.Length == 0 ||
                origin.SelectedPaths.Distinct(StringComparer.Ordinal).Count() != origin.SelectedPaths.Length ||
                origin.SelectedPaths.Any(path => coverage.GetValueOrDefault(path) != "analyzed")) Invalid("Origin outside analyzed selection");
        }
    }

    /// <summary>来源不变量失败属于生产者错误，不能自动修图。</summary>
    private static void Invalid(string message) => throw new HelperException("internal_failure", message);
}
