package dev.codegraph.java;

import java.io.IOException;
import java.nio.file.*;
import java.nio.file.attribute.BasicFileAttributes;
import java.security.MessageDigest;
import java.util.*;
import static dev.codegraph.java.Main.Failure;

/** Frozen source/model inputs, not a sandbox. Every read is checked against both original and copy. */
final class SourceSnapshot implements AutoCloseable {
    private static final Set<String> EXCLUDED = Set.of(".git", ".gradle", ".idea", ".tools", "vendor", "node_modules");
    private final Path original;
    private final Path root;
    private final ProjectModel.Checkpoint checkpoint;
    private final Map<Path, Path> aliases = new LinkedHashMap<>();
    private final Map<Path, byte[]> hashes = new LinkedHashMap<>();
    private final Map<Path, byte[]> external = new LinkedHashMap<>();
    private long bytes;

    private SourceSnapshot(Path original, ProjectModel.Checkpoint checkpoint) throws IOException {
        this.original = original;
        this.checkpoint = checkpoint;
        this.root = Files.createTempDirectory("codegraph-java-input-").toRealPath();
    }

    static SourceSnapshot capture(Path original, List<String> selected, ProjectModel.Checkpoint checkpoint) throws Failure {
        SourceSnapshot snapshot = null;
        try {
            snapshot = new SourceSnapshot(original, checkpoint);
            snapshot.scan(original, true, false);
            snapshot.validatePath("pom.xml", false);
            snapshot.validatePath("narrant-java.json", false);
            for (String path : selected) {
                snapshot.validatePath(path, false);
                snapshot.freeze(ProjectModel.checkedFile(original, path));
            }
            snapshot.verify();
            return snapshot;
        } catch (Failure e) { if (snapshot != null) snapshot.close(); throw e; }
        catch (IOException e) { if (snapshot != null) snapshot.close(); throw io(); }
    }

    Path root() { return root; }
    Path original() { return original; }

    /** Track settings including absence, so creation/removal during import is also detected. */
    void watch(Path path) throws Failure {
        try { external.putIfAbsent(path, Files.exists(path) ? digest(Files.readAllBytes(path)) : null); }
        catch (IOException e) { throw io(); }
    }

    void validatePath(String relative, boolean descendants) throws Failure {
        Path path = Path.of(relative);
        for (Path alias : aliases.keySet()) if (path.startsWith(alias) || (descendants && (relative.isEmpty() || alias.startsWith(path))))
            throw new ProjectModel.BoundaryFailure("unsupported_configuration", "Java input path alias is unsupported");
    }

    private void scan(Path directory, boolean capture, boolean cache) throws IOException, Failure {
        checkpoint.check();
        try (var entries = Files.newDirectoryStream(directory)) {
            for (Path path : entries) {
                checkpoint.check();
                if (Files.isSymbolicLink(path)) {
                    Path relative = original.relativize(path);
                    if (capture) aliases.put(relative, Files.readSymbolicLink(path));
                    else if (!Objects.equals(aliases.get(relative), Files.readSymbolicLink(path))) throw changed();
                    continue;
                }
                if (Files.isDirectory(path)) {
                    String name = path.getFileName().toString();
                    if (!name.equals(".git")) scan(path, capture, cache || EXCLUDED.contains(name));
                } else if (Files.isRegularFile(path) && (!cache || buildInput(path))) {
                    Path relative = original.relativize(path);
                    if (capture) freeze(path);
                    else if (!hashes.containsKey(relative)) throw changed();
                }
            }
        }
    }

    /** Cache binaries are not inputs, but declared Java/module inputs still take precedence over layout exclusions. */
    private boolean buildInput(Path path) {
        String name = path.getFileName().toString();
        return name.endsWith(".java") || name.endsWith(".gradle") || name.endsWith(".gradle.kts") ||
            name.endsWith(".kt") || name.endsWith(".groovy") || name.endsWith(".xml") || name.endsWith(".properties");
    }

    private void freeze(Path path) throws IOException, Failure {
        Path relative = original.relativize(path);
        if (hashes.containsKey(relative)) return;
        checkpoint.check();
        long size = Files.size(path);
        if (size > 32 * 1024 * 1024 || (bytes += size) > 512L * 1024 * 1024 || hashes.size() >= 100000)
            throw new Failure("resource_limit", "Java snapshot exceeds input limit");
        byte[] data = Files.readAllBytes(path);
        Path destination = root.resolve(relative);
        Files.createDirectories(destination.getParent());
        Files.write(destination, data);
        hashes.put(relative, digest(data));
    }

    void verify() throws Failure {
        try {
            scan(original, false, false);
            for (var alias : aliases.entrySet()) {
                if (!Files.isSymbolicLink(original.resolve(alias.getKey())) ||
                    !Files.readSymbolicLink(original.resolve(alias.getKey())).equals(alias.getValue())) throw changed();
            }
            for (var entry : hashes.entrySet()) {
                checkpoint.check();
                for (Path base : List.of(original, root)) {
                    Path path = base.resolve(entry.getKey());
                    if (!path.toRealPath().startsWith(base) || !Arrays.equals(entry.getValue(), digest(Files.readAllBytes(path))))
                        throw changed();
                }
            }
            try (var paths = Files.walk(root)) {
                for (Path path : paths.filter(p -> p.toString().endsWith(".java")).toList())
                    if (!hashes.containsKey(root.relativize(path))) throw changed();
            }
            for (var entry : external.entrySet()) {
                byte[] actual = Files.exists(entry.getKey()) ? digest(Files.readAllBytes(entry.getKey())) : null;
                if (!Arrays.equals(actual, entry.getValue())) throw changed();
            }
        } catch (IOException e) { throw io(); }
    }

    private static byte[] digest(byte[] data) {
        try { return MessageDigest.getInstance("SHA-256").digest(data); }
        catch (java.security.NoSuchAlgorithmException e) { throw new IllegalStateException(e); }
    }
    private static Failure changed() { return new Failure("object_extraction_failed", "Java source or model input changed during analysis"); }
    private static Failure io() { return new Failure("object_extraction_failed", "cannot freeze or verify Java inputs"); }

    @Override public void close() throws Failure {
        try {
            Files.walkFileTree(root, new SimpleFileVisitor<>() {
                @Override public FileVisitResult visitFile(Path file, BasicFileAttributes attrs) throws IOException {
                    Files.delete(file); return FileVisitResult.CONTINUE;
                }
                @Override public FileVisitResult postVisitDirectory(Path directory, IOException failure) throws IOException {
                    if (failure != null) throw failure;
                    Files.delete(directory); return FileVisitResult.CONTINUE;
                }
            });
        } catch (IOException e) { throw new Failure("object_extraction_failed", "cannot remove Java input snapshot"); }
    }
}
