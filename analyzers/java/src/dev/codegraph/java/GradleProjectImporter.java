package dev.codegraph.java;

import dev.codegraph.gradle.SourceModel;
import java.io.*;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.*;
import org.gradle.tooling.*;
import static dev.codegraph.java.Main.Failure;

/** A model-only request against a frozen copy, with isolated daemon ownership. */
final class GradleProjectImporter {
    static boolean declared(Path root) {
        return List.of("settings.gradle","settings.gradle.kts","build.gradle","build.gradle.kts").stream().anyMatch(name -> Files.exists(root.resolve(name)));
    }
    static ImportedProject load(SourceSnapshot snapshot,List<SdkDiscovery.Candidate> jdks,ProjectModel.Checkpoint checkpoint) throws Failure {
        var distribution=GradleDistribution.install(snapshot,jdks,checkpoint);
        Path home=snapshot.outputRoot().resolve("gradle-home");
        try {
            Files.createDirectory(home);
            List<String> arguments=environment(snapshot,home,jdks);
            Path bridge=resource(snapshot,"gradle-bridge.jar");
            Path init=resource(snapshot,"gradle-model.init.gradle");
            arguments.addAll(List.of("--no-configure-on-demand","--no-configuration-cache","-Dorg.gradle.unsafe.isolated-projects=false","--init-script",init.toString(),"--project-cache-dir",snapshot.outputRoot().resolve("project-cache").toString(),
                "-Dcodegraph.gradle.bridge="+bridge,"-Dcodegraph.gradle.input="+snapshot.root(),"-Dorg.gradle.java.installations.auto-download=false",
                "-Porg.gradle.java.installations.auto-download=false","-Dorg.gradle.java.installations.auto-detect=false","-Porg.gradle.java.installations.auto-detect=false"));
            GradleConnector connector=GradleConnector.newConnector().forProjectDirectory(snapshot.root().toFile())
                .useInstallation(distribution.root().toFile()).useGradleUserHomeDir(home.toFile());
            SourceModel model;
            {
                var processes=new OwnedGradleProcesses(home);
                ProjectConnection connection=null;
                var token=GradleConnector.newCancellationTokenSource();
                var result=new CompletableFuture<SourceModel>();
                Map<String,String> env=new HashMap<>(System.getenv());
                for(String key:List.of("JAVA_TOOL_OPTIONS","JDK_JAVA_OPTIONS","_JAVA_OPTIONS","CLASSPATH")) env.remove(key);
                env.put("GRADLE_USER_HOME",home.toString());
                var output=new BoundedOutput();
                try {
                    connection=connector.connect();
                    connection.model(SourceModel.class).setJavaHome(distribution.javaHome().toFile()).setEnvironmentVariables(env)
                        .withArguments(arguments).setStandardOutput(output).setStandardError(output).withCancellationToken(token.token())
                        .get(new ResultHandler<>() {
                            public void onComplete(SourceModel value) { result.complete(value); }
                            public void onFailure(GradleConnectionException failure) { result.completeExceptionally(failure); }
                        });
                    while(!result.isDone()) {
                        processes.observe(); checkpoint.check();
                        if(output.exceeded) throw new Failure("resource_limit","Gradle configuration output exceeds limit");
                        try { Thread.sleep(20); } catch(InterruptedException e) { Thread.currentThread().interrupt(); throw new Failure("resource_limit","Gradle import interrupted"); }
                    }
                    checkpoint.check();
                    if(output.exceeded) throw new Failure("resource_limit","Gradle configuration output exceeds limit");
                    try { model=result.join(); }
                    catch(CompletionException failure) { snapshot.freezeGradleOutputs(List.of()); infrastructure(failure); throw new Failure("unsupported_configuration","Gradle model import failed"); }
                } finally {
                    token.cancel();
                    // Kill and verify before close: closing a connection with a hung request can block.
                    try { processes.close(); }
                    catch (Failure failure) { snapshot.retainForToolCleanup(); throw failure; }
                    finally {
                        try { if(connection!=null) connection.close(); }
                        finally {
                            try { connector.disconnect(); }
                            finally {
                                // Pinned Tooling API 8.14.3 compatibility shim. This engine is one-shot;
                                // stop client executors independently of verified daemon termination.
                                org.gradle.tooling.internal.consumer.DefaultGradleConnector.close();
                            }
                        }
                    }
                    Files.write(snapshot.outputRoot().resolve("gradle-output.log"),output.buffer.toByteArray());
                }
            }
            List<ImportedProject.SourceSet> sets=new ArrayList<>();
            List<String> roots=new ArrayList<>();
            for(var row:model.getSourceSets()) {
                List<String> own=relative(snapshot,strings(row,"roots"),false);
                roots.addAll(own);
                List<String> inputs=relative(snapshot,strings(row,"inputs"),true);
                for(String name:List.of("settings.gradle","settings.gradle.kts","gradle/wrapper/gradle-wrapper.properties"))
                    if(Files.isRegularFile(snapshot.root().resolve(name))) inputs.add(name);
                List<String> binaries=new ArrayList<>();
                for(String binary:strings(row,"binaries")) binaries.add(snapshot.freezeBinary(binary));
                sets.add(new ImportedProject.SourceSet("java-config:gradle:"+row.get("id"),String.valueOf(row.get("module")),Boolean.TRUE.equals(row.get("test")),
                    own,strings(row,"visible").stream().map(id -> "java-config:gradle:"+id).toList(), binaries,
                    new ImportedProject.Compiler(value(row,"release"),value(row,"source"),value(row,"target"),value(row,"toolchain"),value(row,"encoding"),Boolean.TRUE.equals(row.get("apiUnavailable"))),
                    Boolean.TRUE.equals(row.get("complete")),inputs,relative(snapshot,strings(row,"members"),false)));
            }
            snapshot.freezeGradleOutputs(roots);
            snapshot.verify();
            return new ImportedProject(sets);
        } catch(IOException failure) { throw new Failure("object_extraction_failed","cannot prepare Gradle model inputs"); }
    }
    private static void infrastructure(Throwable failure) throws Failure {
        Set<Throwable> seen=Collections.newSetFromMap(new IdentityHashMap<>());
        while(failure!=null && seen.add(failure)) {
            if(failure instanceof IOException || failure instanceof UncheckedIOException || failure instanceof SecurityException)
                throw new Failure("object_extraction_failed","cannot read Gradle model inputs");
            if(failure instanceof OutOfMemoryError) throw new Failure("resource_limit","Gradle model resource limit");
            failure=failure.getCause();
        }
    }
    private static String value(Map<String,Object> row,String name) {
        Object value=row.get(name); if(value==null) return null;
        String text=value.toString(); return text.startsWith("1.") ? text.substring(2) : text;
    }
    @SuppressWarnings("unchecked")
    private static List<String> strings(Map<String,Object> row,String name) { return (List<String>)row.get(name); }
    private static List<String> relative(SourceSnapshot snapshot,List<String> paths,boolean existingOnly) throws Failure {
        List<String> result=new ArrayList<>();
        for(String name:paths) {
            Path path=Path.of(name).toAbsolutePath().normalize();
            if(!path.startsWith(snapshot.root())) throw new ProjectModel.BoundaryFailure("unsupported_configuration","Gradle input outside repository");
            if(existingOnly && !Files.isRegularFile(path)) continue;
            String relative=snapshot.root().relativize(path).toString().replace('\\','/');
            snapshot.validatePath(relative,true); result.add(relative);
        }
        return result;
    }
    private static Path resource(SourceSnapshot snapshot,String name) throws IOException {
        Path path=snapshot.outputRoot().resolve(name);
        try(var in=GradleProjectImporter.class.getResourceAsStream("/"+name)) {
            if(in==null) throw new IOException("missing packaged Gradle resource");
            Files.copy(in,path);
        }
        return path;
    }
    /** User init scripts stay at their original locations, preserving relative script resources. */
    private static List<String> environment(SourceSnapshot snapshot,Path home,List<SdkDiscovery.Candidate> jdks) throws IOException,Failure {
        Path user=System.getenv("GRADLE_USER_HOME")==null ? Path.of(System.getProperty("user.home"),".gradle") : Path.of(System.getenv("GRADLE_USER_HOME")).toAbsolutePath();
        Path properties=user.resolve("gradle.properties"); snapshot.watch(properties);
        Properties settings=new Properties();
        if(Files.exists(properties)) try(var in=Files.newInputStream(properties)) { settings.load(in); }
        settings.setProperty("org.gradle.java.installations.auto-download","false");
        settings.setProperty("org.gradle.java.installations.auto-detect","false");
        settings.setProperty("org.gradle.java.installations.paths",String.join(",",jdks.stream().map(jdk -> jdk.root().toString()).toList()));
        try(var out=Files.newOutputStream(home.resolve("gradle.properties"))) { settings.store(out,null); }
        List<String> args=new ArrayList<>();
        for(String name:List.of("init.gradle","init.gradle.kts")) {
            Path script=user.resolve(name); snapshot.watch(script);
            if(Files.isRegularFile(script)) args.addAll(List.of("--init-script",script.toString()));
        }
        Path init=user.resolve("init.d"); snapshot.watchCondition(init); snapshot.watchListing(init);
        if(Files.isDirectory(init)) try(var files=Files.list(init)) {
            for(Path script:files.sorted().toList()) if(script.toString().endsWith(".gradle") || script.toString().endsWith(".gradle.kts")) {
                snapshot.watch(script); args.addAll(List.of("--init-script",script.toString()));
            }
        }
        return args;
    }
    private static final class BoundedOutput extends OutputStream {
        private long size; volatile boolean exceeded;
        private final ByteArrayOutputStream buffer=new ByteArrayOutputStream();
        public synchronized void write(int value) { if(++size>1024*1024) exceeded=true; else buffer.write(value); }
        public synchronized void write(byte[] data,int offset,int length) { size+=length; if(size>1024*1024) exceeded=true; else buffer.write(data,offset,length); }
    }
}
