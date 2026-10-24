package dev.codegraph.java;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import static dev.codegraph.java.Main.Failure;

/** 只读发现本机已有 JDK；不执行 java、java_home、Maven 或项目命令。 */
final class SdkDiscovery {
    /** 进程内目标发现接口，组件测试可以提供明确的真实 JDK 夹具。 */
    @FunctionalInterface
    interface Provider { List<Candidate> discover() throws Failure; }

    /** 已发现版本与标准库布局共同证明的候选声明。 */
    record Candidate(String id, String version, Path root, String release) {}

    /** 汇总既有 SDK 提示、常见安装目录、PATH 与随包运行 JDK。 */
    static List<Candidate> discover() throws Failure {
        List<Path> roots = new ArrayList<>();
        for (String key : List.of("CODEGRAPH_JAVA_TARGET_JDK", "JAVA_HOME", "JDK_HOME")) addEnvironment(roots, key);
        List<Path> containers = new ArrayList<>(List.of(Path.of("/Library/Java/JavaVirtualMachines"), Path.of("/usr/lib/jvm")));
        String home = System.getenv(System.getProperty("os.name").startsWith("Windows") ? "USERPROFILE" : "HOME");
        if (home != null) {
            Path directory = Path.of(home);
            containers.addAll(List.of(directory.resolve("Library/Java/JavaVirtualMachines"),
                directory.resolve(".sdkman/candidates/java"), directory.resolve(".jdks")));
        }
        for (String key : List.of("ProgramFiles", "ProgramFiles(x86)")) {
            String value = System.getenv(key);
            if (value != null) for (String name : List.of("Java", "Eclipse Adoptium", "Microsoft", "Amazon Corretto", "BellSoft", "Zulu"))
                containers.add(Path.of(value).resolve(name));
        }
        for (Path container : containers) roots.addAll(directories(container));
        for (String prefix : List.of("/opt/homebrew/opt", "/usr/local/opt")) {
            for (Path entry : directories(Path.of(prefix))) {
                String name = entry.getFileName().toString();
                if (name.equals("openjdk") || name.startsWith("openjdk@")) roots.add(entry.resolve("libexec/openjdk.jdk"));
            }
        }
        String search = System.getenv("PATH");
        if (search != null) for (String directory : search.split(Pattern.quote(java.io.File.pathSeparator))) {
            try {
                Path candidate = Path.of(directory).resolve(System.getProperty("os.name").startsWith("Windows") ? "java.exe" : "java");
                if (candidate.isAbsolute()) roots.add(candidate.toRealPath().getParent().getParent());
            } catch (Exception ignored) { /* 缺失 PATH 候选不是安装或运行工具的理由。 */ }
        }
        roots.add(Path.of(System.getProperty("java.home")));
        return inRoots(roots);
    }

    /** 环境只作为既有路径提示；不接受相对路径或执行字符串。 */
    private static void addEnvironment(List<Path> roots, String key) {
        String value = System.getenv(key);
        if (value != null) try { roots.add(Path.of(value)); }
        catch (RuntimeException ignored) { /* 非法目录提示跳过。 */ }
    }

    /** 常见安装容器只读取一层，不扫描用户项目或执行探测命令。 */
    private static List<Path> directories(Path root) {
        try (var entries = Files.list(root)) { return entries.filter(Files::isDirectory).sorted().toList(); }
        catch (Exception ignored) { return List.of(); }
    }

    /** 每个 major 选择最高稳定版本，经典与模块化 JDK 均检查真实标准声明。 */
    static List<Candidate> inRoots(List<Path> roots) throws Failure {
        Map<Integer, Candidate> chosen = new TreeMap<>();
        Map<Integer, long[]> versions = new TreeMap<>();
        for (Path candidate : roots) {
            if (!candidate.isAbsolute()) continue;
            for (Path spelling : List.of(candidate, candidate.resolve("Contents/Home"))) {
                try {
                    Path root = spelling.toRealPath();
                    String contents = Files.readString(root.resolve("release"), StandardCharsets.UTF_8);
                    Matcher match = Pattern.compile("(?m)^JAVA_VERSION=\"([^\"\\r\\n]+)\"$").matcher(contents);
                    if (!match.find()) continue;
                    String version = match.group(1);
                    long[] parts = stableVersion(version);
                    if (parts == null) continue;
                    int major = Math.toIntExact(parts[0]);
                    boolean valid = major < 9 ? Files.isRegularFile(root.resolve("jre/lib/rt.jar")) :
                        Files.isRegularFile(root.resolve("lib/jrt-fs.jar")) && Files.isRegularFile(root.resolve("lib/modules"));
                    if (!valid) continue;
                    if (!chosen.containsKey(major) || Arrays.compare(parts, versions.get(major)) > 0) {
                        chosen.put(major, new Candidate("java" + major, version, root, Integer.toString(major)));
                        versions.put(major, parts);
                    }
                } catch (Exception ignored) { /* 候选声明不完整或无法读取时不冒充可用 SDK。 */ }
            }
        }
        if (chosen.isEmpty()) throw new Failure("target_unavailable", "no installed Java standard declarations; prepare the exact project JDK");
        return List.copyOf(chosen.values());
    }

    /** 兼容经典 1.8.0_update、现代数字版本与 LTS 后缀；EA 和 preview 不参与选择。 */
    static long[] stableVersion(String value) {
        try {
            String stable = value.endsWith("-LTS") ? value.substring(0, value.length() - 4) : value;
            String[] build = stable.split("\\+", -1);
            if (build.length > 2) return null;
            List<String> numbers = new ArrayList<>(List.of(build[0].split("[._]", -1)));
            if (!numbers.isEmpty() && numbers.getFirst().equals("1")) numbers.removeFirst();
            if (numbers.isEmpty() || numbers.size() > 4) return null;
            long[] result = new long[5];
            for (int index = 0; index < numbers.size(); index++) result[index] = number(numbers.get(index));
            if (result[0] == 0 || result[0] > Integer.MAX_VALUE) return null;
            if (build.length == 2) result[4] = number(build[1]);
            return result;
        } catch (RuntimeException ignored) { return null; }
    }

    /** 与原平台发现器相同，版本分量限定为无符号 32 位十进制。 */
    private static long number(String value) {
        if (!value.matches("[0-9]+")) throw new IllegalArgumentException("invalid version component");
        long number = Long.parseLong(value);
        if (number > 0xffff_ffffL) throw new IllegalArgumentException("version component overflow");
        return number;
    }
}
