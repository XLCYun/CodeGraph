"""Read-only process observation; Windows signal zero must never be used as a probe."""
import os
import subprocess
import sys


class ProcessIdentity:
    """Retain a Windows handle/birth identity; fixture termination uses that same handle."""
    def __init__(self, pid, owned=False):
        self.pid = pid
        self.handle = None
        self.owned = owned
        if sys.platform == 'win32':
            import ctypes
            from ctypes import wintypes
            self.native = ctypes.WinDLL('kernel32', use_last_error=True)
            self.native.OpenProcess.argtypes = (wintypes.DWORD, wintypes.BOOL, wintypes.DWORD)
            self.native.OpenProcess.restype = wintypes.HANDLE
            self.native.WaitForSingleObject.argtypes = (wintypes.HANDLE, wintypes.DWORD)
            self.native.WaitForSingleObject.restype = wintypes.DWORD
            self.native.CloseHandle.argtypes = (wintypes.HANDLE,)
            self.native.TerminateProcess.argtypes = (wintypes.HANDLE, wintypes.UINT)
            self.native.GetProcessTimes.argtypes = (wintypes.HANDLE, *([ctypes.POINTER(wintypes.FILETIME)] * 4))
            self.handle = self.native.OpenProcess(0x100000 | 0x1000 | (1 if owned else 0), False, pid)
            if not self.handle:
                error = ctypes.get_last_error()
                if error != 87: raise ctypes.WinError(error)
                self.birth = None
            else:
                stamps = [wintypes.FILETIME() for _ in range(4)]
                if not self.native.GetProcessTimes(self.handle, *(ctypes.byref(stamp) for stamp in stamps)):
                    self.close()
                    raise ctypes.WinError(ctypes.get_last_error())
                self.birth = (stamps[0].dwHighDateTime << 32) | stamps[0].dwLowDateTime
        else:
            self.birth = self._unix_birth()

    def _unix_birth(self):
        result = subprocess.run(['ps', '-p', str(self.pid), '-o', 'lstart=,stat='], capture_output=True, text=True)
        fields = result.stdout.strip().split()
        return ' '.join(fields[:-1]) if len(fields) >= 6 and not fields[-1].startswith('Z') else None

    def alive(self):
        if sys.platform == 'win32':
            if not self.handle: return False
            value = self.native.WaitForSingleObject(self.handle, 0)
            if value not in (0, 258): raise OSError('Cannot inspect retained process handle')
            return value == 258
        return self.birth is not None and self._unix_birth() == self.birth

    def terminate(self):
        if not self.owned: raise AssertionError('Observation does not authorize termination')
        if not self.alive(): return
        if sys.platform == 'win32':
            if not self.native.TerminateProcess(self.handle, 1): raise OSError('Cannot terminate owned fixture process')
        else:
            import signal
            os.kill(self.pid, signal.SIGKILL)

    def close(self):
        if self.handle:
            self.native.CloseHandle(self.handle)
            self.handle = None

    def __enter__(self): return self
    def __exit__(self, *args): self.close()


def process_alive(pid):
    with ProcessIdentity(pid) as identity:
        return identity.alive()
