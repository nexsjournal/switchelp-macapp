# Switchelp

**English** · [简体中文](README.zh-CN.md)

Make third-party providers show up in **Codex's own model picker**, and manage providers, API keys, and each
model's context limit, output limit, input capabilities and reasoning levels.

A local macOS configuration app. Tauri 2 + React 18 + TypeScript shell; every decision lives in a Rust core
(`crates/switch-core`) that does not depend on the window framework.

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

- **Upstream keys never reach `config.toml`**: they only go into the system credential store, and the host only ever
  receives a local gateway token.
- Writing Codex config always goes **plan → digest check (CAS) → atomic replace**. A successful commit stops at
  "waiting for the host to reload"; the app never claims the host has loaded it.
- When upstream speaks `chat/completions`, the gateway translates both ways into Responses. Fields that cannot be
  expressed are **recorded as losses** instead of being pretended into effect.
- **Bilingual UI**: follows the system language by default, and can be pinned to 简体中文 or English under
  Settings → Appearance & language. The switch applies immediately and is remembered. `src/i18n.test.ts` keeps the
  two dictionaries in lockstep, so a missing translation fails the suite instead of silently falling back to Chinese.

## Download and install

Grab a build from [Releases](https://github.com/nexsjournal/switchelp-macapp/releases).

**0.2.0 — the current release — publishes macOS Apple Silicon only.** Windows and Intel Mac bundles are not
attached to it; see the notes below the table for why and what to do instead.

| Platform | File | First launch |
| --- | --- | --- |
| macOS (Apple Silicon) | `Switchelp_0.2.0_aarch64.dmg` | Signed with a Developer ID but **not notarized** — see below |
| macOS (Apple Silicon) | `Switchelp-0.2.0-arm64.zip` | Same as above; unzip and drag `Switchelp.app` into `/Applications` |

**Why macOS warns, and how to get past it**: signing and notarization are two separate gates and this project only
has the first one. macOS will refuse the first launch. Two ways through, easiest first:

1. **System Settings → Privacy & Security**, scroll to the **Security** section, click **Open Anyway** next to the
   blocked app, then confirm with your password. This is the path Apple documents today — its support page no longer
   mentions the older right-click → Open shortcut, so don't be surprised if that does nothing.
2. Terminal, once, then open the app normally:

   ```bash
   xattr -dr com.apple.quarantine /Applications/Switchelp.app
   ```

Notarization needs credentials from the account owner; the steps are in
[signing, notarization and release](docs/development/03-signing-and-release.md). Once configured, the app opens with a
double-click and CI produces notarized builds automatically.

The 0.1.0 artifacts still carry the old `GPTSwitch` name: they were built before the product and the repository
were renamed. The bundle identifier stays `app.gptswitch.desktop` on purpose, so app data and stored credentials
from earlier versions keep working.

**Windows status**: no 0.2.0 artifact is published. The build job exists (`build-windows` in
[`release.yml`](.github/workflows/release.yml)) but only runs on a manual `workflow_dispatch` with `with_windows`
enabled — a tag push never builds it.

Even once built, **the app refuses to apply configuration on Windows**, and says so in the UI instead of writing
anything: the credential helper has no Windows implementation yet, and without it Codex could not authenticate
against the local gateway, so every request would fail. The previous behaviour — writing the config anyway and
reporting success — was worse than useless: it broke a working Codex and blamed the upstream. See the
[evidence index](docs/appendix/01-source-index.md).

**Intel Mac status**: the release matrix does cover `x86_64-apple-darwin`, but the `build-macos` job only runs when
the Apple signing secrets are configured in CI, and the 0.2.0 artifacts were signed locally on Apple Silicon. Build
from source (`pnpm exec tauri build`) if you need an Intel bundle.

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
| Upstream API keys | Only the system credential store (macOS Keychain / Windows Credential Manager); SQLite keeps references and a mask |
| Local gateway | Binds `127.0.0.1` only; a fresh token on every launch; rejects requests that carry `Origin` or browser preflight |
| Token delivery | The helper reads the token file (0600) from the app data directory (0700) and prints only the token on stdout |
| Diagnostic log | **Allowlist-based structured extraction**: a field name outside the allowlist is dropped, and values get a second redaction pass; the diagnostic bundle is previewed before it is saved |

## Current status

The mechanism works end to end and has been **verified against a real third-party provider on macOS**:

- Verified: a real upstream answered a completion routed through the local gateway; a real Codex `model/list`
  returns the managed models; the Desktop app-server starts a session on a managed model (`logs_2.sqlite` records
  `thread/start` with `client_name="Codex Desktop"` and the aliased model id); applying config commits and the host
  restarts; the `chat` protocol adapter, output-limit enforcement, reasoning-level mapping and modality rejection
  are all backed by the request parameters a real upstream received; dropping the upstream cancels the request.
- **Known behaviour worth reading before you apply**: `model_catalog_json` **replaces** the host's model list. While
  the tool is applied, the built-in models are gone from Codex's picker and only come back through
  "restore native mode". Confirmed by `model/list` returning exactly one entry. Tracked, with the planned warning in
  the apply dialog, in the [2026-09-20 audit](docs/audits/2026-09-20-audit-synthesis.md).
- Not verified: real Windows hardware, actual screen-reader behaviour, and the **visual** appearance of the Desktop
  GUI picker — the evidence above comes from the app-server and log layers, not from looking at the menu.

Design and research documents live in [`docs/`](docs/README.md); they are currently written in Chinese only.

## Not distributed with this repository

The nine UI reference screenshots under `referimg/` are third-party product material provided by the user and are
**not part of this repository** (excluded via `.gitignore`). Confirm usage rights before distributing them.

[MIT licensed](LICENSE).
