package dev.codegraph.java;

import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.List;

/** Observe the still-running analyzer before deletion; Unix unlink success alone cannot prove released handles. */
public final class GradleResourcesTest {
    public static void main(String[] args) throws Exception {
        if (args[1].startsWith("analysis")) {
            var input = new AnalysisInput("binary-resource", Path.of(args[0]).toRealPath(), null, false,
                120_000, Main.MAX_BYTES);
            var analyzer = new Analyzer(input, SdkDiscovery::discover);
            var modelField = Analyzer.class.getDeclaredField("model"); modelField.setAccessible(true);
            var snapshotField = ProjectModel.class.getDeclaredField("snapshot"); snapshotField.setAccessible(true);
            var snapshot = (SourceSnapshot) snapshotField.get(modelField.get(analyzer));
            Path container = snapshot.root().getParent();
            try {
                var graph = analyzer.run().getAsJsonObject("graph");
                if (args[1].equals("analysis-failure")) throw new AssertionError("expected graph extraction failure");
                if (!graph.toString().contains("java:outside.External#value()"))
                    throw new AssertionError("external binary was not resolved");
            } catch (Main.Failure failure) {
                if (!args[1].equals("analysis-failure") || !failure.code.equals("object_extraction_failed") ||
                        !failure.getMessage().equals("package does not match source root")) throw failure;
            }
            assertNoSnapshotHandles("/codegraph-java-input-");
            if (Files.exists(container)) throw new AssertionError("snapshot survived graph completion");
            System.out.println("Snapshot deleted before analyzer exit");
            return;
        }
        long deadline = System.nanoTime() + 120_000_000_000L;
        ProjectModel.Checkpoint check = () -> {
            if (System.nanoTime() > deadline) throw new Main.Failure("resource_limit", "analysis timeout");
        };
        Path container;
        try (var snapshot = SourceSnapshot.capture(Path.of(args[0]).toRealPath(), List.of(), check)) {
            container = snapshot.root().getParent();
            boolean failed = false;
            try { GradleProjectImporter.load(snapshot, SdkDiscovery.discover(), check); }
            catch (Main.Failure failure) {
                if (!failure.code.equals("unsupported_configuration")) throw failure;
                failed = true;
            }
            if (failed != Boolean.parseBoolean(args[1])) throw new AssertionError("unexpected model outcome");
            if (System.getProperty("os.name").equals("Mac OS X")) {
                var process = new ProcessBuilder("/usr/sbin/lsof", "-p", Long.toString(ProcessHandle.current().pid()), "-Fn")
                    .redirectError(ProcessBuilder.Redirect.DISCARD).start();
                String listing = new String(process.getInputStream().readAllBytes(), StandardCharsets.UTF_8);
                if (process.waitFor() != 0) throw new AssertionError("handle observer failed");
                long owned = listing.lines().filter(line -> line.contains(container.toString())).count();
                if (owned != 0) throw new AssertionError("owned snapshot handles remain: " + owned);
                System.out.println("Owned snapshot handles before deletion: zero");
            }
        }
        if (Files.exists(container)) throw new AssertionError("snapshot survived product cleanup");
        System.out.println("Snapshot deleted before analyzer exit");
    }

    static void assertNoSnapshotHandles(String ownedPath) throws Exception {
        if (System.getProperty("os.name").equals("Mac OS X")) {
            var process = new ProcessBuilder("/usr/sbin/lsof", "-p", Long.toString(ProcessHandle.current().pid()), "-Fn")
                .redirectError(ProcessBuilder.Redirect.DISCARD).start();
            String listing = new String(process.getInputStream().readAllBytes(), StandardCharsets.UTF_8);
            if (process.waitFor() != 0) throw new AssertionError("handle observer failed");
            var owned = listing.lines().filter(line -> line.contains(ownedPath)).toList();
            if (!owned.isEmpty()) throw new AssertionError("analysis snapshot handles remain: " + owned);
            System.out.println("Analysis snapshot handles after graph completion: zero");
        }
    }
}
