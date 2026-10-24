using System.Text.Json;

namespace CodeGraph.CSharp.Engine;

/// <summary>只用于历史组件夹具的测试驱动，不编入或随包交付生产程序。</summary>
internal static class ComponentDriver
{
    /// <summary>将测试数据直接转换为进程内类型，继续核对语义、来源和 SDK 拒绝策略。</summary>
    private static int Main(string[] args)
    {
        if (args.SequenceEqual(new[] { "--sdk-discovery-tests" })) return SdkDiscoveryTests.Run();
        object result;
        try
        {
            using var document = JsonDocument.Parse(Console.In.ReadToEnd());
            var value = document.RootElement;
            var input = value.GetProperty("input");
            if (input.TryGetProperty("files", out var files) && files.ValueKind != JsonValueKind.Array)
                throw new HelperException("invalid_input", "files must be an array");
            if (input.TryGetProperty("files", out files))
            {
                var selected = new HashSet<string>(StringComparer.Ordinal);
                foreach (var file in files.EnumerateArray())
                    if (file.ValueKind != JsonValueKind.String || !selected.Add(file.GetString()!))
                        throw new HelperException("invalid_input", "Invalid fixture files");
            }
            var request = JsonSerializer.Deserialize<AnalyzeRequest>(value.GetRawText(), Protocol.Json)!;
            using var deadline = new CancellationTokenSource(request.Limits.AnalysisTimeoutMs);
            var output = Analyzer.Analyze(request, deadline.Token);
            result = new { type = "completed", outcome = "ok", graph = output.Graph, evidence = output.Evidence };
            if (System.Text.Encoding.UTF8.GetByteCount(Protocol.Serialize(result)) > request.Limits.MaxResultBytes)
                throw new HelperException("resource_limit", "Result exceeds byte limit");
        }
        catch (OperationCanceledException) { result = new { type = "completed", outcome = "error", error = new { code = "resource_limit", message = "Analysis deadline exceeded" } }; }
        catch (HelperException error) { result = new { type = "completed", outcome = "error", error = new { code = error.Code, message = error.Message, paths = error.Paths } }; }
        Console.WriteLine(Protocol.Serialize(result));
        return 0;
    }
}
