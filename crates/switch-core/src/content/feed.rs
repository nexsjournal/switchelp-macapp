//! 订阅源解析：RSS 2.0 / Atom，以及 GitHub 搜索接口的条目。
//!
//! 用 `quick-xml`（已在本应用依赖图里，经 `plist` 间接引入）而不是手写 XML 扫描：
//! 实体、CDATA、自闭合标签、命名空间前缀都得处理，手写的分支数比依赖多。
//!
//! 解析失败的源**只影响它自己**：调用方保留上一次成功的快照并记下失败原因，
//! 不会让整页变成错误。

use serde::{Deserialize, Serialize};
use time::{format_description::well_known::Rfc2822, OffsetDateTime};

use crate::domain::error::{CoreError, ErrorCode};

/// 单条资讯的摘要长度上限。展示用，不需要全文。
pub const MAX_SUMMARY_CHARS: usize = 400;
/// 一个源单次解析保留的条目上限。
pub const MAX_ITEMS_PER_FEED: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedItem {
    pub title: String,
    pub url: String,
    /// 发布时间；源没给或不认识格式时为 `None`，由调用方用「首次见到的时间」补位。
    pub published_at: Option<i64>,
    pub summary: String,
    /// 源里的稳定标识（`guid` / `id`）。缺省时用 URL。
    pub id: String,
    /// 仅 GitHub 搜索条目有：星标数与仓库全名。RSS 条目一律为 `None`。
    #[serde(default)]
    pub stars: Option<u64>,
    #[serde(default)]
    pub repo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedFeed {
    pub title: Option<String>,
    pub items: Vec<ParsedItem>,
    /// 是否因为条数上限被截断。
    pub truncated: bool,
}

/// 解析一份 RSS / Atom 文档。
pub fn parse(xml: &str, now: i64) -> Result<ParsedFeed, CoreError> {
    let mut reader = quick_xml::Reader::from_str(xml);
    // 不在读取层裁剪空白：解析器会把 `A &amp; B` 切成多个文本事件，
    // 每个事件各自 trim 会把中间的空格吃掉（变成 `A&B`）。统一在 clean() 里压空白。
    reader.config_mut().trim_text(false);
    // 也不把自闭合标签展开：Atom 的 `<link href="…"/>` 需要在 Empty 事件里读属性。
    reader.config_mut().expand_empty_elements = false;

    let mut feed_title: Option<String> = None;
    let mut items: Vec<ParsedItem> = Vec::new();
    let mut current: Option<ItemBuilder> = None;
    let mut path: Vec<String> = Vec::new();
    let mut truncated = false;

    loop {
        match reader.read_event() {
            Err(error) => {
                return Err(CoreError::new(ErrorCode::Internal, "error.feedUnparsable")
                    .with_detail(format!("订阅源内容不是合法 XML：{error}")))
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(quick_xml::events::Event::Start(start)) => {
                let name = local_name(start.name().as_ref());
                // RSS 用 item，Atom 用 entry；两种都认。
                if name == "item" || name == "entry" {
                    if items.len() >= MAX_ITEMS_PER_FEED {
                        truncated = true;
                        // 到上限就不再收集，但要继续读到文档末尾，否则读游标停在半个文档上。
                        current = None;
                    } else {
                        current = Some(ItemBuilder::default());
                    }
                }
                path.push(name);
            }
            Ok(quick_xml::events::Event::End(_)) => {
                let name = path.pop().unwrap_or_default();
                if name == "item" || name == "entry" {
                    if let Some(builder) = current.take() {
                        if let Some(item) = builder.finish() {
                            items.push(item);
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::Empty(start)) => {
                let name = local_name(start.name().as_ref());
                // 自闭合元素是完整的元素，不走 start/end 配对，因此不进 path。
                // Atom 的 `<link href="…"/>` 就在这里取值。
                if name == "link" {
                    let href = attribute(&start, "href");
                    if let Some(target) = current.as_mut() {
                        target.absorb_link(href, attribute(&start, "rel"));
                    }
                }
            }
            Ok(quick_xml::events::Event::Text(text)) => {
                let raw = text.xml10_content().into_owned();
                let resolved = quick_xml::escape::unescape(&raw)
                    .map(|value| value.into_owned())
                    .unwrap_or(raw);
                absorb_text(&mut current, &mut feed_title, &path, &resolved);
            }
            Ok(quick_xml::events::Event::CData(data)) => {
                let text = data.into_inner().into_owned();
                absorb_text(&mut current, &mut feed_title, &path, &text);
            }
            // 实体引用是单独的事件。认不出的一律丢掉，**不猜**它代表什么字符。
            Ok(quick_xml::events::Event::GeneralRef(reference)) => {
                if let Some(text) = resolve_reference(&reference.into_inner()) {
                    absorb_text(&mut current, &mut feed_title, &path, &text);
                }
            }
            // 注释、声明、PI 一律忽略：它们都不是条目内容。
            Ok(_) => {}
        }
    }

    let items = items
        .into_iter()
        .map(|mut item| {
            // 发布时间认不出来的用「见到它的时间」，这样排序不会把它扔到最后。
            if item.published_at.is_none() {
                item.published_at = Some(now);
            }
            item
        })
        .collect();

    Ok(ParsedFeed {
        title: feed_title
            .map(|title| clean(&title))
            .filter(|title| !title.is_empty()),
        items,
        truncated,
    })
}

fn absorb_text(
    current: &mut Option<ItemBuilder>,
    feed_title: &mut Option<String>,
    path: &[String],
    text: &str,
) {
    let Some(tag) = path.last().map(String::as_str) else {
        return;
    };
    match current.as_mut() {
        Some(builder) => builder.absorb(tag, text),
        None => {
            if tag == "title" {
                let entry = feed_title.get_or_insert_with(String::new);
                entry.push_str(text);
            }
        }
    }
}

#[derive(Default)]
struct ItemBuilder {
    title: String,
    link: String,
    id: String,
    summary: String,
    published: String,
}

impl ItemBuilder {
    /// 追加文本。用追加而不是「第一次胜出」，因为解析器会把一段文字按实体引用切开：
    /// `第一条 &amp; 标题` 会到成两个事件，只取第一个就会把标题截断。
    fn absorb(&mut self, tag: &str, text: &str) {
        match tag {
            "title" => self.title.push_str(text),
            "link" => self.link.push_str(text),
            "guid" | "id" => self.id.push_str(text),
            "description" | "summary" | "content" => self.summary.push_str(text),
            "pubDate" | "published" | "updated" | "date" => self.published.push_str(text),
            _ => (),
        }
    }

    /// Atom 的 `<link rel="alternate" href="…"/>`；没有 rel 的按正文链接处理。
    fn absorb_link(&mut self, href: Option<String>, rel: Option<String>) {
        let rel_is_alternate = rel
            .as_deref()
            .map(|rel| rel == "alternate" || rel.is_empty())
            .unwrap_or(true);
        if !rel_is_alternate {
            return;
        }
        if let Some(href) = href {
            if self.link.is_empty() {
                self.link = href;
            }
        }
    }

    fn finish(self) -> Option<ParsedItem> {
        let title = clean(&self.title);
        let url = clean(&self.link);
        // 没有标题也没有链接的条目没有展示价值，直接丢掉，不占版面。
        if title.is_empty() && url.is_empty() {
            return None;
        }
        let id = clean(&self.id);
        let url_for_id = url.clone();
        Some(ParsedItem {
            title: if title.is_empty() { url.clone() } else { title },
            url,
            published_at: parse_timestamp(&clean(&self.published)),
            summary: truncate(&clean(&self.summary)),
            // 源里没有稳定标识时用 URL 兜底，之后不必再判空。
            id: if id.is_empty() { url_for_id } else { id },
            stars: None,
            repo: None,
        })
    }
}

/// 时间戳解析。RSS 用 RFC 2822（`pubDate`），Atom 用 RFC 3339。
/// 两种都认不出来时返回 `None`——**不猜**。
pub fn parse_timestamp(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(parsed) = OffsetDateTime::parse(raw, &Rfc2822) {
        return Some(parsed.unix_timestamp());
    }
    if let Ok(parsed) = OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339) {
        return Some(parsed.unix_timestamp());
    }
    // 少数源给 `2026-09-21 10:00:00` 这种没有时区的写法，按 UTC 解释。
    if let Ok(parsed) = time::PrimitiveDateTime::parse(
        raw,
        &time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
    ) {
        return Some(parsed.assume_utc().unix_timestamp());
    }
    None
}

/// 去掉命名空间前缀：`content:encoded` 与 `encoded` 在我们眼里是同一个标签。
/// 解析一个实体引用（`amp`、`#39`、`#x27`）。认不出来返回 `None`。
fn resolve_reference(name: &str) -> Option<String> {
    let name = name.trim();
    if let Some(digits) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
        return u32::from_str_radix(digits, 16)
            .ok()
            .and_then(char::from_u32)
            .map(|character| character.to_string());
    }
    if let Some(digits) = name.strip_prefix('#') {
        return digits
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(|character| character.to_string());
    }
    match name {
        "amp" => Some("&".to_owned()),
        "lt" => Some("<".to_owned()),
        "gt" => Some(">".to_owned()),
        "quot" => Some("\"".to_owned()),
        "apos" => Some("'".to_owned()),
        _ => None,
    }
}

fn local_name(name: &str) -> String {
    name.rsplit(':').next().unwrap_or(name).to_owned()
}

fn attribute(start: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    start
        .attributes()
        .filter_map(Result::ok)
        .find(|attribute| local_name(attribute.key.as_ref()) == key)
        .map(|attribute| attribute.value.into_owned())
        .map(|value| {
            quick_xml::escape::unescape(&value)
                .map(|unescaped| unescaped.into_owned())
                .unwrap_or(value)
        })
}

/// 压掉换行与连续空白：源里的标题常常带着缩进和换行，直接显示会很难看。
fn clean(text: &str) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.trim().to_owned()
}

fn truncate(text: &str) -> String {
    let mut out = String::new();
    for character in text.chars().take(MAX_SUMMARY_CHARS) {
        out.push(character);
    }
    if text.chars().count() > MAX_SUMMARY_CHARS {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rss_with_cdata_and_entities() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>示例站点</title>
    <item>
      <title>第一条 &amp; 它的标题</title>
      <link>https://example.com/1</link>
      <guid>https://example.com/1</guid>
      <pubDate>Mon, 21 Sep 2026 10:00:00 +0800</pubDate>
      <description><![CDATA[<p>摘要里有 <b>HTML</b></p>]]></description>
    </item>
    <item>
      <title>第二条</title>
      <link>https://example.com/2</link>
    </item>
  </channel>
</rss>"#;
        let feed = parse(xml, 5_000).unwrap();
        assert_eq!(feed.title.as_deref(), Some("示例站点"));
        assert_eq!(feed.items.len(), 2);
        assert_eq!(feed.items[0].title, "第一条 & 它的标题");
        assert_eq!(feed.items[0].url, "https://example.com/1");
        assert_eq!(feed.items[0].published_at, Some(1_789_956_000));
        assert!(feed.items[0].summary.contains("<b>HTML</b>"));
        // 没有 pubDate 的条目用「见到它的时间」补位，而不是丢掉。
        assert_eq!(feed.items[1].published_at, Some(5_000));
    }

    #[test]
    fn parses_atom_with_self_closing_links() {
        let xml = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom 源</title>
  <entry>
    <title>Atom 第一条</title>
    <link rel="alternate" href="https://example.org/a"/>
    <link rel="self" href="https://example.org/feed/a"/>
    <id>tag:example.org,2026:a</id>
    <updated>2026-09-21T02:00:00Z</updated>
    <summary>摘要文本</summary>
  </entry>
</feed>"#;
        let feed = parse(xml, 0).unwrap();
        assert_eq!(feed.title.as_deref(), Some("Atom 源"));
        assert_eq!(feed.items.len(), 1);
        assert_eq!(feed.items[0].url, "https://example.org/a");
        assert_eq!(feed.items[0].id, "tag:example.org,2026:a");
        assert_eq!(feed.items[0].published_at, Some(1_789_956_000));
    }

    #[test]
    fn entries_without_title_or_link_are_dropped() {
        let xml = "<rss><channel><item><guid>only-id</guid></item></channel></rss>";
        let feed = parse(xml, 0).unwrap();
        assert!(feed.items.is_empty(), "没有标题也没有链接的条目不该占版面");
    }

    #[test]
    fn entry_without_a_link_is_kept_with_an_empty_url() {
        let xml = "<rss><channel><item><title>只有标题</title></item></channel></rss>";
        let feed = parse(xml, 0).unwrap();
        assert_eq!(feed.items[0].title, "只有标题");
        assert_eq!(feed.items[0].url, "", "没有链接就是没有，不拿标题冒充链接");
    }

    #[test]
    fn malformed_xml_is_an_error_not_a_panic() {
        // 标签嵌套错乱是真正的格式错误，必须报出来而不是当成空源。
        let error = parse("<rss><channel><item><title>a</title></rss></channel>", 0).unwrap_err();
        assert!(!error.safe_details.is_empty());
        // 被截断的文档不报错，只是少几条——这是列表展示可以接受的结果。
        assert!(parse("<rss><channel><item><title>a</title>", 0).is_ok());
    }

    #[test]
    fn item_cap_is_reported() {
        let mut xml = String::from("<rss><channel>");
        for index in 0..(MAX_ITEMS_PER_FEED + 5) {
            xml.push_str(&format!(
                "<item><title>t{index}</title><link>https://example.com/{index}</link></item>"
            ));
        }
        xml.push_str("</channel></rss>");
        let feed = parse(&xml, 0).unwrap();
        assert_eq!(feed.items.len(), MAX_ITEMS_PER_FEED);
        assert!(feed.truncated);
    }

    #[test]
    fn entity_references_inside_text_are_resolved() {
        let xml = "<rss><channel><item><title>A &amp; B &#65; &#x42;</title><link>u</link><description>&lt;p&gt;hi&lt;/p&gt;</description></item></channel></rss>";
        let feed = parse(xml, 0).unwrap();
        assert_eq!(feed.items[0].title, "A & B A B");
        assert_eq!(feed.items[0].summary, "<p>hi</p>");
    }

    #[test]
    fn unknown_entities_are_dropped_rather_than_guessed() {
        assert_eq!(resolve_reference("amp").as_deref(), Some("&"));
        assert_eq!(resolve_reference("#39").as_deref(), Some("'"));
        assert_eq!(resolve_reference("#x27").as_deref(), Some("'"));
        assert_eq!(resolve_reference("nbsp"), None);
    }

    #[test]
    fn summaries_are_truncated() {
        let long = "字".repeat(MAX_SUMMARY_CHARS + 50);
        let item = ItemBuilder {
            title: "t".to_owned(),
            summary: long,
            ..Default::default()
        }
        .finish()
        .unwrap();
        assert_eq!(item.summary.chars().count(), MAX_SUMMARY_CHARS + 1);
        assert!(item.summary.ends_with('…'));
    }

    #[test]
    fn whitespace_in_titles_is_collapsed() {
        let xml = "<rss><channel><item><title>\n    标题\n    带换行  </title><link>u</link></item></channel></rss>";
        let feed = parse(xml, 0).unwrap();
        assert_eq!(feed.items[0].title, "标题 带换行");
    }

    #[test]
    fn unrecognised_dates_are_left_unknown() {
        assert_eq!(parse_timestamp("昨天"), None);
        assert_eq!(parse_timestamp(""), None);
        assert!(parse_timestamp("2026-09-21 10:00:00").is_some());
    }
}
