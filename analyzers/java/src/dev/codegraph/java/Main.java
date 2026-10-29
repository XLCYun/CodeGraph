package dev.codegraph.java;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonNull;
import com.google.gson.JsonObject;
import com.google.gson.JsonPrimitive;
import com.google.gson.Strictness;
import com.google.gson.stream.JsonReader;
import com.google.gson.stream.JsonToken;
import java.io.ByteArrayOutputStream;
import java.io.StringReader;
import java.math.BigDecimal;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.util.List;
import java.util.Set;

/** Java 公开分析程序；主线程直接处理一次请求，不建立控制线程或分析 worker。 */
public final class Main {
    static final String PROTOCOL = "codegraph.analyzer/1";
    static final String PROFILE = "java-static-v1";
    static final int MAX_BYTES = 128 * 1024 * 1024;

    /** 防止工具类被实例化。 */
    private Main() {}

    /** CLI、完整输入和唯一最终输出由同一语言程序实现。 */
    public static void main(String[] args) {
        try {
            String text = command(args);
            if (text != null) { System.out.print(text); System.out.flush(); return; }
        } catch (IllegalArgumentException problem) {
            System.err.println(problem.getMessage());
            System.exit(2);
            return;
        }
        PublicRequest request = null;
        JsonObject completed;
        int status = 0;
        try {
            request = PublicRequest.decode(readInput());
            request.validateSource();
            JsonObject graph;
            if (request.files() != null && request.files().isEmpty()) graph = emptyGraph();
            else graph = new Analyzer(request.analysisInput(), SdkDiscovery::discover).run().getAsJsonObject("graph");
            completed = base(request.id(), "ok");
            completed.add("graph", graph);
            if (completed.toString().getBytes(StandardCharsets.UTF_8).length + 1 > MAX_BYTES)
                throw new Failure("resource_limit", "result exceeds 128 MiB");
        } catch (Throwable problem) {
            status = 1;
            completed = base(request == null ? null : request.id(), "error");
            JsonObject error = new JsonObject();
            String code = problem instanceof Failure failure ? failure.code : "internal_failure";
            if (!Set.of("invalid_request", "incompatible_protocol", "invalid_input", "invalid_pack", "resource_limit",
                        "timeout", "io_failure", "cleanup_failed", "internal_failure").contains(code)) code = "analysis_failed";
            error.addProperty("code", code);
            String message = String.valueOf(problem.getMessage());
            error.addProperty("message", message.substring(0, Math.min(message.length(), 4096)));
            completed.add("error", error);
        }
        // PrintStream 默认吞掉管道异常；checkError 保证输出失败不能以成功退出。
        byte[] output = (completed + "\n").getBytes(StandardCharsets.UTF_8);
        System.out.write(output, 0, output.length);
        System.out.flush();
        if (System.out.checkError()) status = 1;
        if (status != 0) System.exit(status);
    }

    /** 固定语言入口只接受 analyze 或一次帮助、版本查询。 */
    private static String command(String[] args) {
        if (args.length == 1 && args[0].equals("--version")) return "codegraph-java 0.1.2\n";
        if (args.length == 1 && args[0].equals("--help") || args.length == 2 && args[0].equals("analyze") && args[1].equals("--help"))
            return "Usage: codegraph-java analyze\nOne UTF-8 JSON request on stdin followed by EOF.\n";
        if (args.length == 1 && args[0].equals("analyze")) return null;
        throw new IllegalArgumentException("Usage: codegraph-java analyze");
    }

    /** 有界读取全部 UTF-8 输入；EOF 是正常完成，不承担取消语义。 */
    private static String readInput() throws Exception {
        ByteArrayOutputStream bytes = new ByteArrayOutputStream();
        byte[] chunk = new byte[8192];
        int size;
        while ((size = System.in.read(chunk)) != -1) {
            if (bytes.size() + size > MAX_BYTES) throw new Failure("resource_limit", "request exceeds 128 MiB");
            bytes.write(chunk, 0, size);
        }
        try { return StandardCharsets.UTF_8.newDecoder().decode(ByteBuffer.wrap(bytes.toByteArray())).toString(); }
        catch (java.nio.charset.CharacterCodingException problem) { throw new Failure("invalid_request", "request is not UTF-8"); }
    }

    /** 每个最终结果包含编译期身份，任何请求字段都不能覆盖它。 */
    private static JsonObject base(String id, String outcome) {
        JsonObject object = new JsonObject();
        object.addProperty("protocol", PROTOCOL);
        object.addProperty("engine_version", "0.1.2");
        object.addProperty("pack_id", "java");
        object.addProperty("type", "completed");
        object.add("id", id == null ? JsonNull.INSTANCE : new JsonPrimitive(id));
        object.addProperty("outcome", outcome);
        return object;
    }

    /** 明确空集的图不需要读取配置或发现目标 JDK。 */
    static JsonObject emptyGraph() {
        JsonObject graph = new JsonObject();
        graph.add("root_contains", new JsonArray());
        graph.add("nodes", new JsonArray());
        graph.add("external_symbols", new JsonArray());
        return graph;
    }

    /** 读取有限 JSON 对象并拒绝重复键。 */
    static JsonObject parseObject(String line) throws Failure {
        return parseObject(line, 1_048_576);
    }

    /** 请求和控制配置共用严格解析器，各自保留字节读取边界。 */
    static JsonObject parseObject(String line, int limit) throws Failure {
        if (line.length() > limit) throw new Failure("resource_limit", "JSON input too large");
        try {
            JsonReader reader = new JsonReader(new StringReader(line));
            reader.setStrictness(Strictness.STRICT);
            JsonElement parsed = readJson(reader, 0);
            if (reader.peek() != JsonToken.END_DOCUMENT) throw new IllegalArgumentException("trailing JSON");
            if (!parsed.isJsonObject()) throw new IllegalArgumentException("expected JSON object");
            return parsed.getAsJsonObject();
        } catch (Exception problem) {
            throw new Failure("invalid_input", "invalid JSON frame");
        }
    }

    /** 递归读取 JSON 并在每层拒绝重复键与过深输入。 */
    private static JsonElement readJson(JsonReader reader, int depth) throws Exception {
        if (depth > 64) throw new IllegalArgumentException("JSON nesting too deep");
        return switch (reader.peek()) {
            case BEGIN_OBJECT -> {
                JsonObject object = new JsonObject();
                reader.beginObject();
                while (reader.hasNext()) {
                    String key = reader.nextName();
                    if (object.has(key)) throw new IllegalArgumentException("duplicate JSON key");
                    object.add(key, readJson(reader, depth + 1));
                }
                reader.endObject();
                yield object;
            }
            case BEGIN_ARRAY -> {
                JsonArray array = new JsonArray();
                reader.beginArray();
                while (reader.hasNext()) array.add(readJson(reader, depth + 1));
                reader.endArray();
                yield array;
            }
            case STRING -> new JsonPrimitive(reader.nextString());
            case NUMBER -> new JsonPrimitive(new BigDecimal(reader.nextString()));
            case BOOLEAN -> new JsonPrimitive(reader.nextBoolean());
            case NULL -> { reader.nextNull(); yield JsonNull.INSTANCE; }
            default -> throw new IllegalArgumentException("invalid JSON token");
        };
    }

    /** 验证对象字段闭集。 */
    static void fields(JsonObject object, Set<String> allowed) throws Failure {
        for (String key : object.keySet()) if (!allowed.contains(key))
            throw new Failure("invalid_input", "unknown field: " + key);
        for (String key : allowed) if (!object.has(key))
            throw new Failure("invalid_input", "missing field: " + key);
    }

    /** 提取必填字符串。 */
    static String string(JsonObject object, String key) throws Failure {
        if (!object.has(key) || !object.get(key).isJsonPrimitive() || !object.get(key).getAsJsonPrimitive().isString())
            throw new Failure("invalid_input", "expected string: " + key);
        return object.get(key).getAsString();
    }

    /** 验证固定值字符串。 */
    static void requireString(JsonObject object, String key, String expected) throws Failure {
        if (!expected.equals(string(object, key)))
            throw new Failure("invalid_input", "unsupported " + key);
    }

    /** 提取必填对象。 */
    static JsonObject object(JsonObject parent, String key) throws Failure {
        if (!parent.has(key) || !parent.get(key).isJsonObject())
            throw new Failure("invalid_input", "expected object: " + key);
        return parent.getAsJsonObject(key);
    }

    /** 提取必填数组。 */
    static JsonArray array(JsonObject parent, String key) throws Failure {
        if (!parent.has(key) || !parent.get(key).isJsonArray())
            throw new Failure("invalid_input", "expected array: " + key);
        return parent.getAsJsonArray(key);
    }

    /** 将字符串集合编码为协议数组。 */
    static JsonArray strings(Iterable<String> values) {
        JsonArray result = new JsonArray();
        for (String value : values) result.add(value);
        return result;
    }

    /** 受控失败及其仓内相对来源；公共输出只保留代码与说明。 */
    static final class Failure extends Exception {
        final String code;
        final List<String> paths;

        /** 建立无路径错误。 */
        Failure(String code, String message) { this(code, message, List.of()); }

        /** 建立有路径错误。 */
        Failure(String code, String message, List<String> paths) {
            super(message);
            this.code = code;
            this.paths = paths;
        }
    }
}
