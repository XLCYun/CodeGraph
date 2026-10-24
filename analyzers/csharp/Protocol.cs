using System.Text.Json;

namespace CodeGraph.CSharp.Engine;

/// <summary>封装协议输入错误及明确的失败代码。</summary>
internal sealed class HelperException(string code, string message, params string[] paths) : Exception(message)
{
    public string Code { get; } = code;
    public string[] Paths { get; } = paths;
}

/// <summary>保存一次分析请求及其严格校验后的字段。</summary>
internal sealed record AnalyzeRequest(string Id, Input Input, EnvironmentInput Environment, Limits Limits);

/// <summary>保存固定侧仓库及选集。</summary>
internal sealed record Input(string RepositoryRoot, string[]? Files, bool IncludeTests, string Profile);

/// <summary>保存宿主准许读取的目标平台与 scratch。</summary>
internal sealed record EnvironmentInput(string ScratchRoot, Platform[] Platforms);

/// <summary>描述一个单独批准的目标引用程序集目录。</summary>
internal sealed record Platform(string Id, string Kind, string Version, string Root, PlatformMetadata Metadata);

/// <summary>定义 C# 平台的有限 metadata 字段。</summary>
internal sealed record PlatformMetadata(string Tfm, string ReferencePack, string ReferencePackVersion);

/// <summary>保存本次请求的三个资源上限。</summary>
internal sealed record Limits(int AnalysisTimeoutMs, long MaxResultBytes, int MaxDiagnosticBytes);

/// <summary>按 v1 协议读取消息，拒绝重复字段、缺字段和未知字段。</summary>
internal static class Protocol
{
    public const int MaxFiles = 100000;
    public const string Version = "codegraph.analyzer/1";
    public const long MaxBytes = 128 * 1024 * 1024;
    public static readonly JsonSerializerOptions Json = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
        DictionaryKeyPolicy = JsonNamingPolicy.SnakeCaseLower
    };

    /// <summary>序列化协议响应。</summary>
    public static string Serialize(object value) => JsonSerializer.Serialize(value, Json);
}

/// <summary>描述最终图节点。</summary>
internal sealed class GraphNode
{
    public required string Category { get; init; }
    public required string Id { get; init; }
    public string Language { get; init; } = "csharp";
    public required string NativeKind { get; init; }
    public string? Name { get; init; }
    public string? Signature { get; init; }
    public string? Code { get; init; }
    public string? Source { get; init; }
    public List<string> Contains { get; } = [];
    public List<GraphReference> References { get; } = [];
    public Dictionary<string, object> Extensions { get; } = [];
}

/// <summary>描述语义关系及最终目标分类。</summary>
internal sealed record GraphReference(string Semantic, object Target);

/// <summary>描述图内、外部或无法解析的目标。</summary>
internal sealed record GraphTarget(string Kind, string? Id = null, string? Symbol = null, string? Name = null);

/// <summary>保存来源和覆盖证据。</summary>
internal sealed record Evidence(List<ConfigurationEvidence> Configurations, List<FileEvidence> Files, List<OriginEvidence> Origins);

/// <summary>记录实际使用的有限静态配置。</summary>
internal sealed record ConfigurationEvidence(string Id, string[] SourcePaths, string[] PlatformIds);

/// <summary>记录输入文件对应的分析状态。</summary>
internal sealed record FileEvidence(string Path, string Disposition, string[] ConfigurationIds);

/// <summary>记录图节点对应的选中原始源码。</summary>
internal sealed record OriginEvidence(string NodeId, string[] SelectedPaths);
