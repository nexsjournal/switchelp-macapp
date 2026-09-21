//! 测试用的假 app-server。
//!
//! 只存在于 debug 构建（`#[cfg(debug_assertions)]`）：集成测试需要两根**真的子进程**
//! 来验证合并与路由，而真实的 codex 不可能出现在 CI 里，也不该在单测里被启动。
//! 这个 mock 按 `CODEX_HOME` 扮演其中一根，行为足够回答 bridge 会问的每一个问题。
//!
//! 它存在的唯一理由是测试：release 构建里没有这段代码。

use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// 扮演哪一根：CODEX_HOME 等于 GPTSWITCH_BRIDGE_MOCK_MANAGED 就是托管那根。
fn is_managed() -> bool {
    let managed = std::env::var("GPTSWITCH_BRIDGE_MOCK_MANAGED").unwrap_or_default();
    !managed.is_empty() && std::env::var("CODEX_HOME").unwrap_or_default() == managed
}

fn side() -> &'static str {
    if is_managed() {
        "managed"
    } else {
        "native"
    }
}

fn models() -> Value {
    if is_managed() {
        json!([
            {"id": "gs/mock-managed-1", "model": "gs/mock-managed-1", "displayName": "qiyuan/模型一"},
            {"id": "gs/mock-managed-2", "model": "gs/mock-managed-2", "displayName": "qiyuan/模型二"}
        ])
    } else {
        json!([
            {"id": "gpt-5.6-sol", "model": "gpt-5.6-sol", "displayName": "GPT-5.6 Sol"},
            {"id": "gpt-5.5", "model": "gpt-5.5", "displayName": "GPT-5.5"}
        ])
    }
}

fn threads() -> Value {
    if is_managed() {
        json!([{"id": "managed-thread-1", "modelProvider": "gptswitch", "name": "用我们的模型开的对话"}])
    } else {
        json!([{"id": "native-thread-1", "modelProvider": "openai", "name": "原生对话"}])
    }
}

/// 线程 id 的前缀即归属：`native-*` 只认原生，`managed-*` 只认托管。
fn owns(thread_id: &str) -> bool {
    if is_managed() {
        thread_id.starts_with("managed-")
    } else {
        thread_id.starts_with("native-")
    }
}

fn handle(message: &Value) -> Value {
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    match method {
        "initialize" => json!({
            "codexHome": std::env::var("CODEX_HOME").unwrap_or_default(),
            "mockSide": side(),
            "userAgent": "mock",
        }),
        "model/list" => json!({"data": models(), "nextCursor": null, "cursor": null}),
        "thread/list" => json!({"data": threads(), "nextCursor": null, "cursor": null}),
        "thread/start" => {
            let model = params
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let ours = model.starts_with("gs/");
            if ours != is_managed() {
                return json!({"error": {"code": -32600, "message": format!("{model} 不属于这一根")}});
            }
            let id = format!(
                "{}-start-{}",
                if is_managed() { "managed" } else { "native" },
                model.replace('/', "_")
            );
            json!({"thread": {"id": id, "model": model, "mockSide": side()}})
        }
        "thread/resume" | "thread/read" | "thread/archive" | "thread/unarchive" => {
            let thread_id = params
                .get("threadId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if !owns(&thread_id) {
                return json!({"error": {"code": -32600, "message": format!("这条线程不在 {} 上", side())}});
            }
            json!({"thread": {"id": thread_id, "mockSide": side()}})
        }
        "turn/start" | "thread/name/set" | "thread/items/list" => {
            json!({"ok": true, "mockSide": side()})
        }
        _ => json!({"error": {"code": -32601, "message": format!("mock 不认识的 {method}")}}),
    }
}

pub fn run(_argv: &[String]) -> i32 {
    // 第一条通知（initialized）之类没有 id 的消息：什么都不用回。
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let outcome = handle(&message);
        let reply = if let Some(error) = outcome.get("error") {
            json!({"jsonrpc": "2.0", "id": id, "error": error})
        } else {
            json!({"jsonrpc": "2.0", "id": id, "result": outcome})
        };
        let mut stdout = std::io::stdout();
        if writeln!(stdout, "{reply}").is_err() || stdout.flush().is_err() {
            return 0;
        }
    }
    0
}
