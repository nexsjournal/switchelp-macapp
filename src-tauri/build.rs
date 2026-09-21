//! Tauri 构建脚本。在调用框架之前，先保证 sidecar 文件存在。

use std::path::{Path, PathBuf};

/// 开发/CI 里不会先跑 `scripts/build-bridge.mjs`（只有打包才会），而 Tauri 的 sidecar
/// 必须在**编译期**就存在，否则连 `cargo build` 都失败。
///
/// 这里补一个**会自己报错的占位件**，而不是让构建直接挂掉或悄悄塞一个空文件：
/// 空文件被当成真 bridge 交给宿主，就会变成「菜单里能选、一发请求就失败」——那是本项目
/// 明确不做的事。占位件执行时打印原因并退出，应用侧还会在安装前认出它是脚本并拒绝
/// （见 `switch_core::codex::coexist::install_bridge`），两道闸都在。
fn ensure_sidecar() {
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    let suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let path: PathBuf =
        Path::new("binaries").join(format!("gptswitch-bridge-app-{target}{suffix}"));
    if path.exists() {
        return;
    }
    if std::fs::create_dir_all(path.parent().expect("binaries 目录")).is_err() {
        return;
    }
    let placeholder = "#!/bin/sh\n\
         echo \"gptswitch-bridge: 这是随包分发前的占位件，不是真正的 bridge。\" >&2\n\
         echo \"请先运行 node scripts/build-bridge.mjs（打包时由 tauri 的 beforeBuildCommand 自动执行）。\" >&2\n\
         exit 2\n";
    if std::fs::write(&path, placeholder).is_err() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    }
}

fn main() {
    ensure_sidecar();
    tauri_build::build()
}
