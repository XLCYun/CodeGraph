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
