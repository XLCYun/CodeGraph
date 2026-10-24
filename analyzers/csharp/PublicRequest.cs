using System.Text;
using System.Text.Json;

namespace CodeGraph.CSharp.Engine;

/// <summary>严格解码唯一公共请求；内部类型从不作为生产 JSON 输入。</summary>
internal static class PublicRequest
{
    /// <summary>递归检查所有对象键唯一，JsonDocument 负责语法和有界嵌套。</summary>
    private static void Unique(JsonElement element)
    {
        if (element.ValueKind == JsonValueKind.Object)
        {
            var seen = new HashSet<string>(StringComparer.Ordinal);
            foreach (var property in element.EnumerateObject())
            {
                if (!seen.Add(property.Name)) throw new HelperException("invalid_request", "Duplicate JSON field");
                Unique(property.Value);
            }
        }
        else if (element.ValueKind == JsonValueKind.Array)
            foreach (var value in element.EnumerateArray()) Unique(value);
    }

    /// <summary>字段名严格区分大小写，必须字段与唯一可选字段分开验证。</summary>
    private static void Fields(JsonElement element, string[] required, string? optional = null)
    {
        if (element.ValueKind != JsonValueKind.Object) throw new HelperException("invalid_request", "Expected JSON object");
        foreach (var property in element.EnumerateObject())
            if (!required.Contains(property.Name, StringComparer.Ordinal) && property.Name != optional)
                throw new HelperException("invalid_request", $"Unknown field {property.Name}");
        if (required.Any(name => !element.TryGetProperty(name, out _)))
            throw new HelperException("invalid_request", "Missing required field");
    }

    /// <summary>读取可无损转换成 UTF-8 且不含 NUL 的字符串。</summary>
    private static string Text(JsonElement element, string field)
    {
        var value = element.GetProperty(field);
        if (value.ValueKind != JsonValueKind.String) throw new HelperException("invalid_request", $"Expected string {field}");
        var text = value.GetString()!;
        if (text.Contains('\0')) throw new HelperException("invalid_request", "NUL is forbidden");
        _ = new UTF8Encoding(false, true).GetByteCount(text);
        return text;
    }

    /// <summary>公共选集采用 POSIX 相对路径，实际文件检查在根边界确定后执行。</summary>
    public static bool IsRelative(string path) => path.Length > 0 && !path.Contains(':') && !path.Contains('\\') &&
        !path.Contains('\0') && path.Split('/').All(part => part is not "" and not "." and not "..");

    /// <summary>解码请求并把合法 ID 交给错误输出；显式空集仍校验根与安装边界。</summary>
    public static AnalyzeRequest Parse(byte[] bytes, out string? acceptedId)
    {
        acceptedId = null;
        try { return ParseValue(bytes, out acceptedId); }
        catch (Exception error) when (error is JsonException or InvalidOperationException or FormatException or DecoderFallbackException or EncoderFallbackException or ArgumentException)
        { throw new HelperException("invalid_request", "Invalid UTF-8 JSON request"); }
    }

    /// <summary>只在请求解码阶段归类语法错误，项目配置错误保留分析阶段的失败语义。</summary>
    private static AnalyzeRequest ParseValue(byte[] bytes, out string? acceptedId)
    {
        acceptedId = null;
        _ = new UTF8Encoding(false, true).GetString(bytes);
        using var document = JsonDocument.Parse(bytes, new JsonDocumentOptions { MaxDepth = 64 });
        var root = document.RootElement;
        Unique(root);
        Fields(root, ["protocol", "type", "id", "repository_root", "options"], "files");
        var id = Text(root, "id");
        if (string.IsNullOrWhiteSpace(id) || Encoding.UTF8.GetByteCount(id) > 1024)
            throw new HelperException("invalid_request", "Invalid request id");
        acceptedId = id;
        if (Text(root, "protocol") != Protocol.Version) throw new HelperException("incompatible_protocol", "Unsupported analyzer protocol");
        if (Text(root, "type") != "analyze") throw new HelperException("invalid_request", "Expected analyze request");
        var options = root.GetProperty("options");
        Fields(options, ["include_tests", "rust_dependency_scope"]);
        var include = options.GetProperty("include_tests");
        if (include.ValueKind is not JsonValueKind.True and not JsonValueKind.False)
            throw new HelperException("invalid_request", "include_tests must be boolean");
        if (Text(options, "rust_dependency_scope") is not "repository-only" and not "standard-library" and not "all-dependencies")
            throw new HelperException("invalid_request", "Invalid rust_dependency_scope");
        string[]? files = null;
        if (root.TryGetProperty("files", out var array))
        {
            if (array.ValueKind != JsonValueKind.Array) throw new HelperException("invalid_request", "files must be an array");
            if (array.GetArrayLength() > Protocol.MaxFiles) throw new HelperException("resource_limit", "Selection exceeds file limit");
            var selected = new HashSet<string>(StringComparer.Ordinal);
            files = array.EnumerateArray().Select(value =>
            {
                if (value.ValueKind != JsonValueKind.String) throw new HelperException("invalid_request", "files must contain strings");
                var file = value.GetString()!;
                _ = new UTF8Encoding(false, true).GetByteCount(file);
                if (!IsRelative(file) || !selected.Add(file)) throw new HelperException("invalid_request", "Invalid or duplicate selected path");
                return file;
            }).ToArray();
        }
        var repository = Text(root, "repository_root");
        if (!Path.IsPathFullyQualified(repository)) throw new HelperException("invalid_input", "repository_root must be an absolute directory");
        repository = CanonicalDirectory(repository);
        var installation = CanonicalDirectory(AppContext.BaseDirectory);
        if (Path.GetFileName(installation) == "bin") installation = Path.GetDirectoryName(installation)!;
        if (Under(repository, installation) || Under(installation, repository))
            throw new HelperException("invalid_input", "Source overlaps analyzer installation");
        foreach (var file in files ?? []) Configuration.CheckFile(repository, file);
        return new AnalyzeRequest(id, new Input(repository, files, include.GetBoolean(), "csharp-static-v1"),
            new EnvironmentInput(Path.GetTempPath(), []), new Limits(1800000, Protocol.MaxBytes, 262144));
    }

    /// <summary>逐段解析目录链接；保留 .NET 对 Unicode、URI 字符和 Windows 扩展路径的支持。</summary>
    internal static string CanonicalDirectory(string path)
    {
        var full = Path.GetFullPath(path);
        if (!Directory.Exists(full)) throw new HelperException("invalid_input", "Repository directory is unavailable");
        var current = Path.GetPathRoot(full)!;
        foreach (var part in full[current.Length..].Split(Path.DirectorySeparatorChar, StringSplitOptions.RemoveEmptyEntries))
        {
            current = Path.Combine(current, part);
            var info = new DirectoryInfo(current);
            if (info.Attributes.HasFlag(FileAttributes.ReparsePoint))
                current = info.ResolveLinkTarget(true)?.FullName ?? throw new HelperException("invalid_input", "Unresolvable directory link");
        }
        return Path.TrimEndingDirectorySeparator(current);
    }

    /// <summary>按完整路径段判断包含关系，Windows 使用文件系统的大小写规则。</summary>
    internal static bool Under(string candidate, string root)
    {
        var comparison = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
        var prefix = Path.TrimEndingDirectorySeparator(root);
        if (!Path.EndsInDirectorySeparator(prefix)) prefix += Path.DirectorySeparatorChar;
        return candidate.Equals(root, comparison) || candidate.StartsWith(prefix, comparison);
    }
}
