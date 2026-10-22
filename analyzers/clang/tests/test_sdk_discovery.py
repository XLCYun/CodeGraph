"""旧根平台发现断言直接运行非交付 C++ 组件，不代替真实 Windows 包启动。"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class SdkDiscoveryTests(unittest.TestCase):
    """使用临时 SDK 元数据核对版本和批准根；不安装工具或运行项目构建。"""

    def test_metadata_versions_and_separate_approved_roots(self):
        """平台策略回归通过同一 C++ 发现器，Windows 实际 DLL 启动仍由 CI 验证。"""
        default = Path(__file__).resolve().parents[1] / ('build/codegraph-clang-components.exe' if os.name == 'nt' else 'build/codegraph-clang-components')
        executable = os.environ.get('CODEGRAPH_CLANG_HELPER', str(default))
        with tempfile.TemporaryDirectory(prefix='codegraph sdk roots ') as root:
            result = subprocess.run([executable, '--sdk-discovery-tests', root], capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout.strip(), 'SDK discovery assertions passed')
