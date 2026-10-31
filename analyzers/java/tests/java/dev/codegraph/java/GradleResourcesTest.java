package dev.codegraph.java;

import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.List;

/** Observe the still-running analyzer before deletion; Unix unlink success alone cannot prove released handles. */
public final class GradleResourcesTest {
    public static void main(String[] args) throws Exception {
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
}
