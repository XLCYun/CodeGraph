using System.Text;
using System.Text.Json;

namespace CodeGraph.CSharp.Engine;

/// <summary>self-contained apphost 直接执行一次公共分析，没有控制监听器或分析 worker。</summary>
internal static class Program
{
    /// <summary>以固定 CLI、UTF-8 字节管道和 EOF 确认单次请求边界。</summary>
    private static int Main(string[] args)
    {
        if (args.SequenceEqual(["--help"]) || args.SequenceEqual(["analyze", "--help"]) || args.SequenceEqual(["--version"]))
        {
            Console.WriteLine(args[0] == "--version" ? "codegraph-csharp 0.1.2" : "Usage: codegraph-csharp analyze\nReads one UTF-8 JSON request followed by EOF.");
            return 0;
        }
        if (!args.SequenceEqual(["analyze"])) { Console.Error.WriteLine("Usage: codegraph-csharp analyze"); return 2; }
        string? id = null;
        object result;
        var success = false;
        try
        {
            using var input = Console.OpenStandardInput();
            using var bytes = new MemoryStream();
            var buffer = new byte[65536];
            int count;
            while ((count = input.Read(buffer, 0, buffer.Length)) > 0)
            {
                if (bytes.Length + count > Protocol.MaxBytes) throw new HelperException("resource_limit", "Request exceeds byte limit");
                bytes.Write(buffer, 0, count);
            }
            var request = PublicRequest.Parse(bytes.ToArray(), out id);
            if (request.Input.Files is not { Length: 0 })
                request = request with { Environment = request.Environment with { Platforms = SdkDiscovery.Discover() } };
            foreach (var platform in request.Environment.Platforms)
                if (PublicRequest.Under(platform.Root, request.Input.RepositoryRoot) || PublicRequest.Under(request.Input.RepositoryRoot, platform.Root))
                    throw new HelperException("invalid_input", "Target SDK overlaps source repository");
            using var deadline = new CancellationTokenSource(request.Limits.AnalysisTimeoutMs);
            var output = Analyzer.Analyze(request, deadline.Token);
            OutputChecks.Sources(request, output.Evidence);
            result = new { protocol = Protocol.Version, engine_version = "0.1.2", pack_id = "csharp", type = "completed", id,
                outcome = "ok", graph = output.Graph };
            success = true;
        }
        catch (OperationCanceledException) { result = Error(id, "resource_limit", "Analysis deadline exceeded"); }
        catch (HelperException error) { result = Error(id, Code(error.Code), error.Message); }
        catch (Exception error) when (error is JsonException or InvalidOperationException or FormatException or DecoderFallbackException or EncoderFallbackException)
        { result = Error(id, "analysis_failed", error.Message); }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        { result = Error(id, "io_failure", error.Message); }
        catch (Exception error) { result = Error(id, "internal_failure", error.Message); }
        var encoded = Encoding.UTF8.GetBytes(Protocol.Serialize(result) + "\n");
        if (encoded.LongLength > Protocol.MaxBytes)
        {
            encoded = Encoding.UTF8.GetBytes(Protocol.Serialize(Error(id, "resource_limit", "Result exceeds byte limit")) + "\n");
            success = false;
        }
        using var outputStream = Console.OpenStandardOutput();
        outputStream.Write(encoded);
        outputStream.Flush();
        return success ? 0 : 1;
    }

    /// <summary>公共失败不带内部路径数组、来源或部分图。</summary>
    private static object Error(string? id, string code, string message) => new
    { protocol = Protocol.Version, engine_version = "0.1.2", pack_id = "csharp", type = "completed", id, outcome = "error", error = new { code, message } };

    /// <summary>将语言静态模型的细分失败映射到固定公共错误码。</summary>
    private static string Code(string code) => code is "invalid_request" or "incompatible_protocol" or "invalid_input" or
        "resource_limit" or "timeout" or "io_failure" or "internal_failure" ? code : "analysis_failed";
}
