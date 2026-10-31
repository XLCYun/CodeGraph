package dev.codegraph.java;

import java.io.*;
import java.nio.file.*;
import java.util.*;
import org.apache.maven.model.*;
import org.apache.maven.model.building.*;
import org.apache.maven.model.resolution.*;
import org.apache.maven.model.io.xpp3.MavenXpp3Writer;
import org.apache.maven.repository.internal.MavenRepositorySystemUtils;
import org.apache.maven.settings.Settings;
import org.apache.maven.settings.building.*;
import org.apache.maven.settings.crypto.*;
import org.codehaus.plexus.util.xml.Xpp3Dom;
import org.sonatype.plexus.components.cipher.DefaultPlexusCipher;
import org.sonatype.plexus.components.sec.dispatcher.DefaultSecDispatcher;
import org.eclipse.aether.*;
import org.eclipse.aether.artifact.*;
import org.eclipse.aether.collection.*;
import org.eclipse.aether.connector.basic.BasicRepositoryConnectorFactory;
import org.eclipse.aether.graph.DependencyNode;
import org.eclipse.aether.repository.*;
import org.eclipse.aether.resolution.*;
import org.eclipse.aether.spi.connector.RepositoryConnectorFactory;
import org.eclipse.aether.spi.connector.transport.TransporterFactory;
import org.eclipse.aether.transport.file.FileTransporterFactory;
import org.eclipse.aether.transport.http.HttpTransporterFactory;
import org.eclipse.aether.util.repository.*;
import org.eclipse.aether.util.filter.DependencyFilterUtils;
import org.eclipse.aether.util.graph.visitor.PreorderNodeListGenerator;
import static dev.codegraph.java.Main.Failure;

/** Effective Maven models and dependency graphs; never invokes lifecycle or plugin goals. */
final class MavenProjectImporter {
    private final SourceSnapshot snapshot;
    private final Path root;
    private final ProjectModel.Checkpoint checkpoint;
    private final RepositorySystem system;
    private final DefaultRepositorySystemSession session;
    private final Settings settings;
    private final Properties properties = new Properties();
    private final List<Profile> profiles = new ArrayList<>();
    private final Map<String, Module> modules = new LinkedHashMap<>();
    private final Map<String, Path> reactorPoms = new LinkedHashMap<>();
    private final Map<Path, ModelBuildingRequest> preparedRequests = new LinkedHashMap<>();
    private final Map<Path, ModelBuildingResult> preparedModels = new LinkedHashMap<>();
    private final Set<Path> reactorInputs = new LinkedHashSet<>();
    private final Set<Path> visited = new HashSet<>();
    private final List<RemoteRepository> repositories = new ArrayList<>();
    private record Module(String id, Model model, Path pom, List<String> inputs) {}

    static ImportedProject load(SourceSnapshot snapshot, List<SdkDiscovery.Candidate> platforms,
                                ProjectModel.Checkpoint checkpoint) throws Failure {
        try {
            MavenProjectImporter importer = new MavenProjectImporter(snapshot, platforms, checkpoint);
            try {
                importer.index(snapshot.root().resolve("pom.xml"), new HashSet<>());
                importer.prepareCoordinates();
                importer.read(snapshot.root().resolve("pom.xml"));
                snapshot.verify();
                return importer.result();
            } finally { importer.system.shutdown(); }
        } catch (SnapshotFailure e) { throw e.failure; }
        catch (Failure e) { throw e; }
        catch (IOException e) { throw new Failure("object_extraction_failed", "cannot read Maven model input"); }
        catch (SettingsBuildingException | ModelBuildingException e) {
            infrastructure(e);
            checkpoint.check();
            throw new Failure("unsupported_configuration", "invalid Maven model or settings");
        }
    }

    @SuppressWarnings("deprecation") // Maven 3.9.x embeds Resolver through its fixed ServiceLocator API.
    private MavenProjectImporter(SourceSnapshot snapshot, List<SdkDiscovery.Candidate> platforms,
                                 ProjectModel.Checkpoint checkpoint) throws Failure, SettingsBuildingException {
        this.snapshot = snapshot;
        this.root = snapshot.root();
        this.checkpoint = checkpoint;
        // User/default project JDK drives JDK profile activation; the analyzer VM is not project evidence.
        for (String key : List.of("os.name", "os.arch", "os.version", "user.home"))
            if (System.getProperty(key) != null) properties.setProperty(key, System.getProperty(key));
        System.getenv().forEach((key, value) -> properties.setProperty("env." + key, value));
        platforms.stream().filter(SdkDiscovery.Candidate::defaultEligible).findFirst()
            .ifPresent(candidate -> properties.setProperty("java.version", candidate.version()));
        Path home = Path.of(System.getProperty("user.home"));
        Path user = home.resolve(".m2/settings.xml");
        Path security = home.resolve(".m2/settings-security.xml");
        snapshot.watch(user);
        watchSecurity(System.getProperty("settings.security", security.toString()));
        DefaultSettingsBuildingRequest request = new DefaultSettingsBuildingRequest();
        request.setUserSettingsFile(user.toFile());
        String mavenHome = System.getenv("MAVEN_HOME");
        if (mavenHome != null && !mavenHome.isBlank()) {
            Path global = Path.of(mavenHome).resolve("conf/settings.xml");
            snapshot.watch(global); request.setGlobalSettingsFile(global.toFile());
        }
        request.setSystemProperties(properties);
        settings = new DefaultSettingsBuilderFactory().newInstance().build(request).getEffectiveSettings();
        var decrypted = new DefaultSettingsDecrypter(new DefaultSecDispatcher(new DefaultPlexusCipher(),
            Collections.emptyMap(), security.toString())).decrypt(new DefaultSettingsDecryptionRequest(settings));
        if (!decrypted.getProblems().isEmpty()) throw new Failure("unsupported_configuration", "cannot decrypt Maven settings");
        var locator = MavenRepositorySystemUtils.newServiceLocator();
        locator.addService(RepositoryConnectorFactory.class, BasicRepositoryConnectorFactory.class);
        locator.addService(TransporterFactory.class, FileTransporterFactory.class);
        locator.addService(TransporterFactory.class, HttpTransporterFactory.class);
        system = locator.getService(RepositorySystem.class);
        if (system == null) throw new IllegalStateException("Maven Resolver bootstrap failed");
        session = MavenRepositorySystemUtils.newSession();
        session.setSystemProperties(properties);
        session.setOffline(settings.isOffline());
        session.setConfigProperty("aether.connector.connectTimeout", 10000);
        session.setConfigProperty("aether.connector.requestTimeout", 10000);
        session.setConfigProperty("aether.connector.basic.threads", 1);
        session.setConfigProperty("aether.metadataResolver.threads", 1);
        session.setTransferListener(new org.eclipse.aether.transfer.AbstractTransferListener() {
            @Override public void transferProgressed(org.eclipse.aether.transfer.TransferEvent event)
                    throws org.eclipse.aether.transfer.TransferCancelledException {
                try { checkpoint.check(); }
                catch (Failure e) { throw new org.eclipse.aether.transfer.TransferCancelledException("analysis timeout"); }
            }
        });
        String local = settings.getLocalRepository();
        if (local == null || local.isBlank()) local = home.resolve(".m2/repository").toString();
        session.setLocalRepositoryManager(system.newLocalRepositoryManager(session, new LocalRepository(local)));
        var mirrors = new DefaultMirrorSelector();
        for (var mirror : settings.getMirrors()) mirrors.add(mirror.getId(), mirror.getUrl(), mirror.getLayout(), false,
            mirror.isBlocked(), mirror.getMirrorOf(), mirror.getMirrorOfLayouts());
        session.setMirrorSelector(mirrors);
        var proxies = new DefaultProxySelector();
        for (var proxy : decrypted.getProxies()) if (proxy.isActive()) proxies.add(new org.eclipse.aether.repository.Proxy(
            proxy.getProtocol(), proxy.getHost(), proxy.getPort(), new AuthenticationBuilder()
                .addUsername(proxy.getUsername()).addPassword(proxy.getPassword()).build()), proxy.getNonProxyHosts());
        session.setProxySelector(proxies);
        var authentication = new DefaultAuthenticationSelector();
        for (var server : decrypted.getServers()) {
            authentication.add(server.getId(), new AuthenticationBuilder().addUsername(server.getUsername())
                .addPassword(server.getPassword()).addPrivateKey(server.getPrivateKey(), server.getPassphrase()).build());
            if (server.getConfiguration() instanceof Xpp3Dom config) {
                Xpp3Dom headers = config.getChild("httpHeaders");
                if (headers != null) {
                    Map<String, String> values = new HashMap<>();
                    for (Xpp3Dom header : headers.getChildren("property")) {
                        if (header.getChild("name") != null && header.getChild("value") != null)
                            values.put(header.getChild("name").getValue(), header.getChild("value").getValue());
                    }
                    session.setConfigProperty("aether.connector.http.headers." + server.getId(), values);
                }
            }
        }
        session.setAuthenticationSelector(authentication);
        for (var profile : settings.getProfiles()) profiles.add(settingsProfile(profile));
        repositories.add(new RemoteRepository.Builder("central", "default", "https://repo.maven.apache.org/maven2").build());
        session.setWorkspaceReader(new WorkspaceReader() {
            public WorkspaceRepository getRepository() { return new WorkspaceRepository("codegraph-reactor"); }
            public File findArtifact(Artifact artifact) {
                Module module = modules.get(key(artifact.getGroupId(), artifact.getArtifactId(), artifact.getVersion()));
                if (!artifact.getExtension().equals("pom")) return null;
                Path raw = reactorPoms.get(key(artifact.getGroupId(), artifact.getArtifactId(), artifact.getVersion()));
                return module != null ? module.pom.toFile() : raw == null ? null : raw.toFile();
            }
            public List<String> findVersions(Artifact artifact) {
                return modules.values().stream().map(Module::model)
                    .filter(m -> m.getGroupId().equals(artifact.getGroupId()) && m.getArtifactId().equals(artifact.getArtifactId()))
                    .map(Model::getVersion).toList();
            }
        });
    }

    /** Follow the same local security locations as SecDispatcher, watching each before parsing it. */
    private void watchSecurity(String location) throws Failure {
        if (location.startsWith("~")) location = System.getProperty("user.home") + location.substring(1);
        Set<Path> chain = new HashSet<>();
        while (location != null) {
            checkpoint.check();
            Path path;
            try {
                if (location.contains("://")) {
                    var uri = java.net.URI.create(location);
                    if (!"file".equalsIgnoreCase(uri.getScheme())) throw new IllegalArgumentException();
                    path = Path.of(uri);
                } else path = Path.of(location);
                path = path.toAbsolutePath().normalize();
            } catch (IllegalArgumentException e) {
                throw new Failure("unsupported_configuration", "unsupported Maven security location");
            }
            if (!chain.add(path) || chain.size() > 32)
                throw new Failure("unsupported_configuration", "cyclic or excessive Maven security relocation");
            snapshot.watch(path);
            if (!Files.exists(path)) return; // Plaintext settings do not require a master file.
            try (InputStream stream = Files.newInputStream(path)) {
                location = new org.sonatype.plexus.components.sec.dispatcher.model.io.xpp3.SecurityConfigurationXpp3Reader()
                    .read(stream).getRelocation();
            } catch (IOException e) {
                throw new Failure("object_extraction_failed", "cannot read Maven security configuration");
            } catch (org.codehaus.plexus.util.xml.pull.XmlPullParserException e) {
                throw new Failure("unsupported_configuration", "invalid Maven security configuration");
            }
        }
        snapshot.verify();
    }

    private Profile settingsProfile(org.apache.maven.settings.Profile value) {
        Profile profile = new Profile();
        profile.setId(value.getId()); profile.setProperties(value.getProperties());
        profile.setSource(Profile.SOURCE_SETTINGS);
        if (value.getActivation() != null) {
            var original = value.getActivation();
            Activation activation = new Activation();
            activation.setActiveByDefault(original.isActiveByDefault()); activation.setJdk(original.getJdk());
            if (original.getOs() != null) {
                ActivationOS os = new ActivationOS(); os.setName(original.getOs().getName());
                os.setArch(original.getOs().getArch()); os.setFamily(original.getOs().getFamily());
                os.setVersion(original.getOs().getVersion()); activation.setOs(os);
            }
            if (original.getProperty() != null) {
                ActivationProperty property = new ActivationProperty(); property.setName(original.getProperty().getName());
                property.setValue(original.getProperty().getValue()); activation.setProperty(property);
            }
            if (original.getFile() != null) {
                ActivationFile file = new ActivationFile(); file.setExists(original.getFile().getExists());
                file.setMissing(original.getFile().getMissing()); activation.setFile(file);
            }
            profile.setActivation(activation);
        }
        for (var repository : value.getRepositories()) {
            Repository repo = new Repository(); repo.setId(repository.getId()); repo.setUrl(repository.getUrl());
            repo.setLayout(repository.getLayout());
            if (repository.getReleases() != null) repo.setReleases(settingsPolicy(repository.getReleases()));
            if (repository.getSnapshots() != null) repo.setSnapshots(settingsPolicy(repository.getSnapshots()));
            profile.addRepository(repo);
        }
        return profile;
    }
    private org.apache.maven.model.RepositoryPolicy settingsPolicy(org.apache.maven.settings.RepositoryPolicy value) {
        var policy = new org.apache.maven.model.RepositoryPolicy(); policy.setEnabled(value.isEnabled());
        policy.setUpdatePolicy(value.getUpdatePolicy()); policy.setChecksumPolicy(value.getChecksumPolicy()); return policy;
    }

    /** Index literal reactor coordinates only; all inheritance/interpolation remains ModelBuilder's job. */
    private void index(Path pom, Set<Path> indexed) throws Failure, IOException {
        checkpoint.check();
        pom = pom.normalize();
        if (!pom.startsWith(root)) throw boundary();
        if (!indexed.add(pom)) return;
        if (indexed.size() > 1000) throw new Failure("resource_limit", "too many Maven modules");
        snapshot.validatePath(root.relativize(pom).toString().replace('\\', '/'), false);
        ProjectModel.checkedFile(snapshot.original(), root.relativize(pom).toString().replace('\\', '/'));
        reactorInputs.add(pom);
        Model raw = raw(pom);
        String group = raw.getGroupId() == null && raw.getParent() != null ? raw.getParent().getGroupId() : raw.getGroupId();
        String version = raw.getVersion() == null && raw.getParent() != null ? raw.getParent().getVersion() : raw.getVersion();
        if (group != null && version != null && raw.getArtifactId() != null)
            reactorPoms.putIfAbsent(key(group, raw.getArtifactId(), version), pom);
        for (String child : raw.getModules()) {
            if (!child.contains("${")) index(pom.getParent().resolve(child).resolve("pom.xml"), indexed);
        }
    }

    private Model raw(Path pom) throws IOException, Failure {
        byte[] xml = Files.readAllBytes(pom);
        try (InputStream stream = new ByteArrayInputStream(xml)) {
            return new org.apache.maven.model.io.xpp3.MavenXpp3Reader().read(stream);
        } catch (org.codehaus.plexus.util.xml.pull.XmlPullParserException | IOException e) {
            throw new Failure("unsupported_configuration", "invalid Maven POM");
        }
    }

    private static final class SnapshotFailure extends RuntimeException {
        final Failure failure;
        SnapshotFailure(Failure failure) { super(failure); this.failure = failure; }
    }

    /** Wrap only file activation; Maven still owns interpolation and the activation decision. */
    private ModelBuilder modelBuilder() {
        return new DefaultModelBuilderFactory() {
            @Override protected org.apache.maven.model.profile.activation.ProfileActivator[] newProfileActivators() {
                var activators = super.newProfileActivators();
                var interpolator = newProfileActivationFilePathInterpolator();
                for (int i = 0; i < activators.length; i++) {
                    var delegate = activators[i];
                    if (!(delegate instanceof org.apache.maven.model.profile.activation.FileProfileActivator)) continue;
                    activators[i] = new org.apache.maven.model.profile.activation.ProfileActivator() {
                        public boolean presentInConfig(Profile profile, org.apache.maven.model.profile.ProfileActivationContext context,
                                                       ModelProblemCollector problems) {
                            return delegate.presentInConfig(profile, context, problems);
                        }
                        public boolean isActive(Profile profile, org.apache.maven.model.profile.ProfileActivationContext context,
                                                ModelProblemCollector problems) {
                            ActivationFile file = profile.getActivation() == null ? null : profile.getActivation().getFile();
                            if (file != null) for (String expression : new String[] { file.getExists(), file.getMissing() }) {
                                if (expression == null) continue;
                                try {
                                    String resolved = interpolator.interpolate(expression, context);
                                    if (resolved != null && !resolved.contains("${")) snapshot.watchCondition(Path.of(resolved));
                                } catch (Failure e) { throw new SnapshotFailure(e); }
                                catch (org.codehaus.plexus.interpolation.InterpolationException e) { /* Maven reports malformed expressions. */ }
                            }
                            return delegate.isActive(profile, context, problems);
                        }
                    };
                }
                return activators;
            }
        }.newInstance();
    }

    /** Maven phase one performs profiles, inheritance and interpolation before BOM imports (phase two). */
    private void prepareCoordinates() throws Failure, IOException, ModelBuildingException {
        while (!preparedModels.keySet().containsAll(reactorInputs)) {
            boolean progressed = false;
            ModelBuildingException deferred = null;
            for (Path pom : List.copyOf(reactorInputs)) {
                if (preparedModels.containsKey(pom)) continue;
                checkpoint.check();
                validateLocalParent(pom, new HashSet<>());
                DefaultModelBuildingRequest request = new DefaultModelBuildingRequest();
                request.setPomFile(pom.toFile());
                request.setModelResolver(new Resolver(repositories));
                request.setSystemProperties(properties);
                request.setProfiles(profiles);
                request.setActiveProfileIds(settings.getActiveProfiles());
                request.setValidationLevel(ModelBuildingRequest.VALIDATION_LEVEL_MAVEN_3_1);
                request.setProcessPlugins(false);
                request.setTwoPhaseBuilding(true);
                ModelBuildingResult built;
                try { built = modelBuilder().build(request); }
                catch (ModelBuildingException e) { infrastructure(e); deferred = e; continue; }
                Model model = built.getEffectiveModel();
                reactorPoms.put(key(model.getGroupId(), model.getArtifactId(), model.getVersion()), pom);
                preparedRequests.put(pom, request);
                preparedModels.put(pom, built);
                progressed = true;
                Set<Path> indexed = new HashSet<>();
                for (String child : model.getModules()) index(pom.getParent().resolve(child).resolve("pom.xml"), indexed);
            }
            if (!progressed && deferred != null) throw deferred;
        }
    }

    private void read(Path pom) throws Failure, IOException, ModelBuildingException {
        checkpoint.check();
        pom = pom.normalize();
        if (!pom.startsWith(root)) throw boundary();
        String relative = root.relativize(pom).toString().replace('\\', '/');
        // Validate original paths too: a skipped symlink must not become an apparently absent input.
        ProjectModel.checkedFile(snapshot.original(), relative);
        if (!visited.add(pom)) throw new Failure("unsupported_configuration", "cyclic Maven modules");
        if (visited.size() > 1000) throw new Failure("resource_limit", "too many Maven modules");
        validateLocalParent(pom, new HashSet<>());
        if (!preparedModels.containsKey(pom)) prepareCoordinates();
        ModelBuildingRequest request = preparedRequests.get(pom);
        ModelBuildingResult built = modelBuilder().build(request, preparedModels.get(pom));
        Model model = built.getEffectiveModel();
        List<String> inputs = new ArrayList<>();
        for (String id : built.getModelIds()) {
            Model raw = built.getRawModel(id);
            if (raw != null && raw.getPomFile() != null) {
                Path path = raw.getPomFile().toPath().toAbsolutePath().normalize();
                if (path.startsWith(root)) inputs.add(root.relativize(path).toString().replace('\\', '/'));
                else throw boundary();
            }
        }
        if (!inputs.contains(relative)) inputs.add(relative);
        Collections.reverse(inputs);
        String moduleId = "java-config:" + (pom.getParent().equals(root) ? "." : root.relativize(pom.getParent()).toString().replace('\\', '/'));
        // Resolver consumes the already effective reactor POM; no installation or compiled artifacts needed.
        Path effective = snapshot.outputRoot().resolve(modules.size() + ".pom");
        Files.createDirectories(effective.getParent());
        Model flat = model.clone(); flat.setParent(null); flat.setProfiles(List.of());
        try (Writer writer = Files.newBufferedWriter(effective)) { new MavenXpp3Writer().write(writer, flat); }
        String key = key(model.getGroupId(), model.getArtifactId(), model.getVersion());
        if (modules.putIfAbsent(key, new Module(moduleId, model, effective, List.copyOf(inputs))) != null)
            throw new Failure("unsupported_configuration", "duplicate Maven reactor coordinates");
        Set<Path> indexed = new HashSet<>();
        for (String child : model.getModules()) index(pom.getParent().resolve(child).resolve("pom.xml"), indexed);
        for (String child : model.getModules()) read(pom.getParent().resolve(child).resolve("pom.xml"));
    }

    private void validateLocalParent(Path pom, Set<Path> chain) throws Failure, IOException {
        checkpoint.check();
        if (!chain.add(pom)) throw new Failure("unsupported_configuration", "cyclic Maven parents");
        if (chain.size() > 256) throw new Failure("resource_limit", "Maven parent chain too deep");
        Model raw = raw(pom);
        Parent parent = raw.getParent();
        if (parent == null || "".equals(parent.getRelativePath())) return;
        String relative = parent.getRelativePath() == null ? "../pom.xml" : parent.getRelativePath();
        Path candidate = pom.getParent().resolve(relative).normalize();
        if (Files.isDirectory(candidate)) candidate = candidate.resolve("pom.xml");
        if (!candidate.startsWith(root)) {
            Path original = snapshot.original().resolve(root.relativize(pom)).getParent().resolve(relative).normalize();
            if (Files.exists(original)) throw boundary();
            return; // Missing default ../pom.xml is normal for a remotely resolved parent.
        }
        if (Files.exists(candidate)) {
            String name = root.relativize(candidate).toString().replace('\\', '/');
            ProjectModel.checkedFile(snapshot.original(), name);
            if (!candidate.toRealPath().startsWith(root)) throw boundary();
            validateLocalParent(candidate, chain);
        }
    }

    private ImportedProject result() throws Failure {
        List<ImportedProject.SourceSet> sets = new ArrayList<>();
        for (Module module : modules.values()) for (boolean test : List.of(false, true)) {
            checkpoint.check();
            Model model = module.model;
            String ownRoot = sourceRoot(test ? model.getBuild().getTestSourceDirectory() : model.getBuild().getSourceDirectory());
            List<String> visible = new ArrayList<>();
            if (test) visible.add(module.id + ":main");
            List<String> binaries = new ArrayList<>();
            boolean complete = true;
            List<RemoteRepository> repos = new ArrayList<>();
            for (Repository repo : model.getRepositories()) repos.add(remote(repo));
            for (RemoteRepository repo : repositories) if (repos.stream().noneMatch(existing -> existing.getId().equals(repo.getId()))) repos.add(repo);
            CollectRequest collect = new CollectRequest();
            collect.setRepositories(system.newResolutionRepositories(session, repos));
            for (Dependency dependency : model.getDependencies()) collect.addDependency(convert(dependency));
            if (model.getDependencyManagement() != null)
                for (Dependency dependency : model.getDependencyManagement().getDependencies()) collect.addManagedDependency(convert(dependency));
            CollectResult collected;
            try { collected = system.collectDependencies(session, collect); }
            catch (DependencyCollectionException e) {
                infrastructure(e); checkpoint.check(); complete = false;
                collected = e.getResult();
            }
            if (collected != null && collected.getRoot() != null) {
                var filter = DependencyFilterUtils.classpathFilter(test ? "test" : "compile");
                var visitor = new PreorderNodeListGenerator();
                collected.getRoot().accept(new org.eclipse.aether.util.graph.visitor.FilteringDependencyVisitor(visitor, filter));
                for (DependencyNode node : visitor.getNodes()) {
                    checkpoint.check();
                    Artifact artifact = node.getArtifact();
                    if (artifact == null) continue;
                    Module reactor = modules.get(key(artifact.getGroupId(), artifact.getArtifactId(), artifact.getVersion()));
                    if (reactor != null && artifact.getExtension().equals("jar") && artifact.getClassifier().isEmpty()) {
                        if (!visible.contains(reactor.id + ":main")) visible.add(reactor.id + ":main");
                    } else {
                        try {
                            Artifact resolved = system.resolveArtifact(session, new ArtifactRequest(artifact, node.getRepositories(), null)).getArtifact();
                            String path = resolved.getFile().getAbsolutePath();
                            if (!binaries.contains(path)) binaries.add(path);
                        } catch (ArtifactResolutionException e) { infrastructure(e); checkpoint.check(); complete = false; }
                    }
                }
            }
            sets.add(new ImportedProject.SourceSet(module.id + (test ? ":test" : ":main"), module.id, test,
                List.of(ownRoot), visible, binaries, compiler(model, test), complete, module.inputs));
        }
        snapshot.verify();
        return new ImportedProject(sets);
    }

    private String sourceRoot(String value) throws Failure {
        Path path = Path.of(value).toAbsolutePath().normalize();
        if (!path.startsWith(root)) throw boundary();
        String relative = root.relativize(path).toString().replace('\\', '/');
        Path actual = snapshot.original();
        for (Path part : Path.of(relative)) {
            actual = actual.resolve(part);
            if (Files.isSymbolicLink(actual)) throw boundary();
        }
        return relative;
    }

    private ImportedProject.Compiler compiler(Model model, boolean test) {
        Xpp3Dom config = null;
        boolean ambiguous = false;
        String goal = test ? "testCompile" : "compile";
        for (Plugin plugin : model.getBuild().getPlugins()) if (plugin.getArtifactId().equals("maven-compiler-plugin") &&
                plugin.getGroupId().equals("org.apache.maven.plugins")) {
            if (plugin.getConfiguration() instanceof Xpp3Dom value) config = new Xpp3Dom(value);
            for (PluginExecution execution : plugin.getExecutions()) {
                if (execution.getId().equals("default-" + goal)) {
                    if (execution.getConfiguration() instanceof Xpp3Dom value)
                        config = Xpp3Dom.mergeXpp3Dom(new Xpp3Dom(value), config);
                } else if (execution.getGoals().contains(goal)) ambiguous = true;
            }
        }
        // Additional compiler executions may select different files/settings. Keep membership,
        // but do not certify one invocation's API as the compilation context for all of them.
        if (ambiguous) return new ImportedProject.Compiler(null, null, null, null,
            value(model, config, "encoding", "encoding"), true, moduleOptions(config));
        return new ImportedProject.Compiler(value(model, config, test ? "testRelease" : "release", "release"),
            value(model, config, test ? "testSource" : "source", "source"),
            value(model, config, test ? "testTarget" : "target", "target"),
            config != null && config.getChild("jdkToolchain") != null && config.getChild("jdkToolchain").getChild("version") != null
                ? config.getChild("jdkToolchain").getChild("version").getValue() : null,
            value(model, config, "encoding", "encoding"), false, moduleOptions(config));
    }
    private boolean moduleOptions(Xpp3Dom config) {
        if (config == null) return false;
        if (Set.of("modulepath", "modulesourcepath", "upgrademodulepath", "addreads", "addexports",
                "addmodules", "limitmodules", "patchmodule").contains(config.getName().toLowerCase(Locale.ROOT).replace("-", ""))) return true;
        String value = config.getValue();
        if (value != null && PatternHolder.MODULE_OPTIONS.matcher(value).find()) return true;
        for (Xpp3Dom child : config.getChildren()) if (moduleOptions(child)) return true;
        return false;
    }
    private static final class PatternHolder {
        static final java.util.regex.Pattern MODULE_OPTIONS = java.util.regex.Pattern.compile(
            "(^|\\s)(-p|--module[^\\s=]*|--upgrade-module-path|--add-reads|--add-exports|--add-modules|--limit-modules|--patch-module)([=\\s]|$)");
    }
    private String value(Model model, Xpp3Dom config, String name, String fallback) {
        String value = config == null || config.getChild(name) == null ? null : config.getChild(name).getValue();
        if (value == null) value = model.getProperties().getProperty("maven.compiler." + name);
        if (value == null && !name.equals(fallback)) return value(model, config, fallback, fallback);
        if (value == null && name.equals("encoding")) value = model.getProperties().getProperty("project.build.sourceEncoding");
        return value;
    }
    private org.eclipse.aether.graph.Dependency convert(Dependency d) {
        var type = session.getArtifactTypeRegistry().get(d.getType());
        Artifact artifact = new DefaultArtifact(d.getGroupId(), d.getArtifactId(), d.getClassifier(), null, d.getVersion(), Map.of(), type);
        if (d.getSystemPath() != null) artifact = artifact.setFile(Path.of(d.getSystemPath()).toFile());
        List<org.eclipse.aether.graph.Exclusion> exclusions = d.getExclusions().stream()
            .map(e -> new org.eclipse.aether.graph.Exclusion(e.getGroupId(), e.getArtifactId(), "*", "*")).toList();
        return new org.eclipse.aether.graph.Dependency(artifact, d.getScope(), d.isOptional(), exclusions);
    }
    private static String key(String group, String artifact, String version) { return group + ":" + artifact + ":" + version; }
    private static void infrastructure(Throwable error) throws Failure {
        // Maven's profile selector records callback failures as model problems, not Throwable causes.
        if (error instanceof ModelBuildingException model) for (ModelProblem problem : model.getProblems())
            if (problem.getException() != null) infrastructure(problem.getException());
        for (Throwable cause = error; cause != null; cause = cause.getCause()) {
            if (cause instanceof Failure failure) throw failure;
            if (cause instanceof java.nio.file.FileSystemException || cause instanceof SecurityException)
                throw new Failure("object_extraction_failed", "cannot access Maven input or cache");
            if (cause instanceof InterruptedException)
                throw new Failure("resource_limit", "Maven import interrupted");
        }
    }
    private static Failure boundary() { return new Failure("object_extraction_failed", "Maven input path outside repository or alias"); }
    private RemoteRepository remote(Repository repo) {
        var builder = new RemoteRepository.Builder(repo.getId(), repo.getLayout(), repo.getUrl());
        if (repo.getReleases() != null) builder.setReleasePolicy(policy(repo.getReleases()));
        if (repo.getSnapshots() != null) builder.setSnapshotPolicy(policy(repo.getSnapshots()));
        return builder.build();
    }
    private org.eclipse.aether.repository.RepositoryPolicy policy(org.apache.maven.model.RepositoryPolicy p) {
        return new org.eclipse.aether.repository.RepositoryPolicy(p.isEnabled(), p.getUpdatePolicy(), p.getChecksumPolicy());
    }

    @SuppressWarnings("deprecation") // Maven 3 ModelResolver's public contract uses ModelSource/FileModelSource.
    private final class Resolver implements ModelResolver {
        private final List<RemoteRepository> repos;
        Resolver(List<RemoteRepository> repos) { this.repos = new ArrayList<>(repos); }
        public ModelSource resolveModel(String group, String artifact, String version) throws UnresolvableModelException {
            try {
                checkpoint.check();
                Path reactor = reactorPoms.get(key(group, artifact, version));
                if (reactor != null) return new FileModelSource(reactor.toFile());
                Artifact pom = system.resolveArtifact(session, new ArtifactRequest(new DefaultArtifact(group, artifact, "", "pom", version),
                    system.newResolutionRepositories(session, repos), null)).getArtifact();
                return new FileModelSource(pom.getFile());
            } catch (ArtifactResolutionException | Failure e) { throw new UnresolvableModelException("cannot resolve Maven model", group, artifact, version, e); }
        }
        public ModelSource resolveModel(Parent parent) throws UnresolvableModelException { return resolveModel(parent.getGroupId(), parent.getArtifactId(), parent.getVersion()); }
        public ModelSource resolveModel(Dependency dependency) throws UnresolvableModelException { return resolveModel(dependency.getGroupId(), dependency.getArtifactId(), dependency.getVersion()); }
        public void addRepository(Repository repository) throws InvalidRepositoryException { addRepository(repository, false); }
        public void addRepository(Repository repository, boolean replace) throws InvalidRepositoryException {
            if (replace) repos.removeIf(repo -> repo.getId().equals(repository.getId()));
            if (repos.stream().noneMatch(repo -> repo.getId().equals(repository.getId()))) repos.add(remote(repository));
        }
        public ModelResolver newCopy() { return new Resolver(repos); }
    }
}
