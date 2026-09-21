#!/usr/bin/env node
/**
 * 把 bridge 编译出来并放到 Tauri 认的位置，供 `externalBin` 打进包里。
 *
 * 为什么需要一个脚本：Tauri 的 sidecar 要求文件叫 `<名字>-<target-triple>`，而 cargo
 * 只会产出 `<名字>`。两步都要做（先编译、再改名拷贝），交给构建脚本比写进
 * `beforeBuildCommand` 的一长串 shell 更清楚，也能在缺工具链时报一句人话。
 *
 * 产物：`src-tauri/binaries/gptswitch-bridge-<triple>`，由 `tauri build` 打进
 * `Contents/MacOS/`，应用启动时再拷贝到应用数据目录（见 `codex/coexist.rs`）。
 */
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(dirname(fileURLToPath(import.meta.url)));

/** host triple：`rustc -vV` 里那行 `host:`。 */
function hostTriple() {
  const output = execFileSync('rustc', ['-vV'], { encoding: 'utf8' });
  const line = output.split('\n').find(entry => entry.startsWith('host:'));
  if (!line) throw new Error('无法从 rustc -vV 读出 host triple');
  return line.slice('host:'.length).trim();
}

/**
 * 交叉编译时 target 与 host 不同（CI 的 macOS 矩阵在 arm64 机器上出 x86_64 包）。
 * 那种情况下 sidecar 也必须按 target 编译——`tauri build` 会把它打进包里，
 * 装错架构的 sidecar 在目标机器上根本起不来，而错误要到用户点开共存才出现。
 * Tauri 会把本次 target 放进 TAURI_ENV_TARGET_TRIPLE。
 */
const host = hostTriple();
const triple = process.env.TAURI_ENV_TARGET_TRIPLE?.trim() || host;
const cross = triple !== host;
const profile = process.env.BRIDGE_PROFILE === 'debug'
  ? { flag: [], dir: 'debug' }
  : { flag: ['--release'], dir: 'release' };

console.log(`编译 bridge（${profile.dir}${cross ? `，target ${triple}` : ''}）…`);
execFileSync(
  'cargo',
  ['build', '-p', 'gptswitch-bridge', ...profile.flag, ...(cross ? ['--target', triple] : [])],
  { cwd: root, stdio: 'inherit' },
);

const binary = process.platform === 'win32' ? 'gptswitch-bridge.exe' : 'gptswitch-bridge';
const source = cross
  ? join(root, 'target', triple, profile.dir, binary)
  : join(root, 'target', profile.dir, binary);
const targetDir = join(root, 'src-tauri', 'binaries');
// sidecar 名字带 `-app` 后缀：与开发时那个 bin 区分开，避免 Tauri 拷 sidecar 时
// 覆盖掉 `target/<profile>/gptswitch-bridge`（见 platform::bridge_bundle_name）。
const target = join(targetDir, `gptswitch-bridge-app-${triple}${process.platform === 'win32' ? '.exe' : ''}`);

mkdirSync(targetDir, { recursive: true });
copyFileSync(source, target);
console.log(`sidecar 就绪：${target}`);
