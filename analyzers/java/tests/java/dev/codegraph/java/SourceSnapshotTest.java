package dev.codegraph.java;

import java.nio.file.*;
import java.util.List;

/** Mutation probes exercise the same snapshot boundary used by the public analyzer. */
public final class SourceSnapshotTest {
    public static void main(String[] args) throws Exception {
        Path root = Files.createTempDirectory("snapshot-test-").toRealPath();
        try {
            Files.writeString(root.resolve("A.java"), "class A {}\n");
            for (boolean original : List.of(false, true)) {
                Path copy;
                try (SourceSnapshot snapshot = SourceSnapshot.capture(root, List.of("A.java"), () -> {})) {
                    copy = snapshot.root();
                    Files.writeString((original ? root : copy).resolve("A.java"), "class Changed {}\n");
                    try { snapshot.verify(); throw new AssertionError("mutation accepted"); }
                    catch (Main.Failure expected) {
                        if (!expected.code.equals("object_extraction_failed")) throw expected;
                    }
                }
                if (Files.exists(copy)) throw new AssertionError("snapshot leaked");
                Files.writeString(root.resolve("A.java"), "class A {}\n");
            }
            try (SourceSnapshot snapshot = SourceSnapshot.capture(root, List.of("A.java"), () -> {})) {
                Files.writeString(snapshot.root().resolve("Added.java"), "class Added {}");
                try { snapshot.verify(); throw new AssertionError("added source accepted"); }
                catch (Main.Failure expected) {
                    if (!expected.code.equals("object_extraction_failed")) throw expected;
                }
            }
            for (boolean original : List.of(false, true)) for (boolean existed : List.of(false, true)) {
                Path directory = root.resolve("enable-dir");
                if (existed) Files.createDirectory(directory);
                try (SourceSnapshot snapshot = SourceSnapshot.capture(root, List.of("A.java"), () -> {})) {
                    if (Files.exists(snapshot.root().resolve("enable-dir")) != existed)
                        throw new AssertionError("directory condition not preserved");
                    Path changed = (original ? root : snapshot.root()).resolve("enable-dir");
                    if (existed) Files.delete(changed); else Files.createDirectory(changed);
                    try { snapshot.verify(); throw new AssertionError("directory mutation accepted"); }
                    catch (Main.Failure expected) {
                        if (!expected.code.equals("object_extraction_failed")) throw expected;
                    }
                } finally { Files.deleteIfExists(directory); }
            }
        } finally { Files.deleteIfExists(root.resolve("A.java")); Files.delete(root); }
        System.out.println("snapshot mutation and cleanup PASS");
    }
}
