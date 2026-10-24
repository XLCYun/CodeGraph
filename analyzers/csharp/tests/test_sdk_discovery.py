"""非交付组件直接检查 SDK 发现，保留旧根工厂的稳定版本与来源断言。"""
import subprocess
import unittest
from test_helper import DOTNET, HELPER


class SdkDiscoveryTests(unittest.TestCase):
    """测试同进程发现器，不伪造生产公共请求中的 SDK 路径。"""

    def test_installed_reference_catalog_and_runtime_only_rejection(self):
        """未来 TFM、最高稳定补丁与完整引用根都由实际 C# 组件判定。"""
        result = subprocess.run([DOTNET, str(HELPER), '--sdk-discovery-tests'], capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), 'SDK discovery assertions passed')
