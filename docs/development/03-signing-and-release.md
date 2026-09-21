# 签名、公证与发布

本文回答三件事：**为什么 macOS 会说“应用已损坏”**、**现在这个仓库能签做到哪一步**、
**要让别人双击就能打开还差什么**。所有命令都可直接复制执行。

## 1. 为什么 macOS 会拦

macOS 对**从网络下载**的应用有两道独立的关卡，很多人把它们的表现混在一起：

| 关卡 | 检查什么 | 没有时用户看到什么 |
| --- | --- | --- |
| **签名**（codesign） | 二进制有没有被改过、是谁签的 | Apple Silicon 上**根本没签名**的 arm64 程序会被直接杀掉；有签名但无效则报“已损坏” |
| **公证**（notarization） | Apple 是否扫描过这个包并出具票据 | “无法验证开发者，无法打开”；或提示“已损坏，应该移到废纸篓” |

关键点：**“已损坏”几乎总是下载隔离标记（quarantine）+ 签名/公证缺失的组合**，
不是文件真的坏了。用户可以右键 → 打开，或执行一次：

```bash
xattr -dr com.apple.quarantine /Applications/Switchelp.app
```

但这要求用户懂这些。**要让别人无感双击就打开，必须签名 + 公证 + 装订票据（staple）。**

## 2. 这个仓库现在的状态

本机钥匙串里应当有的身份（用 `security find-identity -v -p codesigning` 查）：

```
Developer ID Application: <你的名字> (TEAMID1234)     ← 分发用，正确
Apple Development: ...                                ← 仅本机调试
Apple Distribution: ...                               ← 仅 App Store 提交
```

下面一律用 `<你的名字>` 与 `TEAMID1234` 占位：真实姓名与团队 ID 属于个人与组织信息，
不写进这个公开仓库。

| 能力 | 现状 |
| --- | --- |
| Developer ID 签名 | ✅ 可以做，本仓库的 release 构建默认用它 |
| 硬运行时（hardened runtime） | ✅ 由 Tauri 在设置签名身份时自动开启 |
| **公证 + staple** | ❌ **缺凭据**，钥匙串里没有 notarytool 配置，也没有 App Store Connect 密钥 |

所以现在产出的 `.dmg` 是**已签名但未公证**的：下载后仍会出现“无法验证开发者”，
右键打开或上面那条 `xattr` 可以有效；不会出现“已损坏”的误导信息。

## 3. 补上公证：两条路，任选一条

公证凭据只有账号所有者能生成，证书本身不包含它。两种方式二选一。

### 方式 A：App Store Connect API 密钥（推荐，适合 CI）

1. 打开 <https://appstoreconnect.apple.com> → **用户和访问** → **集成** → **App Store Connect API**
2. 生成一个 **Team Key**，权限选 **Developer**，下载 `AuthKey_XXXXXXXXXX.p8`（**只能下载一次**）
3. 记下 **Key ID**（文件名里的那串）和页面顶部的 **Issuer ID**

然后在本机验证一次公证是否可用：

```bash
mkdir -p ~/private_keys && mv ~/Downloads/AuthKey_XXXXXXXXXX.p8 ~/private_keys/
xcrun notarytool store-credentials gptswitch \
  --key ~/private_keys/AuthKey_XXXXXXXXXX.p8 \
  --key-id XXXXXXXXXX \
  --issuer 00000000-0000-0000-0000-000000000000
```

### 方式 B：Apple ID 专用密码

1. 打开 <https://appleid.apple.com> → **登录与安全** → **应用专用密码** → 生成一个
2. 执行：

```bash
xcrun notarytool store-credentials gptswitch \
  --apple-id "你的 Apple ID" \
  --password "刚生成的专用密码" \
  --team-id TEAMID1234
```

### 有了凭据之后

本机出包：

```bash
APPLE_SIGNING_IDENTITY="Developer ID Application: <你的名字> (TEAMID1234)" \
APPLE_KEYCHAIN_PROFILE=gptswitch \
APPLE_TEAM_ID=TEAMID1234 \
pnpm exec tauri build --bundles app,dmg
```

Tauri 会在签名后自动提交公证并装订票据。校验：

```bash
codesign --verify --deep --strict --verbose=2 target/release/bundle/macos/Switchelp.app
spctl --assess --type execute -vv target/release/bundle/macos/Switchelp.app   # 期望 accepted
xcrun stapler validate target/release/bundle/dmg/Switchelp_*.dmg             # 期望 validate action worked
```

`spctl` 显示 **accepted** 才代表别人下载后双击就能打开。

## 3.5 更新包的签名（应用内更新的第二套签名）

应用内更新还要一套**独立于 Apple 的**签名：更新包（`Switchelp.app.tar.gz`）由 minisign 私钥签，
应用用它内置的公钥验。密钥只生成一次，生成后**公钥固化在每个已发布的包里**——
换密钥等于让所有老用户再也收不到更新（他们手里的公钥只认旧私钥）。

```bash
# 只做一次；私钥不进仓库
mkdir -p ~/.config/switchelp
pnpm exec tauri signer generate --ci -w ~/.config/switchelp/updater.key -p ""
chmod 600 ~/.config/switchelp/updater.key
```

生成后把 `~/.config/switchelp/updater.key.pub` 的内容填进 `src-tauri/tauri.conf.json` 的
`plugins.updater.pubkey`。本机出包时带上私钥：

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.config/switchelp/updater.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
pnpm exec tauri build --bundles app,dmg
```

产物多出 `Switchelp.app.tar.gz` 与 `Switchelp.app.tar.gz.sig`，由
`scripts/make-latest-json.mjs` 组装成 `latest.json` 一起上传。完整发布步骤、失败面与验收见
[应用内更新](../architecture/06-updates.md)。

## 4. GitHub Actions 需要的 secrets

[release.yml](../../.github/workflows/release.yml) 已经写好：macOS 与 Windows 各自在原生 runner 上构建，
打标签（`v*`）自动出 Release，也可以手动触发并选择先出草稿。
**未配置 secrets 时照样出包，只是产物未签名**——步骤会跳过而不是失败。

| Secret | 用途 | 怎么生成 |
| --- | --- | --- |
| `APPLE_CERTIFICATE` | Developer ID 证书（.p12 的 base64） | 见下 |
| `APPLE_CERTIFICATE_PASSWORD` | 导出 .p12 时设的密码 | 导出时自定 |
| `APPLE_SIGNING_IDENTITY` | 例如 `Developer ID Application: <你的名字> (TEAMID1234)` | 固定字符串 |
| `APPLE_TEAM_ID` | `TEAMID1234` | 固定字符串 |
| `APPLE_API_KEY` | AuthKey .p8 的 base64 | 方式 A 下载的文件 |
| `APPLE_API_KEY_ID` / `APPLE_API_ISSUER` | 方式 A 得到的两个值 | App Store Connect |
| `WINDOWS_CERTIFICATE` / `WINDOWS_CERTIFICATE_PASSWORD` | Windows 代码签名证书 | 见下 |

导出证书为 base64：

```bash
# 钥匙串访问 → 我的证书 → 右键 Developer ID Application → 导出为 .p12（设一个密码）
base64 -i DeveloperID.p12 | pbcopy     # 粘贴为 APPLE_CERTIFICATE
```

Windows 签名是**另一张证书**（不是 Apple 的），需要向 DigiCert / Sectigo 等 CA 购买
OV 或 EV 代码签名证书。没有它程序照样能跑，只是 SmartScreen 会提示“未知发布者”，
用户需要点“仍要运行”。有了它并在 CI 里配置 `WINDOWS_CERTIFICATE`，
Tauri 会给 `.msi` / `.exe` 签名，SmartScreen 的警告随之消失。

## 5. 本机不签名的临时办法

如果只是自己用，不想配公证，可以：

```bash
pnpm exec tauri build --bundles app
xattr -dr com.apple.quarantine /Applications/Switchelp.app   # 拷贝到 /Applications 之后执行一次
```

但**分发**给别人时不要走这条路：要求每个用户执行 `xattr` 是不合理的交付方式。

## 6. 边界

- 本仓库的 `identifier` 是 `app.gptswitch.desktop`，与钥匙串服务名一致；
  改名要同时改 `tauri.conf.json` 和 `src-tauri/src/main.rs` 里的 `SystemVault::new(...)`，
  否则升级后读不到旧凭据。
- 当前图标是 Tauri 默认图标，不是自有品牌标识；正式发布前应替换
  `src-tauri/icons/` 下全部文件，替换后需重新签名。
- Windows 的 `.cmd` 凭据 helper 仍是显式未实现的桩，**Windows 上第三方模型尚不可用**；
  这不影响应用能否打开，但影响实际使用，详见 [证据索引](../appendix/01-source-index.md)。
