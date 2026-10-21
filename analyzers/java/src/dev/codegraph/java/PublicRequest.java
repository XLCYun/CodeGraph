package dev.codegraph.java;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import static dev.codegraph.java.Main.Failure;

/** 已严格解码的公共请求；不携带 SDK、环境或内部消息。 */
record PublicRequest(String id, Path repository, List<String> files, boolean includeTests) {
    /** 接受同一公共字段集，递归重复键和尾随内容由严格解析器拒绝。 */
    static PublicRequest decode(String text) throws Failure {
        try {
            JsonObject message = Main.parseObject(text, Main.MAX_BYTES);
            if (message.has("protocol") && message.get("protocol").isJsonPrimitive()
                && message.get("protocol").getAsJsonPrimitive().isString()
                && !message.get("protocol").getAsString().equals(Main.PROTOCOL))
                throw new Failure("incompatible_protocol", "incompatible analyzer protocol");
            Set<String> names = new HashSet<>(Set.of("protocol", "type", "id", "repository_root", "options"));
            if (message.has("files")) names.add("files");
            Main.fields(message, names);
            Main.requireString(message, "protocol", Main.PROTOCOL);
            Main.requireString(message, "type", "analyze");
            String id = validString(message, "id");
            if (id.isBlank() || id.getBytes(StandardCharsets.UTF_8).length > 1024) throw new Failure("invalid_request", "invalid request id");
            Path root = Path.of(validString(message, "repository_root"));
            if (!root.isAbsolute()) throw new Failure("invalid_request", "repository_root must be absolute");
            JsonObject options = Main.object(message, "options");
            Main.fields(options, Set.of("include_tests", "rust_dependency_scope"));
            if (!options.get("include_tests").isJsonPrimitive() || !options.get("include_tests").getAsJsonPrimitive().isBoolean())
                throw new Failure("invalid_request", "include_tests must be boolean");
            if (!Set.of("repository-only", "standard-library", "all-dependencies").contains(Main.string(options, "rust_dependency_scope")))
                throw new Failure("invalid_request", "invalid rust_dependency_scope");
            List<String> files = null;
            if (message.has("files")) {
                files = new ArrayList<>();
                Set<String> unique = new HashSet<>();
                for (JsonElement element : Main.array(message, "files")) {
                    if (!element.isJsonPrimitive() || !element.getAsJsonPrimitive().isString()) throw new Failure("invalid_request", "files must be strings");
                    String file = element.getAsString();
                    validText(file);
                    ProjectModel.validateRelative(file);
                    if (!unique.add(file)) throw new Failure("invalid_request", "duplicate selected path");
                    files.add(file);
                    if (files.size() > 100000) throw new Failure("invalid_request", "selection exceeds 100000 files");
                }
                files = List.copyOf(files);
            }
            return new PublicRequest(id, root, files, options.get("include_tests").getAsBoolean());
        } catch (Failure problem) {
            if (problem.code.equals("incompatible_protocol")) throw problem;
            throw new Failure("invalid_request", problem.getMessage());
        } catch (RuntimeException problem) { throw new Failure("invalid_request", "invalid analyzer request"); }
    }

    /** 字符串必须可无损编码为 UTF-8，并拒绝 NUL。 */
    private static String validString(JsonObject object, String key) throws Failure {
        String value = Main.string(object, key);
        validText(value);
        return value;
    }

    /** 验证代理项配对，避免身份或路径在编码时被替换。 */
    private static void validText(String value) throws Failure {
        if (value.isEmpty() || value.indexOf('\0') >= 0) throw new Failure("invalid_request", "invalid string");
        for (int index = 0; index < value.length(); index++) {
            char current = value.charAt(index);
            if (Character.isHighSurrogate(current)) {
                if (++index >= value.length() || !Character.isLowSurrogate(value.charAt(index))) throw new Failure("invalid_request", "invalid Unicode string");
            } else if (Character.isLowSurrogate(current)) throw new Failure("invalid_request", "invalid Unicode string");
        }
    }

    /** 验证真实根与选集边界；安装目录和源码目录不能相互包含。 */
    void validateSource() throws Failure {
        try {
            Path root = repository.toRealPath();
            Path classes = Path.of(Main.class.getProtectionDomain().getCodeSource().getLocation().toURI()).toRealPath();
            Path installation = classes.getParent().getParent().getParent();
            if (!Files.isDirectory(root) || root.startsWith(installation) || installation.startsWith(root)) throw new Failure("invalid_input", "source overlaps analyzer installation");
            Set<Path> selected = new HashSet<>();
            for (String file : files == null ? List.<String>of() : files) {
                Path actual = root.resolve(file).toRealPath();
                if (!actual.startsWith(root) || !Files.isRegularFile(actual) || !selected.add(actual)) throw new Failure("invalid_input", "selected source outside repository");
            }
        } catch (Failure problem) { throw problem; }
        catch (Exception problem) { throw new Failure("invalid_input", "repository or selected source is unavailable"); }
    }

    /** 向进程内核心传递必要参数，不再构造私有协议 JSON。 */
    AnalysisInput analysisInput() { return new AnalysisInput(id, repository, files, includeTests, 1_800_000, Main.MAX_BYTES); }
}

/** 进程内分析参数；额外资源限制仅供已有组件测试使用。 */
record AnalysisInput(String id, Path repository, List<String> files, boolean includeTests, long timeoutMillis, long maxResultBytes) {}
