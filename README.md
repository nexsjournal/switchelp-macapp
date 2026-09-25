![Switchelp](./assets/banner-img.png)

# Switchelp

**English** · [简体中文](README.zh-CN.md)

Make third-party providers show up in **Codex's own model picker**, and manage providers, API keys, and each
model's context limit, output limit, input capabilities and reasoning levels.

A local macOS configuration app. Tauri 2 + React 18 + TypeScript shell; every decision lives in a Rust core
(`crates/switch-core`) that does not depend on the window framework.

![The Switchelp app window](./assets/appview.jpg)

## How it works

```
Codex  →  ~/.codex/config.toml (the fields this tool manages)
       →  model_providers.gptswitch → local gateway 127.0.0.1:18765
       →  auth helper reads the local token → routes by alias to the provider → upstream
```

- The **model menu** comes from a compiled catalog file (`model_catalog_json`), not from a list this tool draws
  itself. Note that `model_catalog_json` **replaces** the host's model list rather than extending it: while the tool
  is applied, the built-in models are not in the picker until you restore native mode.
- **Coexist mode (Bridge)** keeps the official models in the menu. Instead of replacing `model_catalog_json` in
  your real config, the app writes a managed profile under its own app-data directory and starts Codex through
  `gptswitch-bridge`: that bridge runs two codex processes (yours, untouched, and the managed one), merges
  `model/list` and `thread/list`, and pins every conversation to the process it started on. macOS only for now;
  it works while the host is launched by this app (reopening Codex from the Dock falls back to plain native, and
  the app says so).

- When upstream speaks `chat/completions`, the gateway translates both ways into Responses. Fields that cannot be
  expressed are **recorded as losses** instead of being pretended into effect.
- **Bilingual UI**: follows the system language by default, and can be pinned to 简体中文 or English under
  Settings → Appearance & language. `src/i18n.test.ts` keeps the two dictionaries in lockstep, so a missing
  translation fails the suite instead of silently falling back to Chinese.

## Download and install

| Platform | Download | How to install |
| --- | --- | --- |
| macOS (Apple Silicon) | [Download DMG](https://github.com/nexsjournal/switchelp-macapp/releases/download/v0.3.5/Switchelp_0.3.5_aarch64.dmg) | Open the DMG, drag `Switchelp.app` into Applications |
| Windows (x64) | [Download installer](https://github.com/nexsjournal/switchelp-macapp/releases/download/v0.3.5/Switchelp_0.3.5_x64-setup.exe) | Run the NSIS installer and follow the prompts |

Every build is on [Releases](https://github.com/nexsjournal/switchelp-macapp/releases), including the older ones.

**First launch (macOS)** — the build is signed with a Developer ID but not notarized, so macOS blocks it once:
**System Settings → Privacy & Security**, scroll to **Security**, click **Open Anyway** next to the blocked app, then
confirm with your password. Or, once, in a terminal:

```bash
xattr -dr com.apple.quarantine /Applications/Switchelp.app
```

**Windows** — preview only. It installs and opens, but writes no configuration and says why: the credential helper
has no Windows implementation yet.

**Notes**

- **Intel Mac**: build from source (`pnpm exec tauri build`).
- **Bundle name**: 0.1.0 artifacts still carry the old `GPTSwitch` name; the bundle identifier stays
  `app.gptswitch.desktop` on purpose, so existing app data and stored credentials keep working.

## Build and verify

```bash
pnpm install
pnpm typecheck && pnpm test          # frontend: types + unit tests
cargo test -p switch-core            # core: integration + unit tests
pnpm exec tauri build --debug --bundles app
```

End-to-end acceptance (real app + real gateway + real Codex, with a local mock upstream):

```bash
pnpm exec tauri build --debug --bundles app
node scripts/g0/probe-full-loop.mjs
```

Run the privacy scan before publishing or pushing. Its rules are generic and the script itself contains no personal
identifiers; keep your own private patterns outside the repository and they get scanned too:

```bash
printf '%s\n' 'api.your-provider.example' > ~/.switchelp-private-patterns
scripts/check-publish-safety.sh
```

## Security boundaries

| Area | Approach |
| --- | --- |
| Upstream API keys | The system credential store only (macOS Keychain / Windows Credential Manager) — **never `config.toml`**; SQLite keeps a reference and a mask |
| Local gateway | Binds `127.0.0.1` only; a fresh token on every launch; rejects requests carrying `Origin` or a browser preflight |
| Config writes | Plan → verify → atomic replace; a failed apply rolls back, and a successful one stops at "waiting for the host to reload" |
| Diagnostics | Allowlist-only field names, a second redaction pass, previewed before saving; no telemetry, no request bodies |

## Current status

The mechanism works end to end and has been **verified against a real third-party provider on macOS**: a real
upstream answered a completion routed through the local gateway, `model/list` returns the managed models, and the
`chat` adapter, output-limit enforcement, reasoning-level mapping and modality rejection are all backed by the
parameters a real upstream actually received.

**Read this before you apply** — `model_catalog_json` **replaces** the host's model list: while the tool is applied,
the built-in models are gone from Codex's picker and come back only through "restore native mode".

**Not verified** — the Desktop picker's on-screen rendering (the evidence above comes from the app-server and log
layers, not from looking at the menu), real Windows hardware, and screen-reader behaviour.

Design and research documents live in [`docs/`](docs/README.md); they are written in Chinese only for now.

[MIT licensed](LICENSE).
