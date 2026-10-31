package dev.codegraph.java;

import java.nio.file.*;
import java.util.List;
import java.util.concurrent.*;

/** Archive-derived inventory, exact cache membership and atomic publication under concurrent requests. */
public final class GradleDistributionCacheTest {
    public static void main(String[] args) throws Exception {
        Path workspace = Path.of(args[0]);
        Path repository = Files.createDirectory(workspace.resolve("repository"));
        Path extracted = Files.createDirectories(workspace.resolve("extracted/gradle-9.1.0/lib"));
        Files.writeString(extracted.resolve("tool.jar"), "verified archive bytes");
        Path source = workspace.resolve("extracted");
        try (var snapshot = SourceSnapshot.capture(repository, List.of(), () -> {})) {
            var pool = Executors.newFixedThreadPool(2);
            try {
                var start = new CountDownLatch(1);
                Callable<GradleDistributionCache.Entry> publish = () -> {
                    start.await();
                    return GradleDistributionCache.publish(source, "a".repeat(64), snapshot, () -> {});
                };
                var one = pool.submit(publish); var two = pool.submit(publish); start.countDown();
                var first = one.get(20, TimeUnit.SECONDS); var second = two.get(20, TimeUnit.SECONDS);
                if (!first.root().equals(second.root())) throw new AssertionError("concurrent cache identity differs");
                first.verify(() -> {}); second.verify(() -> {});
                // Simulate termination after the transaction owner was written and only part of the archive copied.
                Path stale = first.root().getParent().resolve(".publish-" + "a".repeat(64));
                Files.createDirectories(stale.resolve("contents"));
                Files.writeString(stale.resolve("owner"), "codegraph-gradle-distribution/1\n" + "a".repeat(64) + "\n");
                Files.writeString(stale.resolve("contents/partial.jar"), "uncommitted partial archive");
                var recovered = GradleDistributionCache.publish(source, "a".repeat(64), snapshot, () -> {});
                recovered.verify(() -> {});
                if (Files.exists(stale)) throw new AssertionError("stale transaction survived locked recovery");
                Files.createDirectory(stale);
                Files.writeString(stale.resolve("owner"), "unrelated transaction");
                rejected(() -> GradleDistributionCache.publish(source, "a".repeat(64), snapshot, () -> {}));
                if (!Files.exists(stale.resolve("owner"))) throw new AssertionError("unproved transaction was deleted");
                Files.delete(stale.resolve("owner")); Files.delete(stale);
                Path binary = first.root().resolve("gradle-9.1.0/lib/tool.jar");
                Files.writeString(binary, "tampered cache bytes");
                rejected(() -> GradleDistributionCache.publish(source, "a".repeat(64), snapshot, () -> {}));
                Files.writeString(binary, "verified archive bytes");
                Path extra = first.root().resolve("extra.jar"); Files.writeString(extra, "unexpected");
                rejected(() -> first.verify(() -> {})); Files.delete(extra);
                if (!System.getProperty("os.name").startsWith("Windows")) {
                    Files.delete(binary); Files.createSymbolicLink(binary, extracted.resolve("tool.jar"));
                    rejected(() -> first.verify(() -> {})); Files.delete(binary);
                    Files.writeString(binary, "verified archive bytes");
                }
                first.verify(() -> {});
            } finally { pool.shutdownNow(); }
        }
        System.out.println("Cache concurrent publication, reuse, archive-anchored tamper and membership checks passed");
    }
    @FunctionalInterface interface Operation { void run() throws Exception; }
    private static void rejected(Operation operation) throws Exception {
        try { operation.run(); }
        catch (Main.Failure failure) {
            if (!failure.code.equals("object_extraction_failed")) throw failure;
            return;
        }
        throw new AssertionError("corrupt cache accepted");
    }
}
