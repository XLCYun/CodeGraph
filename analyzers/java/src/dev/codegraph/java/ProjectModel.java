package dev.codegraph.java;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.NoSuchFileException;
import java.nio.file.Path;
import java.nio.file.attribute.BasicFileAttributes;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import org.eclipse.jdt.core.JavaCore;
import org.eclipse.jdt.core.dom.AST;
import org.eclipse.jdt.core.dom.ASTParser;
import org.eclipse.jdt.core.dom.FileASTRequestor;
import org.eclipse.core.runtime.IProgressMonitor;
import static dev.codegraph.java.Main.Failure;

/** 源码归属、可选目标声明和只读平台匹配。 */
final class ProjectModel implements AutoCloseable {
    private SourceSnapshot snapshot;
    private boolean discardedModel;
    private final Path repository;
    private final List<Configuration> configurations;
    private final Map<String, List<Configuration>> selected;
    private final List<String> files;
    private final boolean includeTests;
    private static final int MAX_FILES = 100000;
    private final Checkpoint checkpoint;
    private final Map<String, List<String>> inventories = new HashMap<>();

    /** 将目录和配置加载纳入同一次请求的期限。 */
    @FunctionalInterface
    interface Checkpoint { void check() throws Failure; }

    /** 一份静态配置所控制的源码根及 release。 */
    private static final class Configuration {
        final String id;
        final String release;
        final List<String> sourcePaths;
        final List<SourceRoot> roots;
        Target target;
        ImportedProject.SourceSet imported;
        Set<String> memberFiles;
        boolean selectionOnly;
        final Set<Boolean> moduleContexts = new HashSet<>();
        List<SourceRoot> visibleRoots = List.of();

        /** 保存已验证的单一配置。 */
        Configuration(String id, String release, List<String> sourcePaths, List<SourceRoot> roots) {
            this(id, release, sourcePaths, roots, null);
        }

        /** 为配置绑定其 release 对应的唯一真实 JDK。 */
        Configuration(String id, String release, List<String> sourcePaths, List<SourceRoot> roots, Target target) {
            this.id = id;
            this.release = release;
            this.sourcePaths = sourcePaths;
            this.roots = roots;
            this.target = target;
        }
    }

    /** 有明确生产/测试归属的源码根。 */
    private static final class SourceRoot {
        final String path;
        final boolean test;

        /** 记录一个相对仓库的源码根。 */
        SourceRoot(String path, boolean test) { this.path = path; this.test = test; }
    }

    /** 完成路径及静态来源检查后保存请求模型。 */
    private ProjectModel(Path repository, List<Configuration> configurations,
                         Map<String, List<Configuration>> selected, List<String> files, boolean includeTests, Checkpoint checkpoint) {
        this.repository = repository;
        this.configurations = configurations;
        this.selected = selected;
        this.files = files;
        this.includeTests = includeTests;
        this.checkpoint = checkpoint;
    }

    /** 从固定输入发现 standalone 声明或有限 Maven POM。 */
    static ProjectModel discover(AnalysisInput input, SdkDiscovery.Provider targetProvider, Checkpoint checkpoint) throws Failure {
        boolean discovery = input.files() == null;
        Path root = input.repository();
        if (!root.isAbsolute() || !Files.isDirectory(root)) throw new Failure("invalid_input", "invalid repository_root");
        try { root = root.toRealPath(); } catch (Exception e) { throw new Failure("invalid_input", "unreadable repository_root"); }
        boolean includeTests = input.includeTests();
        List<String> requested = discovery ? List.of() : input.files();
        if (requested.size() > MAX_FILES) throw new Failure("resource_limit", "Java selection exceeds file limit");
        List<String> files = new ArrayList<>();
        Set<String> unique = new HashSet<>();
        for (String file : requested) {
            validateRelative(file);
            if (!file.endsWith(".java") || !unique.add(file)) throw new Failure("invalid_input", "duplicate or non-Java file: " + file);
            files.add(file);
        }
        for (String file : files) { checkpoint.check(); checkedFile(root, file); }
        if (!discovery && files.isEmpty())
            return new ProjectModel(root, List.of(), Map.of(), files, includeTests, checkpoint);
        List<SdkDiscovery.Candidate> platforms = targetProvider.discover();
        for (SdkDiscovery.Candidate platform : platforms) {
            Path sdk = platform.root().toAbsolutePath().normalize();
            if (sdk.startsWith(root) || root.startsWith(sdk))
                throw new Failure("invalid_input", "target JDK root overlaps repository");
        }
        SourceSnapshot snapshot = SourceSnapshot.capture(root, files, checkpoint);
        Path originalRoot = root;
        root = snapshot.root();
        try {
            List<Configuration> configs = new ArrayList<>();
            boolean discardedModel = false;
            Path metadata = root.resolve("narrant-java.json");
            Path pom = root.resolve("pom.xml");
            try {
                if ((Files.exists(metadata) ? 1 : 0) + (Files.exists(pom) ? 1 : 0) + (GradleProjectImporter.declared(root) ? 1 : 0) > 1)
                    throw new Failure("unsupported_configuration", "ambiguous Java metadata and POM");
                if (Files.exists(metadata)) {
                    for (Configuration config : readMetadata(originalRoot, originalRoot.resolve("narrant-java.json"), checkpoint))
                        for (SourceRoot source : config.roots) snapshot.validatePath(source.path, true);
                    configs.addAll(readMetadata(root, metadata, checkpoint));
                }
                else if (Files.exists(pom)) configs.addAll(imported(MavenProjectImporter.load(snapshot, platforms, checkpoint), root));
                else if (GradleProjectImporter.declared(root)) configs.addAll(imported(GradleProjectImporter.load(snapshot, platforms, checkpoint), root));
                else configs.addAll(infer(root, discovery ? null : files, checkpoint));
            } catch (Failure failure) {
                if (discovery || failure instanceof BoundaryFailure ||
                    !Set.of("unsupported_configuration", "invalid_input").contains(failure.code)) throw failure;
                snapshot.verify(); // Never conceal changed input or copy state behind explicit-file fallback.
                discardedModel = true;
                configs.clear();
                configs.addAll(infer(root, files, checkpoint));
                for (Configuration config : configs) config.selectionOnly = true;
            }
            for (Configuration config : configs) if (!config.id.startsWith("java-config:inferred:"))
                for (SourceRoot source : config.roots) snapshot.validatePath(source.path, true);
            if (discovery) Observation.record(input.id());
            Map<String, List<Configuration>> selected = new HashMap<>();
            ProjectModel model = new ProjectModel(root, configs, selected, files, includeTests, checkpoint);
            model.discardedModel = discardedModel;
            if (discovery) {
                for (Configuration config : configs) for (SourceRoot sourceRoot : config.roots)
                    for (String file : model.inventory(sourceRoot)) {
                        checkpoint.check();
                        if (confirmedMember(config,file) && snapshot.originalMember(file) && unique.add(file)) files.add(file);
                        if (files.size() > MAX_FILES) throw new Failure("resource_limit", "Java discovery exceeds file limit");
                    }
                files.sort(Comparator.naturalOrder());
            }
            model.selectFiles();
            // Enumerate and check paths outside the recoverable validation boundary. Only invalid
            // compiler/encoding evidence below may discard an otherwise imported model.
            List<ParseConfiguration> parsing = new ArrayList<>();
            model.forEachActiveConfiguration(parsing::add);
            try {
                model.validateCompilerContexts(parsing);
            } catch (Failure failure) {
                if (discovery || failure instanceof BoundaryFailure || !failure.code.equals("unsupported_configuration")) throw failure;
                snapshot.verify();
                model.discardedModel = true;
                configs.clear();
                configs.addAll(infer(root, files, checkpoint));
                for (Configuration config : configs) config.selectionOnly = true;
                selected.clear();
                model.selectFiles();
                parsing.clear();
                model.forEachActiveConfiguration(parsing::add);
            }
            for (ParseConfiguration parse : parsing)
                if (model.unsupportedModules(parse, snapshot)) parse.config.moduleContexts.add(parse.tests);
            for (Configuration config : configs) {
                if (files.stream().anyMatch(file -> model.activeConfigurations(file).contains(config)))
                    config.target = config.imported != null && config.imported.compiler().apiUnavailable() ? null :
                        chooseTarget(originalRoot, platforms, apiTarget(config));
            }
            snapshot.verify();
            model.snapshot = snapshot;
            return model;
        } catch (Failure | RuntimeException e) { snapshot.close(); throw e; }
    }

    private void selectFiles() throws Failure {
        for (String file : files) {
            checkpoint.check();
            List<Configuration> matches = new ArrayList<>();
            for (Configuration config : configurations) for (SourceRoot sourceRoot : config.roots) {
                if (confirmedMember(config,file) && under(file, sourceRoot.path) && (!config.id.startsWith("java-config:inferred:") ||
                    inferRoot(repository, file).equals(sourceRoot.path))) {
                    if (!matches.contains(config)) matches.add(config);
                }
            }
            if (matches.isEmpty()) {
                Configuration independent = infer(repository, List.of(file), checkpoint).getFirst();
                if (configurations.stream().anyMatch(config -> config.imported != null && config.imported.memberFiles() != null))
                    independent.selectionOnly = true;
                configurations.add(independent);
                matches.add(independent);
            }
            selected.put(file, List.copyOf(matches));
        }
    }

    /** Validate only compiler evidence and encodings that will be consumed by active parsing. */
    private void validateCompilerContexts(List<ParseConfiguration> parsing) throws Failure {
        for (ParseConfiguration parse : parsing) {
            Configuration config = parse.config;
            apiTarget(config);
            if (config.imported != null) {
                var compiler = config.imported.compiler();
                for (String version : new String[] { compiler.source(), compiler.target(), compiler.toolchain() })
                    if (version != null) validateRelease(version.startsWith("1.") ? version.substring(2) : version);
            }
            for (String file : parse.context) encoding(file);
            rootEncodings(config, parse.tests);
        }
    }

    /** Descriptors remain context evidence even when omitted from the output selection/members. */
    private boolean unsupportedModules(ParseConfiguration parse, SourceSnapshot snapshot) throws Failure {
        if (parse.config.imported != null && parse.config.imported.compiler().moduleVisibilityUnavailable()) return true;
        if (parse.config.imported != null) for (String visible : parse.config.imported.visibleSourceSets())
            if (configurations.stream().anyMatch(config -> config.id.equals(visible) &&
                    config.imported.compiler().moduleVisibilityUnavailable())) return true;
        for (SourceRoot source : parsingRoots(parse.config)) {
            if (source.test && !parse.tests) continue;
            String relative = (source.path.isEmpty() ? "" : source.path + "/") + "module-info.java";
            checkpoint.check();
            snapshot.validatePath(relative, false);
            try {
                Files.readAttributes(repository.resolve(relative), BasicFileAttributes.class, LinkOption.NOFOLLOW_LINKS);
                checkedFile(repository, relative);
                return true;
            } catch (NoSuchFileException absent) { /* A nonmodular source root is supported. */ }
            catch (IOException | SecurityException failure) {
                throw new Failure("object_extraction_failed", "cannot inspect Java module descriptor");
            }
        }
        return false;
    }

    /** Fixed bounded messages never interpolate model logs, host paths or credentials. */
    List<String> diagnostics() throws Failure {
        Set<String> reasons = new java.util.TreeSet<>();
        if (discardedModel) reasons.add("model_discarded: Failed project context was discarded; analyzing the explicit selection.");
        for (Configuration config : configurations) {
            if (files.stream().noneMatch(file -> activeConfigurations(file).contains(config))) continue;
            if (!config.moduleContexts.isEmpty())
                reasons.add("module_visibility_unavailable: Module visibility could not be proved; bindings are disabled.");
            if (config.imported != null && config.imported.compiler().apiUnavailable())
                reasons.add("compiler_unavailable: Explicit compiler context could not be proved; bindings are disabled.");
            else if (config.target == null)
                reasons.add(apiTarget(config) == null
                    ? "platform_unavailable: No analysis platform is available; bindings are disabled."
                    : "target_platform_missing: The declared API target has no matching local SDK; bindings are disabled.");
            else if (apiTarget(config) == null)
                reasons.add("default_platform: Project API target is unknown; reliable bindings use the default analysis platform.");
            if (config.imported != null && !config.imported.dependenciesComplete())
                reasons.add("context_incomplete: Dependencies or source context are incomplete; unproved references remain unresolved.");
        }
        return reasons.stream().map(reason -> "codegraph-java: " + reason).toList();
    }

    private static List<Configuration> imported(ImportedProject project, Path root) throws Failure {
        Map<String, ImportedProject.SourceSet> sets = new HashMap<>();
        for (var set : project.sourceSets()) sets.put(set.id(), set);
        List<Configuration> result = new ArrayList<>();
        for (var set : project.sourceSets()) {
            String target = set.compiler().release();
            if (target != null && target.startsWith("1.")) target = target.substring(2);
            if (target != null) validateRelease(target);
            List<SourceRoot> own = new ArrayList<>();
            for (String path : set.roots()) { checkedRoot(root, path); own.add(new SourceRoot(path, set.test())); }
            Configuration config = new Configuration(set.id(), target, set.modelInputs(), own);
            config.imported = set;
            config.memberFiles = set.memberFiles() == null ? null : Set.copyOf(set.memberFiles());
            List<SourceRoot> visible = new ArrayList<>();
            for (String id : set.visibleSourceSets()) {
                var other = sets.get(id);
                if (other == null) throw new Failure("unsupported_configuration", "unknown visible source set");
                for (String path : other.roots()) { checkedRoot(root, path); visible.add(new SourceRoot(path, false)); }
            }
            config.visibleRoots = List.copyOf(visible);
            result.add(config);
        }
        return result;
    }

    /** Bytecode target and source syntax do not identify the API platform. */
    private static String apiTarget(Configuration config) throws Failure {
        String value = config.release;
        if (value == null && config.imported != null) value = config.imported.compiler().toolchain();
        if (value != null && value.startsWith("1.")) value = value.substring(2);
        if (value != null) validateRelease(value);
        return value;
    }

    void verifyInputs() throws Failure { if (snapshot != null) snapshot.verify(); }
    @Override public void close() throws Failure { if (snapshot != null) snapshot.close(); }


    private static boolean under(String file, String root) {
        return root.isEmpty() || file.startsWith(root + "/");
    }

    /** Infer independent source contexts without traversing generated or vendored trees. */
    private static List<Configuration> infer(Path repository, List<String> selection, Checkpoint checkpoint) throws Failure {
        List<String> sources = selection == null ? discoverSources(repository, checkpoint) : selection;
        Map<String, SourceRoot> roots = new java.util.TreeMap<>();
        for (String file : sources) {
            checkpoint.check();
            String root = inferRoot(repository, file);
            boolean test = ("/" + root + "/").contains("/src/test/") || root.equals("test") || root.equals("tests");
            roots.putIfAbsent(root, new SourceRoot(root, test));
        }
        for (SourceRoot source : List.copyOf(roots.values())) {
            if (source.path.endsWith("src/test/java")) {
                String main = source.path.substring(0, source.path.length() - "src/test/java".length()) + "src/main/java";
                if (Files.isDirectory(repository.resolve(main))) roots.putIfAbsent(main, new SourceRoot(main, false));
            }
        }
        List<Configuration> configs = new ArrayList<>();
        Set<String> pairedTests = new HashSet<>();
        for (SourceRoot source : roots.values()) if (source.path.endsWith("src/main/java")) {
            String test = source.path.substring(0, source.path.length() - "src/main/java".length()) + "src/test/java";
            if (roots.containsKey(test)) pairedTests.add(test);
        }
        for (SourceRoot source : roots.values()) {
            if (pairedTests.contains(source.path)) continue;
            List<SourceRoot> members = new ArrayList<>(List.of(source));
            if (source.path.endsWith("src/main/java")) {
                String test = source.path.substring(0, source.path.length() - "src/main/java".length()) + "src/test/java";
                if (pairedTests.contains(test)) members.add(roots.get(test));
            }
            configs.add(new Configuration("java-config:inferred:" +
                (source.path.isEmpty() ? "." : source.path), null, List.of(), List.copyOf(members)));
        }
        return configs;
    }

    private static String inferRoot(Path repository, String file) throws Failure {
        String source;
        try { source = Files.readString(checkedFile(repository, file)); }
        catch (IOException e) { throw new Failure("object_extraction_failed", "cannot read selected source", List.of(file)); }
        String root = file.contains("/") ? file.substring(0, file.lastIndexOf('/')) : "";
        ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
        Map<String, String> options = new HashMap<>();
        JavaCore.setComplianceOptions(JavaCore.latestSupportedJavaVersion(), options);
        parser.setCompilerOptions(options);
        parser.setSource(source.toCharArray());
        var unit = (org.eclipse.jdt.core.dom.CompilationUnit) parser.createAST(null);
        if (unit.getPackage() != null) {
            String suffix = unit.getPackage().getName().getFullyQualifiedName().replace('.', '/');
            if (root.equals(suffix)) root = "";
            else if (root.endsWith("/" + suffix)) root = root.substring(0, root.length() - suffix.length() - 1);
        }
        return root;
    }

    private static List<String> discoverSources(Path repository, Checkpoint checkpoint) throws Failure {
        List<String> files = new ArrayList<>();
        discoverSources(repository, repository, files, checkpoint);
        return files;
    }

    private static void discoverSources(Path repository, Path directory, List<String> files, Checkpoint checkpoint) throws Failure {
        Set<String> excluded = Set.of(".git", ".gradle", ".idea", ".tools", "build", "target", "out", "vendor", "node_modules");
        checkpoint.check();
        try (var entries = Files.newDirectoryStream(directory)) {
            for (Path path : entries) {
                checkpoint.check();
                if (Files.isSymbolicLink(path) || excluded.contains(path.getFileName().toString())) continue;
                if (Files.isDirectory(path)) discoverSources(repository, path, files, checkpoint);
                else if (path.toString().endsWith(".java") && Files.isRegularFile(path)) {
                    files.add(repository.relativize(path).toString().replace('\\', '/'));
                    if (files.size() > MAX_FILES) throw new Failure("resource_limit", "Java discovery exceeds file limit");
                }
            }
        } catch (IOException e) { throw new Failure("object_extraction_failed", "cannot enumerate source root"); }
    }

    /** Stable semantic provenance belongs on nodes, never host paths or environment variable names. */
    void addPlatformExtensions(String file, JsonObject extensions) {
        JsonArray platforms = new JsonArray();
        for (Configuration config : activeConfigurations(file)) {
            JsonObject platform = new JsonObject();
            platform.addProperty("target", config.release == null ? "unknown" : config.release);
            platform.addProperty("status", config.target == null ? "missing" : "available");
            if (config.target != null) platform.addProperty("version", config.target.version());
            if (config.imported != null) {
                var compiler = config.imported.compiler();
                platform.addProperty("api_source", compiler.release() != null ? "release" : compiler.toolchain() != null ? "toolchain" : "default");
                JsonObject evidence = new JsonObject();
                if (compiler.release() != null) evidence.addProperty("release", compiler.release());
                if (compiler.source() != null) evidence.addProperty("source", compiler.source());
                if (compiler.target() != null) evidence.addProperty("target", compiler.target());
                if (compiler.toolchain() != null) evidence.addProperty("toolchain", compiler.toolchain());
                if (compiler.encoding() != null) evidence.addProperty("encoding", compiler.encoding());
                platform.add("compiler", evidence);
                platform.addProperty("dependencies_complete", config.imported.dependenciesComplete());
            }
            if (!platforms.contains(platform)) platforms.add(platform);
        }
        List<JsonElement> ordered = new ArrayList<>();
        platforms.forEach(ordered::add);
        ordered.sort(Comparator.comparing(JsonElement::toString));
        JsonArray stable = new JsonArray();
        ordered.forEach(stable::add);
        extensions.add("java.platforms", stable);
    }

    /** Boundary failures must never become a successful model fallback. */
    static final class BoundaryFailure extends Failure {
        BoundaryFailure(String code, String message) { super(code, message); }
        BoundaryFailure(String code, String message, List<String> paths) { super(code, message, paths); }
    }

    /** 仅接受规范仓内相对路径。 */
    static void validateRelative(String path) throws Failure {
        if (path.isEmpty() || path.startsWith("/") || path.indexOf('\\') >= 0 || path.indexOf('\0') >= 0 ||
            path.indexOf(':') >= 0) throw new BoundaryFailure("invalid_input", "invalid relative path");
        for (String part : path.split("/", -1)) if (part.isEmpty() || part.equals(".") || part.equals(".."))
            throw new BoundaryFailure("invalid_input", "invalid relative path");
    }

    /** 对每次读取执行真实路径越界检查。 */
    static Path checkedFile(Path root, String relative) throws Failure {
        try {
            Path path = root.resolve(relative).toRealPath();
            if (!path.startsWith(root) || !Files.isRegularFile(path) || Files.isSymbolicLink(root.resolve(relative)))
                throw new Failure("invalid_input", "file outside repository", List.of(relative));
            return path;
        } catch (Failure e) { throw e; }
        catch (Exception e) { throw new Failure("object_extraction_failed", "cannot read selected source", List.of(relative)); }
    }

    /** 验证目标声明与 JDK release 文件严格一致。 */
    private static Target chooseTarget(Path repository, List<SdkDiscovery.Candidate> platforms, String release) throws Failure {
        Target chosen = null;
        Set<String> ids = new HashSet<>();
        for (SdkDiscovery.Candidate platform : platforms) {
            String id = platform.id();
            if (id.isEmpty() || !ids.add(id)) throw new Failure("invalid_input", "duplicate platform id");
            String version = platform.version();
            if (version.isEmpty()) throw new Failure("invalid_input", "empty platform version");
            if (release == null ? !platform.defaultEligible() : !release.equals(platform.release())) continue;
            Path path = platform.root();
            if (!path.isAbsolute()) throw new Failure("invalid_input", "target JDK root must be absolute");
            try {
                path = path.toRealPath();
                // 标准声明必须来自仓外 SDK；共同祖先同样会扩大来源授权。
                if (path.startsWith(repository) || repository.startsWith(path))
                    throw new Failure("invalid_input", "target JDK root overlaps repository");
                String data = Files.readString(path.resolve("release"), StandardCharsets.UTF_8);
                Matcher match = Pattern.compile("(?m)^JAVA_VERSION=\"([^\"\\r\\n]+)\"\\s*$").matcher(data);
                if (!match.find() || !version.equals(match.group(1)) || !platform.release().equals(jdkRelease(version)))
                    throw new Failure("target_unavailable", "catalog JDK does not match target release");
            } catch (Failure e) { throw e; }
            catch (Exception e) { throw new Failure("target_unavailable", "target JDK unavailable"); }
            if (chosen != null) {
                if (release != null) throw new Failure("invalid_input", "ambiguous target JDK platforms");
                continue;
            }
            chosen = new Target(id, path, systemLibraries(path, platform.release()), platform.version());
            if (release == null) break;
        }
        return chosen;
    }

    /** 从 release 文件的真实版本读取稳定 major，兼容 1.8.0_XXX 格式。 */
    private static String jdkRelease(String version) throws Failure {
        long[] parts = SdkDiscovery.stableVersion(version);
        if (parts != null) return Long.toString(parts[0]);
        throw new Failure("target_unavailable", "target JDK has no stable Java version");
    }

    /** 仅从目标 JDK 读取系统库，不纳入扩展目录、项目 JAR 或运行时系统库。 */
    private static List<String> systemLibraries(Path target, String release) throws Failure {
        List<String> libraries = new ArrayList<>();
        if (Integer.parseInt(release) < 9) {
            Path directory = target.resolve("jre/lib");
            if (!Files.isRegularFile(directory.resolve("rt.jar")))
                throw new Failure("target_unavailable", "target JDK lacks classic jre/lib/rt.jar");
            for (String name : List.of("resources.jar", "rt.jar", "jsse.jar", "jce.jar", "charsets.jar")) {
                Path archive = directory.resolve(name);
                if (Files.isRegularFile(archive)) libraries.add(archive.toString());
            }
        } else {
            Path archive = target.resolve("lib/jrt-fs.jar");
            if (!Files.isRegularFile(archive) || !Files.isRegularFile(target.resolve("lib/modules")))
                throw new Failure("target_unavailable", "target JDK lacks lib/jrt-fs.jar or lib/modules");
            libraries.add(archive.toString());
        }
        return List.copyOf(libraries);
    }

    /** 匹配后的显式平台标识、真实 JDK 路径及该版本的标准库。 */
    record Target(String id, Path path, List<String> systemLibraries, String version) {}

    /** 读取版本化的声明式源码根。 */
    private static List<Configuration> readMetadata(Path repository, Path file, Checkpoint checkpoint) throws Failure {
        try {
            Path actual = file.toRealPath();
            if (!actual.startsWith(repository) || !Files.isRegularFile(actual))
                throw new BoundaryFailure("unsupported_configuration", "Java metadata outside repository");
            JsonObject metadata = Main.parseObject(Files.readString(actual, StandardCharsets.UTF_8));
            List<Configuration> configurations = new ArrayList<>();
            if (metadata.has("configurations")) {
                Main.fields(metadata, Set.of("schema", "configurations"));
                Main.requireString(metadata, "schema", Main.PROFILE);
                Set<String> ids = new HashSet<>();
                for (JsonElement element : Main.array(metadata, "configurations")) {
                    checkpoint.check();
                    if (!element.isJsonObject()) throw new Failure("unsupported_configuration", "invalid Java configuration");
                    JsonObject entry = element.getAsJsonObject();
                    Main.fields(entry, entry.has("release") ? Set.of("id", "release", "source_roots") : Set.of("id", "source_roots"));
                    String id = Main.string(entry, "id");
                    if (id.isEmpty() || !ids.add(id)) throw new Failure("unsupported_configuration", "duplicate Java configuration id");
                    String release = entry.has("release") ? Main.string(entry, "release") : null;
                    if (release != null) validateRelease(release);
                    configurations.add(new Configuration("java-config:metadata:" + id, release,
                        List.of("narrant-java.json"), readRoots(repository, entry, checkpoint)));
                }
            } else {
                Main.fields(metadata, metadata.has("release") ? Set.of("schema", "release", "source_roots") : Set.of("schema", "source_roots"));
                Main.requireString(metadata, "schema", Main.PROFILE);
                String release = metadata.has("release") ? Main.string(metadata, "release") : null;
                if (release != null) validateRelease(release);
                configurations.add(new Configuration("java-config:metadata", release,
                    List.of("narrant-java.json"), readRoots(repository, metadata, checkpoint)));
            }
            if (configurations.isEmpty()) throw new Failure("unsupported_configuration", "no Java configurations");
            return configurations;
        } catch (Failure e) { throw e; }
        catch (IOException e) { throw new Failure("object_extraction_failed", "cannot read narrant-java.json"); }
        catch (Exception e) { throw new Failure("unsupported_configuration", "invalid narrant-java.json"); }
    }

    /** 读取并校验一个明确配置的源码根。 */
    private static List<SourceRoot> readRoots(Path repository, JsonObject metadata, Checkpoint checkpoint) throws Failure {
        List<SourceRoot> roots = new ArrayList<>();
        Set<String> paths = new HashSet<>();
        for (JsonElement item : Main.array(metadata, "source_roots")) {
                checkpoint.check();
                if (!item.isJsonObject()) throw new Failure("unsupported_configuration", "invalid source root");
                JsonObject entry = item.getAsJsonObject();
                Main.fields(entry, Set.of("path", "role"));
                String path = Main.string(entry, "path");
                validateRelative(path);
                if (!paths.add(path)) throw new Failure("unsupported_configuration", "duplicate source root");
                String role = Main.string(entry, "role");
                if (!role.equals("main") && !role.equals("test"))
                    throw new Failure("unsupported_configuration", "unknown source root role");
                Path actual;
                try { actual = repository.resolve(path).toRealPath(); }
                catch (NoSuchFileException problem) { throw new Failure("unsupported_configuration", "source root missing: " + path); }
                catch (IOException problem) { throw new Failure("object_extraction_failed", "cannot read source root", List.of(path)); }
                if (!actual.startsWith(repository))
                    throw new BoundaryFailure("unsupported_configuration", "source root outside repository", List.of(path));
                if (!Files.isDirectory(actual))
                    throw new Failure("unsupported_configuration", "source root missing: " + path);
                roots.add(new SourceRoot(path, role.equals("test")));
        }
        if (roots.isEmpty()) throw new Failure("unsupported_configuration", "source_roots is empty");
        return roots;
    }

    /** 按固定 JDT 编译器实际能力校验规范 release，不接受预览或别名。 */
    private static void validateRelease(String release) throws Failure {
        if (!release.matches("[1-9][0-9]*") || !JavaCore.isJavaSourceVersionSupportedByCompiler(compilerVersion(release)))
            throw new Failure("unsupported_configuration", "unsupported Java release: " + release);
    }

    /** 将用户使用的整数 release 转换为 JDT 历史版本命名。 */
    static String compilerVersion(String release) {
        return release.length() == 1 && release.charAt(0) < '9' ? "1." + release : release;
    }

    /** 将实际需要解析的配置依次交给分析器，保持配置级期限与错误顺序。 */
    void forEachActiveConfiguration(ConfigurationAction action) throws Failure {
        for (Configuration config : configurations) {
            checkpoint.check();
            if (files.stream().noneMatch(file -> activeConfigurations(file).contains(config))) continue;
            if (files.stream().anyMatch(file -> activeConfigurations(file).contains(config) && !isTest(file, config))) {
                List<String> context = context(config, false);
                if (!context.isEmpty()) action.accept(new ParseConfiguration(config, context, false));
            }
            if (includeTests && files.stream().anyMatch(file -> activeConfigurations(file).contains(config) && isTest(file, config))) {
                List<String> context = context(config, true);
                if (!context.isEmpty()) action.accept(new ParseConfiguration(config, context, true));
            }
        }
    }

    /** 一次配置解析的调用边界，JDT 绑定索引仍由分析器在此边界内独立创建。 */
    @FunctionalInterface
    interface ConfigurationAction { void accept(ParseConfiguration config) throws Failure; }

    /** 封装一份配置的解析环境、上下文及本次选集，不暴露底层集合。 */
    final class ParseConfiguration {
        private final Configuration config;
        private final List<String> context;
        private final boolean tests;

        /** 保存已经按角色过滤并验证的本次配置上下文。 */
        private ParseConfiguration(Configuration config, List<String> context, boolean tests) {
            this.config = config;
            this.context = context;
            this.tests = tests;
        }

        /** 用该配置自己的 release、系统库和源码根执行一次 JDT 解析。 */
        void parse(FileASTRequestor requestor, IProgressMonitor monitor) throws Failure, IOException {
            ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
            Map<String, String> options = new HashMap<>();
            JavaCore.setComplianceOptions(config.release == null ? JavaCore.latestSupportedJavaVersion() : compilerVersion(config.release), options);
            options.put(JavaCore.COMPILER_RELEASE, config.release != null ? JavaCore.ENABLED : JavaCore.DISABLED);
            // JDT requires modular libraries at compliance 9+, even when source syntax is independent.
            // A default/toolchain-only Java 8 platform supplies rt.jar; keep modern syntax without
            // inventing a project release or importing the runtime JVM's newer APIs.
            if (config.release == null && config.target != null && jdkRelease(config.target.version()).equals("8")) {
                options.put(JavaCore.COMPILER_COMPLIANCE, JavaCore.VERSION_1_8);
                options.put(JavaCore.COMPILER_CODEGEN_TARGET_PLATFORM, JavaCore.VERSION_1_8);
            }
            if (config.imported != null && config.imported.compiler().source() != null) {
                String source = config.imported.compiler().source();
                options.put(JavaCore.COMPILER_SOURCE, source.startsWith("1.") ? source : compilerVersion(source));
            }
            if (config.imported != null && config.imported.compiler().target() != null && config.release == null) {
                String target = config.imported.compiler().target();
                options.put(JavaCore.COMPILER_CODEGEN_TARGET_PLATFORM, target.startsWith("1.") ? target : compilerVersion(target));
            }
            options.put(JavaCore.COMPILER_PB_ENABLE_PREVIEW_FEATURES, JavaCore.DISABLED);
            parser.setCompilerOptions(options);
            parser.setResolveBindings(config.target != null && !config.moduleContexts.contains(tests));
            parser.setBindingsRecovery(true);
            parser.setStatementsRecovery(true);
            List<String> roots = new ArrayList<>();
            for (SourceRoot root : parsingRoots(config))
                if (!config.selectionOnly && !confirmedMembers(config) && (!root.test || tests) && Files.isDirectory(repository.resolve(root.path))) roots.add(repository.resolve(root.path).toString());
            if (config.target == null || config.moduleContexts.contains(tests)) {
                // Parse syntax without any environment or binding recovery; compiler options remain explicit.
                for (String file : context) {
                    checkpoint.check();
                    ASTParser syntax = ASTParser.newParser(AST.getJLSLatest());
                    syntax.setCompilerOptions(options);
                    syntax.setStatementsRecovery(true);
                    syntax.setSource(parserSource(file, readSource(file)).toCharArray());
                    syntax.setUnitName(file);
                    requestor.acceptAST(repository.resolve(file).toString(),
                        (org.eclipse.jdt.core.dom.CompilationUnit) syntax.createAST(monitor));
                }
                return;
            }
            List<String> classpath = new ArrayList<>(config.target.systemLibraries());
            if (config.imported != null) classpath.addAll(config.imported.binaries());
            parser.setEnvironment(classpath.toArray(String[]::new),
                roots.toArray(String[]::new), rootEncodings(config, tests), false);
            String[] paths = context.stream().map(file -> repository.resolve(file).toString()).toArray(String[]::new);
            String[] encodings = new String[context.size()];
            for (int i = 0; i < encodings.length; i++) encodings[i] = encoding(context.get(i));
            parser.createASTs(paths, encodings, new String[0], requestor, monitor);
        }

        /** Match JDT's UTF-8 file decoder while retaining raw text separately for file.source. */
        String parserSource(String file, String source) throws Failure {
            return encoding(file).equals("UTF-8") && source.startsWith("\ufeff") ? source.substring(1) : source;
        }

        /** 只有选中且在当前配置中未被测试过滤的文件才贡献图节点。 */
        boolean selects(String file) {
            return selected.containsKey(file) && activeConfigurations(file).contains(config) && isTest(file, config) == tests;
        }

        /** 包名使用第一个匹配的声明根，重叠根仍保留原有顺序语义。 */
        void verifyPackage(String file, String actual) throws Failure {
            // Standalone selections need not be stored in their package's conventional directory.
            if (config.id.startsWith("java-config:inferred:")) return;
            String expected = null;
            for (SourceRoot root : parsingRoots(config)) {
                if (!under(file, root.path)) continue;
                String local = file.substring(root.path.isEmpty() ? 0 : root.path.length() + 1);
                int slash = local.lastIndexOf('/');
                expected = slash < 0 ? "" : local.substring(0, slash).replace('/', '.');
                break;
            }
            if (expected == null) throw new Failure("object_extraction_failed", "source outside configuration roots", List.of(file));
            if (!expected.equals(actual))
                throw new Failure("object_extraction_failed", "package does not match source root", List.of(file));
        }

        /** 保留上下文完整性检查，不允许 JDT 静默遗漏应解析的源码。 */
        void requireParsed(Set<String> parsed) throws Failure {
            for (String file : context) if (!parsed.contains(file))
                throw new Failure("object_extraction_failed", "JDT omitted source", List.of(file));
        }
    }

    private String[] rootEncodings(Configuration config, boolean tests) throws Failure {
        if (confirmedMembers(config)) return new String[0];
        List<String> result = new ArrayList<>();
        for (SourceRoot root : parsingRoots(config)) if (!config.selectionOnly && !confirmedMembers(config) && (!root.test || tests) && Files.isDirectory(repository.resolve(root.path))) result.add(encoding(root.path + "/Probe.java"));
        return result.toArray(String[]::new);
    }

    private String encoding(String file) throws Failure {
        String chosen = null;
        for (Configuration config : configurations) {
            if (config.imported == null || config.imported.compiler().encoding() == null ||
                (!confirmedMember(config,file) || config.roots.stream().noneMatch(root -> under(file, root.path)))) continue;
            String value;
            try { value = java.nio.charset.Charset.forName(config.imported.compiler().encoding()).name(); }
            catch (IllegalArgumentException e) { throw new Failure("unsupported_configuration", "unsupported Java source encoding"); }
            if (chosen != null && !chosen.equals(value)) throw new Failure("unsupported_configuration", "conflicting Java source encodings");
            chosen = value;
        }
        return chosen == null ? "UTF-8" : chosen;
    }

    /** 把 JDT 的真实源码路径还原为仓库身份。 */
    String relativePath(String path) {
        return repository.relativize(Path.of(path)).toString().replace('\\', '/');
    }

    /** 每次读取重新检查仓库边界，并拒绝被替换解码的非法源码。 */
    String readSource(String relative) throws IOException, Failure {
        Path file = checkedFile(repository, relative);
        byte[] bytes = snapshot == null ? Files.readAllBytes(file) : snapshot.readSourceBytes(relative);
        String source = java.nio.charset.Charset.forName(encoding(relative)).newDecoder()
            .decode(java.nio.ByteBuffer.wrap(bytes)).toString();
        if (source.indexOf('\uFFFD') >= 0)
            throw new Failure("object_extraction_failed", "invalid UTF-8 source", List.of(relative));
        return source;
    }

    /** 一次生成全部模型来源；纯测试选集没有活动配置时仍列出原匹配配置。 */
    JsonObject evidence() {
        JsonObject evidence = new JsonObject();
        JsonArray configs = new JsonArray();
        for (Configuration config : configurations) {
            JsonObject item = new JsonObject();
            item.addProperty("id", config.id);
            item.add("source_paths", Main.strings(config.sourcePaths));
            item.add("platform_ids", Main.strings(config.target == null ? List.of() : List.of(config.target.id())));
            configs.add(item);
        }
        evidence.add("configurations", configs);
        JsonArray selectedFiles = new JsonArray();
        for (String path : files) {
            JsonObject item = new JsonObject();
            item.addProperty("path", path);
            item.addProperty("disposition", !includeTests && isTest(path) ? "excluded_test" : "analyzed");
            List<Configuration> relevant = activeConfigurations(path);
            if (relevant.isEmpty()) relevant = selected.get(path);
            item.add("configuration_ids", Main.strings(relevant.stream().map(config -> config.id).toList()));
            selectedFiles.add(item);
        }
        evidence.add("files", selectedFiles);
        return evidence;
    }

    /** 在图边界复核模型授权、文件覆盖及来源路径，保留独立于生成过程的检查。 */
    void validateEvidence(JsonObject evidence) throws Failure {
        Map<String, JsonObject> configs = GraphChecks.indexed(Main.array(evidence, "configurations"), "id");
        if (files.size() > MAX_FILES) GraphChecks.fail("selection exceeds file limit");
        Set<String> approved = new HashSet<>();
        for (Configuration config : configurations) if (config.target != null) approved.add(config.target.id());
        for (JsonObject config : configs.values()) {
            for (String path : GraphChecks.strings(Main.array(config, "source_paths"))) checkedFile(repository, path);
            if (!approved.containsAll(GraphChecks.strings(Main.array(config, "platform_ids")))) GraphChecks.fail("unknown configuration platform");
        }
        Map<String, JsonObject> covered = GraphChecks.indexed(Main.array(evidence, "files"), "path");
        if (!covered.keySet().equals(new HashSet<>(files))) GraphChecks.fail("file coverage differs from selection");
        for (var entry : covered.entrySet()) {
            validateRelative(entry.getKey());
            checkedFile(repository, entry.getKey());
            JsonObject file = entry.getValue();
            String disposition = Main.string(file, "disposition");
            if (!Set.of("analyzed", "excluded_test").contains(disposition) || includeTests && disposition.equals("excluded_test"))
                GraphChecks.fail("invalid test exclusion");
            Set<String> ids = GraphChecks.strings(Main.array(file, "configuration_ids"));
            if (ids.isEmpty() || !configs.keySet().containsAll(ids)) GraphChecks.fail("missing file configuration");
        }
    }

    /** 判断所选文件在所有匹配配置中是否都仅属于测试根。 */
    private boolean isTest(String file) {
        return selected.get(file).stream().allMatch(config -> isTest(file, config));
    }

    /** 判断某配置中的文件是否仅来自测试根，重叠的生产根优先。 */
    private boolean isTest(String file, Configuration config) {
        boolean matched = false;
        for (SourceRoot root : config.roots) if (under(file, root.path)) {
            matched = true;
            if (!root.test) return false;
        }
        return matched;
    }

    /** 返回本次实际分析此文件的配置集合。 */
    private List<Configuration> activeConfigurations(String file) {
        return selected.get(file).stream().filter(config -> includeTests || !isTest(file, config)).toList();
    }

    /** 验证源码根的真实边界，禁止目录别名或外向链接参与枚举。 */
    private static Path checkedRoot(Path repository, String relative) throws Failure {
        Path directory = repository.resolve(relative);
        Path current = repository;
        for (Path part : Path.of(relative)) {
            current = current.resolve(part);
            if (Files.isSymbolicLink(current)) throw new BoundaryFailure("unsupported_configuration", "source directory symlink unsupported", List.of(relative));
        }
        if (!sourceDirectoryExists(directory, relative)) return directory;
        try {
            if (!directory.toRealPath().startsWith(repository))
                throw new BoundaryFailure("unsupported_configuration", "invalid source root", List.of(relative));
        } catch (Failure e) { throw e; }
        catch (Exception e) { throw new BoundaryFailure("unsupported_configuration", "cannot read source root", List.of(relative)); }
        return directory;
    }

    /** 只把属性读取明确报告的缺失目录视为空根，保留访问失败和其他 I/O 错误。 */
    private static boolean sourceDirectoryExists(Path directory, String relative) throws Failure {
        try {
            BasicFileAttributes attributes = Files.readAttributes(directory, BasicFileAttributes.class, LinkOption.NOFOLLOW_LINKS);
            if (!attributes.isDirectory())
                throw new BoundaryFailure("unsupported_configuration", "invalid source root", List.of(relative));
            return true;
        } catch (NoSuchFileException e) { return false; }
        catch (IOException | SecurityException e) {
            throw new BoundaryFailure("unsupported_configuration", "cannot read source root", List.of(relative));
        }
    }

    /** 对声明根仅枚举一次，发现与语义上下文共享有界且可取消的清单。 */
    private List<String> inventory(SourceRoot sourceRoot) throws Failure {
        checkpoint.check();
        if (inventories.containsKey(sourceRoot.path)) return inventories.get(sourceRoot.path);
        Path directory = checkedRoot(repository, sourceRoot.path);
        if (configurations.stream().anyMatch(c -> c.id.startsWith("java-config:inferred:") && c.roots.contains(sourceRoot))) {
            List<String> scanned = new ArrayList<>();
            discoverSources(repository, directory, scanned, checkpoint);
            List<String> members = new ArrayList<>();
            for (String file : scanned) {
                checkpoint.check();
                if (inferRoot(repository, file).equals(sourceRoot.path)) members.add(file);
            }
            List<String> result = members.stream().sorted().toList();
            inventories.put(sourceRoot.path, result);
            return result;
        }
        List<String> discovered = new ArrayList<>();
        if (sourceDirectoryExists(directory, sourceRoot.path)) try (var stream = Files.walk(directory)) {
            var paths = stream.iterator();
            while (paths.hasNext()) {
                checkpoint.check();
                Path path = paths.next();
                if (Files.isSymbolicLink(path))
                    throw new BoundaryFailure("unsupported_configuration", "source tree symlink unsupported");
                if (!Files.isRegularFile(path) || !path.toString().endsWith(".java")) continue;
                String relative = repository.relativize(path).toString().replace('\\', '/');
                validateRelative(relative);
                checkedFile(repository, relative);
                discovered.add(relative);
                if (discovered.size() > MAX_FILES) throw new Failure("resource_limit", "Java source root exceeds file limit");
            }
        } catch (Failure e) { throw e; }
        catch (Exception e) { throw new Failure("object_extraction_failed", "cannot enumerate source root"); }
        discovered.sort(Comparator.naturalOrder());
        List<String> result = List.copyOf(discovered);
        inventories.put(sourceRoot.path, result);
        return result;
    }

    private List<SourceRoot> parsingRoots(Configuration config) {
        List<SourceRoot> roots = new ArrayList<>(config.roots);
        roots.addAll(config.visibleRoots);
        return roots;
    }

    private static boolean confirmedMembers(Configuration config) {
        return config.memberFiles != null;
    }
    private static boolean confirmedMember(Configuration config,String file) {
        return !confirmedMembers(config) || config.memberFiles.contains(file);
    }

    /** 枚举配置允许的上下文源码，测试角色不改变生产配置。 */
    private List<String> context(Configuration config, boolean tests) throws Failure {
        if (config.selectionOnly) return files.stream().filter(file -> selected.get(file).contains(config) &&
            (!isTest(file, config) || tests)).sorted().toList();
        Set<String> discovered = new HashSet<>();
        if (confirmedMembers(config)) {
            discovered.addAll(config.imported.memberFiles());
            for (String visible : config.imported.visibleSourceSets()) {
                Configuration other = configurations.stream().filter(candidate -> candidate.id.equals(visible)).findFirst().orElseThrow();
                if (!confirmedMembers(other)) throw new Failure("unsupported_configuration", "mixed source membership model");
                discovered.addAll(other.imported.memberFiles());
            }
            if (discovered.size() > MAX_FILES) throw new Failure("resource_limit", "Java context exceeds file limit");
            return discovered.stream().sorted().toList();
        }
        for (SourceRoot sourceRoot : parsingRoots(config)) {
            checkpoint.check();
            if (sourceRoot.test && !tests) continue;
            for (String file : inventory(sourceRoot)) {
                checkpoint.check();
                discovered.add(file);
                if (discovered.size() > MAX_FILES) throw new Failure("resource_limit", "Java context exceeds file limit");
            }
        }
        return discovered.stream().sorted().toList();
    }
}
