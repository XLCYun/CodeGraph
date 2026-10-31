package dev.codegraph.java;

import java.nio.file.*;
import java.util.List;
import org.eclipse.jdt.core.dom.*;
import org.eclipse.core.runtime.NullProgressMonitor;

/** The post-import parser callback cannot pair a fixed AST with changed source bytes. */
public final class ParsingSnapshotTest {
    public static void main(String[] args) throws Exception {
        Path root = Files.createTempDirectory("java-parse-snapshot-").toRealPath();
        try {
            Files.writeString(root.resolve("A.java"), "class A { void helper(){} void run(){helper();} }");
            AnalysisInput input = new AnalysisInput("parse", root, List.of("A.java"), false, 30000, Main.MAX_BYTES);
            try (ProjectModel model = ProjectModel.discover(input, SdkDiscovery::discover, () -> {})) {
                model.verifyInputs();
                boolean[] checked = {false};
                model.forEachActiveConfiguration(config -> {
                    try {
                        config.parse(new FileASTRequestor() {
                            @Override public void acceptAST(String path, CompilationUnit unit) {
                                try {
                                    String frozen = model.readSource("A.java");
                                    if (!frozen.contains("helper")) throw new AssertionError("original bytes missing");
                                    Files.writeString(Path.of(path), "class Changed {}");
                                    try { model.readSource("A.java"); throw new AssertionError("changed callback bytes accepted"); }
                                    catch (Main.Failure expected) {
                                        if (!expected.code.equals("object_extraction_failed")) throw expected;
                                    }
                                    checked[0] = true;
                                } catch (Exception failure) { throw new RuntimeException(failure); }
                            }
                        }, new NullProgressMonitor());
                    } catch (java.io.IOException failure) { throw new RuntimeException(failure); }
                });
                if (!checked[0]) throw new AssertionError("JDT callback not exercised");
                try { model.verifyInputs(); throw new AssertionError("post-parse mutation accepted"); }
                catch (Main.Failure expected) {
                    if (!expected.code.equals("object_extraction_failed")) throw expected;
                }
                if (!Files.readString(root.resolve("A.java")).contains("helper")) throw new AssertionError("original modified");
            }
        } finally { Files.deleteIfExists(root.resolve("A.java")); Files.delete(root); }
        System.out.println("post-import and post-parse frozen source PASS");
    }
}
