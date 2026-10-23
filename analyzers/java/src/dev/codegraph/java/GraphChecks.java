package dev.codegraph.java;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import java.util.ArrayDeque;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Map;
import java.util.Set;
import static dev.codegraph.java.Main.Failure;

/** 自然构图输出边界的一次校验；私有来源用于校验但不进入公共结果。 */
final class GraphChecks {
    /** 验证覆盖、来源、六类节点、关系目标和根可达性，不修补失败结果。 */
    static void validate(JsonObject result, ProjectModel model) throws Failure {
        JsonObject graph = Main.object(result, "graph"), evidence = Main.object(result, "evidence");
        model.validateEvidence(evidence);
        Map<String, JsonObject> files = indexed(Main.array(evidence, "files"), "path");
        Map<String, JsonObject> nodes = indexed(Main.array(graph, "nodes"), "id");
        Map<String, JsonObject> origins = indexed(Main.array(evidence, "origins"), "node_id");
        if (!nodes.keySet().equals(origins.keySet())) fail("nodes and origins differ");
        for (JsonObject origin : origins.values()) {
            Set<String> paths = strings(Main.array(origin, "selected_paths"));
            if (paths.isEmpty()) fail("missing selected origin");
            for (String path : paths) if (!files.containsKey(path) || !Main.string(files.get(path), "disposition").equals("analyzed")) fail("node origin is not analyzed selection");
        }
        Set<String> parents = new HashSet<>(), external = new HashSet<>();
        JsonArray roots = Main.array(graph, "root_contains");
        children(roots, nodes, parents);
        for (var entry : nodes.entrySet()) {
            JsonObject node = entry.getValue();
            if (!Set.of("container", "type", "callable", "value", "execution_region", "extension").contains(Main.string(node, "category"))
                || !Main.string(node, "language").equals("java") || !entry.getKey().startsWith("java:") || entry.getKey().length() <= 5
                || Main.string(node, "native_kind").isEmpty()) fail("invalid node identity or category");
            for (String key : Set.of("name", "signature", "code", "source")) {
                JsonElement value = node.get(key);
                if (value == null || !value.isJsonNull() && (!value.isJsonPrimitive() || !value.getAsJsonPrimitive().isString())) fail("missing nullable self content");
            }
            for (String key : Main.object(node, "extensions").keySet()) if (!namespaced(key, '.')) fail("invalid extension namespace");
            children(Main.array(node, "contains"), nodes, parents);
            Set<String> facts = new HashSet<>();
            for (JsonElement element : Main.array(node, "references")) {
                JsonObject reference = element.getAsJsonObject();
                if (!namespaced(Main.string(reference, "semantic"), '.') || !facts.add(reference.toString())) fail("invalid or duplicate reference");
                JsonObject target = Main.object(reference, "target");
                String kind = Main.string(target, "kind");
                switch (kind) {
                    case "node" -> {
                        Main.fields(target, Set.of("kind", "id"));
                        if (!nodes.containsKey(Main.string(target, "id"))) fail("missing local target");
                    }
                    case "external" -> {
                        Main.fields(target, Set.of("kind", "symbol"));
                        String symbol = Main.string(target, "symbol");
                        if (!namespaced(symbol, ':') || nodes.containsKey(symbol)) fail("invalid external target");
                        external.add(symbol);
                    }
                    case "unresolved" -> {
                        Main.fields(target, Set.of("kind", "name"));
                        if (Main.string(target, "name").isEmpty()) fail("empty unresolved target");
                    }
                    default -> fail("unknown target kind");
                }
            }
        }
        Set<String> reachable = new HashSet<>();
        ArrayDeque<String> pending = new ArrayDeque<>(strings(roots));
        while (!pending.isEmpty()) {
            String id = pending.removeLast();
            if (!reachable.add(id)) fail("containment cycle");
            for (JsonElement child : Main.array(nodes.get(id), "contains")) pending.add(child.getAsString());
        }
        if (!reachable.equals(nodes.keySet())) fail("node is not reachable from root");
        Map<String, JsonObject> descriptors = indexed(Main.array(graph, "external_symbols"), "symbol");
        if (!descriptors.keySet().equals(external)) fail("external descriptions differ from references");
        for (JsonObject symbol : descriptors.values()) if (Main.string(symbol, "name").isEmpty()
            || !Set.of("repository", "dependency").contains(Main.string(symbol, "scope"))) fail("invalid external description");
    }

    /** 对数组建立唯一索引，重复身份不能被 Map 覆盖。 */
    static Map<String, JsonObject> indexed(JsonArray values, String field) throws Failure {
        Map<String, JsonObject> result = new HashMap<>();
        for (JsonElement element : values) {
            if (!element.isJsonObject()) { fail("expected graph object"); }
            JsonObject object = element.getAsJsonObject();
            String id = Main.string(object, field);
            if (id.isEmpty() || id.indexOf('\0') >= 0 || result.put(id, object) != null) fail("empty or duplicate graph identity");
        }
        return result;
    }

    /** 字符串数组必须唯一，并且所有值均非空。 */
    static Set<String> strings(JsonArray values) throws Failure {
        Set<String> result = new HashSet<>();
        for (JsonElement value : values) {
            if (!value.isJsonPrimitive() || !value.getAsJsonPrimitive().isString()) fail("expected string array");
            String text = value.getAsString();
            if (text.isEmpty() || text.indexOf('\0') >= 0 || !result.add(text)) fail("empty or duplicate graph string");
        }
        return result;
    }

    /** 所有根及父节点共享一个声明位置约束。 */
    private static void children(JsonArray children, Map<String, JsonObject> nodes, Set<String> parents) throws Failure {
        for (String child : strings(children)) if (!nodes.containsKey(child) || !parents.add(child)) fail("missing child or multiple parents");
    }

    /** 命名空间与语义名两端都非空，并拒绝 NUL。 */
    private static boolean namespaced(String value, char separator) {
        int at = value.indexOf(separator);
        return at > 0 && at < value.length() - 1 && value.indexOf('\0') < 0;
    }

    /** 统一把内部构图不变量失败标为分析失败。 */
    static void fail(String message) throws Failure { throw new Failure("analysis_failed", message); }
}
