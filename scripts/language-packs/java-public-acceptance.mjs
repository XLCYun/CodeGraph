import { withJavaRepository } from './java-public-dependencies.mjs';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { processSnapshot, observeTree, liveObservedProcesses } from './public-cli-session.mjs';
import { writePublicProject, assertPublicProjectUnchanged } from './public-cli-projects.mjs';

const pom = (name, body = '') => `<project><modelVersion>4.0.0</modelVersion><groupId>fixture</groupId><artifactId>${name}</artifactId><version>1</version>${body}</project>`;
const release = '<properties><maven.compiler.release>17</maven.compiler.release></properties>';
const source = 'package p; class App { String label="中文😀"; int caller(){return Lib.target();} void helper(){} void local(){helper();} Object modern(){return java.util.List.of();} }\r\n';
const library = 'package p; public class Lib { public static int target(){return 1;} }';
const method = (graph, name) => { const node = graph.nodes.find(node => node.name === name && node.native_kind === 'method'); assert.ok(node, name); return node; };
const calls = (graph, name) => method(graph, name).references.filter(ref => ref.semantic === 'code.calls');
const structure = graph => graph.nodes.map(({ id, native_kind, name, signature, source, code, contains }) => ({ id, native_kind, name, signature, source, code, contains }));

/** Real installed bytes; fixtures use normal model inputs and the retained public caller owner. */
export async function verifyJavaPublicAcceptance({ executable, identity, scratch, exchangeCli }) {
  let number = 0;
  const fixture = content => writePublicProject(path.join(scratch, `java-model-${++number} # 中文`), content);
  const invoke = async (project, label, { files, tests = false, failure, diagnostics = [], ...options } = {}) => {
    const request = { protocol: identity.protocol, type: 'analyze', id: `java-${label}`, repository_root: project.root, ...(files === undefined ? {} : { files }), options: { include_tests: tests, rust_dependency_scope: 'repository-only' } };
    if (options.beforeReap) await fs.rm(options.environment.FIXTURE_MARKER, { force: true });
    const graph = await exchangeCli(executable, 'java', request, identity, failure, { expectedDiagnostics: diagnostics, ...options });
    await assertPublicProjectUnchanged(project);
    if (graph) for (const privatePath of [project.root, scratch, process.env.HOME, process.env.JAVA_HOME, process.env.JDK_HOME].filter(Boolean)) assert.ok(!JSON.stringify(graph).includes(privatePath), 'Private path leaked');
    return graph;
  };
  const unconfigured = { 'src/main/java/p/App.java': source, 'src/main/java/p/Lib.java': library, 'src/test/java/p/Check.java': 'package p; class Check {}' };
  const plain = await fixture(unconfigured);
  const defaults = await invoke(plain, 'default', { diagnostics: ['default_platform'] });
  assert.equal(defaults.nodes.find(node => node.native_kind === 'file' && node.name.endsWith('App.java')).source, source);
  assert.equal(method(defaults, 'caller').source, 'int caller(){return Lib.target();}');
  assert.ok(calls(defaults, 'caller').some(ref => ref.target.kind === 'node'));
  assert.ok(!defaults.nodes.some(node => node.name === 'Check'));
  const withTests = await invoke(plain, 'default-tests', { tests: true, diagnostics: ['default_platform'] });
  assert.ok(withTests.nodes.some(node => node.name === 'Check'));
  const clone = await fixture(unconfigured);
  assert.deepEqual(await invoke(clone, 'clone', { diagnostics: ['default_platform'] }), defaults);
  const sdk = process.env.JDK_HOME;
  assert.ok(sdk, 'Standard JDK17 fixture missing');
  const env = { ...process.env, JAVA_HOME: sdk, JDK_HOME: sdk, JAVA_TOOL_OPTIONS: '-Dinvalid.injected=true -not-a-jvm-option', JDK_JAVA_OPTIONS: '-not-a-jvm-option', CLASSPATH: 'untrusted' };
  const onHome = await invoke(plain, 'sdk-home', { environment: env, diagnostics: ['default_platform'] });
  const onPath = await invoke(plain, 'sdk-path', { environment: { ...env, JAVA_HOME: '', JDK_HOME: '', PATH: path.join(sdk, 'bin') + path.delimiter + process.env.PATH }, diagnostics: ['default_platform'] });
  assert.deepEqual(onHome, onPath);
  assert.deepEqual(structure(defaults), structure(onHome));
  assert.ok(calls(defaults, 'modern').every(ref => ref.target.kind === 'unresolved'), 'Default JDK8 acquired newer API');
  assert.ok(calls(onHome, 'modern').some(ref => ref.target.kind === 'external'));

  const bridge = path.resolve(path.dirname(executable), '../runtime/analyzer/classes/gradle-bridge.jar');
  const javap = execFileSync(path.join(sdk, `bin/javap${process.platform === 'win32' ? '.exe' : ''}`), ['-verbose', '-classpath', bridge, 'dev.codegraph.gradle.SourceModel'], { encoding: 'utf8' });
  assert.match(javap, /major version: 52/);
  const bom = await fixture({ 'src/main/java/p/App.java': '\ufeff' + source, 'src/main/java/p/Lib.java': library });
  const bomGraph = await invoke(bom, 'bom-crlf-unicode', { diagnostics: ['default_platform'] });
  const bomFile = bomGraph.nodes.find(node => node.native_kind === 'file' && node.name.endsWith('App.java'));
  assert.equal(bomFile.source, '\ufeff' + source);
  assert.ok(bomFile.code.startsWith('\ufeff'), 'File self-code lost its BOM prefix');
  assert.equal(method(bomGraph, 'caller').source, method(defaults, 'caller').source);
  for (const node of bomGraph.nodes) {
    const previous = defaults.nodes.find(old => old.id === node.id);
    assert.ok(previous, 'BOM changed lexical identity');
    assert.equal(node.code.replace(/^\ufeff/, ''), previous.code);
  }

  const reactor = await fixture({
    'pom.xml': pom('root', `<packaging>pom</packaging>${release}<modules><module>lib</module><module>app</module></modules>`),
    'lib/pom.xml': pom('lib', release),
    'app/pom.xml': pom('app', `${release}<dependencies><dependency><groupId>fixture</groupId><artifactId>lib</artifactId><version>1</version></dependency></dependencies>`),
    'lib/src/main/java/p/Lib.java': library, 'app/src/main/java/p/App.java': source,
    'outside/p/Extra.java': 'package p; class Extra {}', 'app/src/test/java/p/Check.java': 'package p; class Check {}',
  });
  const maven = await invoke(reactor, 'maven-reactor');
  assert.ok(calls(maven, 'caller').some(ref => ref.target.kind === 'node'));
  assert.ok(!maven.nodes.some(node => ['Extra', 'Check'].includes(node.name)));
  const exact = await invoke(reactor, 'maven-exact', { files: ['app/src/main/java/p/App.java'] });
  assert.ok(calls(exact, 'caller').some(ref => ref.target.kind === 'external' && exact.external_symbols.some(symbol => symbol.symbol === ref.target.symbol && symbol.scope === 'repository')));
  assert.ok(exact.nodes.every(node => node.id.startsWith('java:app/')));
  await invoke(reactor, 'maven-outside', { files: ['outside/p/Extra.java'], diagnostics: ['default_platform'] });
  const bad = await fixture({ 'pom.xml': '<project>secret https://user:password@private.invalid</broken>', 'A.java': 'class A { void run(){} }' });
  await invoke(bad, 'bad-discovery', { failure: 'analysis_failed' });
  await invoke(bad, 'bad-explicit', { files: ['A.java'], diagnostics: ['model_discarded', 'default_platform'] });
  assert.equal((await invoke(bad, 'bad-empty', { files: [] })).nodes.length, 0);

  // Probe supported majors through the actual product, selecting an actually absent platform.
  let missing;
  for (const target of ['24', '23', '22', '21', '20', '19', '18', '16', '15', '14', '13', '12', '11', '10', '9']) {
    const project = await fixture({ 'narrant-java.json': JSON.stringify({ schema: 'java-static-v1', release: target, source_roots: [{ path: 'src', role: 'main' }] }), 'src/A.java': 'class A { void helper(){} void run(){helper();} }' });
    const graph = await invoke(project, `candidate-${target}`, { expectedDiagnostics: undefined, diagnostics: null });
    if (graph.nodes[0].extensions['java.platforms'][0].status === 'missing') {
      missing = target;
      const confirmed = await invoke(project, `missing-${target}`, { diagnostics: ['target_platform_missing'] });
      assert.ok(calls(confirmed, 'run').every(ref => ref.target.kind === 'unresolved'));
      break;
    }
  }
  assert.ok(missing, 'No absent supported target for missing-SDK acceptance');
  console.log(`Java installed missing-platform evidence: declared ${missing}, actual product discovery found no matching SDK`);

  const archive = process.env.CODEGRAPH_GRADLE_9_ZIP;
  assert.ok(archive, 'Real Gradle wrapper fixture is mandatory');
  const wrapper = `distributionUrl=${pathToFileURL(archive).href}\ndistributionSha256Sum=${createHash('sha256').update(await fs.readFile(archive)).digest('hex')}\n`;
  const marker = path.join(scratch, 'java-import-marker.json');
  const business = path.join(scratch, 'java-business-task');
  const home = path.join(scratch, 'java-caller-gradle-home');
  await fs.mkdir(home);
  const environment = { ...process.env, GRADLE_USER_HOME: home, FIXTURE_MARKER: marker, FIXTURE_BUSINESS: business, CUSTOM_BUILD_VALUE: 'forwarded' };
  const sentinel = `assert System.getenv('CUSTOM_BUILD_VALUE') == 'forwarded'; new File(System.getenv('FIXTURE_MARKER')).text=groovy.json.JsonOutput.toJson([pid:java.lang.management.ManagementFactory.runtimeMXBean.name.split('@')[0].toLong(), root:rootDir.absolutePath]); println 'PRIVATE_BUILD_LOG'; defaultTasks('build'); tasks.configureEach { doFirst { new File(System.getenv('FIXTURE_BUSINESS')).text='executed'; throw new GradleException('business task requested') } };`;
  const beforeReap = async () => {
    const { pid, root } = JSON.parse(await fs.readFile(marker, 'utf8'));
    const aliveAtProductCompletion = (await processSnapshot()).some(item => item.pid === pid && !item.state?.startsWith('Z'));
    assert.equal(aliveAtProductCompletion, false, 'Product returned with its daemon alive before caller reap');
    assert.equal(await fs.stat(path.dirname(root)).then(() => true, error => { if (error.code !== 'ENOENT') throw error; return false; }), false, 'Product did not remove owned snapshot');
    assert.equal(await fs.stat(business).then(() => true, () => false), false, 'Business task ran');
    return { daemonPid: pid, aliveAtProductCompletion, ownedSnapshotDeleted: true, businessTaskExecuted: false };
  };
  const gradleContent = {
    'gradle/wrapper/gradle-wrapper.properties': wrapper,
    'settings.gradle': "include 'lib', 'app'",
    'build.gradle': `subprojects { apply plugin:'java'; tasks.withType(JavaCompile).configureEach { options.release=17 }; ${sentinel} }`,
    'lib/build.gradle': '', 'lib/src/main/java/p/Lib.java': library,
    'app/build.gradle': "dependencies { implementation project(':lib') }; sourceSets.main.java.exclude '**/Excluded.java'; sourceSets { integrationTest { java.srcDir 'checks'; compileClasspath += sourceSets.main.output + configurations.testCompileClasspath } }; tasks.named('compileIntegrationTestJava') { options.release=8 }",
    'app/src/main/java/p/App.java': source, 'app/src/main/java/p/Excluded.java': 'package p; class Excluded {}', 'app/checks/p/Check.java': 'package p; class Check { int check(){return Lib.target();} }',
  };
  const gradle = await fixture(gradleContent);
  const full = await invoke(gradle, 'gradle-members', { environment, beforeReap });
  assert.ok(calls(full, 'caller').some(ref => ref.target.kind === 'node'));
  assert.ok(!full.nodes.some(node => ['Excluded', 'Check'].includes(node.name)));
  const test = await invoke(gradle, 'gradle-tests', { environment, beforeReap, tests: true });
  assert.equal(method(test, 'check').extensions['java.platforms'][0].compiler.release, '8');
  assert.ok(calls(test, 'check').some(ref => ref.target.kind === 'node'));
  const filtered = await invoke(gradle, 'gradle-filtered-explicit', { environment, beforeReap, files: ['app/src/main/java/p/Excluded.java'], diagnostics: ['default_platform'] });
  assert.ok(filtered.nodes.every(node => node.id.includes('Excluded.java')));
  await withJavaRepository(scratch, sdk, async ({ url, mavenHome, dependency }) => {
    const sourceFiles = {
      'src/main/java/p/App.java': 'package p; class App { int external(){return ext.Api.value();} int hidden(){return ext.TestApi.value();} }',
      'src/test/java/p/Check.java': 'package p; class Check { int visible(){return ext.TestApi.value();} }',
    };
    const mavenDependencies = await fixture({ ...sourceFiles, 'pom.xml': `<project><modelVersion>4.0.0</modelVersion><parent><groupId>fixture.external</groupId><artifactId>parent</artifactId><version>1</version><relativePath/></parent><artifactId>app</artifactId><dependencyManagement><dependencies>${dependency('bom', '<type>pom</type><scope>import</scope>')}</dependencies></dependencyManagement><profiles><profile><id>active</id><activation><property><name>fixture.enabled</name></property></activation><dependencies><dependency><groupId>fixture.external</groupId><artifactId>api</artifactId></dependency>${dependency('testapi', '<scope>test</scope>')}</dependencies></profile></profiles></project>` });
    const mavenExternal = await invoke(mavenDependencies, 'maven-settings-bom-profile', { tests: true, environment: { ...environment, MAVEN_HOME: mavenHome } });
    assert.ok(calls(mavenExternal, 'external').some(ref => ref.target.kind === 'external'));
    assert.ok(calls(mavenExternal, 'hidden').every(ref => ref.target.kind === 'unresolved'));
    assert.ok(calls(mavenExternal, 'visible').some(ref => ref.target.kind === 'external'));
    const incomplete = await fixture({ ...sourceFiles, 'pom.xml': pom('incomplete', `${release}<dependencies>${dependency('api')}${dependency('missing')}</dependencies>`) });
    const incompleteGraph = await invoke(incomplete, 'maven-incomplete-context', { environment: { ...environment, MAVEN_HOME: mavenHome }, diagnostics: ['context_incomplete'] });
    assert.ok(calls(incompleteGraph, 'external').some(ref => ref.target.kind === 'external'));
    assert.equal(method(incompleteGraph, 'external').extensions['java.platforms'][0].dependencies_complete, false);
    const gradleDependencies = await fixture({ ...sourceFiles,
      'gradle/wrapper/gradle-wrapper.properties': wrapper,
      'settings.gradle': "rootProject.name='external'",
      'build.gradle': `plugins { id 'java' }; tasks.withType(JavaCompile).configureEach { options.release=17 }; repositories { maven { url='${url}'; allowInsecureProtocol=true } }; dependencies { implementation 'fixture.external:api:1'; testImplementation 'fixture.external:testapi:1' }; ${sentinel}`,
    });
    const gradleExternal = await invoke(gradleDependencies, 'gradle-external', { tests: true, environment, beforeReap });
    assert.ok(calls(gradleExternal, 'external').some(ref => ref.target.kind === 'external'));
    assert.ok(calls(gradleExternal, 'hidden').every(ref => ref.target.kind === 'unresolved'));
    assert.ok(calls(gradleExternal, 'visible').some(ref => ref.target.kind === 'external'));
  });
  await fs.writeFile(path.join(home, 'gradle.properties'), 'fixtureUser=from-user\n');
  await fs.mkdir(path.join(home, 'init.d'));
  await fs.writeFile(path.join(home, 'init.d/fixture.gradle'), "allprojects { ext.fixtureInit='from-init' }");
  const kotlin = await fixture({
    'settings.gradle.kts': 'rootProject.name="kotlin"',
    'buildSrc/build.gradle': "plugins { id 'java-gradle-plugin' }; gradlePlugin { plugins { fixture { id='fixture.plugin'; implementationClass='fixture.Helper' } } }",
    'buildSrc/src/main/java/fixture/Helper.java': 'package fixture; public class Helper implements org.gradle.api.Plugin<org.gradle.api.Project> { public void apply(org.gradle.api.Project p) { p.getExtensions().getExtraProperties().set("helperValue", System.getenv("CUSTOM_BUILD_VALUE")); } }',
    'build.gradle.kts': `import java.lang.management.ManagementFactory
plugins { java; id("fixture.plugin") }
check(property("fixtureUser") == "from-user")
check(extra["fixtureInit"] == "from-init")
check(extra["helperValue"] == "forwarded")
tasks.withType<JavaCompile>().configureEach { options.release.set(17) }
file(System.getenv("FIXTURE_MARKER")).writeText("{\\\"pid\\\":" + ManagementFactory.getRuntimeMXBean().name.split("@")[0] + ",\\\"root\\\":\\\"" + rootDir.absolutePath.replace("\\\\", "/") + "\\\"}")
tasks.configureEach { doFirst { error("business task requested") } }
`,
    'src/main/java/p/App.java': 'package p; class App { void helper(){} void local(){helper();} }',
  });
  const kotlinGraph = await invoke(kotlin, 'default-distribution-kotlin-buildsrc', { environment, beforeReap, timeoutMs: 240_000 });
  assert.ok(calls(kotlinGraph, 'local').some(ref => ref.target.kind === 'node'));
  const unprovable = await fixture({
    'gradle/wrapper/gradle-wrapper.properties': wrapper, 'settings.gradle': "rootProject.name='unprovable'",
    'build.gradle': `plugins { id 'java' }; tasks.withType(JavaCompile).configureEach { options.release=17 }; tasks.named('compileJava') { javaCompiler=providers.provider { throw new GradleException('unprovable compiler') } }; ${sentinel}`,
    'src/main/java/p/App.java': 'package p; class App { void helper(){} void local(){helper();} }',
  });
  const unprovableGraph = await invoke(unprovable, 'compiler-unprovable-with-sdk17', { environment, beforeReap, diagnostics: ['compiler_unavailable', 'context_incomplete'] });
  assert.ok(calls(unprovableGraph, 'local').every(ref => ref.target.kind === 'unresolved'));
  const generated = await fixture({
    'gradle/wrapper/gradle-wrapper.properties': wrapper, 'settings.gradle': "rootProject.name='generated'",
    'build.gradle': `plugins { id 'java' }; tasks.withType(JavaCompile).configureEach { options.release=17 }; sourceSets.main.java.srcDir 'build/generated'; def f=file('build/generated/p/Generated.java'); f.parentFile.mkdirs(); f.text='package p; class Generated { static int value(){return 1;} }'; ${sentinel}`,
    'src/main/java/p/App.java': 'package p; class App { int generated(){return Generated.value();} }',
  });
  const generatedGraph = await invoke(generated, 'generated-context', { environment, beforeReap });
  assert.ok(!generatedGraph.nodes.some(node => node.id.includes('Generated.java')));
  assert.ok(calls(generatedGraph, 'generated').some(ref => ref.target.kind === 'external'));
  const mutated = await fixture({ ...gradleContent, 'build.gradle': `${sentinel} file('app/src/main/java/p/App.java').text='package p; class Changed {}'; throw new GradleException('changed copy')` });
  await invoke(mutated, 'copy-mutation', { environment, beforeReap, files: ['app/src/main/java/p/App.java'], failure: 'analysis_failed' });
  const failed = await fixture({ ...gradleContent, 'build.gradle': `${sentinel} throw new GradleException('credential https://user:password@private.invalid')` });
  await invoke(failed, 'gradle-failure', { environment, beforeReap, failure: 'analysis_failed' });
  await invoke(failed, 'gradle-fallback', { environment, beforeReap, files: ['app/src/main/java/p/App.java'], diagnostics: ['model_discarded', 'default_platform'] });
  await fs.rm(marker);
  await invoke(failed, 'gradle-empty', { environment, files: [] });
  await assert.rejects(fs.stat(marker), { code: 'ENOENT' });

  const cancelling = await fixture({ ...gradleContent, 'build.gradle': `${sentinel} Thread.sleep(120000)` });
  let ownedSnapshot;
  const cancelImport = async ({ observed, closed, terminate }) => {
    const deadline = Date.now() + 90_000;
    while (true) {
      observeTree(await processSnapshot(), observed);
      assert.ok(!closed(), 'Import finished before cancellation');
      const markerText = await fs.readFile(marker, 'utf8').catch(error => { if (error.code !== 'ENOENT') throw error; });
      if (markerText) {
        let markerValue;
        try { markerValue = JSON.parse(markerText); } catch (error) { if (!(error instanceof SyntaxError)) throw error; await delay(30); continue; }
        const current = await processSnapshot(); observeTree(current, observed);
        assert.ok(liveObservedProcesses(current, observed).some(item => item.pid === markerValue.pid), 'Import daemon not in owned tree');
        ownedSnapshot = path.dirname(markerValue.root);
        assert.match(path.basename(ownedSnapshot), /^codegraph-java-input-/);
        assert.equal(await fs.realpath(path.dirname(ownedSnapshot)), await fs.realpath(os.tmpdir()));
        await terminate(); return;
      }
      assert.ok(Date.now() < deadline, 'Actual Gradle import marker missing');
      await delay(30);
    }
  };
  await invoke(cancelling, 'gradle-import-cancel', { environment, cancel: true, cancelImport, timeoutMs: 120_000 });
  // External cancellation cannot run finally; remove only the marker-proven private snapshot after owner reap.
  if (ownedSnapshot) await fs.rm(ownedSnapshot, { recursive: true, force: true });
  console.log('Java installed acceptance: default/declared/missing platform; stable roots/environment; Maven reactor; Gradle source sets/filter/releases; failure/empty/import cancellation; product cleanup before caller reap');
}
