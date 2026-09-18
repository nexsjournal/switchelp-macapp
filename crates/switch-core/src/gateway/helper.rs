//! 凭据 helper：宿主以 `<command> --instance <id>` 调用它，并把 stdout 当作本机令牌。
//!
//! 为什么落地成一个脚本：宿主只接受**一个可执行文件路径**，参数由内核固定拼成
//! `--instance <id>`（见 `codex/config.rs` 的 `ProviderAuth::Command` 渲染），
//! 所以 helper 必须自己从磁盘取令牌。
//!
//! 安全边界：
//! - 令牌每次启动重新生成，只写入应用数据目录（目录 0700、文件 0600）；
//! - 令牌只授权本机网关的推理接口，不是上游 Key；
//! - 上游 Key 始终留在系统凭据库，helper 不接触它；
//! - helper 校验 `--instance`，避免被用来服务另一个实例。

use std::path::{Path, PathBuf};

use super::auth::GatewayToken;
use crate::domain::error::CoreError;

/// helper 所在路径。与 `GatewayLayout::auth_helper` 必须一致。
pub fn helper_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("bin").join(helper_file_name())
}

/// 令牌文件路径。与 helper 脚本里的取值必须一致。
pub fn token_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("gateway-token")
}

/// 文件名交给平台策略，避免这里和 `platform` 各写一份 cfg。
fn helper_file_name() -> &'static str {
    crate::platform::helper_file_name(crate::platform::Platform::current())
}

/// 写入令牌并把 helper 安装到应用数据目录，返回 helper 绝对路径。
///
/// 每次调用都会覆盖令牌：上一次运行的令牌立即失效。
pub fn install(
    app_data_dir: &Path,
    instance_id: &str,
    token: &GatewayToken,
) -> Result<PathBuf, CoreError> {
    // 实例标识会被插进脚本文本，必须先证明它只含安全字符；
    // 否则一个带引号或分号的实例名就能改写脚本内容。
    if !is_shell_safe(instance_id) {
        return Err(CoreError::validation(
            "实例标识含不安全字符，无法生成凭据 helper",
        ));
    }
    let bin_dir = app_data_dir.join("bin");
    std::fs::create_dir_all(&bin_dir)
        .map_err(|_| CoreError::internal("无法创建 helper 目录"))?;
    restrict(&bin_dir, 0o700)?;

    let token_file = token_path(app_data_dir);
    // 不写结尾换行：helper 用 cat 原样输出，令牌里不应混入空白。
    std::fs::write(&token_file, token.expose())
        .map_err(|_| CoreError::internal("无法写入网关令牌"))?;
    restrict(&token_file, 0o600)?;

    let helper = helper_path(app_data_dir);
    std::fs::write(&helper, script(instance_id))
        .map_err(|_| CoreError::internal("无法写入凭据 helper"))?;
    restrict(&helper, 0o700)?;
    Ok(helper)
}

/// 删除令牌文件。进程退出时调用，避免令牌留在磁盘上。
pub fn revoke(app_data_dir: &Path) {
    let _ = std::fs::remove_file(token_path(app_data_dir));
}

fn is_shell_safe(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'))
}

/// 权限位按平台策略取；Windows 返回 None，表示交给用户目录 ACL。
fn restrict(path: &Path, unix_mode: u32) -> Result<(), CoreError> {
    let platform = crate::platform::Platform::current();
    let mode = if unix_mode == 0o600 {
        crate::platform::private_file_mode(platform)
    } else {
        crate::platform::private_dir_mode(platform)
    };
    crate::platform::restrict(path, mode).map_err(|_| CoreError::internal("无法设置文件权限"))
}

#[cfg(unix)]
fn script(instance_id: &str) -> String {
    format!(
        r#"#!/bin/sh
# Switchelp 本机网关凭据 helper。
# 由宿主以 `--instance <id>` 调用；只输出本机访问令牌，不接触上游 Key。
set -eu
instance=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --instance) instance="${{2:-}}"; shift 2 2>/dev/null || shift ;;
        *) shift ;;
    esac
done
if [ -n "$instance" ] && [ "$instance" != "{instance_id}" ]; then
    echo "gptswitch-auth-helper: instance mismatch" >&2
    exit 1
fi
dir="$(cd "$(dirname "$0")/.." && pwd)"
cat "$dir/gateway-token"
"#
    )
}

#[cfg(windows)]
fn script(_instance_id: &str) -> String {
    // Windows 版本尚未在真机验证；这里显式留空，由 install 之外的装配层决定是否启用。
    String::from("@echo off\r\necho gptswitch-auth-helper: not implemented for windows >&2\r\nexit /b 1\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> GatewayToken {
        GatewayToken::from_raw("a".repeat(64))
    }

    #[test]
    fn install_writes_a_helper_that_prints_exactly_the_token() {
        let dir = tempfile::tempdir().unwrap();
        let helper = install(dir.path(), "inst_1", &token()).unwrap();

        assert_eq!(helper, helper_path(dir.path()));
        assert!(helper.exists());
        assert_eq!(
            std::fs::read_to_string(token_path(dir.path())).unwrap(),
            token().expose(),
            "令牌文件不得带结尾换行"
        );
    }

    #[cfg(unix)]
    #[test]
    fn helper_returns_the_token_and_rejects_another_instance() {
        use std::process::Command;
        let dir = tempfile::tempdir().unwrap();
        let helper = install(dir.path(), "inst_1", &token()).unwrap();

        let output = Command::new(&helper)
            .args(["--instance", "inst_1"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            token().expose(),
            "宿主读到的必须是纯令牌"
        );

        let mismatch = Command::new(&helper)
            .args(["--instance", "inst_other"])
            .output()
            .unwrap();
        assert!(!mismatch.status.success(), "不得服务其他实例");
        assert!(mismatch.stdout.is_empty(), "失败时不能吐出令牌");
    }

    #[cfg(unix)]
    #[test]
    fn helper_and_token_are_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let helper = install(dir.path(), "inst_1", &token()).unwrap();

        let helper_mode = std::fs::metadata(&helper).unwrap().permissions().mode() & 0o777;
        let token_mode = std::fs::metadata(token_path(dir.path()))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(helper_mode, 0o700);
        assert_eq!(token_mode, 0o600, "令牌不能对其他用户可读");
    }

    #[test]
    fn reinstalling_rotates_the_token() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "inst_1", &token()).unwrap();
        let rotated = GatewayToken::from_raw("b".repeat(64));
        install(dir.path(), "inst_1", &rotated).unwrap();

        assert_eq!(
            std::fs::read_to_string(token_path(dir.path())).unwrap(),
            rotated.expose(),
            "上一次运行的令牌必须失效"
        );
    }

    #[test]
    fn revoke_removes_the_token_file() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "inst_1", &token()).unwrap();
        revoke(dir.path());
        assert!(!token_path(dir.path()).exists());
        // 幂等：重复撤销不报错。
        revoke(dir.path());
    }

    #[test]
    fn unsafe_instance_ids_are_refused_instead_of_being_written_into_the_script() {
        let dir = tempfile::tempdir().unwrap();
        for bad in [
            "inst\"; rm -rf /",
            "inst$(whoami)",
            "inst_1\nrm -rf /",
            "inst/../..",
            "",
        ] {
            assert!(
                install(dir.path(), bad, &token()).is_err(),
                "不安全的实例标识必须被拒绝：{bad:?}"
            );
        }
    }
}
