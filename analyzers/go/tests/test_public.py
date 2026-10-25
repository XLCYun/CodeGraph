"""验证直接入口，复用 Task 3 三个原生程序的公共验收夹具。"""
import os
from pathlib import Path
import sys

ANALYZERS = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ANALYZERS / 'go/tests'))
import native_public


class PublicTests(native_public.NativePublicTests):
    """真实公开入口覆盖固定身份、EOF、精确选集和调用方整树终止。"""
    PACK = 'go'
    ANALYZER = Path(os.environ.get('CODEGRAPH_GO_ANALYZER', str(ANALYZERS / 'go/bin/codegraph-go')))
    SDK = Path(os.environ.get('CODEGRAPH_TEST_TARGET_SDK', os.environ.get('CODEGRAPH_GO_TARGET_SDK', str(ANALYZERS.parent / '.tools/independent-analyzers-go/go-target-1.23.8/go'))))
    if os.name == 'nt' and ANALYZER.suffix != '.exe':
        ANALYZER = ANALYZER.with_suffix('.exe')

    def test_directive_text_is_not_a_directive(self):
        """真实公共入口分析指令示例文本，显式选集与配置发现均产生完整函数节点。"""
        self.prepare()
        source = ('package direct\n'
                  '// Example: //go:embed file.txt; import "C"\n'
                  'func Diagnostic() string { return `import "C"; //go:embed` }\n')
        (self.root / 'Main.go').write_text(source, encoding='utf-8')
        before = native_public.snapshot(self.root)
        for request in [self.request(['Main.go']), self.request(discovery=True)]:
            with self.subTest(discovery='files' not in request):
                result = self.run_public(request)
                self.assertEqual(result['outcome'], 'ok', result)
                diagnostic = next(node for node in result['graph']['nodes']
                                  if node.get('name') == 'Diagnostic')
                self.assertIn('//go:embed', diagnostic['code'])
                self.assertIn('//go:embed', diagnostic['source'])
        self.assertEqual(native_public.snapshot(self.root), before)

    def test_actual_special_sources_still_fail(self):
        """实际 cgo 分组导入和 embed 指令仍明确失败，不返回空图掩盖不支持的语义。"""
        self.prepare()
        for source in ['package direct\nimport (\n"C"\n)\n',
                       'package direct\nimport _ "embed"\n//go:embed file.txt\nvar File string\n']:
            (self.root / 'Main.go').write_text(source, encoding='utf-8')
            for request in [self.request(['Main.go']), self.request(discovery=True)]:
                with self.subTest(source=source, discovery='files' not in request):
                    result = self.run_public(request)
                    self.assertEqual(result['outcome'], 'error', result)
                    self.assertEqual(result['error']['code'], 'analysis_failed', result)
                    self.assertEqual(result['error']['message'],
                                     'cgo and embed directives are outside the verified static profile')
