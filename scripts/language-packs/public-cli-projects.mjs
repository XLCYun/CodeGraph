import fs from 'node:fs/promises';
import path from 'node:path';
import assert from 'node:assert/strict';

/** 各语言声明真实成员、独立测试角色和未参与文件；不以统一后缀扫描代替原生配置。 */
export const fixtures = {
  rust: {
    files: ['src/lib.rs', 'src/api.rs'], selected: ['src/lib.rs'], prerequisite: 'Cargo.toml',
    content: { 'Cargo.toml': '[package]\nname="codegraph_acceptance"\nversion="0.1.0"\nedition="2021"\n', 'src/lib.rs': 'mod api;\npub fn caller() -> i32 { api::target() }\n#[test]\nfn test_only() { caller(); }\n', 'src/api.rs': 'pub fn target() -> i32 { 1 }\n', 'unused.rs': 'pub fn excluded_member() {}\n' },
    empty: { 'Cargo.toml': '[workspace]\nmembers=[]\n' },
  },
  'typescript-javascript': {
    files: ['main.ts', 'target.js', 'checks.test.ts'], selected: ['main.ts'], prerequisite: 'tsconfig.json', implicit: true,
    content: { 'tsconfig.json': '{"compilerOptions":{"allowJs":true},"files":["main.ts","target.js","checks.test.ts"]}\n', 'main.ts': "import {target} from './target.js'; export function caller() { return target(); }\n", 'target.js': 'export function target() { return 1; }\n', 'checks.test.ts': 'export function test_only() {}\n', 'unused.ts': 'export function excluded_member() {}\n' },
    empty: { 'tsconfig.json': '{"files":[]}\n' },
    missing: { 'tsconfig.json': '{"extends":"./missing.json","files":["main.ts"]}\n', 'main.ts': 'export function caller() {}\n' },
    configless: { 'main.ts': "import {target} from './target'; export function caller() { return target(); }\n", 'target.ts': 'export function target() { return 1; }\n' },
  },
  python: {
    files: ['src/main.py', 'src/api.py', 'src/test_api.py'], selected: ['src/main.py'], prerequisite: 'ty.toml', implicit: true,
    content: { 'ty.toml': '[environment]\nroot=["src"]\n', 'src/main.py': 'from api import target\ndef caller():\n    return target()\n', 'src/api.py': 'def target():\n    return 1\n', 'src/test_api.py': 'def test_only():\n    pass\n', 'unused.py': 'def excluded_member():\n    pass\n' },
    empty: { 'ty.toml': '[src]\nexclude=["**/*.py"]\n', 'excluded.py': 'def excluded_member(): pass\n' },
    configless: { 'main.py': 'from api import target\ndef caller():\n    return target()\n', 'api.py': 'def target():\n    return 1\n' },
  },
  java: {
    files: ['src/Sample.java', 'src/Api.java', 'checks/Checks.java'], selected: ['src/Sample.java'], prerequisite: 'narrant-java.json',
    content: { 'src/Sample.java': 'public class Sample { public int caller() { return Api.target(); } }\n', 'src/Api.java': 'class Api { static int target() { return 1; } }\n', 'checks/Checks.java': 'class Checks { void test_only() {} }\n', 'unused/Unused.java': 'class Unused { void excluded_member() {} }\n', 'narrant-java.json': JSON.stringify({ schema: 'java-static-v1', release: '17', source_roots: [{ path: 'src', role: 'main' }, { path: 'checks', role: 'test' }] }) + '\n' },
    empty: { 'pom.xml': '<project><groupId>fixture</groupId><artifactId>empty</artifactId><version>1</version><properties><maven.compiler.release>17</maven.compiler.release></properties></project>\n' },
  },
  go: {
    files: ['main.go', 'api.go', 'main_test.go'], selected: ['main.go'], prerequisite: 'go.mod',
    content: { 'go.mod': 'module example.com/codegraph-acceptance\n\ngo 1.23\n', 'main.go': 'package acceptance\nfunc caller() int { return target() }\n', 'api.go': 'package acceptance\nfunc target() int { return 1 }\n', 'main_test.go': 'package acceptance\nfunc test_only() {}\n', 'testdata/unused.go': 'package unused\nfunc excluded_member() {}\n' },
    empty: { 'go.mod': 'module example.com/empty\ngo 1.23\n' },
  },
  csharp: {
    files: ['App/Sample.cs', 'App/Api.cs', 'Checks/Checks.cs'], selected: ['App/Sample.cs'], prerequisite: 'App/Sample.csproj',
    content: { 'App/Sample.csproj': '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup><ItemGroup><Compile Remove="Unused.cs"/></ItemGroup></Project>\n', 'App/Sample.cs': 'public static class Sample { public static int caller() { return Api.target(); } }\n', 'App/Api.cs': 'public static class Api { public static int target() { return 1; } }\n', 'App/Unused.cs': 'class Unused { void excluded_member() {} }\n', 'Checks/Checks.csproj': '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework><IsTestProject>true</IsTestProject></PropertyGroup></Project>\n', 'Checks/Checks.cs': 'class Checks { void test_only() {} }\n' },
    empty: { 'Empty.csproj': '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>\n' },
    // 另一个合法项目不能让缺失的所选项目伪装成成功；使用单项目缺配置对照。
    missing: { 'Sample.cs': 'class Sample { int caller()=>1; }\n' },
  },
  'c-cpp': {
    files: ['main.cpp', 'api.inc'], selected: ['main.cpp'], prerequisite: 'compile_commands.json', testRole: false,
    content: { 'main.cpp': '#include "api.inc"\nint caller() { return target(); }\nvoid test_only() {}\n', 'api.inc': 'inline int target() { return 1; }\n', 'unused.cpp': 'void excluded_member() {}\n' },
    empty: { 'main.cpp': '' },
  },
};

/** 在真实仓库创建快照；CDB 记录实际绝对根，工厂选择批准的宿主 SDK。 */
export async function writePublicProject(directory, content, database = false) {
  await fs.mkdir(directory, { recursive: true });
  const root = await fs.realpath(directory);
  content = { ...content };
  if (database) content['compile_commands.json'] = JSON.stringify([{ directory: root, file: 'main.cpp', arguments: ['clang++', '-std=c++20', '-c', 'main.cpp'] }]) + '\n';
  for (const [name, bytes] of Object.entries(content)) { await fs.mkdir(path.dirname(path.join(root, name)), { recursive: true }); await fs.writeFile(path.join(root, name), bytes); }
  return { root, content };
}

/** 返回配置、精确选集和输入快照，验收必须使用实际落盘字节。 */
export async function preparePublicProject(id, directory) {
  const fixture = structuredClone(fixtures[id]);
  return { ...fixture, ...await writePublicProject(directory, fixture.content, id === 'c-cpp') };
}

/** 源码与元数据都保持原字节，分析也不能新增、删除或替换为符号链接。 */
export async function assertPublicProjectUnchanged(project) {
  for (const [name, content] of Object.entries(project.content)) {
    assert.ok((await fs.lstat(path.join(project.root, name))).isFile(), `Analysis replaced ${name}`);
    assert.equal(await fs.readFile(path.join(project.root, name), 'utf8'), content, `Analysis changed ${name}`);
  }
  const entries = await fs.readdir(project.root, { recursive: true, withFileTypes: true });
  assert.ok(!entries.some(item => item.isSymbolicLink()), 'Analysis added a symbolic link');
  assert.deepEqual(entries.filter(item => item.isFile()).map(item => path.relative(project.root, path.join(item.parentPath, item.name)).replaceAll('\\', '/')).sort(), Object.keys(project.content).sort(), 'Analysis wrote files into the project');
  const directories = new Set();
  for (const name of Object.keys(project.content)) { let parent = path.posix.dirname(name); while (parent !== '.') { directories.add(parent); parent = path.posix.dirname(parent); } }
  assert.deepEqual(entries.filter(item => item.isDirectory()).map(item => path.relative(project.root, path.join(item.parentPath, item.name)).replaceAll('\\', '/')).sort(), [...directories].sort(), 'Analysis wrote directories into the project');
}

/** 放大真实已配置源集，使取消覆盖真实目录发现/加载，避免用替身 helper 代替。 */
export async function prepareCancellationProject(id, directory) {
  const content = { ...fixtures[id].content };
  const count = 600;
  for (let index = 0; index < count; index++) {
    const functions = Array.from({ length: 100 }, (_, n) => `value_${index}_${n}`);
    if (id === 'rust') { content['src/lib.rs'] += `mod part_${index};\n`; content[`src/part_${index}.rs`] = functions.map(name => `pub fn ${name}()->i32 { 1 }`).join('\n'); }
    if (id === 'typescript-javascript') content[`part_${index}.ts`] = functions.map(name => `export function ${name}(){ return 1; }`).join('\n');
    if (id === 'python') content[`src/part_${index}.py`] = functions.map(name => `def ${name}():\n    return 1\n`).join('\n');
    if (id === 'go') content[`part_${index}.go`] = 'package acceptance\n' + functions.map(name => `func ${name}() int { return 1 }`).join('\n');
    if (id === 'java') content[`src/Part${index}.java`] = `class Part${index} {` + functions.map(name => `int ${name}(){return 1;}`).join('\n') + '}';
    if (id === 'csharp') content[`App/Part${index}.cs`] = `class Part${index} {` + functions.map(name => `int ${name}()=>1;`).join('\n') + '}';
    if (id === 'c-cpp') { content['main.cpp'] += `#include "part_${index}.inc"\n`; content[`part_${index}.inc`] = functions.map(name => `int ${name}(){return 1;}`).join('\n'); }
  }
  if (id === 'typescript-javascript') content['tsconfig.json'] = '{"compilerOptions":{"allowJs":true},"include":["*.ts","*.js"],"exclude":["unused.ts"]}\n';
  return writePublicProject(directory, content, id === 'c-cpp');
}
