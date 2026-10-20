using System.Text.Json;

namespace CodeGraph.CSharp.Engine;

/// <summary>只在验收显式启用时记录实际源读取；不控制加载或更改协议。</summary>
internal static class Observation
{
    /// <summary>完整写入私有新文件后发布，目标已存在时必须失败而不是覆盖。</summary>
    public static void Record(string requestId)
    {
        var target = Environment.GetEnvironmentVariable("CODEGRAPH_TEST_DISCOVERY_PROBE");
        if (target is null) return;
        var bytes = JsonSerializer.SerializeToUtf8Bytes(new { schema_version = 1,
            helper_id = "codegraph-csharp-engine", request_id = requestId,
            pid = Environment.ProcessId, milestone = "source-text-read" });
        if (bytes.Length > 4096) throw new IOException("Acceptance observation exceeds limit");
        var pending = Path.ChangeExtension(target, ".pending");
        // 文件的 CreateNew 同时排除已有目录/链接；Move(overwrite:false) 拒绝已有目标。
        using (var file = new FileStream(pending, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            file.Write(bytes);
        try { File.Move(pending, target, overwrite: false); }
        finally { File.Delete(pending); }
    }
}
