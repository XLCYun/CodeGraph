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
            try (SourceSnapshot snapshot = SourceSnapshot.capture(root, List.of("A.java"), () -> {})) {
                Path generated=snapshot.root().resolve("build/generated/G.java");
                Files.createDirectories(generated.getParent()); Files.writeString(generated,"class G {}");
                snapshot.freezeGradleOutputs(List.of("build/generated")); snapshot.verify();
                if(snapshot.originalMember("build/generated/G.java")) throw new AssertionError("generated member selected");
                Files.writeString(generated,"class Changed {}");
                expectChanged(snapshot);
            }
            Path binary=Files.createTempFile("snapshot-binary-",".jar");
            try {
                for(boolean copied:List.of(false,true)) {
                    Files.write(binary,new byte[] {1,2,3});
                    try(SourceSnapshot snapshot=SourceSnapshot.capture(root,List.of("A.java"),()->{})) {
                        Path frozen=Path.of(snapshot.freezeBinary(binary.toString())); snapshot.verify();
                        Files.write(copied?frozen:binary,new byte[] {4}); expectChanged(snapshot);
                    }
                }
            } finally { Files.delete(binary); }
            Path tree=Files.createTempDirectory("snapshot-binary-tree-");
            try {
                Files.write(tree.resolve("A.class"),new byte[] {1});
                try(SourceSnapshot snapshot=SourceSnapshot.capture(root,List.of("A.java"),()->{})) {
                    snapshot.freezeBinary(tree.toString()); snapshot.verify();
                    Files.write(tree.resolve("Added.class"),new byte[] {2}); expectChanged(snapshot);
                }
            } finally { Files.deleteIfExists(tree.resolve("Added.class")); Files.delete(tree.resolve("A.class")); Files.delete(tree); }
        } finally { Files.deleteIfExists(root.resolve("A.java")); Files.delete(root); }
        System.out.println("snapshot mutation and cleanup PASS");
    }
    private static void expectChanged(SourceSnapshot snapshot) throws Exception {
        try { snapshot.verify(); throw new AssertionError("context mutation accepted"); }
        catch(Main.Failure expected) { if(!expected.code.equals("object_extraction_failed")) throw expected; }
    }
}
