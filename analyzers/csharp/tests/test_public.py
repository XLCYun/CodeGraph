"""验证直接入口，复用 Task 3 三个原生程序的公共验收夹具。"""
import os
from pathlib import Path
import sys

ANALYZERS = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ANALYZERS / 'go/tests'))
import native_public


class PublicTests(native_public.NativePublicTests):
    """真实公开入口覆盖固定身份、EOF、精确选集和调用方整树终止。"""
    PACK = 'csharp'
    ANALYZER = Path(os.environ.get('CODEGRAPH_CSHARP_ANALYZER', str(ANALYZERS / ('csharp/publish/win-x64/codegraph-csharp.exe' if os.name == 'nt' else 'csharp/publish/osx-arm64/codegraph-csharp'))))
    SDK = Path(os.environ.get('CODEGRAPH_TEST_TARGET_SDK', os.environ.get('CODEGRAPH_DOTNET_SDK', str(Path(os.environ.get('CODEGRAPH_TEST_DOTNET', str(ANALYZERS.parent / '.tools/csharp-factory-target/dotnet'))).parent))))
