package dev.codegraph.java;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import java.io.IOException;
import java.io.InputStream;
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
import javax.xml.XMLConstants;
import javax.xml.parsers.DocumentBuilderFactory;
import org.eclipse.jdt.core.JavaCore;
import org.eclipse.jdt.core.dom.AST;
import org.eclipse.jdt.core.dom.ASTParser;
import org.eclipse.jdt.core.dom.FileASTRequestor;
import org.eclipse.core.runtime.IProgressMonitor;
import org.w3c.dom.Document;
import org.w3c.dom.Element;
import org.w3c.dom.Node;
import org.w3c.dom.NodeList;
import static dev.codegraph.java.Main.Failure;

/** 仓库内有限静态配置及显式目标 JDK 模型。 */
final class ProjectModel {
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
        final Target target;

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
        // 明确缺少受支持配置的错误保持原有优先级；后续耗时加载保留内部期限。
        if ((discovery || !requested.isEmpty()) && !Files.exists(root.resolve("narrant-java.json")) &&
            !Files.exists(root.resolve("pom.xml")))
            throw new Failure("unsupported_configuration", "no explicit Java source roots or target release");
        for (String file : files) { checkpoint.check(); checkedFile(root, file); }
        if (!discovery && files.isEmpty())
            return new ProjectModel(root, List.of(), Map.of(), files, includeTests, checkpoint);
        List<Configuration> configs = new ArrayList<>();
        Path metadata = root.resolve("narrant-java.json");
        Path pom = root.resolve("pom.xml");
        if (Files.exists(metadata) && Files.exists(pom))
            throw new Failure("unsupported_configuration", "ambiguous Java metadata and POM");
        if (Files.exists(metadata)) configs.addAll(readMetadata(root, metadata, checkpoint));
        else if (Files.exists(pom)) readPomTree(root, root, configs, new HashSet<>(), new HashMap<>(), new HashMap<>(), checkpoint);
        else if (Files.exists(root.resolve("build.gradle")) || Files.exists(root.resolve("build.gradle.kts")) ||
                 Files.exists(root.resolve("settings.gradle")) || Files.exists(root.resolve("settings.gradle.kts")))
            throw new Failure("unsupported_configuration", "Gradle needs narrant-java.json static metadata");
        else throw new Failure("unsupported_configuration", "no explicit Java source roots or target release");
        if (configs.isEmpty()) throw new Failure("unsupported_configuration", "no Java configurations");
        if (discovery) Observation.record(input.id());
        List<SdkDiscovery.Candidate> platforms = targetProvider.discover();
        Map<String, Target> targets = new HashMap<>();
        List<Configuration> bound = new ArrayList<>();
        for (Configuration config : configs) {
            checkpoint.check();
            Target target = targets.get(config.release);
            if (target == null) {
                target = chooseTarget(root, platforms, config.release);
                targets.put(config.release, target);
            }
            bound.add(new Configuration(config.id, config.release, config.sourcePaths, config.roots, target));
        }
        configs = List.copyOf(bound);
        Map<String, List<Configuration>> selected = new HashMap<>();
        ProjectModel model = new ProjectModel(root, configs, selected, files, includeTests, checkpoint);
        if (discovery) {
            for (Configuration config : configs) for (SourceRoot sourceRoot : config.roots)
                for (String file : model.inventory(sourceRoot)) {
                    checkpoint.check();
                    if (unique.add(file)) files.add(file);
                    if (files.size() > MAX_FILES) throw new Failure("resource_limit", "Java discovery exceeds file limit");
                }
            files.sort(Comparator.naturalOrder());
        }
        for (String file : files) {
            checkpoint.check();
            List<Configuration> matches = new ArrayList<>();
            for (Configuration config : configs) for (SourceRoot sourceRoot : config.roots) {
                if (file.startsWith(sourceRoot.path + "/")) {
                    if (!matches.contains(config)) matches.add(config);
                }
            }
            if (matches.isEmpty()) throw new Failure("unsupported_configuration", "selected file outside declared source roots", List.of(file));
            selected.put(file, List.copyOf(matches));
        }
        return model;
    }

    /** 仅接受规范仓内相对路径。 */
    static void validateRelative(String path) throws Failure {
        if (path.isEmpty() || path.startsWith("/") || path.indexOf('\\') >= 0 || path.indexOf('\0') >= 0 ||
            path.indexOf(':') >= 0) throw new Failure("invalid_input", "invalid relative path");
        for (String part : path.split("/", -1)) if (part.isEmpty() || part.equals(".") || part.equals(".."))
            throw new Failure("invalid_input", "invalid relative path");
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
            if (!release.equals(platform.release())) continue;
            Path path = platform.root();
            if (!path.isAbsolute()) throw new Failure("invalid_input", "target JDK root must be absolute");
            try {
                path = path.toRealPath();
                // 标准声明必须来自仓外 SDK；共同祖先同样会扩大来源授权。
                if (path.startsWith(repository) || repository.startsWith(path))
                    throw new Failure("invalid_input", "target JDK root overlaps repository");
                String data = Files.readString(path.resolve("release"), StandardCharsets.UTF_8);
                Matcher match = Pattern.compile("(?m)^JAVA_VERSION=\"([^\"\\r\\n]+)\"\\s*$").matcher(data);
                if (!match.find() || !version.equals(match.group(1)) || !release.equals(jdkRelease(version)))
                    throw new Failure("target_unavailable", "catalog JDK does not match target release");
            } catch (Failure e) { throw e; }
            catch (Exception e) { throw new Failure("target_unavailable", "target JDK unavailable"); }
            if (chosen != null) throw new Failure("invalid_input", "ambiguous target JDK platforms");
            chosen = new Target(id, path, systemLibraries(path, release));
        }
        if (chosen == null) throw new Failure("target_unavailable", "no exact target JDK for Java " + release);
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
    record Target(String id, Path path, List<String> systemLibraries) {}

    /** 读取版本化的声明式源码根。 */
    private static List<Configuration> readMetadata(Path repository, Path file, Checkpoint checkpoint) throws Failure {
        try {
            Path actual = file.toRealPath();
            if (!actual.startsWith(repository) || !Files.isRegularFile(actual))
                throw new Failure("unsupported_configuration", "Java metadata outside repository");
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
                    Main.fields(entry, Set.of("id", "release", "source_roots"));
                    String id = Main.string(entry, "id");
                    if (id.isEmpty() || !ids.add(id)) throw new Failure("unsupported_configuration", "duplicate Java configuration id");
                    String release = Main.string(entry, "release");
                    validateRelease(release);
                    configurations.add(new Configuration("java-config:metadata:" + id, release,
                        List.of("narrant-java.json"), readRoots(repository, entry, checkpoint)));
                }
            } else {
                Main.fields(metadata, Set.of("schema", "release", "source_roots"));
                Main.requireString(metadata, "schema", Main.PROFILE);
                String release = Main.string(metadata, "release");
                validateRelease(release);
                configurations.add(new Configuration("java-config:metadata", release,
                    List.of("narrant-java.json"), readRoots(repository, metadata, checkpoint)));
            }
            if (configurations.isEmpty()) throw new Failure("unsupported_configuration", "no Java configurations");
            return configurations;
        } catch (Failure e) { throw e; }
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
                catch (Exception problem) { throw new Failure("unsupported_configuration", "source root missing: " + path); }
                if (!actual.startsWith(repository) || !Files.isDirectory(actual))
                    throw new Failure("unsupported_configuration", "source root missing: " + path);
                roots.add(new SourceRoot(path, role.equals("test")));
        }
        if (roots.isEmpty()) throw new Failure("unsupported_configuration", "source_roots is empty");
        return roots;
    }

    /** 只读递归解释仓内 Maven 模块的受限字段。 */
    private static void readPomTree(Path repository, Path module, List<Configuration> output,
                                    Set<Path> visited, Map<Path, Element> documents,
                                    Map<Path, PomResult> resolvedPoms, Checkpoint checkpoint) throws Failure {
        checkpoint.check();
        Path pom = module.resolve("pom.xml");
        try {
            pom = pom.toRealPath();
            if (!pom.startsWith(repository) || !visited.add(pom))
                throw new Failure("unsupported_configuration", "POM cycle or external path");
            Element project = readPom(pom, documents, checkpoint);
            if (!"project".equals(project.getTagName())) throw new Failure("unsupported_configuration", "invalid POM root");
            if (child(project, "profiles") != null || child(project, "repositories") != null ||
                child(project, "pluginRepositories") != null)
                throw new Failure("unsupported_configuration", "dynamic Maven configuration unsupported");
            Element build = child(project, "build");
            if (build != null && (child(build, "plugins") != null || child(build, "pluginManagement") != null ||
                                  child(build, "extensions") != null))
                throw new Failure("unsupported_configuration", "Maven plugins unsupported");
            PomResult resolved = effectivePom(repository, pom, new HashSet<>(), documents, resolvedPoms, checkpoint);
            String release = resolved.release;
            Element modules = child(project, "modules");
            List<String> names = modules == null ? List.of() : texts(modules, "module");
            if (!names.isEmpty() && release == null) {
                // 聚合 POM 可把目标 release 留给每个子模块。
            } else if (release != null) validateRelease(release);
            String modulePath = repository.relativize(module).toString().replace('\\', '/');
            String main = resolved.mainSource;
            String test = resolved.testSource;
            List<SourceRoot> roots = new ArrayList<>();
            addRoot(repository, modulePath, main == null ? "src/main/java" : main, false, roots);
            addRoot(repository, modulePath, test == null ? "src/test/java" : test, true, roots);
            if ((names.isEmpty() && release != null) || roots.stream().anyMatch(source -> Files.exists(repository.resolve(source.path)))) {
                if (release == null) throw new Failure("unsupported_configuration", "Maven module lacks explicit maven.compiler.release");
                output.add(new Configuration("java-config:" + (modulePath.isEmpty() ? "." : modulePath), release,
                    resolved.sourcePaths, roots));
            }
            for (String name : names) {
                validateRelative(name);
                readPomTree(repository, module.resolve(name), output, visited, documents, resolvedPoms, checkpoint);
            }
        } catch (Failure e) { throw e; }
        catch (Exception e) { throw new Failure("unsupported_configuration", "cannot statically read POM"); }
    }

    /** 有限 POM 父链的有效 release、源码根、坐标及实际读取来源。 */
    private record PomResult(String release, String group, String artifact, String version,
                             List<String> sourcePaths, String mainSource, String testSource) {}

    /** 沿仓内 parent 真正求值，而不借用模块遍历时的聚合 POM。 */
    private static PomResult effectivePom(Path repository, Path file, Set<Path> chain,
                                          Map<Path, Element> documents, Map<Path, PomResult> resolvedPoms,
                                          Checkpoint checkpoint) throws Failure {
        checkpoint.check();
        try {
            Path pom = file.toRealPath();
            if (!pom.startsWith(repository) || !Files.isRegularFile(pom) || !chain.add(pom))
                throw new Failure("unsupported_configuration", "external or cyclic Maven parent");
            if (resolvedPoms.containsKey(pom)) { chain.remove(pom); return resolvedPoms.get(pom); }
            if (chain.size() > 256) throw new Failure("resource_limit", "Maven parent chain too deep");
            Element project = readPom(pom, documents, checkpoint);
            if (!"project".equals(project.getTagName()) || child(project, "profiles") != null ||
                child(project, "repositories") != null || child(project, "pluginRepositories") != null)
                throw new Failure("unsupported_configuration", "dynamic Maven parent unsupported");
            Element build = child(project, "build");
            if (build != null && (child(build, "plugins") != null || child(build, "pluginManagement") != null ||
                                  child(build, "extensions") != null))
                throw new Failure("unsupported_configuration", "Maven parent plugins unsupported");
            PomResult parentResult = null;
            Element parent = child(project, "parent");
            if (parent != null) {
                String relative = text(parent, "relativePath");
                if (relative == null) relative = "../pom.xml";
                if (relative.isEmpty()) throw new Failure("unsupported_configuration", "external Maven parent unsupported");
                Path parentPom = pom.getParent().resolve(relative).normalize();
                if (Files.isDirectory(parentPom)) parentPom = parentPom.resolve("pom.xml");
                parentResult = effectivePom(repository, parentPom, chain, documents, resolvedPoms, checkpoint);
                if (!parentResult.group.equals(text(parent, "groupId")) ||
                    !parentResult.artifact.equals(text(parent, "artifactId")) ||
                    !parentResult.version.equals(text(parent, "version")))
                    throw new Failure("unsupported_configuration", "Maven parent coordinates mismatch");
            }
            String group = text(project, "groupId");
            String artifact = text(project, "artifactId");
            String version = text(project, "version");
            if (group == null && parentResult != null) group = parentResult.group;
            if (version == null && parentResult != null) version = parentResult.version;
            if (group == null || artifact == null || version == null ||
                group.contains("${") || artifact.contains("${") || version.contains("${"))
                throw new Failure("unsupported_configuration", "unresolved Maven coordinates");
            Element props = child(project, "properties");
            String release = props == null ? null : text(props, "maven.compiler.release");
            if (release == null && build != null) release = text(build, "release");
            if (release == null && parentResult != null) release = parentResult.release;
            if (release != null) validateRelease(release);
            String mainSource = build == null ? null : text(build, "sourceDirectory");
            String testSource = build == null ? null : text(build, "testSourceDirectory");
            if (mainSource == null && parentResult != null) mainSource = parentResult.mainSource;
            if (testSource == null && parentResult != null) testSource = parentResult.testSource;
            if (mainSource != null) validateRelative(mainSource);
            if (testSource != null) validateRelative(testSource);
            List<String> paths = new ArrayList<>();
            if (parentResult != null) paths.addAll(parentResult.sourcePaths);
            paths.add(repository.relativize(pom).toString().replace('\\', '/'));
            chain.remove(pom);
            PomResult result = new PomResult(release, group, artifact, version, List.copyOf(paths), mainSource, testSource);
            resolvedPoms.put(pom, result);
            return result;
        } catch (Failure e) { throw e; }
        catch (Exception e) { throw new Failure("unsupported_configuration", "cannot statically read Maven parent"); }
    }

    /** 只解析一次 POM，模块遍历与父链共享同一份静态文档。 */
    private static Element readPom(Path path, Map<Path, Element> documents, Checkpoint checkpoint) throws Exception {
        checkpoint.check();
        if (documents.containsKey(path)) return documents.get(path);
        DocumentBuilderFactory factory = DocumentBuilderFactory.newInstance();
        factory.setFeature("http://apache.org/xml/features/disallow-doctype-decl", true);
        factory.setFeature("http://xml.org/sax/features/external-general-entities", false);
        factory.setFeature("http://xml.org/sax/features/external-parameter-entities", false);
        factory.setFeature(XMLConstants.FEATURE_SECURE_PROCESSING, true);
        factory.setXIncludeAware(false);
        factory.setExpandEntityReferences(false);
        Document doc;
        try (InputStream stream = Files.newInputStream(path)) { doc = factory.newDocumentBuilder().parse(stream); }
        checkpoint.check();
        Element element = doc.getDocumentElement();
        documents.put(path, element);
        return element;
    }

    /** 保留声明的源码根；尚未创建源码目录的有效 POM 仍有真实配置证据。 */
    private static void addRoot(Path repo, String module, String local, boolean test,
                                List<SourceRoot> roots) throws Failure {
        validateRelative(local);
        String relative = module.isEmpty() ? local : module + "/" + local;
        checkedRoot(repo, relative);
        roots.add(new SourceRoot(relative, test));
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

    /** 查找直接 XML 子元素，避免误读嵌套插件字段。 */
    private static Element child(Element parent, String name) {
        for (Node node = parent.getFirstChild(); node != null; node = node.getNextSibling())
            if (node instanceof Element element && name.equals(element.getTagName())) return element;
        return null;
    }

    /** 读取直接 XML 子元素的字面文本。 */
    private static String text(Element parent, String name) {
        Element child = child(parent, name);
        return child == null ? null : child.getTextContent().trim();
    }

    /** 读取重复的直接 XML 子元素。 */
    private static List<String> texts(Element parent, String name) {
        List<String> result = new ArrayList<>();
        for (Node node = parent.getFirstChild(); node != null; node = node.getNextSibling())
            if (node instanceof Element element && name.equals(element.getTagName())) result.add(element.getTextContent().trim());
        return result;
    }

    /** 将实际需要解析的配置依次交给分析器，保持配置级期限与错误顺序。 */
    void forEachActiveConfiguration(ConfigurationAction action) throws Failure {
        for (Configuration config : configurations) {
            checkpoint.check();
            if (files.stream().noneMatch(file -> activeConfigurations(file).contains(config))) continue;
            List<String> context = context(config);
            for (String file : context) if (file.endsWith("/module-info.java") || file.equals("module-info.java"))
                throw new Failure("unsupported_configuration", "JPMS module descriptors require a separate profile", List.of(file));
            if (!context.isEmpty()) action.accept(new ParseConfiguration(config, context));
        }
    }

    /** 一次配置解析的调用边界，JDT 绑定索引仍由分析器在此边界内独立创建。 */
    @FunctionalInterface
    interface ConfigurationAction { void accept(ParseConfiguration config) throws Failure; }

    /** 封装一份配置的解析环境、上下文及本次选集，不暴露底层集合。 */
    final class ParseConfiguration {
        private final Configuration config;
        private final List<String> context;

        /** 保存已经按角色过滤并验证的本次配置上下文。 */
        private ParseConfiguration(Configuration config, List<String> context) {
            this.config = config;
            this.context = context;
        }

        /** 用该配置自己的 release、系统库和源码根执行一次 JDT 解析。 */
        void parse(FileASTRequestor requestor, IProgressMonitor monitor) {
            ASTParser parser = ASTParser.newParser(AST.getJLSLatest());
            Map<String, String> options = new HashMap<>();
            JavaCore.setComplianceOptions(compilerVersion(config.release), options);
            options.put(JavaCore.COMPILER_RELEASE, JavaCore.ENABLED);
            options.put(JavaCore.COMPILER_PB_ENABLE_PREVIEW_FEATURES, JavaCore.DISABLED);
            parser.setCompilerOptions(options);
            parser.setResolveBindings(true);
            parser.setBindingsRecovery(true);
            parser.setStatementsRecovery(false);
            List<String> roots = new ArrayList<>();
            for (SourceRoot root : config.roots)
                if (!root.test || includeTests) roots.add(repository.resolve(root.path).toString());
            parser.setEnvironment(config.target.systemLibraries().toArray(String[]::new),
                roots.toArray(String[]::new), utf8Encodings(roots.size()), false);
            String[] paths = context.stream().map(file -> repository.resolve(file).toString()).toArray(String[]::new);
            parser.createASTs(paths, utf8Encodings(paths.length), new String[0], requestor, monitor);
        }

        /** 只有选中且在当前配置中未被测试过滤的文件才贡献图节点。 */
        boolean selects(String file) {
            return selected.containsKey(file) && activeConfigurations(file).contains(config);
        }

        /** 包名使用第一个匹配的声明根，重叠根仍保留原有顺序语义。 */
        void verifyPackage(String file, String actual) throws Failure {
            String expected = null;
            for (SourceRoot root : config.roots) {
                if (!file.startsWith(root.path + "/")) continue;
                String local = file.substring(root.path.length() + 1);
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

    /** 为所有显式源码与源码根固定 UTF-8，不依赖运行机器默认编码。 */
    private static String[] utf8Encodings(int count) {
        String[] encodings = new String[count];
        java.util.Arrays.fill(encodings, "UTF-8");
        return encodings;
    }

    /** 把 JDT 的真实源码路径还原为仓库身份。 */
    String relativePath(String path) {
        return repository.relativize(Path.of(path)).toString().replace('\\', '/');
    }

    /** 每次读取重新检查仓库边界，并拒绝被替换解码的非法源码。 */
    String readSource(String relative) throws IOException, Failure {
        String source = Files.readString(checkedFile(repository, relative), StandardCharsets.UTF_8);
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
            item.add("platform_ids", Main.strings(List.of(config.target.id())));
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
        for (Configuration config : configurations) approved.add(config.target.id());
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
        for (SourceRoot root : config.roots) if (file.startsWith(root.path + "/")) {
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
            if (Files.isSymbolicLink(current)) throw new Failure("unsupported_configuration", "source directory symlink unsupported", List.of(relative));
        }
        if (!sourceDirectoryExists(directory, relative)) return directory;
        try {
            if (!directory.toRealPath().startsWith(repository))
                throw new Failure("unsupported_configuration", "invalid source root", List.of(relative));
        } catch (Failure e) { throw e; }
        catch (Exception e) { throw new Failure("unsupported_configuration", "cannot read source root", List.of(relative)); }
        return directory;
    }

    /** 只把属性读取明确报告的缺失目录视为空根，保留访问失败和其他 I/O 错误。 */
    private static boolean sourceDirectoryExists(Path directory, String relative) throws Failure {
        try {
            BasicFileAttributes attributes = Files.readAttributes(directory, BasicFileAttributes.class, LinkOption.NOFOLLOW_LINKS);
            if (!attributes.isDirectory())
                throw new Failure("unsupported_configuration", "invalid source root", List.of(relative));
            return true;
        } catch (NoSuchFileException e) { return false; }
        catch (IOException | SecurityException e) {
            throw new Failure("unsupported_configuration", "cannot read source root", List.of(relative));
        }
    }

    /** 对声明根仅枚举一次，发现与语义上下文共享有界且可取消的清单。 */
    private List<String> inventory(SourceRoot sourceRoot) throws Failure {
        checkpoint.check();
        if (inventories.containsKey(sourceRoot.path)) return inventories.get(sourceRoot.path);
        Path directory = checkedRoot(repository, sourceRoot.path);
        List<String> discovered = new ArrayList<>();
        if (sourceDirectoryExists(directory, sourceRoot.path)) try (var stream = Files.walk(directory)) {
            var paths = stream.iterator();
            while (paths.hasNext()) {
                checkpoint.check();
                Path path = paths.next();
                if (Files.isSymbolicLink(path))
                    throw new Failure("unsupported_configuration", "source tree symlink unsupported");
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

    /** 枚举配置允许的上下文源码，测试角色不改变生产配置。 */
    private List<String> context(Configuration config) throws Failure {
        Set<String> discovered = new HashSet<>();
        for (SourceRoot sourceRoot : config.roots) {
            checkpoint.check();
            if (sourceRoot.test && !includeTests) continue;
            for (String file : inventory(sourceRoot)) {
                checkpoint.check();
                discovered.add(file);
                if (discovered.size() > MAX_FILES) throw new Failure("resource_limit", "Java context exceeds file limit");
            }
        }
        return discovered.stream().sorted().toList();
    }
}
