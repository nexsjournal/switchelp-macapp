#!/usr/bin/env bash
#
# 发布前隐私扫描。只使用**通用规则**，脚本自身不含任何个人标识——
# 任何属于你的私有特征（供应商域名、内部主机名等）都放在仓库之外的清单里：
#
#   printf '%s\n' 'api.your-provider.example' 'internal-host' > ~/.switchelp-private-patterns
#   scripts/check-publish-safety.sh
#
# 退出码非零表示发现了需要先处理的内容。CI 也会跑这个脚本（.github/workflows/ci.yml）。
#
# 会扫描已跟踪文件与**未跟踪的新文件**：新文件在 `git add` 之前也应该被检查，
# 否则最容易出问题的那一刻（刚加进来的密钥/本机路径）恰好是唯一漏掉的一刻。

set -uo pipefail

fail=0
section() { printf '\n\033[1m%s\033[0m\n' "$1"; }
flag() { printf '  \033[31m✗\033[0m %s\n' "$1"; fail=1; }
# 提示项：需要人工确认，但按惯例可能是文档示例，因此不直接判定失败。
warn() { printf '  \033[33m!\033[0m %s\n' "$1"; }
ok() { printf '  \033[32m✓\033[0m %s\n' "$1"; }

# 只扫描会进入提交的文件；排除本脚本自身——它含有规则文本，否则会自己命中自己。
SELF=':(exclude)scripts/check-publish-safety.sh'
tracked() { git grep -nIE "$1" -- . "$SELF" 2>/dev/null; }
# 未跟踪文件（未被 .gitignore 排除）同样要查。
# 不要在这里加 `|| true`：它会让函数恒返回 0，`if hits=$(scan …)` 就永远成立，
# 于是「没有命中」也会被当成命中报出来。
untracked() {
  git ls-files --others --exclude-standard -z \
    | xargs -0 -r grep -nIE "$1" 2>/dev/null \
    | grep -v '^scripts/check-publish-safety.sh:'
}
# 两路都要查，并且按「有没有输出」决定退出状态：`if hits=$(scan …)` 依赖它。
# 直接写 `tracked; untracked` 只会返回最后一条命令的状态，前者命中也会被当成没命中。
scan() {
  local hits
  hits="$( { tracked "$1"; untracked "$1"; } )"
  [ -n "$hits" ] && printf '%s\n' "$hits"
}

user="$(whoami)"
printf '检查 %s 个已跟踪文件 + %s 个未跟踪文件（当前用户：%s）\n' \
  "$(git ls-files | wc -l | tr -d ' ')" \
  "$(git ls-files --others --exclude-standard | wc -l | tr -d ' ')" "$user"

section '① 本机绝对路径里的用户名'
# 不只看当前用户：换台机器、或提交者与检查者不同时，别人的名字同样不该进仓库。
# 放行的是仓库里约定的中性占位（/Users/example、/Users/me、/home/user…），
# 真实姓名不在其中，所以换机器也拦得住。
PATHS='/Users/[A-Za-z0-9._-]+|/home/[A-Za-z0-9._-]+|C:\\{1,2}Users\\{1,2}[A-Za-z0-9._-]+'
PLACEHOLDER='/(Users|home)/(example|someone|demo|me|user|username|test|you|developer|nobody)([/"]|$)|C:\\{1,2}Users\\{1,2}(example|me|user)'
if hits=$(scan "$PATHS" | grep -vE "$PLACEHOLDER"); then
  printf '%s\n' "$hits" | head -20
  flag "把真实用户名换成中性占位（例如 /Users/example）后再提交"
else
  ok "未发现带用户名的绝对路径（当前用户：${user}）"
fi

section '② 私网地址（IPv4 四段完整匹配，避免误报版本号）'
# 必须四段齐全：三位版本号（如 10.4.2）不构成地址，否则依赖清单会淹没结果。
# RFC 5737 保留地址（192.0.2.x / 198.51.100.x / 203.0.113.x）是文档示例，直接放行。
if hits=$(scan "(^|[^0-9.])(10\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}|192\.168\.[0-9]{1,3}\.[0-9]{1,3}|172\.(1[6-9]|2[0-9]|3[01])\.[0-9]{1,3}\.[0-9]{1,3})([^0-9.]|$)"); then
  printf '%s\n' "$hits" | head -20
  # 私网地址说明「这台机器在哪个网络里」：过去只给黄色提示，脚本仍然退出 0，
  # 真实内网地址因此可以一路发出去。
  flag "私网地址会暴露内网结构；文档示例请改用 RFC 5737 地址（192.0.2.x / 198.51.100.x / 203.0.113.x）"
else
  ok "未发现私网地址"
fi

section '③ 密钥与令牌形态'
# 覆盖常见的现代格式：OpenAI 的 sk-proj-/sk-svcacct-、Anthropic 的 sk-ant-、
# GitHub 的 github_pat_/ghs_/ghu_、GitLab 的 glpat-、Google 的 AIza、JWT。
# 合成夹具要放行，否则仓库里那些「断言脱敏生效」的假密钥会把门禁永远染红。
# 只放行明确的占位写法：canary / synthetic / 顺序字母数字 / 全同字符。
SYNTHETIC='canary|synthetic|example|redacted|0123456789abcdef|abcdefghijklmnop|ffffffffffffffff|aaaaaaaaaaaaaaaa|\*\*\*|••'
if hits=$(scan "sk-(proj-|svcacct-|ant-|live-|test-)?[A-Za-z0-9_-]{20,}|github_pat_[A-Za-z0-9_]{20,}|gh[pso]_[A-Za-z0-9]{20,}|glpat-[A-Za-z0-9_-]{16,}|AIza[0-9A-Za-z_-]{35}|xox[baprs]-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{12,}|eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}|-----BEGIN [A-Z ]*PRIVATE KEY" | grep -vEi "$SYNTHETIC"); then
  printf '%s\n' "$hits" | head -20
  flag "疑似真实密钥；确认是否只是脱敏规则或合成夹具"
else
  ok "未发现密钥形态"
fi

section '④ 本机凭据库与浏览器凭据引用'
if hits=$(scan "keychain:|SecKeychain|AppleKeychain|login\.keychain|\.codex/auth\.json"); then
  printf '%s\n' "$hits" | head -20
  flag "凭据库引用可能指向你的真实条目"
else
  ok "未发现凭据库引用"
fi

section '⑤ 个人联系方式'
# 本地部分必须以字母开头：否则 `128x128@2x.png` 这类文件名会被当成邮箱。
# 域名部分排除依赖包作用域与被当成 TLD 的文件扩展名（@2x.png 里的 png）。
EMAILS='[A-Za-z][A-Za-z0-9._%+-]*@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}'
NOT_EMAIL='example\.|users\.noreply|schema\.org|whatwg\.org|json-schema'
SCOPES='@(tauri-apps|radix-ui|testing-library|vitejs|vitest|babel|esbuild|rollup|jridgewell|types|swc|napi-rs|pnpm|github|npmjs)\.'
FILE_EXT='@[A-Za-z0-9-]*(\.(png|icns|ico|svg|jpe?g|gif|webp|json|toml|rs|tsx?|md|lock|ya?ml|css|html|sh|mjs))'
if hits=$(scan "$EMAILS" | grep -viE "$NOT_EMAIL" | grep -viE "$SCOPES" | grep -viE "$FILE_EXT"); then
  printf '%s\n' "$hits" | head -20
  flag "确认是否为文档示例邮箱"
else
  ok "未发现真实邮箱"
fi

section '⑥ 签名身份与团队标识（需要人工判断）'
# 这些不是密钥，但会公开你的真实姓名与 Apple 团队。文档里用 `<你的名字>` 与
# `TEAMID1234` 占位，这两种占位不算命中。
# 团队 ID 的形态是括号里的 10 位大写字母数字；必须带括号，否则纯数字会误伤（如 2147483647）。
IDENTITY="Developer ID (Application|Installer):|Apple Distribution:[[:alnum:]_ .-]*|\\([A-Z0-9]{10}\\)"
IDENTITY_PLACEHOLDER="<你的名字>|TEAMID1234|Developer ID (Application|Installer): \\.\\.\\.|Apple Distribution: \\.\\.\\."
if hits=$(scan "$IDENTITY" | grep -vE "$IDENTITY_PLACEHOLDER"); then
  printf '%s\n' "$hits" | head -20
  warn "签名身份/团队 ID 可能是纪实内容；确认是你要公开的信息，否则改成占位"
else
  ok "未发现签名身份信息"
fi

section '⑦ 私有特征清单（仓库外维护）'
# 两个历史文件名都认：产品曾名 GPTSwitch，README 一度写的是旧的 `~/.gptswitch-private-patterns`，
# 而脚本读的是新的。只认一个的话，照旧文档建文件的人会**静默地**得不到任何扫描——这一类漏检
# 比报错更危险，所以这里把两个都当默认值，环境变量仍然优先。
patterns="${SWITCHELP_PRIVATE_PATTERNS:-${GPTSWITCH_PRIVATE_PATTERNS:-}}"
if [ -z "$patterns" ]; then
  for candidate in "$HOME/.switchelp-private-patterns" "$HOME/.gptswitch-private-patterns"; do
    if [ -s "$candidate" ]; then patterns="$candidate"; break; fi
  done
  # 两个都不存在时，提示里只推新名字。
  patterns="${patterns:-$HOME/.switchelp-private-patterns}"
fi
if [ -s "$patterns" ]; then
  # 从文件读取模式：清单本身不进仓库，因此这里的匹配不会把特征写进脚本。
  if hits=$(git grep -nIF -f "$patterns" -- . 2>/dev/null); then
    printf '%s\n' "$hits" | head -20
    flag "命中私有清单 $patterns 中的条目"
  else
    ok "对照 $patterns 未命中"
  fi
else
  printf '  · 未配置 %s（可选）\n' "$patterns"
  printf '    建议写入你的供应商域名或内部主机名，每行一条：\n'
  printf "    printf '%%s\\\\n' 'api.your-provider.example' > %s\n" "$patterns"
fi

printf '\n'
if [ "$fail" -eq 0 ]; then
  printf '\033[32m可以发布：未发现需要先处理的内容。\033[0m\n'
else
  printf '\033[31m先处理上面标出的内容，再提交或推送。\033[0m\n'
fi
exit "$fail"
