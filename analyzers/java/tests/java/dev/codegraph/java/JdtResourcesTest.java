package dev.codegraph.java;

import java.nio.file.*;
import java.util.*;
import java.util.zip.ZipFile;
import org.eclipse.core.runtime.NullProgressMonitor;
import org.eclipse.core.runtime.OperationCanceledException;
import org.eclipse.jdt.core.JavaCore;
import org.eclipse.jdt.core.dom.*;
import org.eclipse.jdt.internal.compiler.batch.ClasspathJar;

/** Real binary handles across parser success, cancellation before the first AST, and callback failure. */
public final class JdtResourcesTest {
    public static void main(String[] args) throws Exception {
        for (String stage : List.of("success", "before-ast", "callback")) {
            Path container;
            try (var snapshot = SourceSnapshot.capture(Path.of(args[0]).toRealPath(), List.of(), () -> {})) {
                container = snapshot.root().getParent();
                String binary = snapshot.freezeBinary(snapshot.root().resolve("external.jar").toString());
                boolean[] delivered = { false }, opened = { false };
                try (var resources = new JdtResources()) {
                    ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
                    Map<String, String> options = new HashMap<>();
                    JavaCore.setComplianceOptions("17", options);
                    parser.setCompilerOptions(options);
                    parser.setResolveBindings(true);
                    parser.setEnvironment(new String[] { Path.of(args[1]).resolve("lib/jrt-fs.jar").toString(), binary },
                        new String[0], new String[0], false);
                    resources.attach(parser);
                    try {
                        parser.createASTs(new String[] { snapshot.root().resolve("src/main/java/p/A.java").toString() },
                            new String[] { "UTF-8" }, new String[0], new FileASTRequestor() {
                                @Override public void acceptAST(String path, CompilationUnit unit) {
                                    delivered[0] = true;
                                    if (stage.equals("callback")) throw new IllegalStateException("fixture callback failure");
                                }
                            }, new NullProgressMonitor() {
                                @Override public boolean isCanceled() {
                                    if (!stage.equals("before-ast")) return false;
                                    opened[0] |= binaryOpen(resources, binary);
                                    return opened[0];
                                }
                            });
                        if (!stage.equals("success")) throw new AssertionError("expected parser failure: " + stage);
                    } catch (OperationCanceledException expected) {
                        if (!stage.equals("before-ast") || delivered[0] || !opened[0]) throw new AssertionError("wrong cancellation boundary");
                    } catch (IllegalStateException expected) {
                        if (!stage.equals("callback") || !delivered[0] || !expected.getMessage().equals("fixture callback failure")) throw expected;
                    }
                }
                GradleResourcesTest.assertNoSnapshotHandles(container.toString());
            }
            if (Files.exists(container)) throw new AssertionError("snapshot survived parser completion");
            System.out.println("Parser " + stage + ": snapshot deleted before analyzer exit");
        }
        ASTParser incompatible = ASTParser.newParser(AST.getJLSLatest());
        var field = ASTParser.class.getDeclaredField("unitResolver");
        field.setAccessible(true); field.set(incompatible, null);
        try (var resources = new JdtResources()) {
            try { resources.attach(incompatible); throw new AssertionError("incompatible hook accepted"); }
            catch (Main.Failure failure) {
                if (!failure.code.equals("internal_failure") || failure.getCause() == null) throw failure;
            }
        }
        System.out.println("Incompatible parser hook fails closed before parse");
    }

    private static boolean binaryOpen(JdtResources resources, String binary) {
        try {
            var field = JdtResources.class.getDeclaredField("classpaths"); field.setAccessible(true);
            var zipField = ClasspathJar.class.getDeclaredField("zipFile"); zipField.setAccessible(true);
            for (Object path : (Set<?>) field.get(resources)) if (path instanceof ClasspathJar) {
                ZipFile zip = (ZipFile) zipField.get(path);
                if (zip != null && zip.getName().equals(binary) && zip.size() > 0) return true;
            }
            return false;
        } catch (ReflectiveOperationException failure) { throw new AssertionError(failure); }
    }
}
