"""Real model-only Gradle imports through the public product entrypoint."""
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile
import unittest
from test_helper import PUBLIC_COMMAND, TARGET, write, node, snapshot_files

FIXTURES = pathlib.Path(__file__).resolve().parents[3] / '.tools/java-native'
DIST = pathlib.Path(os.environ.get('CODEGRAPH_GRADLE_9_ZIP', str(FIXTURES/'gradle-9.1.0-bin.zip')))
DIST8 = pathlib.Path(os.environ.get('CODEGRAPH_GRADLE_8_ZIP', str(FIXTURES/'gradle-8.14.3-bin.zip')))
DIST7 = pathlib.Path(os.environ.get('CODEGRAPH_GRADLE_7_ZIP', str(FIXTURES/'gradle-7.6.4-bin.zip')))
DIST_CURRENT = pathlib.Path(os.environ.get('CODEGRAPH_GRADLE_CURRENT_ZIP', str(FIXTURES/'gradle-9.8.2-bin.zip')))

@unittest.skipUnless(DIST.exists(), 'real Gradle distribution required')
class GradleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='codegraph-gradle-test-')
        self.addCleanup(self.temp.cleanup)
        self.base=pathlib.Path(self.temp.name).resolve()
        self.root=self.base/'project'; self.root.mkdir()
        self.home=self.base/'user'; self.home.mkdir()
        self.wrapper(DIST)
        write(self.root,'settings.gradle', "rootProject.name='fixture'\n")
        write(self.root,'src/main/java/p/A.java','package p; class A { int run(){return 1;} }')
        write(self.root,'build.gradle', "plugins { id 'java' }; tasks.withType(JavaCompile).configureEach { options.release=17 }\n" + self.sentinel())

    def wrapper(self, distribution):
        write(self.root,'gradle/wrapper/gradle-wrapper.properties','distributionUrl='+distribution.as_uri()+'\ndistributionSha256Sum='+hashlib.sha256(distribution.read_bytes()).hexdigest()+'\n')

    def sentinel(self):
        return "new File(System.getenv('CODEGRAPH_FIXTURE_PID')).text=java.lang.management.ManagementFactory.runtimeMXBean.name.split('@')[0]; println 'CONFIGURATION_STDOUT_SENTINEL'; defaultTasks('build'); tasks.configureEach { doFirst { throw new RuntimeException('BUSINESS_TASK_EXECUTED') } }\n"

    def public(self, files=None, tests=False, env=None):
        message={'protocol':'codegraph.analyzer/1','type':'analyze','id':'gradle','repository_root':str(self.root),'options':{'include_tests':tests,'rust_dependency_scope':'repository-only'}}
        if files is not None: message['files']=files
        environment=dict(os.environ,CODEGRAPH_JAVA_TARGET_JDK=str(TARGET),GRADLE_USER_HOME=str(self.home),CODEGRAPH_FIXTURE_PID=str(self.base/'daemon.pid'))
        environment.update(env or {})
        result=subprocess.run([PUBLIC_COMMAND[0], '--enable-native-access=ALL-UNNAMED', *PUBLIC_COMMAND[1:], 'analyze'],input=json.dumps(message),text=True,capture_output=True,env=environment,timeout=120)
        self.assertEqual(result.stderr,'',result.stderr)
        self.assertEqual(len(result.stdout.splitlines()),1,result.stdout)
        if (self.base/'daemon.pid').exists():
            pid=int((self.base/'daemon.pid').read_text())
            with self.assertRaises(ProcessLookupError): os.kill(pid,0)
        return json.loads(result.stdout)

    def test_real_model(self):
        before=snapshot_files(self.root)
        result=self.public()
        self.assertEqual(result['outcome'],'ok',result)
        self.assertEqual(before,snapshot_files(self.root))
        self.assertTrue(result['graph']['nodes'])
        self.assertEqual(result['graph']['nodes'][0]['extensions']['java.platforms'][0]['compiler']['release'],'17')

    def test_modules_custom_classpath_and_tests(self):
        write(self.root,'settings.gradle', "include 'app', 'lib'\n")
        write(self.root,'build.gradle', "subprojects { apply plugin:'java'; tasks.withType(JavaCompile).configureEach { options.release=17 }; "+self.sentinel()+" }\n")
        write(self.root,'lib/build.gradle', '')
        write(self.root,'lib/src/main/java/p/Lib.java','package p; public class Lib { public static int value(){return 1;} }')
        write(self.root,'app/build.gradle', "dependencies { implementation project(':lib') }; sourceSets { integrationTest { java.srcDir 'checks'; compileClasspath += sourceSets.main.output + configurations.testCompileClasspath } }; tasks.named('compileIntegrationTestJava') { options.release=8 }")
        write(self.root,'app/checks/p/Check.java','package p; class Check { int run(){return Lib.value();} }')
        result=self.public(tests=True, env={'JAVA_HOME':os.environ['CODEGRAPH_JAVA_TARGET_8']})
        self.assertEqual(result['outcome'],'ok',result)
        call=node(result['graph'],'/method:run()')
        self.assertEqual(call['extensions']['java.platforms'][0]['compiler']['release'],'8')
        self.assertTrue(any(ref['semantic']=='code.calls' and ref['target']['kind']=='node' for ref in call['references']),result)
        excluded=self.public()
        self.assertFalse(any('Check.java' in n['id'] for n in excluded['graph']['nodes']))

    def test_missing_toolchain_keeps_members(self):
        write(self.root,'build.gradle', "plugins { id 'java' }; java { toolchain { languageVersion=JavaLanguageVersion.of(21) } }; "+self.sentinel())
        result=self.public()
        self.assertEqual(result['outcome'],'ok',result)
        platform=result['graph']['nodes'][0]['extensions']['java.platforms'][0]
        self.assertEqual(platform['compiler']['toolchain'],'21')
        self.assertFalse(platform['dependencies_complete'])

    def test_bad_model_exact_and_empty(self):
        write(self.root,'build.gradle', self.sentinel()+"throw new RuntimeException('bad model')")
        self.assertEqual(self.public()['outcome'],'error')
        self.assertEqual(self.public(['src/main/java/p/A.java'])['outcome'],'ok')
        (self.base/'daemon.pid').unlink(missing_ok=True)
        self.assertEqual(self.public([])['outcome'],'ok')
        self.assertFalse((self.base/'daemon.pid').exists())

    @unittest.skipUnless(DIST8.exists(), 'Gradle 8 distribution required')
    def test_kotlin_buildsrc_environment_and_user_settings(self):
        self.wrapper(DIST8)
        (self.root/'build.gradle').unlink()
        write(self.home,'gradle.properties','userProperty=from-user\n')
        write(self.home,'init.d/user.gradle', "allprojects { ext.userInit='from-init' }")
        write(self.root,'buildSrc/build.gradle', "plugins { id 'java-gradle-plugin' }; gradlePlugin { plugins { fixture { id='fixture.plugin'; implementationClass='fixture.Helper' } } }")
        write(self.root,'buildSrc/src/main/java/fixture/Helper.java', 'package fixture; public class Helper implements org.gradle.api.Plugin<org.gradle.api.Project> { public void apply(org.gradle.api.Project p) { p.getExtensions().getExtraProperties().set("helperValue", System.getenv("CUSTOM_BUILD_VALUE")); } }')
        write(self.root,'build.gradle.kts', '''import java.lang.management.ManagementFactory
plugins { java; id("fixture.plugin") }
check(property("userProperty") == "from-user")
check(extra["userInit"] == "from-init")
check(extra["helperValue"] == "forwarded")
tasks.withType<JavaCompile>().configureEach { options.release.set(17) }
println("KOTLIN_CONFIGURATION_SENTINEL")
file(System.getenv("CODEGRAPH_FIXTURE_PID")).writeText(ManagementFactory.getRuntimeMXBean().name.split("@")[0])
tasks.configureEach { doFirst { error("BUSINESS_TASK_EXECUTED") } }
''')
        result=self.public(env={'CUSTOM_BUILD_VALUE':'forwarded','JAVA_HOME':os.environ['CODEGRAPH_JAVA_TARGET_8']})
        self.assertEqual(result['outcome'],'ok',result)
        self.assertFalse((self.root/'buildSrc/build').exists())

    def test_generated_context_is_frozen_not_selected(self):
        write(self.root,'build.gradle', "plugins { id 'java' }; tasks.withType(JavaCompile).configureEach { options.release=17 }; sourceSets.main.java.srcDir 'build/generated'; def generated=file('build/generated/p/Generated.java'); generated.parentFile.mkdirs(); generated.text='package p; class Generated { static int value(){return 1;} }'; "+self.sentinel())
        write(self.root,'src/main/java/p/A.java','package p; class A { int run(){return Generated.value();} }')
        result=self.public()
        self.assertEqual(result['outcome'],'ok',result)
        self.assertFalse(any('Generated.java' in n['id'] for n in result['graph']['nodes']))
        self.assertNotIn('codegraph-java-input-',json.dumps(result))
        self.assertTrue(any(r['semantic']=='code.calls' and r['target']['kind']=='external' for r in node(result['graph'],'/method:run()')['references']),result)

    def test_copy_mutation_does_not_fallback(self):
        write(self.root,'build.gradle', "plugins { id 'java' }; file('src/main/java/p/A.java').text='package p; class Changed {}'; "+self.sentinel()+"throw new RuntimeException('bad')")
        result=self.public(['src/main/java/p/A.java'])
        self.assertEqual(result['outcome'],'error',result)

    def test_checksum_is_enforced(self):
        path=self.root/'gradle/wrapper/gradle-wrapper.properties'
        path.write_text(path.read_text().split('distributionSha256Sum=')[0]+'distributionSha256Sum='+'0'*64+'\n')
        result=self.public(['src/main/java/p/A.java'])
        self.assertEqual(result['outcome'],'error',result)
        self.assertFalse((self.base/'daemon.pid').exists())

    def test_internal_timeout_cleans_tools(self):
        from test_helper import CP, HELPER, RUNTIME
        write(self.root,'build.gradle', "plugins { id 'java' }; "+self.sentinel()+"Thread.sleep(120000)")
        command=[str(RUNTIME/'bin/java'),'--enable-native-access=ALL-UNNAMED','-cp',str(HELPER/'build/test-classes')+os.pathsep+str(HELPER/'build/classes')+os.pathsep+CP,'dev.codegraph.java.GradleModelDriver',str(self.root),'20000']
        env=dict(os.environ,CODEGRAPH_JAVA_TARGET_JDK=str(TARGET),GRADLE_USER_HOME=str(self.home),CODEGRAPH_FIXTURE_PID=str(self.base/'daemon.pid'))
        result=subprocess.run(command,text=True,capture_output=True,env=env,timeout=40)
        self.assertNotEqual(result.returncode,0)
        self.assertIn('resource_limit: analysis timeout',result.stdout)
        self.assertTrue((self.base/'daemon.pid').exists())
        with self.assertRaises(ProcessLookupError): os.kill(int((self.base/'daemon.pid').read_text()),0)

    def test_filtered_jar_does_not_claim_source_visibility(self):
        write(self.root,'settings.gradle', "include 'app','lib'")
        write(self.root,'build.gradle', "subprojects { apply plugin:'java'; tasks.withType(JavaCompile).configureEach { options.release=17 }; "+self.sentinel()+" }")
        write(self.root,'lib/build.gradle', "tasks.jar { exclude '**/Lib.class' }")
        write(self.root,'app/build.gradle', "dependencies { implementation project(':lib') }")
        write(self.root,'lib/src/main/java/p/Lib.java','package p; public class Lib { public static int value(){return 1;} }')
        write(self.root,'app/src/main/java/p/App.java','package p; class App { int run(){return Lib.value();} }')
        result=self.public(['app/src/main/java/p/App.java'])
        self.assertEqual(result['outcome'],'ok',result)
        call=node(result['graph'],'/method:run()')
        self.assertFalse(call['extensions']['java.platforms'][0]['dependencies_complete'])
        self.assertTrue(any(r['semantic']=='code.calls' and r['target']['kind']=='unresolved' for r in call['references']))

    def test_original_mutation_is_detected(self):
        write(self.root,'build.gradle', "plugins { id 'java' }; new File(System.getenv('ORIGINAL_SOURCE')).text='package p; class Changed {}'; "+self.sentinel())
        result=self.public(['src/main/java/p/A.java'],env={'ORIGINAL_SOURCE':str(self.root/'src/main/java/p/A.java')})
        self.assertEqual(result['outcome'],'error',result)

    def test_external_binary_dependency(self):
        from test_helper import RUNTIME
        classes=self.base/'classes'; classes.mkdir()
        write(self.base,'External.java','package outside; public class External { public static int value(){return 2;} }')
        subprocess.run([str(RUNTIME/'bin/javac'),'--release','17','-d',str(classes),str(self.base/'External.java')],check=True,capture_output=True)
        subprocess.run([str(RUNTIME/'bin/jar'),'cf',str(self.root/'external.jar'),'-C',str(classes),'.'],check=True,capture_output=True)
        write(self.root,'build.gradle', "plugins { id 'java' }; tasks.withType(JavaCompile).configureEach { options.release=17 }; dependencies { implementation files('external.jar') }; "+self.sentinel())
        write(self.root,'src/main/java/p/A.java','package p; class A { int run(){return outside.External.value();} }')
        result=self.public()
        self.assertEqual(result['outcome'],'ok',result)
        self.assertTrue(any(r['semantic']=='code.calls' and r['target']['kind']=='external' for r in node(result['graph'],'/method:run()')['references']),result)

    def test_external_caller_cancellation(self):
        write(self.root,'build.gradle', "plugins { id 'java' }; "+self.sentinel()+"new File(System.getenv('CODEGRAPH_FIXTURE_SNAPSHOT')).text=rootDir.parentFile.absolutePath; Thread.sleep(120000)")
        message={'protocol':'codegraph.analyzer/1','type':'analyze','id':'cancel','repository_root':str(self.root),'options':{'include_tests':False,'rust_dependency_scope':'repository-only'}}
        marker=self.base/'daemon.pid'
        config={'command':[PUBLIC_COMMAND[0],'--enable-native-access=ALL-UNNAMED',*PUBLIC_COMMAND[1:],'analyze'],'root':str(self.root),'marker':str(marker),'snapshotMarker':str(self.base/'snapshot.path'),'message':message,'environment':dict(os.environ,CODEGRAPH_JAVA_TARGET_JDK=str(TARGET),GRADLE_USER_HOME=str(self.home),CODEGRAPH_FIXTURE_PID=str(marker),CODEGRAPH_FIXTURE_SNAPSHOT=str(self.base/'snapshot.path'))}
        path=self.base/'cancel.json'; path.write_text(json.dumps(config))
        result=subprocess.run(['node',str(pathlib.Path(__file__).with_name('gradle_cancel.mjs')),str(path)],text=True,capture_output=True,timeout=100)
        self.assertEqual(result.returncode,0,result.stdout+result.stderr)
        self.assertFalse(json.loads(result.stdout)['alive'])

    def test_side_daemon_is_not_stopped(self):
        import signal, time, zipfile
        from test_helper import CP, HELPER, RUNTIME
        side=self.base/'side'; side.mkdir()
        home=self.base/'side-home'; home.mkdir()
        write(side,'settings.gradle',"rootProject.name='side'")
        installation=self.base/'side-distribution'
        with zipfile.ZipFile(DIST) as archive: archive.extractall(installation)
        command=[str(RUNTIME/'bin/java'),'--enable-native-access=ALL-UNNAMED','-cp',str(HELPER/'build/test-classes')+os.pathsep+str(HELPER/'build/classes')+os.pathsep+CP,'dev.codegraph.java.GradleSideDaemonDriver',str(side),str(installation/'gradle-9.1.0'),str(home),str(TARGET),str(self.base/'ready')]
        side_process=subprocess.Popen(command,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        pids=[]
        try:
            until=time.monotonic()+30
            while not (self.base/'ready').exists() and time.monotonic()<until: time.sleep(.05)
            self.assertTrue((self.base/'ready').exists())
            pids=[int(path.name.split('-')[1].split('.')[0]) for path in home.glob('daemon/*/daemon-*.out.log')]
            self.assertTrue(pids)
            for pid in pids: os.kill(pid,0)
            self.assertEqual(self.public(env={'GRADLE_USER_HOME':str(home)})['outcome'],'ok')
            for pid in pids: os.kill(pid,0)
        finally:
            try: side_process.communicate(timeout=20)
            except subprocess.TimeoutExpired:
                side_process.kill(); side_process.communicate(timeout=5)
            pids=[int(path.name.split('-')[1].split('.')[0]) for path in home.glob('daemon/*/daemon-*.out.log')]
            for pid in pids:
                try: os.kill(pid,signal.SIGTERM)
                except ProcessLookupError: continue
                until=time.monotonic()+5
                while time.monotonic()<until:
                    try: os.kill(pid,0)
                    except ProcessLookupError: break
                    time.sleep(.05)
                else:
                    os.kill(pid,signal.SIGKILL)
                    time.sleep(.1)
                with self.assertRaises(ProcessLookupError): os.kill(pid,0)

    def test_distribution_auth_proxy_and_redirect_scope(self):
        import base64, http.server, threading
        expected='Basic '+base64.b64encode(b'synthetic:credential').decode()
        events=[]
        payload=DIST.read_bytes()
        class Download(http.server.BaseHTTPRequestHandler):
            def log_message(self,*args): pass
            def do_GET(self):
                events.append((self.path,self.headers.get('Authorization')))
                if self.path.startswith('http://') and self.headers.get('Proxy-Authorization')!=expected:
                    self.send_response(407); self.send_header('Proxy-Authenticate','Basic realm="fixture"'); self.end_headers(); return
                if self.path.startswith('http://download.invalid/'):
                    if self.headers.get('Authorization')!=expected:
                        self.send_response(401); self.end_headers(); return
                    self.send_response(302)
                    self.send_header('Location',f'http://127.0.0.1:{self.server.server_port}/gradle-9.1.0-bin.zip')
                    self.end_headers(); return
                self.send_response(200); self.send_header('Content-Length',str(len(payload))); self.end_headers(); self.wfile.write(payload)
        server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Download)
        thread=threading.Thread(target=server.serve_forever,daemon=True); thread.start()
        try:
            write(self.home,'gradle.properties',f'systemProp.http.proxyHost=127.0.0.1\nsystemProp.http.proxyPort={server.server_port}\nsystemProp.http.proxyUser=synthetic\nsystemProp.http.proxyPassword=credential\nsystemProp.http.nonProxyHosts=127.0.0.1\nsystemProp.gradle.download_invalid.wrapperUser=synthetic\nsystemProp.gradle.download_invalid.wrapperPassword=credential\n')
            write(self.root,'gradle/wrapper/gradle-wrapper.properties','distributionUrl=http://download.invalid/gradle-9.1.0-bin.zip\ndistributionSha256Sum='+hashlib.sha256(payload).hexdigest()+'\n')
            result=self.public()
            self.assertEqual(result['outcome'],'ok',result)
            self.assertEqual(events[0][1],expected)
            self.assertIsNone(events[-1][1],events)
            self.assertNotIn('credential',json.dumps(result))
        finally:
            server.shutdown(); server.server_close(); thread.join()

    def test_daemon_criteria_never_downloads_jdk(self):
        import http.server, threading
        requests=[]
        class Download(http.server.BaseHTTPRequestHandler):
            def log_message(self,*args): pass
            def do_GET(self):
                requests.append(self.path); self.send_response(404); self.end_headers()
        server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Download)
        thread=threading.Thread(target=server.serve_forever,daemon=True); thread.start()
        try:
            write(self.root,'gradle/gradle-daemon-jvm.properties','toolchainVersion=17\n')
            self.assertEqual(self.public()['outcome'],'ok')
            write(self.root,'gradle/gradle-daemon-jvm.properties',f'toolchainVersion=21\ntoolchainUrl.MAC_OS.AARCH64=http\\://127.0.0.1\\:{server.server_port}/jdk.zip\ntoolchainUrl.WINDOWS.X86_64=http\\://127.0.0.1\\:{server.server_port}/jdk.zip\n')
            result=self.public()
            self.assertEqual(result['outcome'],'error',result)
            self.assertEqual(requests,[])
        finally:
            server.shutdown(); server.server_close(); thread.join()

    def test_task_compiler_overrides_extension_and_failed_provider(self):
        write(self.root,'build.gradle', "plugins { id 'java' }; java.toolchain.languageVersion=JavaLanguageVersion.of(25); tasks.named('compileJava') { javaCompiler=javaToolchains.compilerFor { languageVersion=JavaLanguageVersion.of(17) } }; "+self.sentinel())
        write(self.root,'src/main/java/p/A.java','package p; class A { String run(){return java.util.List.of("x").getFirst();} }')
        result=self.public(env={'JAVA_HOME':os.environ['CODEGRAPH_JAVA_TARGET_8']})
        self.assertEqual(result['outcome'],'ok',result)
        platform=node(result['graph'],'/method:run()')['extensions']['java.platforms'][0]
        self.assertEqual(platform['compiler']['toolchain'],'17',result)
        self.assertTrue(platform['version'].startswith('17.'))
        self.assertTrue(any(r['semantic']=='code.calls' and r['target']['kind']=='unresolved' for r in node(result['graph'],'/method:run()')['references']))
        write(self.root,'build.gradle', "plugins { id 'java' }; java.toolchain.languageVersion=JavaLanguageVersion.of(17); tasks.named('compileJava') { javaCompiler=providers.provider { throw new GradleException('unavailable task compiler') } }; "+self.sentinel())
        failed=self.public()
        self.assertEqual(failed['outcome'],'ok',failed)
        platform=node(failed['graph'],'/method:run()')['extensions']['java.platforms'][0]
        self.assertEqual(platform['status'],'missing',failed)
        self.assertNotIn('toolchain',platform['compiler'])
        self.assertFalse(platform['dependencies_complete'])

    @unittest.skipUnless(all(p.exists() for p in (DIST8,DIST7,DIST_CURRENT)), 'representative Gradle distributions required')
    def test_compiler_shim_representative_versions(self):
        from test_helper import RUNTIME
        paths=[DIST8,DIST7,DIST_CURRENT]
        for distribution in paths:
            with self.subTest(distribution=distribution.name):
                self.assertTrue(distribution.exists(),distribution)
                self.wrapper(distribution)
                write(self.root,'build.gradle', "plugins { id 'java' }; tasks.named('compileJava') { javaCompiler=javaToolchains.compilerFor { languageVersion=JavaLanguageVersion.of(17) } }; "+self.sentinel())
                result=self.public(env={'CODEGRAPH_JAVA_TARGET_JDK':str(RUNTIME),'JDK_HOME':str(TARGET)})
                self.assertEqual(result['outcome'],'ok',result)
                platform=result['graph']['nodes'][0]['extensions']['java.platforms'][0]
                self.assertEqual(platform['status'],'available',result)
                self.assertEqual(platform['compiler']['toolchain'],'17',result)

    def test_custom_compiler_convention_is_not_default_platform(self):
        write(self.root,'build.gradle', "plugins { id 'java' }; tasks.named('compileJava') { javaCompiler.convention(javaToolchains.compilerFor { languageVersion=JavaLanguageVersion.of(17) }) }; "+self.sentinel())
        result=self.public()
        self.assertEqual(result['outcome'],'ok',result)
        self.assertEqual(result['graph']['nodes'][0]['extensions']['java.platforms'][0]['status'],'missing',result)

    def test_filtered_membership_and_batch_context(self):
        write(self.root,'gradle.properties','org.gradle.configureondemand=true\norg.gradle.configuration-cache=true\norg.gradle.unsafe.isolated-projects=true\n')
        write(self.root,'settings.gradle', "include 'lib'")
        write(self.root,'build.gradle', "plugins { id 'java' }; tasks.withType(JavaCompile).configureEach { options.release=17 }; sourceSets.main.java { include '**/p/*.java'; exclude '**/Excluded.java' }; dependencies { implementation project(':lib') }; "+self.sentinel())
        write(self.root,'lib/build.gradle',"plugins { id 'java' }; tasks.withType(JavaCompile).configureEach { options.release=17 }; "+self.sentinel())
        write(self.root,'lib/src/main/java/p/Lib.java','package p; public class Lib { public static int value(){return 1;} }')
        write(self.root,'src/main/java/p/Allowed.java','package p; class Allowed { static int value(){return 2;} }')
        write(self.root,'src/main/java/p/Excluded.java','package p; class Excluded { static int value(){return Allowed.value();} }')
        write(self.root,'src/main/java/p/A.java','package p; class A { int run(){return Allowed.value()+Excluded.value()+Lib.value();} }')
        result=self.public()
        self.assertEqual(result['outcome'],'ok',result)
        self.assertFalse(any('Excluded.java' in n['id'] for n in result['graph']['nodes']))
        calls=[r['target'] for r in node(result['graph'],'/method:run()')['references'] if r['semantic']=='code.calls']
        self.assertEqual(sum(t['kind']=='node' for t in calls),2,result)
        self.assertEqual(sum(t['kind']=='unresolved' for t in calls),1,result)
        explicit=self.public(['src/main/java/p/Excluded.java'])
        self.assertEqual(explicit['outcome'],'ok',explicit)
        calls=[r['target'] for r in node(explicit['graph'],'/method:value()')['references'] if r['semantic']=='code.calls']
        self.assertTrue(calls and all(t['kind']=='unresolved' for t in calls),explicit)
