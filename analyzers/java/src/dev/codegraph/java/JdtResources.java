package dev.codegraph.java;

import java.util.Collections;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import org.eclipse.core.runtime.IProgressMonitor;
import org.eclipse.jdt.core.ICompilationUnit;
import org.eclipse.jdt.core.IJavaProject;
import org.eclipse.jdt.core.WorkingCopyOwner;
import org.eclipse.jdt.core.dom.ASTParser;
import org.eclipse.jdt.core.dom.ASTRequestor;
import org.eclipse.jdt.core.dom.CompilationUnit;
import org.eclipse.jdt.core.dom.FileASTRequestor;
import org.eclipse.jdt.internal.compiler.batch.FileSystem.Classpath;
import org.eclipse.jdt.internal.core.dom.ICompilationUnitResolver;
import static dev.codegraph.java.Main.Failure;

/** Owns JDT 3.47.0's batch classpaths until all graph binding work has finished. */
final class JdtResources implements AutoCloseable {
    private final Set<Classpath> classpaths = Collections.newSetFromMap(new IdentityHashMap<>());

    /** The pinned ASTParser has no close API; intercept its exact resolver seam before parsing. */
    void attach(ASTParser parser) throws Failure {
        try {
            var field = ASTParser.class.getDeclaredField("unitResolver");
            if (field.getType() != ICompilationUnitResolver.class) throw new IllegalStateException("resolver type changed");
            field.setAccessible(true);
            var delegate = (ICompilationUnitResolver) field.get(parser);
            if (!delegate.getClass().getName().equals("org.eclipse.jdt.core.dom.CompilationUnitResolver$ECJCompilationUnitResolver"))
                throw new IllegalStateException("resolver implementation changed");
            var owned = new Resolver(delegate);
            field.set(parser, owned);
            if (field.get(parser) != owned) throw new IllegalStateException("resolver hook not installed");
        } catch (ReflectiveOperationException | RuntimeException failure) {
            var error = new Failure("internal_failure", "cannot manage Java parser resources");
            error.initCause(failure);
            throw error;
        }
    }

    @Override public void close() throws Failure {
        Failure failure = null;
        for (Classpath path : classpaths) {
            // FileSystem.cleanup uses this same operation, including each ClasspathJar's ZipFile.
            try { path.reset(); }
            catch (RuntimeException problem) {
                if (failure == null) {
                    failure = new Failure("cleanup_failed", "cannot close Java parser resources");
                    failure.initCause(problem);
                } else failure.addSuppressed(problem);
            }
        }
        classpaths.clear();
        if (failure != null) throw failure;
    }

    private final class Resolver implements ICompilationUnitResolver {
        private final ICompilationUnitResolver delegate;
        Resolver(ICompilationUnitResolver delegate) { this.delegate = delegate; }

        @Override public void resolve(String[] paths, String[] encodings, String[] keys, FileASTRequestor requestor,
                int level, Map<String, String> options, List<Classpath> environment, int flags, IProgressMonitor monitor) {
            classpaths.addAll(environment); // Before delegate initialization, cancellation or the first AST callback.
            delegate.resolve(paths, encodings, keys, requestor, level, options, environment, flags, monitor);
        }

        @Override public CompilationUnit toCompilationUnit(org.eclipse.jdt.internal.compiler.env.ICompilationUnit source,
                boolean bindings, IJavaProject project, List<Classpath> environment, int position, int level,
                Map<String, String> options, WorkingCopyOwner parsedOwner, WorkingCopyOwner typeOwner, int flags, IProgressMonitor monitor) {
            classpaths.addAll(environment);
            return delegate.toCompilationUnit(source, bindings, project, environment, position, level, options, parsedOwner, typeOwner, flags, monitor);
        }

        @Override public void parse(ICompilationUnit[] units, ASTRequestor requestor, int level,
                Map<String, String> options, int flags, IProgressMonitor monitor) {
            delegate.parse(units, requestor, level, options, flags, monitor);
        }

        @Override public void parse(String[] paths, String[] encodings, FileASTRequestor requestor, int level,
                Map<String, String> options, int flags, IProgressMonitor monitor) {
            delegate.parse(paths, encodings, requestor, level, options, flags, monitor);
        }

        @Override public void resolve(ICompilationUnit[] units, String[] keys, ASTRequestor requestor, int level,
                Map<String, String> options, IJavaProject project, WorkingCopyOwner owner, int flags, IProgressMonitor monitor) {
            delegate.resolve(units, keys, requestor, level, options, project, owner, flags, monitor);
        }
    }
}
