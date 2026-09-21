#!/usr/bin/env node
/**
 * 生成应用内更新的清单文件 `latest.json`。
 *
 * 为什么需要它：Tauri 的更新插件按一份固定格式的清单决定「有没有新版、去哪儿下载、签名是什么」。
 * CLI 只负责产出 `Switchelp.app.tar.gz` 与 `.sig`，清单要由发布者生成并作为 Release 附件上传。
 * 手写这份 JSON 迟早会漏字段（漏了 `.sig` 或 `url` 就是「按钮亮着但装不上」），所以放进脚本。
 *
 * 用法：
 *
 *   node scripts/make-latest-json.mjs --version 0.3.0 \
 *     --sig target/release/bundle/macos/Switchelp.app.tar.gz.sig \
 *     --notes-file /tmp/notes.md > latest.json
 *
 * 多架构：每个 target 各出一条清单，再合并（CI 的 macOS 矩阵就是这么用的）：
 *
 *   node scripts/make-latest-json.mjs --merge latest-aarch64.json latest-x86_64.json > latest.json
 *
 * 清单与发布步骤见 docs/architecture/06-updates.md §5、§8。
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const REPO = 'nexsjournal/switchelp-macapp';
/** 插件按 `{os}-{arch}` 找平台条目；本项目当前只出 Apple Silicon。 */
const DEFAULT_TARGET = 'darwin-aarch64';

function parseArgs(argv) {
  const args = { target: DEFAULT_TARGET, notes: '', url: null, merge: [] };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (flag === '--version') { args.version = value; index += 1; }
    else if (flag === '--sig') { args.sig = value; index += 1; }
    else if (flag === '--notes-file') { args.notes = readFileSync(resolve(value), 'utf8'); index += 1; }
    else if (flag === '--notes') { args.notes = value; index += 1; }
    else if (flag === '--url') { args.url = value; index += 1; }
    else if (flag === '--target') { args.target = value; index += 1; }
    else if (flag === '--merge') { args.merge = argv.slice(index + 1); break; }
    else if (flag === '--help' || flag === '-h') { args.help = true; }
    else throw new Error(`不认识的参数：${flag}`);
  }
  return args;
}

function usage() {
  return [
    '用法：node scripts/make-latest-json.mjs --version <x.y.z> --sig <Switchelp.app.tar.gz.sig> [选项]',
    '      node scripts/make-latest-json.mjs --merge <清单...>',
    '',
    '  --version <x.y.z>      这次发布的版本号（不带 v 前缀）',
    '  --sig <路径>           更新包的 minisign 签名（tauri build 产出的 .sig 文件）',
    '  --notes <文本>         更新说明，直接给字符串',
    '  --notes-file <路径>    更新说明，从文件读（Markdown）',
    '  --target <os-arch>     平台键，默认 ' + DEFAULT_TARGET,
    '  --url <地址>           更新包地址；默认指向本仓库该 tag 的 Release 附件',
    '  --merge <清单...>      把多份清单合成一份（各 target 的 platforms 合并；版本不一致直接报错）',
  ].join('\n');
}

/**
 * 合并多份清单：平台键取并集。
 *
 * 版本或说明不一致时**报错退出**，不挑一个"看起来对"的：
 * 两个架构出成不同版本的清单，会让一半用户永远更新不到。
 */
function mergeManifests(paths) {
  const merged = { version: null, notes: '', pub_date: null, platforms: {} };
  for (const path of paths) {
    const manifest = JSON.parse(readFileSync(resolve(path), 'utf8'));
    if (merged.version && manifest.version !== merged.version) {
      throw new Error(`${path} 的版本是 ${manifest.version}，与前面的 ${merged.version} 不一致`);
    }
    merged.version = manifest.version;
    if (!merged.notes && manifest.notes) merged.notes = manifest.notes;
    if (!merged.pub_date || (manifest.pub_date ?? '') > merged.pub_date) merged.pub_date = manifest.pub_date ?? merged.pub_date;
    for (const [target, entry] of Object.entries(manifest.platforms ?? {})) {
      if (merged.platforms[target]) throw new Error(`平台 ${target} 在两份清单里都出现了`);
      merged.platforms[target] = entry;
    }
  }
  if (!merged.version) throw new Error('合并结果里没有版本号');
  return merged;
}

function main() {
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (error) {
    console.error(`make-latest-json: ${error.message}\n\n${usage()}`);
    process.exit(2);
  }
  if (args.help) { console.log(usage()); return; }
  if (args.merge.length) {
    let merged;
    try {
      merged = mergeManifests(args.merge);
    } catch (error) {
      console.error(`make-latest-json: ${error.message}`);
      process.exit(2);
    }
    process.stdout.write(`${JSON.stringify(merged, null, 2)}\n`);
    return;
  }
  if (!args.version) { console.error(`make-latest-json: 缺少 --version\n\n${usage()}`); process.exit(2); }

  const signature = args.sig ? readFileSync(resolve(args.sig), 'utf8').trim() : '';
  // 空签名会让清单看起来正常、装的时候才失败——这里直接拦下。
  if (!signature) {
    console.error('make-latest-json: --sig 指向的文件是空的（或没给 --sig）。清单里没有签名，更新会全部装不上。');
    process.exit(2);
  }

  const url = args.url ?? `https://github.com/${REPO}/releases/download/v${args.version}/Switchelp.app.tar.gz`;
  const manifest = {
    version: args.version,
    notes: args.notes.trim(),
    pub_date: new Date().toISOString(),
    platforms: { [args.target]: { signature, url } },
  };
  // 末尾换行：直接重定向成文件时，diff 与编辑器都正常。
  process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
}

main();
