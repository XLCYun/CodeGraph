package dev.codegraph.java;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.util.List;
import java.util.function.Consumer;

/** 真实 JDT 输出上的公共不变量反例，验证失败不会自动修改图。 */
public final class GraphChecksTest {
    /** 节点、来源、包含与引用分别被破坏时都必须拒绝。 */
    public static void main(String[] args) throws Exception {
        Path root = Files.createTempDirectory("codegraph Java 图校验-");
        try {
            Files.createDirectories(root.resolve("src"));
            Files.writeString(root.resolve("narrant-java.json"), "{\"schema\":\"java-static-v1\",\"release\":\"25\",\"source_roots\":[{\"path\":\"src\",\"role\":\"main\"}]}");
            Files.writeString(root.resolve("src/Main.java"), "class Main { void run(){target(); System.getProperty(\"key\");} void target(){} }");
            AnalysisInput input = new AnalysisInput("graph", root, List.of("src/Main.java"), false, 30000, Main.MAX_BYTES);
            ProjectModel model = ProjectModel.discover(input, SdkDiscovery::discover, () -> {});
            JsonObject result = new Analyzer(input, SdkDiscovery::discover).run();
            reject(result, model, value -> value.getAsJsonObject("graph").getAsJsonArray("nodes").add(value.getAsJsonObject("graph").getAsJsonArray("nodes").get(0).deepCopy()));
            reject(result, model, value -> value.getAsJsonObject("evidence").add("origins", new JsonArray()));
            reject(result, model, value -> value.getAsJsonObject("evidence").add("files", new JsonArray()));
            reject(result, model, value -> value.getAsJsonObject("evidence").getAsJsonArray("configurations")
                .get(0).getAsJsonObject().add("platform_ids", Main.strings(List.of("unapproved-jdk"))));
            reject(result, model, value -> value.getAsJsonObject("evidence").getAsJsonArray("configurations")
                .get(0).getAsJsonObject().add("source_paths", Main.strings(List.of("missing.json"))));
            reject(result, model, value -> value.getAsJsonObject("evidence").getAsJsonArray("files")
                .get(0).getAsJsonObject().addProperty("disposition", "unknown"));
            reject(result, model, value -> value.getAsJsonObject("evidence").getAsJsonArray("files")
                .get(0).getAsJsonObject().add("configuration_ids", Main.strings(List.of("missing-config"))));
            reject(result, model, value -> value.getAsJsonObject("graph").add("root_contains", new JsonArray()));
            reject(result, model, value -> value.getAsJsonObject("graph").getAsJsonArray("nodes").get(0).getAsJsonObject().addProperty("category", "unknown"));
            reject(result, model, value -> value.getAsJsonObject("graph").add("external_symbols", new JsonArray()));
            reject(result, model, value -> {
                JsonObject target = new JsonObject(); target.addProperty("kind", "node"); target.addProperty("id", "java:missing");
                JsonObject reference = new JsonObject(); reference.addProperty("semantic", "code.calls"); reference.add("target", target);
                value.getAsJsonObject("graph").getAsJsonArray("nodes").get(0).getAsJsonObject().getAsJsonArray("references").add(reference);
            });
            System.out.println("graph boundary assertions passed");
        } finally {
            try (var paths = Files.walk(root)) { for (Path path : paths.sorted(Comparator.reverseOrder()).toList()) Files.delete(path); }
        }
    }

    /** 反例由契约定义，校验前后字节相等证明没有自动修图。 */
    private static void reject(JsonObject baseline, ProjectModel model, Consumer<JsonObject> mutation) throws Exception {
        JsonObject value = baseline.deepCopy();
        mutation.accept(value);
        String before = value.toString();
        try { GraphChecks.validate(value, model); }
        catch (Main.Failure expected) {
            if (!value.toString().equals(before)) throw new AssertionError("validator repaired graph");
            return;
        }
        throw new AssertionError("broken graph accepted");
    }
}
