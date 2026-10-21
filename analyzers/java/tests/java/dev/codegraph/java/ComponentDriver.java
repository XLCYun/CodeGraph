package dev.codegraph.java;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import java.nio.charset.StandardCharsets;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

/** 不随包交付的旧语义夹具驱动，仅调用同进程组件，不提供生产兼容协议。 */
public final class ComponentDriver {
    /** 把历史 fixture 的明确参数传给核心，并保留原始来源断言。 */
    public static void main(String[] args) throws Exception {
        JsonObject completed = new JsonObject();
        completed.addProperty("type", "completed");
        try {
            JsonObject fixture = Main.parseObject(new String(System.in.readAllBytes(), StandardCharsets.UTF_8), Main.MAX_BYTES);
            JsonObject input = Main.object(fixture, "input"), limits = Main.object(fixture, "limits");
            List<String> files = input.has("files") ? new ArrayList<>() : null;
            if (files != null) for (JsonElement file : Main.array(input, "files")) {
                if (!file.isJsonPrimitive() || !file.getAsJsonPrimitive().isString()) throw new Main.Failure("invalid_input", "files must be paths");
                files.add(file.getAsString());
            }
            AnalysisInput context = new AnalysisInput(Main.string(fixture, "id"), Path.of(Main.string(input, "repository_root")),
                files, input.get("include_tests").getAsBoolean(), limits.get("analysis_timeout_ms").getAsLong(), limits.get("max_result_bytes").getAsLong());
            List<SdkDiscovery.Candidate> targets = new ArrayList<>();
            for (JsonElement element : Main.array(Main.object(fixture, "environment"), "platforms")) {
                JsonObject target = element.getAsJsonObject();
                targets.add(new SdkDiscovery.Candidate(Main.string(target, "id"), Main.string(target, "version"),
                    Path.of(Main.string(target, "root")), Main.string(Main.object(target, "metadata"), "release")));
            }
            Analyzer analyzer = new Analyzer(context, () -> targets);
            JsonObject result = analyzer.run();
            if (result.toString().getBytes(StandardCharsets.UTF_8).length > analyzer.maxResultBytes())
                throw new Main.Failure("resource_limit", "result exceeds max_result_bytes");
            completed.addProperty("outcome", "ok");
            completed.add("graph", result.get("graph"));
            completed.add("evidence", result.get("evidence"));
        } catch (Main.Failure problem) {
            completed.addProperty("outcome", "error");
            JsonObject error = new JsonObject();
            error.addProperty("code", problem.code);
            error.addProperty("message", problem.getMessage());
            error.add("paths", Main.strings(problem.paths));
            completed.add("error", error);
        }
        byte[] output = (completed + "\n").getBytes(StandardCharsets.UTF_8);
        System.out.write(output, 0, output.length);
        System.out.flush();
    }
}
