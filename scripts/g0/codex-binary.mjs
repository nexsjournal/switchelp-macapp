// 解析「真实的 codex 可执行文件」在哪。
//
// ChatGPT 26.924（2026-09-26）把内置 CLI 从 `Contents/Resources/codex` 挪进了
// `codex-cli/`：真二进制在 `CodexCLI.app` 里，`bin/codex` 只是个转发脚本，位置由
// `codex-package.json` 的 `entrypoint` 声明。探针脚本不能写死一个路径——写死了就会在
// 宿主升级后以「文件不存在」失败，而这层验证本来就只能人工跑，坏了不容易被发现。
//
// 口径与 Rust 侧 `crates/switch-core/src/codex/detect.rs` 保持一致：
// 显式环境变量 > 描述文件声明的入口 > 固定候选列表。

import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

const APP = '/Applications/ChatGPT.app';
const PACKAGE = join(APP, 'Contents/Resources/codex-cli/codex-package.json');
const CANDIDATES = [
  join(APP, 'Contents/Resources/codex-cli/CodexCLI.app/Contents/MacOS/codex'),
  join(APP, 'Contents/Resources/codex-cli/bin/codex'),
  join(APP, 'Contents/Resources/codex'),
];

/** 描述文件里的入口只接受老实的相对路径：绝对路径与 `..` 不认（它是要被执行的路径）。 */
function declaredEntrypoint() {
  if (!existsSync(PACKAGE)) return null;
  try {
    const entry = JSON.parse(readFileSync(PACKAGE, 'utf8')).entrypoint;
    if (typeof entry !== 'string') return null;
    const relative = entry.trim().replace(/^\.\//, '');
    if (relative === '' || relative.startsWith('/') || relative.split('/').includes('..')) {
      return null;
    }
    return join(dirname(PACKAGE), relative);
  } catch {
    return null;
  }
}

export function resolveCodexBinary() {
  const fromEnv = process.env.GPTSWITCH_CODEX_BINARY;
  if (fromEnv) return fromEnv;
  const declared = declaredEntrypoint();
  if (declared && existsSync(declared)) return declared;
  const found = CANDIDATES.find(candidate => existsSync(candidate));
  if (found) return found;
  throw new Error(
    '找不到真实的 codex 可执行文件（宿主可能又换了 bundle 内布局）。' +
      `用 GPTSWITCH_CODEX_BINARY 指定，或检查：${CANDIDATES.join(' / ')}`,
  );
}
