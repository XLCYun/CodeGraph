package dev.codegraph.java;

import java.io.*;
import java.net.URI;
import java.nio.file.*;
import java.security.MessageDigest;
import java.util.*;
import java.util.zip.ZipInputStream;
import static dev.codegraph.java.Main.Failure;

/** Wrapper declarations select the tool distribution, independently of the target platform. */
final class GradleDistribution {
    record Installed(Path root, String version, Path javaHome) {}
    static Installed install(SourceSnapshot snapshot, List<SdkDiscovery.Candidate> jdks,
                             ProjectModel.Checkpoint checkpoint) throws Failure {
        Path wrapper = snapshot.root().resolve("gradle/wrapper/gradle-wrapper.properties");
        Properties properties = new Properties();
        try {
            snapshot.watch(snapshot.original().resolve("gradle/wrapper/gradle-wrapper.properties"));
            if (Files.exists(wrapper)) try (var reader = Files.newBufferedReader(wrapper)) { properties.load(reader); }
            if (Files.exists(wrapper) && properties.getProperty("distributionUrl") == null)
                throw new Failure("unsupported_configuration", "Gradle wrapper has no distribution URL");
            String url = properties.getProperty("distributionUrl", "https://services.gradle.org/distributions/gradle-9.1.0-bin.zip");
            var matcher = java.util.regex.Pattern.compile("(?:^|/)gradle-([0-9]+\\.[0-9]+(?:\\.[0-9]+)?)-(?:bin|all)\\.zip$").matcher(Optional.ofNullable(URI.create(url).getPath()).orElse(""));
            if (!matcher.find()) throw new Failure("unsupported_configuration", "unsupported Gradle distribution version");
            String version = matcher.group(1);
            Path javaHome = toolJvm(version, jdks, snapshot);
            String sha = properties.getProperty("distributionSha256Sum", properties.isEmpty() ?
                "a17ddd85a26b6a7f5ddb71ff8b05fc5104c0202c6e64782429790c933686c806" : null);
            if (sha != null && !sha.matches("[a-fA-F0-9]{64}")) throw new Failure("unsupported_configuration", "invalid Gradle distribution checksum");
            URI uri = URI.create(url);
            if (!uri.isAbsolute()) uri = snapshot.original().resolve("gradle/wrapper").toUri().resolve(uri);
            Path zip = snapshot.outputRoot().resolve("gradle-distribution.zip");
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            try (var in = GradleDownload.open(uri,properties,snapshot); var out = Files.newOutputStream(zip)) {
                byte[] block = new byte[65536]; long total=0; int size;
                while ((size=in.read(block))!=-1) {
                    checkpoint.check(); total+=size;
                    if (total>512L*1024*1024) throw new Failure("resource_limit", "Gradle distribution exceeds download limit");
                    digest.update(block,0,size); out.write(block,0,size);
                }
            }
            if (sha != null && !sha.equalsIgnoreCase(HexFormat.of().formatHex(digest.digest())))
                throw new ProjectModel.BoundaryFailure("unsupported_configuration", "Gradle distribution checksum mismatch");
            Path destination=Files.createDirectory(snapshot.outputRoot().resolve("distribution"));
            try (var in = new ZipInputStream(Files.newInputStream(zip))) {
                java.util.zip.ZipEntry entry; long total=0; int count=0;
                byte[] block=new byte[65536];
                while ((entry=in.getNextEntry())!=null) {
                    checkpoint.check();
                    Path path=destination.resolve(entry.getName()).normalize();
                    if (!path.startsWith(destination) || ++count>100000)
                        throw new Failure("resource_limit", "invalid Gradle distribution archive");
                    if (entry.isDirectory()) { Files.createDirectories(path); continue; }
                    Files.createDirectories(path.getParent());
                    try(var out=Files.newOutputStream(path,StandardOpenOption.CREATE_NEW)) {
                        int size;
                        while((size=in.read(block))!=-1) {
                            checkpoint.check(); total+=size;
                            if(total>1024L*1024*1024) throw new Failure("resource_limit", "Gradle distribution exceeds extraction limit");
                            out.write(block,0,size);
                        }
                    }
                }
            }
            return new Installed(destination.resolve("gradle-"+version),version,javaHome);
        } catch(Failure failure) { throw failure; }
        catch(IOException failure) { throw new Failure("object_extraction_failed", "cannot obtain Gradle distribution"); }
        catch(java.security.NoSuchAlgorithmException failure) { throw new IllegalStateException(failure); }
        catch(IllegalArgumentException failure) { throw new Failure("unsupported_configuration", "invalid Gradle distribution declaration"); }
    }
    private static Path toolJvm(String version,List<SdkDiscovery.Candidate> jdks,SourceSnapshot snapshot) throws Failure,IOException {
        Properties criteria=new Properties();
        Path file=snapshot.root().resolve("gradle/gradle-daemon-jvm.properties");
        if(Files.exists(file)) try(var in=Files.newInputStream(file)) { criteria.load(in); }
        String requested=criteria.getProperty("toolchainVersion");
        if(Files.exists(file) && (requested==null || !requested.matches("[1-9][0-9]*")))
            throw new Failure("unsupported_configuration","invalid Gradle daemon JVM criteria");
        for(String key:criteria.stringPropertyNames()) if(!Set.of("toolchainVersion","toolchainVendor","toolchainImplementation").contains(key) && !key.startsWith("toolchainUrl."))
            throw new Failure("unsupported_configuration","unsupported Gradle daemon JVM criteria");
        String[] parts=version.split("\\.");
        int major=Integer.parseInt(parts[0]), minor=Integer.parseInt(parts[1]);
        if (major<7 || major>9 || (major==7 && minor<6))
            throw new Failure("unsupported_configuration", "unsupported Gradle distribution version");
        int min=major==9 ? 17 : 8;
        int max=major==7 ? 19 : major==8 ?
            (minor<3 ? 19 : minor<5 ? 20 : minor<8 ? 21 : minor<10 ? 22 : minor<14 ? 23 : 24) :
            (minor==0 ? 24 : minor<4 ? 25 : minor<8 ? 26 : 27);
        for(var jdk:jdks.stream().sorted(Comparator.comparing(candidate -> candidate.source().equals("bundled"))).toList()) {
            int release=Integer.parseInt(jdk.release());
            if(release>=min && release<=max && (requested==null || requested.equals(jdk.release())) && localJvmMatches(jdk.root(),criteria)) return jdk.root();
        }
        throw new Failure("unsupported_configuration", "no compatible existing Gradle tool JVM");
    }

    /** Only local release evidence is used; an API-readable foreign-architecture JDK is not a runnable tool. */
    private static boolean localJvmMatches(Path root,Properties criteria) throws IOException {
        Properties release=new Properties();
        try(var in=Files.newInputStream(root.resolve("release"))) { release.load(in); }
        String arch=unquote(release.getProperty("OS_ARCH",""));
        String current=System.getProperty("os.arch");
        if(arch.equals("x86_64")) arch="amd64";
        if(current.equals("x86_64")) current="amd64";
        if(!arch.isEmpty() && !arch.equalsIgnoreCase(current)) return false;
        String system=unquote(release.getProperty("OS_NAME",""));
        String actual=System.getProperty("os.name");
        if(system.equals("Darwin")) system="Mac OS X";
        if(!system.isEmpty() && !actual.toLowerCase(Locale.ROOT).startsWith(system.toLowerCase(Locale.ROOT))) return false;
        Path java=root.resolve(actual.startsWith("Windows")?"bin/java.exe":"bin/java");
        if(!Files.isRegularFile(java) || !actual.startsWith("Windows") && !Files.isExecutable(java)) return false;
        String vendor=criteria.getProperty("toolchainVendor");
        if(vendor!=null) {
            String expected=vendor.equalsIgnoreCase("ADOPTIUM")?"Eclipse Adoptium":vendor;
            if(!expected.equalsIgnoreCase(unquote(release.getProperty("IMPLEMENTOR","")))) return false;
        }
        String implementation=criteria.getProperty("toolchainImplementation","VENDOR_SPECIFIC");
        if(implementation.equalsIgnoreCase("VENDOR_SPECIFIC") || implementation.equalsIgnoreCase("vendor-specific")) return true;
        return implementation.equalsIgnoreCase("J9") && unquote(release.getProperty("JVM_VARIANT","")).equalsIgnoreCase("Openj9");
    }
    private static String unquote(String value) {
        return value.length()>1 && value.startsWith("\"") && value.endsWith("\"") ? value.substring(1,value.length()-1) : value;
    }
}
