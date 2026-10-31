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
    private final Path outputRoot;
    private final ProjectModel.Checkpoint checkpoint;
    private final Set<Path> directories = new HashSet<>();
    private final Map<Path, Path> aliases = new LinkedHashMap<>();
    private final Map<Path, byte[]> hashes = new LinkedHashMap<>();
    private final Map<Path, Integer> conditions = new LinkedHashMap<>();
    private final Map<Path, byte[]> external = new LinkedHashMap<>();
    private final Map<Path, Path> binaryCopies = new LinkedHashMap<>();
    private final Map<Path, Set<Path>> binaryTrees = new LinkedHashMap<>();
    private final Map<Path, List<String>> listings = new LinkedHashMap<>();
    private long bytes;
    private boolean retained;
    private boolean generatedFrozen;
    private final Set<Path> generatedDirectories = new HashSet<>();
    private final Map<Path, byte[]> generated = new LinkedHashMap<>();

    private SourceSnapshot(Path original, ProjectModel.Checkpoint checkpoint) throws IOException {
        this.original = original;
        this.checkpoint = checkpoint;
        Path container = Files.createTempDirectory("codegraph-java-input-").toRealPath();
        this.root = Files.createDirectory(container.resolve("input"));
        this.outputRoot = Files.createDirectory(container.resolve("outputs"));
    }

    static SourceSnapshot capture(Path original, List<String> selected, ProjectModel.Checkpoint checkpoint) throws Failure {
        SourceSnapshot snapshot = null;
        try {
            snapshot = new SourceSnapshot(original, checkpoint);
            snapshot.scan(original, true, false);
            for (String input : List.of("settings.gradle", "settings.gradle.kts", "build.gradle", "build.gradle.kts",
                "gradle/wrapper/gradle-wrapper.properties", "gradle/gradle-daemon-jvm.properties")) snapshot.validatePath(input, false);
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
    Path outputRoot() { return outputRoot; }

    /** JDT consumes owned immutable binaries, never a live external dependency cache. */
    String freezeBinary(String name) throws Failure {
        try {
            Path source=Path.of(name).toRealPath();
            if(binaryCopies.containsKey(source)) return binaryCopies.get(source).toString();
            Path destination=outputRoot.resolve("binaries").resolve(Integer.toString(binaryCopies.size())).resolve(source.getFileName()==null?"root":source.getFileName().toString());
            if(Files.isDirectory(source)) {
                Set<Path> members=new HashSet<>();
                try(var paths=Files.walk(source)) {
                    for(Path path:paths.toList()) {
                        checkpoint.check();
                        if(Files.isSymbolicLink(path)) throw new ProjectModel.BoundaryFailure("unsupported_configuration","binary directory alias unsupported");
                        Path relative=source.relativize(path); members.add(relative);
                        if(Files.isDirectory(path)) Files.createDirectories(destination.resolve(relative));
                        else copyBinary(path,destination.resolve(relative));
                    }
                }
                binaryTrees.put(source,Set.copyOf(members));
                binaryTrees.put(destination,Set.copyOf(members));
            } else copyBinary(source,destination);
            binaryCopies.put(source,destination);
            return destination.toString();
        } catch(IOException e) { throw io(); }
    }

    private void copyBinary(Path source,Path destination) throws IOException,Failure {
        checkpoint.check();
        long size=Files.size(source);
        if(!Files.isRegularFile(source) || size>128L*1024*1024 || (bytes+=size)>512L*1024*1024 || external.size()>100000)
            throw new Failure("resource_limit","Gradle binary context exceeds input limit");
        watch(source);
        Files.createDirectories(destination.getParent());
        Files.copy(source,destination);
        external.put(destination,external.get(source));
    }

    /** Track settings including absence, so creation/removal during import is also detected. */
    void watch(Path path) throws Failure {
        try {
            if (!external.containsKey(path)) external.put(path, Files.exists(path) ? digest(Files.readAllBytes(path)) : null);
        }
        catch (IOException e) { throw io(); }
    }

    void watchListing(Path directory) throws Failure {
        try { if(!listings.containsKey(directory)) listings.put(directory,listing(directory)); }
        catch(IOException e) { throw io(); }
    }
    private List<String> listing(Path directory) throws IOException,Failure {
        if(!Files.exists(directory)) return List.of();
        try(var entries=Files.list(directory)) {
            List<String> names=entries.limit(100001).map(path -> path.getFileName().toString()).sorted().toList();
            if(names.size()>100000) throw new Failure("resource_limit","Gradle user input listing exceeds limit");
            return names;
        }
    }

    /** Preserve Maven's file-activation inputs, including absent paths and cache-local markers. */
    void watchCondition(Path evaluated) throws Failure {
        evaluated = evaluated.toAbsolutePath().normalize();
        Path originalPath = evaluated.startsWith(root) ? original.resolve(root.relativize(evaluated)) : evaluated;
        try {
            if (conditions.containsKey(evaluated)) return;
            int kind = conditionKind(originalPath);
            conditions.put(evaluated, kind);
            if (!evaluated.startsWith(root) || kind == 0) return;
            String relative = root.relativize(evaluated).toString().replace('\\', '/');
            validatePath(relative, false);
            Path directory = kind == 1 ? evaluated : evaluated.getParent();
            for (Path current = directory; current.startsWith(root); current = current.getParent())
                directories.add(root.relativize(current));
            Files.createDirectories(directory);
            if (kind == 2) freeze(originalPath);
        } catch (IOException e) { throw io(); }
    }

    private int conditionKind(Path path) throws IOException, Failure {
        try {
            var attributes = Files.readAttributes(path, BasicFileAttributes.class);
            if (attributes.isDirectory()) return 1;
            if (attributes.isRegularFile()) return 2;
            throw new Failure("object_extraction_failed", "unsupported Maven activation input type");
        } catch (NoSuchFileException e) { return 0; }
    }

    void validatePath(String relative, boolean descendants) throws Failure {
        Path path = Path.of(relative);
        for (Path alias : aliases.keySet()) if (path.startsWith(alias) || (descendants && (relative.isEmpty() || alias.startsWith(path))))
            throw new ProjectModel.BoundaryFailure("unsupported_configuration", "Java input path alias is unsupported");
    }

    private void scan(Path directory, boolean capture, boolean cache) throws IOException, Failure {
        checkpoint.check();
        Path relativeDirectory = original.relativize(directory);
        if (capture) {
            directories.add(relativeDirectory);
            Files.createDirectories(root.resolve(relativeDirectory));
        } else if (!directories.contains(relativeDirectory)) throw changed();
        if (directory.getFileName().toString().equals(".git")) return; // Preserve the existence condition, never Git internals.
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
                    scan(path, capture, cache || EXCLUDED.contains(name));
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

    /** Return only bytes matching the original/generated freeze, including reads after JDT parsing. */
    byte[] readSourceBytes(String name) throws IOException, Failure {
        Path relative = Path.of(name);
        byte[] expected = hashes.containsKey(relative) ? hashes.get(relative) : generated.get(relative);
        byte[] contents = Files.readAllBytes(root.resolve(relative));
        if (expected == null || !Arrays.equals(expected, digest(contents))) throw changed();
        return contents;
    }

    boolean originalMember(String relative) { return hashes.containsKey(Path.of(relative)); }

    /** Admit only auxiliary build output and model-declared context, then freeze it before JDT. */
    void freezeGradleOutputs(List<String> sourceRoots) throws Failure {
        if (generatedFrozen) throw new IllegalStateException("Gradle output snapshot already frozen");
        generatedFrozen=true;
        try (var paths = Files.walk(root)) {
            for (Path path : paths.toList()) {
                checkpoint.check();
                Path relative = root.relativize(path);
                if (directories.contains(relative) || hashes.containsKey(relative)) continue;
                if (Files.isSymbolicLink(path)) throw changed();
                String name = relative.toString().replace('\\','/');
                boolean auxiliary = Arrays.stream(name.split("/")).anyMatch(part -> part.equals("build") || part.equals(".gradle"));
                boolean declared = sourceRoots.stream().anyMatch(source -> source.isEmpty() || name.equals(source) || name.startsWith(source + "/") || source.startsWith(name + "/"));
                if (Files.isDirectory(path)) {
                    if (!auxiliary && !declared) throw changed();
                    generatedDirectories.add(relative);
                } else if (Files.isRegularFile(path)) {
                    if (!auxiliary && !declared && path.toString().endsWith(".java")) throw changed();
                    if (Files.size(path)>32L*1024*1024 || (bytes+=Files.size(path))>512L*1024*1024 || generated.size()>=100000)
                        throw new Failure("resource_limit", "Gradle generated context exceeds input limit");
                    generated.put(relative,digest(Files.readAllBytes(path)));
                }
            }
        } catch (IOException e) { throw io(); }
    }

    void verify() throws Failure {
        try {
            scan(original, false, false);
            for (Path directory : directories) for (Path base : List.of(original, root)) {
                Path path = base.resolve(directory);
                if (!Files.isDirectory(path, LinkOption.NOFOLLOW_LINKS) || !path.toRealPath().startsWith(base)) throw changed();
            }
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
                for (Path path : paths.toList()) {
                    Path relative = root.relativize(path);
                    if (Files.isDirectory(path, LinkOption.NOFOLLOW_LINKS) && !directories.contains(relative) && !generatedDirectories.contains(relative)) throw changed();
                    if ((path.toString().endsWith(".java") || generatedFrozen && Files.isRegularFile(path)) && !hashes.containsKey(relative) && !generated.containsKey(relative)) throw changed();
                }
            }
            for (Path directory : generatedDirectories) if (!Files.isDirectory(root.resolve(directory),LinkOption.NOFOLLOW_LINKS)) throw changed();
            for (var entry : generated.entrySet()) {
                Path path = root.resolve(entry.getKey());
                if (!path.toRealPath().startsWith(root) || !Arrays.equals(entry.getValue(),digest(Files.readAllBytes(path)))) throw changed();
            }
            for (var condition : conditions.entrySet()) {
                Path evaluated = condition.getKey();
                Path actual = evaluated.startsWith(root) ? original.resolve(root.relativize(evaluated)) : evaluated;
                if (conditionKind(actual) != condition.getValue() || conditionKind(evaluated) != condition.getValue()) throw changed();
            }
            for(var listing:listings.entrySet()) if(!listing.getValue().equals(listing(listing.getKey()))) throw changed();
            for(var tree:binaryTrees.entrySet()) try(var paths=Files.walk(tree.getKey())) {
                Set<Path> actual=new HashSet<>();
                for(Path path:paths.toList()) {
                    checkpoint.check();
                    if(Files.isSymbolicLink(path)) throw changed();
                    actual.add(tree.getKey().relativize(path));
                }
                if(!actual.equals(tree.getValue())) throw changed();
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

    void retainForToolCleanup() { retained = true; }

    @Override public void close() throws Failure {
        if (retained) return;
        try {
            Files.walkFileTree(root.getParent(), new SimpleFileVisitor<>() {
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
