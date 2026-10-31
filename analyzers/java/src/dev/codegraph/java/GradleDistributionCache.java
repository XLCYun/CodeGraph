package dev.codegraph.java;

import java.io.IOException;
import java.nio.channels.FileChannel;
import java.nio.channels.FileLock;
import java.nio.channels.OverlappingFileLockException;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;
import static dev.codegraph.java.Main.Failure;

/** Only verified distribution bytes persist; the archive-derived inventory is never read from a cache manifest. */
final class GradleDistributionCache {
    record Entry(Path root, Map<String,String> inventory) {
        void verify(ProjectModel.Checkpoint checkpoint) throws Failure {
            if (!inventory.equals(GradleDistributionCache.inventory(root, checkpoint)))
                throw new Failure("object_extraction_failed", "Gradle distribution cache changed");
        }
    }

    static Entry publish(Path extracted, String archiveHash, SourceSnapshot snapshot,
                         ProjectModel.Checkpoint checkpoint) throws Failure {
        Map<String,String> expected = inventory(extracted, checkpoint);
        Path base = Path.of(System.getProperty("user.home"), ".cache", "codegraph", "gradle-distributions").toAbsolutePath().normalize();
        if (base.startsWith(snapshot.original()) || snapshot.original().startsWith(base))
            throw new Failure("object_extraction_failed", "Gradle cache overlaps repository");
        Path destination = base.resolve(archiveHash);
        Path transaction = base.resolve(".publish-" + archiveHash);
        Path staging = null;
        try {
            for (Path part = base; part != null; part = part.getParent())
                if (Files.isSymbolicLink(part)) throw new IOException("cache alias");
            Files.createDirectories(base);
            try (var channel = FileChannel.open(base.resolve(archiveHash + ".lock"),
                    StandardOpenOption.CREATE, StandardOpenOption.WRITE, LinkOption.NOFOLLOW_LINKS);
                 FileLock lock = acquire(channel, checkpoint)) {
                try {
                    if (Files.exists(transaction, LinkOption.NOFOLLOW_LINKS)) removeStaging(transaction, archiveHash);
                    if (!Files.exists(destination, LinkOption.NOFOLLOW_LINKS)) {
                        Files.createDirectory(transaction);
                        try {
                            Files.writeString(transaction.resolve("owner"), transactionMarker(archiveHash), StandardOpenOption.CREATE_NEW);
                        } catch (IOException failure) {
                            Files.deleteIfExists(transaction.resolve("owner")); Files.delete(transaction);
                            throw failure;
                        }
                        staging = transaction;
                        Path contents = Files.createDirectory(staging.resolve("contents"));
                        for (var entry : expected.entrySet()) {
                            checkpoint.check();
                            Path target = contents.resolve(entry.getKey());
                            if (entry.getValue().equals("directory")) Files.createDirectories(target);
                            else {
                                Files.createDirectories(target.getParent());
                                Files.copy(extracted.resolve(entry.getKey()), target);
                            }
                        }
                        if (!expected.equals(inventory(contents, checkpoint))) throw new IOException("changed staged distribution");
                        Files.move(contents, destination, StandardCopyOption.ATOMIC_MOVE);
                        removeStaging(staging, archiveHash);
                        staging = null;
                    }
                    Entry entry = new Entry(destination, expected);
                    entry.verify(checkpoint);
                    return entry;
                } finally {
                    if (staging != null) removeStaging(staging, archiveHash);
                }
            }
        } catch (Failure failure) { throw failure; }
        catch (IOException | SecurityException failure) {
            throw new Failure("object_extraction_failed", "cannot publish or verify Gradle distribution cache");
        }
    }

    private static FileLock acquire(FileChannel channel, ProjectModel.Checkpoint checkpoint) throws IOException, Failure {
        while (true) {
            checkpoint.check();
            try {
                FileLock lock = channel.tryLock();
                if (lock != null) return lock;
            } catch (OverlappingFileLockException busy) { /* Another request in this JVM owns publication. */ }
            try { Thread.sleep(20); }
            catch (InterruptedException interrupted) {
                Thread.currentThread().interrupt();
                throw new Failure("resource_limit", "Gradle cache publication interrupted");
            }
        }
    }

    private static Map<String,String> inventory(Path root, ProjectModel.Checkpoint checkpoint) throws Failure {
        Map<String,String> entries = new TreeMap<>();
        long total = 0;
        try {
            if (Files.isSymbolicLink(root) || !Files.isDirectory(root, LinkOption.NOFOLLOW_LINKS)) throw new IOException("invalid cache root");
            Path actualRoot = root.toRealPath();
            if (!actualRoot.equals(root.toAbsolutePath().normalize())) throw new IOException("cache root alias");
            try (var paths = Files.walk(root)) {
                var iterator = paths.iterator();
                while (iterator.hasNext()) {
                    checkpoint.check();
                    Path file = iterator.next();
                    if (file.equals(root)) continue;
                    String relative = root.relativize(file).toString().replace('\\', '/');
                    if (Files.isSymbolicLink(file) || !file.toRealPath().equals(file.toAbsolutePath().normalize()))
                        throw new IOException("cache alias");
                    String digest;
                    if (Files.isDirectory(file, LinkOption.NOFOLLOW_LINKS)) digest = "directory";
                    else {
                        if (!Files.isRegularFile(file, LinkOption.NOFOLLOW_LINKS)) throw new IOException("invalid cache entry");
                        MessageDigest hash = MessageDigest.getInstance("SHA-256");
                        try (var input = Files.newInputStream(file)) {
                            byte[] bytes = new byte[65536]; int count;
                            while ((count = input.read(bytes)) != -1) {
                                checkpoint.check();
                                total += count;
                                if (total > 1024L * 1024 * 1024) throw new Failure("resource_limit", "Gradle distribution cache exceeds limit");
                                hash.update(bytes, 0, count);
                            }
                        }
                        digest = HexFormat.of().formatHex(hash.digest());
                    }
                    entries.put(relative, digest);
                    if (entries.size() > 100000) throw new Failure("resource_limit", "Gradle distribution cache exceeds file limit");
                }
            }
            return Collections.unmodifiableMap(entries);
        } catch (Failure failure) { throw failure; }
        catch (IOException | java.io.UncheckedIOException | SecurityException failure) {
            throw new Failure("object_extraction_failed", "cannot read Gradle distribution cache");
        } catch (java.security.NoSuchAlgorithmException impossible) { throw new IllegalStateException(impossible); }
    }

    private static String transactionMarker(String archiveHash) {
        return "codegraph-gradle-distribution/1\n" + archiveHash + "\n";
    }

    /** Recover only a marker-proven transaction for the hash whose lock is currently held. */
    private static void removeStaging(Path root, String archiveHash) throws Failure {
        try {
            Path marker = root.resolve("owner");
            if (!root.getFileName().toString().equals(".publish-" + archiveHash) ||
                    Files.isSymbolicLink(root) || !Files.isDirectory(root, LinkOption.NOFOLLOW_LINKS) ||
                    !root.toRealPath().equals(root.toAbsolutePath().normalize()) ||
                    !Files.isRegularFile(marker, LinkOption.NOFOLLOW_LINKS) ||
                    Files.size(marker) > 128 || !Files.readString(marker).equals(transactionMarker(archiveHash)))
                throw new IOException("unproved cache transaction");
            List<Path> owned = new ArrayList<>();
            try (var files = Files.walk(root)) {
                var iterator = files.iterator();
                while (iterator.hasNext()) {
                    Path file = iterator.next();
                    if (Files.isSymbolicLink(file) || !file.toRealPath().equals(file.toAbsolutePath().normalize()))
                        throw new IOException("transaction alias");
                    owned.add(file);
                    if (owned.size() > 100003) throw new IOException("transaction exceeds limit");
                }
            }
            for (Path file : owned.stream().sorted(Comparator.reverseOrder()).toList()) Files.delete(file);
        } catch (IOException | java.io.UncheckedIOException | SecurityException failure) {
            throw new Failure("object_extraction_failed", "cannot recover Gradle cache transaction");
        }
    }
}
