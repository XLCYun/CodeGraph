package dev.codegraph.java;

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.util.List;

/** 以临时标准库布局独立验证 SDK 发现，不启动项目工具。 */
public final class SdkDiscoveryTest {
    /** 检查最高稳定版本、经典布局、macOS bundle 和缺失声明。 */
    public static void main(String[] args) throws Exception {
        Path root = Files.createTempDirectory("codegraph JDK 发现-");
        try {
            Path old = installation(root.resolve("old"), "17.0.1", false);
            Path modern = installation(root.resolve("modern"), "17.0.20.1", false);
            Path classic = installation(root.resolve("classic"), "1.8.0_504", true);
            Path bundle = installation(root.resolve("custom.jdk/Contents/Home"), "42.0.2", false);
            Path ea = installation(root.resolve("ea"), "17.0.99-ea", false);
            Path incomplete = installation(root.resolve("incomplete"), "99.0.1", false);
            Files.delete(incomplete.resolve("lib/modules"));
            var found = SdkDiscovery.inRoots(List.of(old, classic, modern, bundle.getParent().getParent(), ea, incomplete, Path.of("relative")));
            require(found.size() == 3, "expected three stable majors");
            require(found.get(0).root().equals(classic.toRealPath()), "classic JDK source");
            require(found.get(1).root().equals(modern.toRealPath()), "highest stable patch");
            require(found.get(2).root().equals(bundle.toRealPath()), "macOS bundle source");
            for (String value : List.of("17-ea", "25-preview", "1.8.0_504-b01", "0", "17..1", "17+", "17+4294967296"))
                require(SdkDiscovery.stableVersion(value) == null, "unstable version accepted: " + value);
            require(SdkDiscovery.stableVersion("25.0.4.1+1")[4] == 1, "build version");
            require(SdkDiscovery.stableVersion("17.0.1-LTS")[0] == 17, "LTS version");
            System.out.println("SDK discovery assertions passed");
        } finally {
            try (var paths = Files.walk(root)) { for (Path path : paths.sorted(Comparator.reverseOrder()).toList()) Files.delete(path); }
        }
    }

    /** 创建只供发现器读取的 release 与标准声明存在性夹具。 */
    private static Path installation(Path root, String version, boolean classic) throws Exception {
        Files.createDirectories(root);
        Files.writeString(root.resolve("release"), "JAVA_VERSION=\"" + version + "\"\n");
        for (String file : classic ? List.of("jre/lib/rt.jar") : List.of("lib/jrt-fs.jar", "lib/modules")) {
            Files.createDirectories(root.resolve(file).getParent());
            Files.write(root.resolve(file), new byte[0]);
        }
        return root;
    }

    /** 用明确断言失败代替依赖 JVM -ea 开关的 assert。 */
    private static void require(boolean condition, String message) { if (!condition) throw new AssertionError(message); }
}
