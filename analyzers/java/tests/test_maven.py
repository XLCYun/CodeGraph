"""Actual Maven ModelBuilder/Resolver through the public one-shot entrypoint."""
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest
from test_helper import PUBLIC_COMMAND, RUNTIME, TARGET, write, node, snapshot_files


def pom(artifact, content='', group='fixture', version='1'):
    return ('<project><modelVersion>4.0.0</modelVersion><groupId>' + group + '</groupId><artifactId>' + artifact +
            '</artifactId><version>' + version + '</version>' + content + '</project>')


def dependency(artifact, scope='compile', version='1', extra=''):
    return ('<dependency><groupId>fixture</groupId><artifactId>' + artifact + '</artifactId>' +
            ('<version>' + version + '</version>' if version else '') + '<scope>' + scope + '</scope>' + extra + '</dependency>')


class MavenTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='codegraph-maven-')
        self.addCleanup(self.temp.cleanup)
        self.base = pathlib.Path(self.temp.name)
        self.root = self.base / 'project'; self.root.mkdir()
        self.home = self.base / 'home'; self.home.mkdir()
        self.remote = self.base / 'remote'; self.remote.mkdir()
        self.local = self.base / 'cache'
        self.settings('<offline>true</offline>')

    def settings(self, extra=''):
        write(self.home, '.m2/settings.xml', '<settings><localRepository>' + str(self.local) + '</localRepository>' + extra + '</settings>')

    def public(self, files=None, tests=False):
        message = {'protocol': 'codegraph.analyzer/1', 'type': 'analyze', 'id': 'maven', 'repository_root': str(self.root),
                   'options': {'include_tests': tests, 'rust_dependency_scope': 'repository-only'}}
        if files is not None: message['files'] = files
        env = dict(os.environ, CODEGRAPH_JAVA_TARGET_JDK=str(TARGET))
        result = subprocess.run([PUBLIC_COMMAND[0], '-Duser.home=' + str(self.home), *PUBLIC_COMMAND[1:], 'analyze'],
                                input=json.dumps(message), text=True, capture_output=True, env=env, timeout=40)
        self.assertEqual(result.stderr, '', result.stderr)
        self.assertEqual(len(result.stdout.splitlines()), 1, result.stdout)
        return json.loads(result.stdout)

    def install(self, artifact, source=None, content='', version='1'):
        folder = self.remote / 'fixture' / artifact / version
        folder.mkdir(parents=True, exist_ok=True)
        (folder / (artifact + '-' + version + '.pom')).write_text(pom(artifact, content, version=version))
        if source:
            src = self.base / ('src-' + artifact); classes = self.base / ('classes-' + artifact)
            src.mkdir(); classes.mkdir()
            java = src / (artifact.capitalize() + '.java'); java.write_text(source)
            subprocess.run([str(RUNTIME / 'bin/javac'), '-d', str(classes), str(java)], check=True, capture_output=True)
            subprocess.run([str(RUNTIME / 'bin/jar'), 'cf', str(folder / (artifact + '-' + version + '.jar')), '-C', str(classes), '.'], check=True, capture_output=True)
        for file in list(folder.iterdir()):
            (folder / (file.name + '.sha1')).write_text(hashlib.sha1(file.read_bytes()).hexdigest())

    def calls(self, result, suffix):
        self.assertEqual(result['outcome'], 'ok', result)
        return [ref['target'] for ref in node(result['graph'], suffix)['references'] if ref['semantic'] == 'code.calls']

    def test_reactor_without_binaries_and_exact_selection(self):
        write(self.root, 'pom.xml', pom('root', '<packaging>pom</packaging><modules><module>app</module><module>lib</module></modules><properties><maven.compiler.release>17</maven.compiler.release></properties>'))
        parent = '<parent><groupId>fixture</groupId><artifactId>root</artifactId><version>1</version></parent>'
        for name, body in [('app', '<dependencies>' + dependency('lib') + '</dependencies>'), ('lib', '')]:
            write(self.root, name + '/pom.xml', '<project><modelVersion>4.0.0</modelVersion>' + parent + '<artifactId>' + name + '</artifactId>' + body + '</project>')
        write(self.root, 'lib/src/main/java/p/Lib.java', 'package p; public class Lib { public static int value(){return 1;} }')
        write(self.root, 'app/src/main/java/p/App.java', 'package p; class App { int run(){return Lib.value();} }')
        before = snapshot_files(self.root)
        full = self.public()
        self.assertTrue(any(t['kind'] == 'node' and 'Lib' in t['id'] for t in self.calls(full, '/method:run()')), full)
        exact = self.public(['app/src/main/java/p/App.java'])
        self.assertTrue(any(t['kind'] == 'external' for t in self.calls(exact, '/method:run()')), exact)
        self.assertTrue(all(n['id'].startswith('java:app/') for n in exact['graph']['nodes']))
        self.assertEqual(before, snapshot_files(self.root))
        self.assertFalse(list(self.root.rglob('*.class')))

    def test_parent_bom_properties_profile_mirror_cache_real_jar(self):
        self.install('api', 'package ext; public class Api { public static int value(){return 2;} }')
        self.install('bom', content='<packaging>pom</packaging><dependencyManagement><dependencies>' + dependency('api') + '</dependencies></dependencyManagement>')
        self.install('parent', content='<packaging>pom</packaging><properties><chosen.release>17</chosen.release></properties>')
        self.settings('<mirrors><mirror><id>fixture</id><mirrorOf>*</mirrorOf><url>' + self.remote.as_uri() + '</url></mirror></mirrors><profiles><profile><id>user</id><properties><enabled>yes</enabled></properties></profile></profiles><activeProfiles><activeProfile>user</activeProfile></activeProfiles>')
        content = '''<parent><groupId>fixture</groupId><artifactId>parent</artifactId><version>1</version><relativePath/></parent>
<artifactId>app</artifactId><properties><maven.compiler.release>${chosen.release}</maven.compiler.release></properties>
<dependencyManagement><dependencies>''' + dependency('bom', 'import', extra='<type>pom</type>') + '''</dependencies></dependencyManagement>
<profiles><profile><id>jdk17</id><activation><jdk>[17,18)</jdk></activation><dependencies>''' + dependency('api', version=None) + '''</dependencies></profile></profiles>'''
        write(self.root, 'pom.xml', '<project><modelVersion>4.0.0</modelVersion>' + content + '</project>')
        write(self.root, 'src/main/java/p/App.java', 'package p; import ext.Api; class App { int run(){return Api.value();} }')
        result = self.public()
        self.assertTrue(any(t['kind'] == 'external' for t in self.calls(result, '/method:run()')), result)
        self.assertEqual(node(result['graph'], '/method:run()')['extensions']['java.platforms'][0]['target'], '17')
        self.assertTrue((self.local / 'fixture/api/1/api-1.jar').is_file())
        shutil.rmtree(self.remote)
        self.assertEqual(self.public()['graph'], result['graph'])

    def test_main_test_visibility(self):
        self.install('api', 'package ext; public class Api { public static void value(){} }')
        self.settings('<mirrors><mirror><id>fixture</id><mirrorOf>*</mirrorOf><url>' + self.remote.as_uri() + '</url></mirror></mirrors>')
        write(self.root, 'pom.xml', pom('app', '<dependencies>' + dependency('api', 'test') + '</dependencies>'))
        write(self.root, 'src/main/java/p/App.java', 'package p; class App { static void helper(){} void run(){ext.Api.value(); Only.test();} }')
        write(self.root, 'src/test/java/p/Only.java', 'package p; class Only { static void test(){} void check(){ext.Api.value(); App.helper();} }')
        result = self.public(tests=True)
        self.assertTrue(all(t['kind'] == 'unresolved' for t in self.calls(result, '/method:run()')), result)
        self.assertEqual({t['kind'] for t in self.calls(result, '/method:check()')}, {'external', 'node'})
        self.assertFalse(any('src/test/' in n['id'] for n in self.public()['graph']['nodes']))

    def test_bad_model_and_unavailable_dependency(self):
        write(self.root, 'src/main/java/p/App.java', 'package p; class App { void helper(){} void run(){helper();} }')
        write(self.root, 'pom.xml', '<broken')
        self.assertEqual(self.public()['outcome'], 'error')
        self.assertEqual(self.public(['src/main/java/p/App.java'])['outcome'], 'ok')
        write(self.root, 'pom.xml', pom('app', '<dependencies>' + dependency('missing') + '</dependencies>'))
        result = self.public()
        self.assertTrue(any(t['kind'] == 'node' for t in self.calls(result, '/method:run()')))

    def test_optional_exclusions(self):
        self.install('optional', 'package ext; public class Optional { public static void value(){} }')
        self.install('excluded', 'package ext; public class Excluded { public static void value(){} }')
        self.install('api', 'package ext; public class Api { public static void value(){} }',
                     '<dependencies>' + dependency('optional', extra='<optional>true</optional>') + dependency('excluded') + '</dependencies>')
        self.settings('<mirrors><mirror><id>fixture</id><mirrorOf>*</mirrorOf><url>' + self.remote.as_uri() + '</url></mirror></mirrors>')
        exclude = '<exclusions><exclusion><groupId>fixture</groupId><artifactId>excluded</artifactId></exclusion></exclusions>'
        write(self.root, 'pom.xml', pom('app', '<dependencies>' + dependency('api', extra=exclude) + '</dependencies>'))
        write(self.root, 'src/main/java/p/App.java', 'package p; class App { void run(){ext.Api.value(); ext.Optional.value(); ext.Excluded.value();} }')
        kinds = [t['kind'] for t in self.calls(self.public(), '/method:run()')]
        self.assertEqual(kinds.count('external'), 1)
        self.assertGreaterEqual(kinds.count('unresolved'), 1)
        self.assertFalse((self.local / 'fixture/optional/1/optional-1.jar').exists())
        self.assertFalse((self.local / 'fixture/excluded/1/excluded-1.jar').exists())

    def test_compiler_plugin_properties_and_file_profile(self):
        write(self.root, 'enable-profile', 'yes')
        write(self.root, 'pom.xml', pom('app', '''<properties><language>17</language></properties>
<profiles><profile><id>file</id><activation><file><exists>enable-profile</exists></file></activation>
<build><plugins><plugin><artifactId>maven-compiler-plugin</artifactId><configuration><release>${language}</release></configuration></plugin></plugins></build>
</profile></profiles>'''))
        write(self.root, 'src/main/java/p/App.java', 'package p; class App {}')
        result = self.public()
        self.assertEqual(result['outcome'], 'ok', result)
        self.assertEqual(result['graph']['nodes'][0]['extensions']['java.platforms'][0]['target'], '17')

    def test_snapshot_rejects_mutation_and_cleans_copy(self):
        from test_helper import HELPER, CP
        result = subprocess.run([str(RUNTIME / 'bin/java'), '-cp', str(HELPER / 'build/test-classes') + os.pathsep +
                                 str(HELPER / 'build/classes') + os.pathsep + CP, 'dev.codegraph.java.SourceSnapshotTest'],
                                text=True, capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('PASS', result.stdout)

    def test_changed_source_during_public_model_import_fails(self):
        import http.server
        import threading
        self.install('parent', content='<packaging>pom</packaging>')
        ready = threading.Event(); resume = threading.Event()
        remote = self.remote
        class Handler(http.server.SimpleHTTPRequestHandler):
            def __init__(self, *args, **kwargs): super().__init__(*args, directory=str(remote), **kwargs)
            def log_message(self, *args): pass
            def do_GET(self):
                if self.path.endswith('.pom'):
                    ready.set(); resume.wait(15)
                super().do_GET()
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True); thread.start()
        self.settings('<mirrors><mirror><id>fixture</id><mirrorOf>*</mirrorOf><url>http://127.0.0.1:' + str(server.server_port) + '</url></mirror></mirrors>')
        write(self.root, 'pom.xml', '<project><modelVersion>4.0.0</modelVersion><parent><groupId>fixture</groupId><artifactId>parent</artifactId><version>1</version><relativePath/></parent><artifactId>app</artifactId></project>')
        write(self.root, 'src/main/java/p/App.java', 'package p; class App {}')
        result = []; failures = []
        def analyze():
            try: result.append(self.public())
            except BaseException as error: failures.append(error)
        worker = threading.Thread(target=analyze); worker.start()
        try:
            self.assertTrue(ready.wait(15), 'Resolver did not request actual parent')
            write(self.root, 'src/main/java/p/App.java', 'package p; class Changed {}')
        finally:
            resume.set(); worker.join(20); server.shutdown(); server.server_close(); thread.join(5)
        if failures: raise failures[0]
        self.assertEqual(result[0]['outcome'], 'error', result)
        self.assertNotIn('graph', result[0])

    def test_reactor_bom_and_transitive_sources(self):
        write(self.root, 'pom.xml', pom('root', '<packaging>pom</packaging><modules><module>app</module><module>middle</module><module>lib</module><module>bom</module></modules>'))
        write(self.root, 'bom/pom.xml', pom('bom', '<packaging>pom</packaging><dependencyManagement><dependencies>' + dependency('middle') + '</dependencies></dependencyManagement>'))
        write(self.root, 'app/pom.xml', pom('app', '<dependencyManagement><dependencies>' + dependency('bom', 'import', extra='<type>pom</type>') + '</dependencies></dependencyManagement><dependencies>' + dependency('middle', version=None) + '</dependencies>'))
        write(self.root, 'middle/pom.xml', pom('middle', '<dependencies>' + dependency('lib') + '</dependencies>'))
        write(self.root, 'lib/pom.xml', pom('lib'))
        write(self.root, 'lib/src/main/java/p/Lib.java', 'package p; public class Lib { public static void value(){} }')
        write(self.root, 'app/src/main/java/p/App.java', 'package p; class App { void run(){Lib.value();} }')
        self.assertTrue(any(t['kind'] == 'node' for t in self.calls(self.public(), '/method:run()')))

    def test_effective_source_encoding_and_no_business_goal(self):
        write(self.root, 'pom.xml', pom('app', '''<properties><project.build.sourceEncoding>ISO-8859-1</project.build.sourceEncoding></properties>
<build><plugins><plugin><groupId>not.available</groupId><artifactId>must-never-run</artifactId><version>1</version>
<executions><execution><phase>validate</phase><goals><goal>business</goal></goals></execution></executions></plugin></plugins></build>'''))
        path = self.root / 'src/main/java/p/App.java'; path.parent.mkdir(parents=True)
        source = 'package p; class App { String value(){return "café";} }'
        path.write_bytes(source.encode('iso-8859-1'))
        result = self.public()
        self.assertEqual(result['outcome'], 'ok', result)
        self.assertIn('café', json.dumps(result['graph'], ensure_ascii=False))
        self.assertEqual(path.read_bytes(), source.encode('iso-8859-1'))

    def test_failed_model_does_not_retain_sibling_context(self):
        write(self.root, 'pom.xml', '<broken')
        write(self.root, 'src/main/java/p/App.java', 'package p; class App { void own(){} void run(){own(); Helper.value();} }')
        write(self.root, 'src/main/java/p/Helper.java', 'package p; class Helper { static void value(){} }')
        targets = self.calls(self.public(['src/main/java/p/App.java']), '/method:run()')
        self.assertEqual({target['kind'] for target in targets}, {'node', 'unresolved'})

    def test_declared_source_root_inside_normally_excluded_layout(self):
        write(self.root, 'pom.xml', pom('app', '<build><sourceDirectory>vendor/code</sourceDirectory></build>'))
        write(self.root, 'vendor/code/p/App.java', 'package p; class App { void helper(){} void run(){helper();} }')
        self.assertTrue(any(t['kind'] == 'node' for t in self.calls(self.public(), '/method:run()')))
