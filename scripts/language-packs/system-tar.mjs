import path from 'node:path';

/** 固定系统 bsdtar，避免 Git Bash 的 GNU tar 误解 Windows 盘符或不支持 ZIP。 */
export function systemTar({ platform = process.platform, env = process.env } = {}) {
  if (platform !== 'win32') return '/usr/bin/tar';
  const root = env.SystemRoot;
  if (typeof root !== 'string' || !/^[a-z]:[\\/]/i.test(root)) throw new Error('Windows SystemRoot must be an absolute drive path');
  return path.win32.join(root, 'System32', 'tar.exe');
}
