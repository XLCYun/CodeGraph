package dev.codegraph.java;

import com.google.gson.JsonObject;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardOpenOption;
import static dev.codegraph.java.Main.Failure;

/** 独立验收观察只报告已发生的加载，不扩展协议或控制执行。 */
final class Observation {
    /** 实际静态配置解析后发布一次有界标记；原子硬链接不会覆盖旧文件或链接。 */
    static void record(String requestId) throws Failure {
        String target = System.getenv("CODEGRAPH_TEST_DISCOVERY_PROBE");
        if (target == null) return;
        JsonObject marker = new JsonObject();
        marker.addProperty("schema_version", 1);
        marker.addProperty("helper_id", "codegraph-java-engine");
        marker.addProperty("request_id", requestId);
        marker.addProperty("pid", ProcessHandle.current().pid());
        marker.addProperty("milestone", "project-configurations-loaded");
        byte[] bytes = marker.toString().getBytes(StandardCharsets.UTF_8);
        if (bytes.length > 4096) throw new Failure("resource_limit", "acceptance observation exceeds limit");
        Path destination = Path.of(target), pending = destination.resolveSibling("reached.pending");
        try {
            Files.createDirectory(pending);
            Path temporary = pending.resolve("marker.json");
            try {
                Files.write(temporary, bytes, StandardOpenOption.CREATE_NEW, StandardOpenOption.WRITE);
                Files.createLink(destination, temporary);
            } finally { Files.deleteIfExists(temporary); Files.delete(pending); }
        } catch (Exception error) { throw new Failure("internal_failure", "acceptance observation failed: " + error); }
    }
}
