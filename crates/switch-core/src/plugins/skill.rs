//! 技能文档：`SKILL.md` 的解析与指纹。
//!
//! 刻意不引入 YAML 依赖：我们只用 front-matter 里的三个信息（`name`、
//! `description`、`requires_bins`），而写入目标目录、显示正文、算指纹都不依赖 YAML 语义。
//! 引入一个 YAML 解析器换来的额外能力，抵不上它带来的解析差异和依赖面。
//!
//! 解析不出来时**不阻止安装**，只是让 `name` 回落到目录名并在结果里标记
//! `front_matter_parsed = false`——界面照实说明「这份文档的头部没能解析」。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// 单个文件的大小上限。技能是给人读的说明文档，超过这个量级基本不是技能。
pub const MAX_FILE_BYTES: usize = 256 * 1024;
/// 一个技能包含的文件数上限（含 `SKILL.md`）。
pub const MAX_FILES_PER_SKILL: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDocument {
    /// front-matter 的 `name`；解析不到时用调用方给的回落值。
    pub id: String,
    pub title: Option<String>,
    pub description: Option<String>,
    /// front-matter 里声明的外部命令依赖（`metadata.gate.requires_bins`）。
    /// 这些是提示，不在这里校验是否已安装。
    pub requires_bins: Vec<String>,
    /// `SKILL.md` 去掉 front-matter 之后的正文。
    pub body: String,
    /// front-matter 是否解析成功。false 时 `id` 来自回落值。
    pub front_matter_parsed: bool,
}

/// 从 `SKILL.md` 全文解析出技能文档。
///
/// `fallback_id` 是解析不出 `name` 时用的标识（调用方一般给目录名）。
pub fn parse(markdown: &str, fallback_id: &str) -> SkillDocument {
    let (front_matter, body) = split_front_matter(markdown);
    let mut parsed = SkillDocument {
        id: fallback_id.to_owned(),
        title: None,
        description: None,
        requires_bins: Vec::new(),
        body,
        front_matter_parsed: false,
    };
    let Some(front_matter) = front_matter else {
        return parsed;
    };

    let mut saw_any_key = false;
    for line in front_matter.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim_start().starts_with('#') {
            continue;
        }
        let indent = trimmed.len() - trimmed.trim_start().len();
        let Some((key, value)) = trimmed.trim_start().split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        // 只认顶层键；嵌套结构由 requires_bins 的定向扫描负责。
        if indent == 0 {
            match key {
                "name" => {
                    if let Some(text) = unquote(value) {
                        if !text.is_empty() {
                            parsed.id = text;
                            saw_any_key = true;
                        }
                    }
                }
                "title" => {
                    parsed.title = unquote(value);
                    saw_any_key = true;
                }
                "description" => {
                    parsed.description = unquote(value);
                    saw_any_key = true;
                }
                _ => {}
            }
        }
        if (key == "requires_bins" || key == "bins") && !value.is_empty() {
            parsed.requires_bins = parse_inline_list(value);
            saw_any_key = true;
        }
    }
    parsed.front_matter_parsed = saw_any_key;
    parsed
}

/// 切出 front-matter 与正文。没有 front-matter 时返回 `(None, 全文)`。
fn split_front_matter(markdown: &str) -> (Option<String>, String) {
    let trimmed_start = markdown.trim_start_matches(['\u{feff}', '\n', '\r']);
    let Some(rest) = trimmed_start.strip_prefix("---") else {
        return (None, markdown.to_owned());
    };
    // 第一行必须是单独的分隔线，否则可能是正文里的水平线。
    let Some(rest) = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))
    else {
        return (None, markdown.to_owned());
    };
    for (index, _) in rest.match_indices("---") {
        let at_line_start = rest[..index]
            .chars()
            .next_back()
            .map(|character| character == '\n')
            .unwrap_or(false);
        if !at_line_start {
            continue;
        }
        let after = &rest[index..];
        let closing_len = if after.starts_with("---") {
            3
        } else {
            continue;
        };
        let front = rest[..index].to_owned();
        let body = after[closing_len..]
            .trim_start_matches(['\r', '\n'])
            .to_owned();
        return (Some(front), body);
    }
    (None, markdown.to_owned())
}

/// 去掉包裹的引号。值本身含引号时保守处理：只在首尾成对时剥离。
fn unquote(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            let inner = &value[1..value.len() - 1];
            return Some(inner.replace(&format!("\\{quote}"), &quote.to_string()));
        }
    }
    Some(value.to_owned())
}

/// 解析 `["a", "b"]` 或 `[a, b]` 形式的行内列表。
fn parse_inline_list(value: &str) -> Vec<String> {
    let value = value.trim();
    let inner = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(value);
    inner
        .split(',')
        .filter_map(unquote)
        .map(|item| item.trim().to_owned())
        .filter(|item| !item.is_empty())
        .collect()
}

/// 文件内容指纹。卸载前用它确认「这个文件还是我们装的那个」。
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_shape_a_real_skill_uses() {
        let text = r#"---
name: xs-github
description: "管理 GitHub 仓库、Issue、Pull Request、Release 与 Actions。"
metadata:
  author: 星算助手
  agenticx:
    gate:
      requires_bins: ["gh"]
---

# GitHub

正文第一段。
"#;
        let document = parse(text, "fallback");
        assert_eq!(document.id, "xs-github");
        assert_eq!(
            document.description.as_deref(),
            Some("管理 GitHub 仓库、Issue、Pull Request、Release 与 Actions。")
        );
        assert_eq!(document.requires_bins, vec!["gh".to_owned()]);
        assert!(document.front_matter_parsed);
        assert!(document.body.contains("正文第一段"));
        assert!(
            !document.body.contains("name: xs-github"),
            "正文不能带 front-matter"
        );
    }

    #[test]
    fn unbracketed_list_also_works() {
        let text = "---\nname: a\nrequires_bins: gh, ffmpeg\n---\nbody";
        let document = parse(text, "fallback");
        assert_eq!(
            document.requires_bins,
            vec!["gh".to_owned(), "ffmpeg".to_owned()]
        );
    }

    #[test]
    fn missing_front_matter_falls_back_without_failing() {
        let document = parse("# 只有正文\n", "dir-name");
        assert_eq!(document.id, "dir-name");
        assert!(!document.front_matter_parsed);
        assert!(document.body.contains("只有正文"));
    }

    #[test]
    fn front_matter_without_name_reports_fallback_id() {
        let document = parse("---\ndescription: 没有名字\n---\n正文", "dir-name");
        assert_eq!(document.id, "dir-name");
        assert!(document.front_matter_parsed, "有 front-matter 就算解析过");
    }

    #[test]
    fn horizontal_rule_is_not_treated_as_front_matter() {
        let document = parse("正文\n\n---\n\nmore\n", "dir-name");
        assert!(!document.front_matter_parsed);
        assert!(document.body.contains("more"));
    }

    #[test]
    fn nested_keys_are_not_mistaken_for_top_level_ones() {
        let text = "---\nmetadata:\n  name: not-the-skill-name\n---\nbody";
        let document = parse(text, "fallback");
        assert_eq!(document.id, "fallback");
    }

    #[test]
    fn single_quoted_values_are_unwrapped() {
        let document = parse("---\nname: 'quoted'\n---\n", "fallback");
        assert_eq!(document.id, "quoted");
    }

    #[test]
    fn fingerprint_is_stable_and_content_sensitive() {
        assert_eq!(fingerprint(b"abc"), fingerprint(b"abc"));
        assert_ne!(fingerprint(b"abc"), fingerprint(b"abd"));
    }
}
