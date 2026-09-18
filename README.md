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

- The **model menu** comes from a compiled catalog file (`model_catalog_json`), not from a list this tool draws itself.
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

Grab a build from [Releases](https://github.com/nexsjournal/switchelp-macapp/releases):

| Platform | File | First launch |
| --- | --- | --- |
| macOS (Apple Silicon) | `GPTSwitch_0.1.0_aarch64.dmg` | Signed with a Developer ID but **not notarized**: use **right-click → Open**, or run `xattr -dr com.apple.quarantine /Applications/GPTSwitch.app` once |
| macOS (Apple Silicon) | `GPTSwitch-0.1.0-arm64.zip` | Same as above; unzip and drag the `.app` into `/Applications` |
| Windows (x64) | `GPTSwitch_0.1.0_x64-setup.exe` (NSIS installer)<br>`GPTSwitch_0.1.0_x64_en-US.msi` | Unsigned, so SmartScreen reports "Unknown publisher" — choose "Run anyway" |

The 0.1.0 artifacts still carry the old `GPTSwitch` name: they were built before the product and the repository
were renamed. From the next tagged release the files and the app bundle are named `Switchelp`. The bundle
identifier stays `app.gptswitch.desktop` on purpose, so existing app data and stored credentials keep working.

**Why macOS warns**: signing and notarization are two separate gates, and this project only has the first one.
Notarization needs credentials from the account owner; the steps are in
[signing, notarization and release](docs/development/03-signing-and-release.md). Once configured, the app opens with a
double-click and CI produces notarized builds automatically.

**Windows status**: the app opens, but **third-party models are not usable on Windows yet** — the credential helper's
`.cmd` implementation is still an explicitly unfinished stub, which fails diagnosably rather than pretending to work.
See the [evidence index](docs/appendix/01-source-index.md).

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
printf '%s\n' 'api.your-provider.example' > ~/.gptswitch-private-patterns
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

The mechanism works end to end, but it has **not been verified against a real third-party provider yet**:

- Verified: a real Codex app-server lists custom models and routes them to the local gateway; the `chat` protocol
  adapter, output-limit enforcement, reasoning-level mapping and modality rejection are all backed by the request
  parameters a real upstream received; dropping the upstream cancels the request.
- Not verified: the Desktop **GUI** model picker (the evidence so far comes from the app-server layer), response
  quality from a real provider, real Windows hardware, and actual screen-reader behaviour.

Design and research documents live in [`docs/`](docs/README.md); they are currently written in Chinese only.

## Not distributed with this repository

The nine UI reference screenshots under `referimg/` are third-party product material provided by the user and are
**not part of this repository** (excluded via `.gitignore`). Confirm usage rights before distributing them.

This repository currently ships without an open-source license.
