//! 入口：顶替宿主眼里的 codex CLI。
//!
//! 真实用法（由 Switchelp 发起）：
//!
//! ```text
//! CODEX_CLI_PATH=<app-data>/bin/gptswitch-bridge \
//! GPTSWITCH_BRIDGE_CODEX=/Applications/ChatGPT.app/Contents/Resources/codex-cli/bin/codex \
//! GPTSWITCH_BRIDGE_MANAGED_HOME=<app-data>/codex-home \
//!   open -a ChatGPT
//! ```
//!
//! 判断与路由的规则都在 [`gptswitch_bridge`] 里；这里只负责把退出码交出去。

fn main() {
    std::process::exit(gptswitch_bridge::run());
}
