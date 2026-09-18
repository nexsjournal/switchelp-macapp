#!/usr/bin/env bash
#
# 发布前隐私扫描。只使用**通用规则**，脚本自身不含任何个人标识——
# 任何属于你的私有特征（供应商域名、内部主机名等）都放在仓库之外的清单里：
#
#   printf '%s\n' 'api.your-provider.example' 'internal-host' > ~/.gptswitch-private-patterns
#   scripts/check-publish-safety.sh
#
# 退出码非零表示发现了需要先处理的内容。建议在 push 前跑一次。

set -uo pipefail

fail=0
section() { printf '\n\033[1m%s\033[0m\n' "$1"; }
flag() { printf '  \033[31m✗\033[0m %s\n' "$1"; fail=1; }
# 提示项：需要人工确认，但按惯例可能是文档示例，因此不直接判定失败。
warn() { printf '  \033[33m!\033[0m %s\n' "$1"; }
ok() { printf '  \033[32m✓\033[0m %s\n' "$1"; }

# 只扫描会进入提交的文件；排除本脚本自身——它含有规则文本，否则会自己命中自己。
SELF=':(exclude)scripts/check-publish-safety.sh'
scan() { git grep -nIE "$1" -- . "$SELF" 2>/dev/null; }

user="$(whoami)"
printf '检查 %s 个已跟踪文件（当前用户：%s）\n' "$(git ls-files | wc -l | tr -d ' ')" "$user"

section '① 本机用户名出现在绝对路径里'
if hits=$(scan "/Users/${user}(/|\"|\$)|/home/${user}(/|\"|\$)"); then
  printf '%s\n' "$hits" | head -20
  flag "把本机用户名换成中性占位（例如 /Users/example）后再提交"
else
  ok "未发现 /Users/${user} 形式的路径"
fi

section '② 私网地址（IPv4 四段完整匹配，避免误报版本号）'
# 必须四段齐全：三位版本号（如 10.4.2）不构成地址，否则依赖清单会淹没结果。
if hits=$(scan "(^|[^0-9.])(10\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}|192\.168\.[0-9]{1,3}\.[0-9]{1,3}|172\.(1[6-9]|2[0-9]|3[01])\.[0-9]{1,3}\.[0-9]{1,3})([^0-9.]|$)"); then
  printf '%s\n' "$hits" | head -20
  warn "逐条确认是文档示例（如 RFC 5737 保留地址）而不是你的实际网络"
else
  ok "未发现私网地址"
fi

section '③ 密钥与令牌形态'
if hits=$(scan "sk-[A-Za-z0-9]{20,}|ghp_[A-Za-z0-9]{20,}|gho_[A-Za-z0-9]{20,}|xoxb-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{12,}|-----BEGIN [A-Z ]*PRIVATE KEY"); then
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
if hits=$(scan "[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.(com|cn|net|org|dev|io)" | grep -viE "example\.|@types|users\.noreply|schema\.org|whatwg\.org|json-schema"); then
  printf '%s\n' "$hits" | head -20
  flag "确认是否为文档示例邮箱"
else
  ok "未发现真实邮箱"
fi

section '⑥ 私有特征清单（仓库外维护）'
patterns="${GPTSWITCH_PRIVATE_PATTERNS:-$HOME/.gptswitch-private-patterns}"
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
