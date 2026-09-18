//! 配置备份：写 Codex 配置之前先留一份原样副本。
//!
//! 规则来自 [安全与跨平台](../../../../docs/architecture/05-security-and-platforms.md) 第 5 节：
//! - 原始配置可能含**其他工具**写入的密钥，因此单独权限保护（目录 0700、文件 0600），
//!   并标记“可能含密钥”，界面预览只给遮罩；
//! - 备份**不混入诊断包**——诊断包只从脱敏日志构建，与本模块无关；
//! - 定期清理保留最近 N 份；清理只删本工具自己的备份。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::error::CoreError;
use crate::platform::{self, Platform};

/// 默认保留份数。超过后从最旧的开始删。
pub const DEFAULT_KEEP: usize = 20;
/// 单份备份上限：正常 Codex 配置远小于此，超过说明路径选错了。
pub const MAX_BACKUP_BYTES: u64 = 4 * 1024 * 1024;

/// 一份备份的元数据。正文另存为同名 `.toml`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    /// 备份标识，等于文件名去掉扩展名。
    pub id: String,
    /// 原文件路径。
    pub source_path: String,
    pub created_at: String,
    pub content_hash: String,
    pub bytes: u64,
    /// 内容里出现了疑似密钥：预览必须遮罩，且不要外发。
    pub may_contain_secrets: bool,
}

/// 备份目录。
pub struct BackupStore {
    root: PathBuf,
}

impl BackupStore {
    /// 备份根目录固定为 `<appData>/backups`。
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            root: app_data_dir.join("backups"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 备份一个文件。源文件不存在时返回 `NotFound`——没有原文件就无从备份。
    pub fn create(
        &self,
        source: &Path,
        now_unix_ms: i64,
        created_at: &str,
    ) -> Result<BackupEntry, CoreError> {
        let text = std::fs::read_to_string(source).map_err(|error| {
            CoreError::not_found("待备份的配置文件").with_detail(format!(
                "无法读取 {}：{}",
                source.display(),
                error
            ))
        })?;
        if text.len() as u64 > MAX_BACKUP_BYTES {
            return Err(CoreError::validation(format!(
                "配置文件 {} 字节超过单份备份上限 {}",
                text.len(),
                MAX_BACKUP_BYTES
            )));
        }
        let content_hash = crate::codex::config::hash(&text);
        let id = format!("{now_unix_ms}-{}", &content_hash[..8]);
        std::fs::create_dir_all(&self.root).map_err(|_| CoreError::internal("无法创建备份目录"))?;
        platform::restrict(&self.root, platform::private_dir_mode(Platform::current()))
            .map_err(|_| CoreError::internal("无法设置备份目录权限"))?;

        let body = self.root.join(format!("{id}.toml"));
        std::fs::write(&body, &text).map_err(|_| CoreError::internal("无法写入备份"))?;
        platform::restrict(&body, platform::private_file_mode(Platform::current()))
            .map_err(|_| CoreError::internal("无法设置备份文件权限"))?;

        let entry = BackupEntry {
            id: id.clone(),
            source_path: source.display().to_string(),
            created_at: created_at.to_owned(),
            content_hash,
            bytes: text.len() as u64,
            may_contain_secrets: looks_like_it_contains_secrets(&text),
        };
        let meta = self.root.join(format!("{id}.json"));
        std::fs::write(
            &meta,
            serde_json::to_vec_pretty(&entry)
                .map_err(|_| CoreError::internal("备份元数据序列化失败"))?,
        )
        .map_err(|_| CoreError::internal("无法写入备份元数据"))?;
        platform::restrict(&meta, platform::private_file_mode(Platform::current()))
            .map_err(|_| CoreError::internal("无法设置备份元数据权限"))?;
        Ok(entry)
    }

    /// 全部备份，最新的在前。元数据损坏的条目直接跳过而不是让列表整体失败。
    pub fn list(&self) -> Result<Vec<BackupEntry>, CoreError> {
        let directory = match std::fs::read_dir(&self.root) {
            Ok(directory) => directory,
            // 还没备份过不算错误。
            Err(_) => return Ok(Vec::new()),
        };
        let mut entries: Vec<BackupEntry> = Vec::new();
        for item in directory.flatten() {
            let path = item.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(entry) = serde_json::from_str::<BackupEntry>(&text) else {
                continue;
            };
            // 元数据在但正文不见了：跳过，避免界面给出一个点不动的恢复按钮。
            if !self.root.join(format!("{}.toml", entry.id)).exists() {
                continue;
            }
            entries.push(entry);
        }
        entries.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(entries)
    }

    pub fn read(&self, id: &str) -> Result<String, CoreError> {
        // 只接受本目录内的 id，防止用 ../ 跳出备份目录。
        if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
            return Err(CoreError::validation("备份标识非法"));
        }
        let path = self.root.join(format!("{id}.toml"));
        std::fs::read_to_string(&path).map_err(|_| CoreError::not_found("备份"))
    }

    /// 预览：内容里疑似密钥的部分全部遮罩。返回遮罩后的文本。
    pub fn read_masked(&self, id: &str) -> Result<String, CoreError> {
        let text = self.read(id)?;
        Ok(text.lines().map(mask_line).collect::<Vec<_>>().join("\n"))
    }

    /// 保留最近 `keep` 份，其余删除。返回删除份数。
    pub fn prune(&self, keep: usize) -> Result<usize, CoreError> {
        let entries = self.list()?;
        let mut removed = 0;
        for entry in entries.into_iter().skip(keep) {
            let body = self.root.join(format!("{}.toml", entry.id));
            let meta = self.root.join(format!("{}.json", entry.id));
            if std::fs::remove_file(body).is_ok() {
                removed += 1;
            }
            let _ = std::fs::remove_file(meta);
        }
        Ok(removed)
    }
}

/// 一行里出现疑似密钥时打码。只打码，不改变行结构——预览要能看出结构，看不出值。
fn mask_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for token in split_tokens(line) {
        if looks_like_secret(token) {
            out.push_str("••••••••");
        } else {
            out.push_str(token);
        }
    }
    out
}

/// 按引号与空白切分，保留分隔符，便于只替换值本身。
fn split_tokens(line: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = 0;
    for (index, ch) in line.char_indices() {
        if ch.is_whitespace() || ch == '"' || ch == '\'' || ch == '=' || ch == ',' {
            if start < index {
                tokens.push(&line[start..index]);
            }
            tokens.push(&line[index..index + ch.len_utf8()]);
            start = index + ch.len_utf8();
        }
    }
    if start < line.len() {
        tokens.push(&line[start..]);
    }
    tokens
}

/// 判定一个片段像不像密钥。与诊断模块同一套保守规则：宁可漏，不可误伤正常内容。
fn looks_like_secret(token: &str) -> bool {
    let trimmed = token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
    if trimmed.len() < 24 {
        return false;
    }
    if trimmed.starts_with("sk-") || trimmed.starts_with("ghp_") || trimmed.starts_with("xoxb-") {
        return true;
    }
    if trimmed.len() >= 40 && trimmed.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return true;
    }
    trimmed.len() >= 48
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

/// 文件里是否存在疑似密钥：决定界面是否给出“可能含密钥”的警告。
pub fn looks_like_it_contains_secrets(text: &str) -> bool {
    text.lines().any(|line| {
        // `env_key = "OPENAI_API_KEY"` 这类只是变量名，不算秘密本身。
        if line.contains("env_key") || line.contains("api_key_env") {
            return false;
        }
        split_tokens(line).into_iter().any(looks_like_secret)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET_LINE: &str = "experimental_bearer_token = \"sk-live-0123456789abcdefghijklmnop\"";
    const PLAIN_CONFIG: &str = "model = \"gpt-5-codex\"\napproval_policy = \"on-request\"\n";

    fn store() -> (tempfile::TempDir, BackupStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = BackupStore::new(dir.path());
        (dir, store)
    }

    fn write_source(dir: &Path, text: &str) -> PathBuf {
        let path = dir.join("config.toml");
        std::fs::write(&path, text).unwrap();
        path
    }

    #[test]
    fn create_stores_a_byte_identical_copy_with_metadata() {
        let (dir, store) = store();
        let source = write_source(dir.path(), PLAIN_CONFIG);

        let entry = store
            .create(&source, 1_700_000_000_000, "2026-09-18T00:00:00Z")
            .unwrap();

        assert_eq!(entry.source_path, source.display().to_string());
        assert_eq!(entry.bytes, PLAIN_CONFIG.len() as u64);
        assert!(!entry.may_contain_secrets);
        assert_eq!(
            store.read(&entry.id).unwrap(),
            PLAIN_CONFIG,
            "备份必须是原样副本"
        );
        assert_eq!(store.list().unwrap().len(), 1);
    }

    #[test]
    fn a_backup_containing_a_secret_is_flagged_and_masked_in_preview() {
        let (dir, store) = store();
        let source = write_source(dir.path(), &format!("{PLAIN_CONFIG}{SECRET_LINE}\n"));

        let entry = store
            .create(&source, 1_700_000_000_000, "2026-09-18T00:00:00Z")
            .unwrap();

        assert!(entry.may_contain_secrets, "含疑似密钥时必须标记");
        let preview = store.read_masked(&entry.id).unwrap();
        assert!(
            !preview.contains("sk-live-0123456789abcdefghijklmnop"),
            "预览必须遮罩"
        );
        assert!(
            preview.contains("experimental_bearer_token"),
            "结构要保留，否则看不出是什么字段"
        );
        // 原始内容仍然完整可读：恢复需要真值。
        assert!(store.read(&entry.id).unwrap().contains("sk-live-"));
    }

    #[test]
    fn env_key_names_are_not_mistaken_for_secrets() {
        assert!(!looks_like_it_contains_secrets(
            "env_key = \"OPENAI_API_KEY\"\n"
        ));
        assert!(looks_like_it_contains_secrets(SECRET_LINE));
    }

    #[test]
    fn listing_is_newest_first() {
        let (dir, store) = store();
        let source = write_source(dir.path(), PLAIN_CONFIG);
        store
            .create(&source, 1_700_000_000_000, "2026-09-18T00:00:00Z")
            .unwrap();
        std::fs::write(
            &source,
            format!("{PLAIN_CONFIG}model_provider = \"gptswitch\"\n"),
        )
        .unwrap();
        let newer = store
            .create(&source, 1_700_000_100_000, "2026-09-18T01:00:00Z")
            .unwrap();

        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, newer.id, "最新的排在最前");
    }

    #[test]
    fn prune_keeps_the_newest_and_removes_the_rest() {
        let (dir, store) = store();
        let source = write_source(dir.path(), PLAIN_CONFIG);
        for index in 0..5 {
            std::fs::write(&source, format!("{PLAIN_CONFIG}# {index}\n")).unwrap();
            store
                .create(
                    &source,
                    1_700_000_000_000 + index,
                    &format!("2026-09-18T00:00:0{index}Z"),
                )
                .unwrap();
        }

        assert_eq!(store.prune(2).unwrap(), 3);
        let list = store.list().unwrap();
        assert_eq!(list.len(), 2);
        // 时间戳以 Z 结尾，所以要匹配完整秒位而不是最后一个字符。
        assert!(
            list[0].created_at.ends_with(":04Z"),
            "保留最新的：{:?}",
            list[0].created_at
        );
        assert!(
            list[1].created_at.ends_with(":03Z"),
            "其次是次新的：{:?}",
            list[1].created_at
        );
    }

    #[test]
    fn a_missing_backup_file_is_skipped_instead_of_shown_as_restorable() {
        let (dir, store) = store();
        let source = write_source(dir.path(), PLAIN_CONFIG);
        let entry = store
            .create(&source, 1_700_000_000_000, "2026-09-18T00:00:00Z")
            .unwrap();
        std::fs::remove_file(store.root().join(format!("{}.toml", entry.id))).unwrap();

        assert!(
            store.list().unwrap().is_empty(),
            "正文没了就不该出现在列表里"
        );
    }

    #[test]
    fn identifiers_cannot_escape_the_backup_directory() {
        let (_dir, store) = store();
        for bad in ["", "../config", "a/b", "..", "a\\b"] {
            assert!(store.read(bad).is_err(), "非法标识必须拒绝：{bad}");
        }
    }

    #[test]
    fn backing_up_a_missing_file_is_a_not_found() {
        let (dir, store) = store();
        let error = store
            .create(&dir.path().join("nope.toml"), 1, "2026-09-18T00:00:00Z")
            .unwrap_err();
        assert_eq!(error.code, crate::domain::error::ErrorCode::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn backups_are_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, store) = store();
        let source = write_source(dir.path(), PLAIN_CONFIG);
        let entry = store
            .create(&source, 1_700_000_000_000, "2026-09-18T00:00:00Z")
            .unwrap();

        let mode = std::fs::metadata(store.root().join(format!("{}.toml", entry.id)))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "备份可能含密钥，不能对其他用户可读");
    }
}
