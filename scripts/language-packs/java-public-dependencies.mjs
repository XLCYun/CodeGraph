import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import http from 'node:http';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';

/** Serve actual compiled dependencies to the installed analyzer, without warmed Resolver caches. */
export async function withJavaRepository(scratch, sdk, action) {
  const artifacts = new Map(), requests = [];
  const base = path.join(scratch, 'java-dependency-fixture');
  await fs.mkdir(base);
  const pom = (name, body = '') => `<project><modelVersion>4.0.0</modelVersion><groupId>fixture.external</groupId><artifactId>${name}</artifactId><version>1</version>${body}</project>`;
  const dependency = (name, extra = '') => `<dependency><groupId>fixture.external</groupId><artifactId>${name}</artifactId><version>1</version>${extra}</dependency>`;
  const put = (name, extension, bytes) => {
    const url = `/fixture/external/${name}/1/${name}-1.${extension}`;
    const content = Buffer.from(bytes); artifacts.set(url, content);
    artifacts.set(`${url}.sha1`, Buffer.from(createHash('sha1').update(content).digest('hex')));
  };
  for (const [name, type] of [['api', 'Api'], ['testapi', 'TestApi']]) {
    const directory = path.join(base, name); await fs.mkdir(directory);
    const source = path.join(directory, `${type}.java`);
    await fs.writeFile(source, `package ext; public class ${type} { public static int value(){return 1;} }`);
    const classes = path.join(directory, 'classes'); await fs.mkdir(classes);
    const suffix = process.platform === 'win32' ? '.exe' : '';
    execFileSync(path.join(sdk, `bin/javac${suffix}`), ['--release', '8', '-d', classes, source], { stdio: 'pipe' });
    const jar = path.join(directory, `${name}.jar`);
    execFileSync(path.join(sdk, `bin/jar${suffix}`), ['cf', jar, '-C', classes, '.'], { stdio: 'pipe' });
    put(name, 'jar', await fs.readFile(jar)); put(name, 'pom', pom(name));
  }
  put('parent', 'pom', pom('parent', '<packaging>pom</packaging><properties><maven.compiler.release>17</maven.compiler.release></properties>'));
  put('bom', 'pom', pom('bom', `<packaging>pom</packaging><dependencyManagement><dependencies>${dependency('api')}</dependencies></dependencyManagement>`));
  const server = http.createServer((request, response) => {
    requests.push(request.url);
    const bytes = artifacts.get(request.url);
    response.writeHead(bytes ? 200 : 404); response.end(bytes ?? 'missing');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const url = `http://127.0.0.1:${server.address().port}`;
  const mavenHome = path.join(base, 'maven-home'); await fs.mkdir(path.join(mavenHome, 'conf'), { recursive: true });
  const local = path.join(base, 'maven-cache');
  await fs.writeFile(path.join(mavenHome, 'conf/settings.xml'), `<settings><localRepository>${local}</localRepository><mirrors><mirror><id>fixture</id><mirrorOf>*</mirrorOf><url>${url}</url></mirror></mirrors><profiles><profile><id>fixture-active</id><properties><fixture.enabled>true</fixture.enabled></properties></profile></profiles><activeProfiles><activeProfile>fixture-active</activeProfile></activeProfiles></settings>`);
  try {
    await action({ url, mavenHome, dependency, pom });
    for (const name of ['api', 'testapi']) assert.ok(requests.includes(`/fixture/external/${name}/1/${name}-1.jar`), `Actual download not observed: ${name}`);
    for (const name of ['parent', 'bom']) assert.ok(requests.includes(`/fixture/external/${name}/1/${name}-1.pom`), `Actual model download not observed: ${name}`);
  } finally { await new Promise(resolve => server.close(resolve)); }
}
