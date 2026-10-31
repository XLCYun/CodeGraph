"""The observer must keep a real child alive on both supported operating systems."""
import subprocess
import sys
import unittest
from process_identity import ProcessIdentity, process_alive


class ProcessIdentityTests(unittest.TestCase):
    def test_observation_does_not_terminate_and_retained_handle_reports_exit(self):
        child = subprocess.Popen([sys.executable, '-c', 'import sys; print("ready", flush=True); sys.stdin.read()'], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        try:
            self.assertEqual(child.stdout.readline(), b'ready\n')
            with ProcessIdentity(child.pid) as identity:
                self.assertIsNotNone(identity.birth)
                self.assertTrue(identity.alive())
                self.assertTrue(process_alive(child.pid))
                self.assertIsNone(child.poll(), 'read-only observation killed the child')
                child.communicate(timeout=5)
                self.assertFalse(identity.alive())
        finally:
            if child.poll() is None: child.kill()
            child.communicate(timeout=5)
