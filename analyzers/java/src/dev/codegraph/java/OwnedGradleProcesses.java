package dev.codegraph.java;

import java.io.IOException;
import java.nio.file.*;
import java.time.Instant;
import java.util.*;
import static dev.codegraph.java.Main.Failure;

/** Retains process identities while the Tooling API is active, including reparented daemons. */
final class OwnedGradleProcesses implements AutoCloseable {
    private final Path home;
    private boolean closed;
    private boolean interrupted;
    private boolean unverifiable;
    private final Instant started = Instant.now();
    private final Map<Long, ProcessHandle> owned = new LinkedHashMap<>();
    private final Map<Long, Instant> identities = new HashMap<>();
    OwnedGradleProcesses(Path home) { this.home=home; }
    void observe() throws Failure {
        ProcessHandle.current().descendants().forEach(this::remember);
        // A daemon can detach between descendant observations. Its private registry log plus
        // JVM identity and start time establish ownership independently of its current parent.
        Path daemon=home.resolve("daemon");
        if (!Files.exists(daemon)) return;
        try(var files=Files.walk(daemon,2)) {
            for(Path file:files.toList()) {
                var match=java.util.regex.Pattern.compile("daemon-([0-9]+)\\.out\\.log").matcher(file.getFileName().toString());
                if(!match.matches()) continue;
                ProcessHandle.of(Long.parseLong(match.group(1))).ifPresent(process -> {
                    var info=process.info();
                    if(info.startInstant().map(time -> !time.isBefore(started.minusSeconds(1))).orElse(false) &&
                        info.commandLine().orElse("").contains("org.gradle.launcher.daemon.bootstrap.GradleDaemon") &&
                        info.commandLine().orElse("").contains(home.getParent().resolve("distribution").toString())) remember(process);
                });
            }
        } catch(IOException failure) { throw new Failure("object_extraction_failed", "cannot inspect owned Gradle processes"); }
        for(var process:List.copyOf(owned.values())) if(same(process)) process.descendants().forEach(this::remember);
    }
    private void remember(ProcessHandle process) {
        var start=process.info().startInstant();
        if(start.isPresent()) { owned.putIfAbsent(process.pid(),process); identities.putIfAbsent(process.pid(),start.get()); }
        else if(process.isAlive()) unverifiable=true;
    }
    private boolean same(ProcessHandle process) {
        if(!process.isAlive()) return false;
        var start=process.info().startInstant();
        if(start.isEmpty()) { if(process.isAlive()) unverifiable=true; return false; }
        return start.get().equals(identities.get(process.pid()));
    }
    @Override public void close() throws Failure {
        if (closed) return;
        interrupted=Thread.interrupted();
        try {
        Failure observation = null;
        try { observe(); } catch (Failure failure) { observation=failure; }
        for(var process:owned.values()) if(same(process)) process.destroy();
        long deadline=System.nanoTime()+5_000_000_000L;
        while(System.nanoTime()<deadline && owned.values().stream().anyMatch(this::same)) pause();
        try { observe(); } catch (Failure failure) { observation=failure; }
        for(var process:owned.values()) if(same(process)) process.destroyForcibly();
        deadline=System.nanoTime()+5_000_000_000L;
        while(System.nanoTime()<deadline && owned.values().stream().anyMatch(this::same)) pause();
        if(owned.values().stream().anyMatch(this::same)) throw new Failure("cleanup_failed", "owned Gradle process failed to exit");
        if (unverifiable) throw new Failure("cleanup_failed","cannot verify owned Gradle process identity");
        if (observation != null) throw new Failure("cleanup_failed", "cannot verify owned Gradle process exit");
        closed=true;
        } finally { if(interrupted) Thread.currentThread().interrupt(); }
    }
    private void pause() {
        try { Thread.sleep(20); }
        catch(InterruptedException e) { interrupted=true; }
    }
}
