# 需求审核报告（2026-09-22）

角色：需求审核员（向产品总监汇报） · 仓库：`/Users/example/Code/ProjDev/gptswitch-macapp` · 工作区版本 **0.3.0**（`git describe` = `v0.2.0-17-ge345bbd`）
基线：`docs/audits/2026-09-20-requirements-parity.md` · 章程：`docs/audits/2026-09-22-product-director-charter.md`
方法：只读源码 / 测试 / 文档；每条结论给出 `文件:行`（行号当场 `sed -n` 核过）或测试名。**未构建、未运行应用、未执行测试**（本机 CI 全绿由章程既有事实承担）。
本轮不改任何源码，本文件是唯一新增产物。

---

## 0. 结论先行

**按 PRD §3 的 P0 文本逐条对照，0.3.0 在「接口可用性」这一层已经从 0.2.0 的「多处缺失」走到「基本齐备」：历史上认账的 4 条 P0（多 Key 闭环、供应商搜索、模型级协议字段、应用差异声明）本轮复核为**「已修复」**。但「达到 P0 承诺」仍不能成立，原因是三类缺口同时压在价值主链上：**

1. **价值主张本身未验证**——「模型出现在 Codex 原生选择器里」这一条（PRD P0 第 6 项、R03）自 09-19 起连续多轮都是「未验证」，本轮复核**仍为未验证**，本机环境无法自动化，且没有 Plan B。它不成立，下面所有 P0 都退化成「config.toml 图形界面」。
2. **三条 P0 承诺被 PRD 自己重新定义为「待产品决策」，代码侧仍是空的**——供应商预设（P0-3）、Chat Completions 工具调用门禁（P0-5）、Windows 签名安装包（P0-7）。其中前两条是 2026-09-20 批次 E 明确「不补、改标清楚」的有意决定，第三条是范围未完成。**这三条是否继续算 P0，是产品总监要拍的板，不是需求侧的缺陷认定**（见 §2）。
3. **AC-01~AC-17 里，AC-13 没有任何测试指向它，AC-15 只覆盖到发现路径、网关路径无针对性测试**（见 §3）。

结论一句话：**需求侧的「已实现」面显著改善，但「达到 P0 承诺」不成立；缺口集中在 1 条无法自动化的核心验证 + 3 条待产品定性的 P0 + 2 条 AC 覆盖空洞。**

---

## 1. PRD §3 P0 清单逐条对照

判定口径：**已实现且有自动化覆盖** / **已实现未验证**（代码在、测试缺） / **部分** / **未实现**。历史已修项一律记「已修复」，不照抄旧结论。

| # | P0 条目（PRD §3） | 落地状态 | 证据 |
| --- | --- | --- | --- |
| 1a | 供应商增删改 | **已实现且有自动化覆盖** | `crates/switch-core/src/application/mod.rs`（save/delete provider）；`crates/switch-core/tests/workspace_service.rs::deleting_a_provider_removes_its_keys_and_models` |
| 1b | 供应商搜索 | **已修复**（历史 P0-2：死代码） | 输入框已补：`src/app/App.tsx:354`（`<input type="search" … onChange={event => setQuery(event.target.value)} />`）；过滤逻辑 `src/app/App.tsx:263` |
| 1c | 自定义 URL | **已实现且有自动化覆盖** | `crates/switch-core/src/domain/url.rs`（归一化/校验） |
| 1d | 供应商预设 | **未实现（PRD 已自行降级为产品决策）** | 全仓库无 `listPresets`；类型仍在 `src/contracts/types.ts:70`、`crates/switch-core/src/domain/provider.rs:61`。批次 E 明确删除死接口（`docs/audits/2026-09-20-batch-E-deviations.md` §1） |
| 2 | 每供应商多 Key；默认固定当前 Key；新增/替换/禁用/检测；系统凭据库 | **已修复**（历史 P0-1：只有替换/切换/检测） | 命令面 `src-tauri/src/commands.rs:149/164/179/257`（add/replace/select/delete）；禁用写入路径 `crates/switch-core/src/domain/credential.rs:268`（`credential.status = Disabled`）+ `application/mod.rs:334-365`（`set_disabled`，当前 Key 拒绝禁用 → `error.activeCredentialCannotBeDisabled`）；UI `src/features/providers/ProviderForm.tsx:140,277,314`；测试 `workspace_service.rs:571 a_key_can_be_disabled_and_enabled_again`、`:613 the_active_key_cannot_be_disabled` |
| 3 | `/models` 发现 + 手动新增；精确保留 ID 大小写/斜杠/Unicode | **已实现且有自动化覆盖** | `crates/switch-core/tests/sqlite_repository.rs::model_identity_uses_inherited_protocol_and_is_case_sensitive`；`src-tauri/src/commands.rs`（`models_discover`） |
| 4 | 独立模型编辑：显示名/上游 ID/**协议**/上下文/输出/输入能力/工具能力/推理档位 | **已修复**（历史 P0-4：缺协议字段） | 草稿字段 `crates/switch-core/src/application/mod.rs:55 protocol_override`，写入路径 `:413 model.protocol_override = draft.protocol_override`；UI `src/features/models/ModelEditorPage.tsx:144-146`；落地验证 `crates/switch-core/tests/apply_service.rs:1104 a_model_level_protocol_override_reaches_the_route` |
| 5 | 接入检测、差异预览、一次应用、失败恢复、还原上次配置 | **已实现且有自动化覆盖** | `crates/switch-core/tests/apply_service.rs`（`:388 external_config_change_blocks_the_commit…`、`:491 unconfirmed_reload_stops_at_pending…`、`:617 recovery_enters_conflict…`）；`crates/switch-core/tests/config_golden.rs:377 same_revision_applied_twice_is_idempotent` |
| 6 | 模型在受支持版本 Codex 原生选择器可见，选中后走对应服务商和 Key | **未验证（连续多轮，核心价值）** | 代码路径完整（`codex/catalog.rs`、`application/apply.rs`）；app-server 层证据不足以覆盖 GUI。**本次未验证** |
| 7 | 应用差异必须声明「Codex 的模型菜单会被替换」并列出替换后模型 | **已修复**（历史认账项） | 计划侧 `src/features/codex/CodexConfigPage.test.tsx:287`（确认之前说清「菜单会被替换」并列出模型）；`:317`（还原不显示该警告）；替换语义 `apply_service.rs::applying_replaces_the_host_menu_with_exactly_the_published_aliases` |
| 8a | Responses 原生兼容路径 | **已实现且有自动化覆盖** | `crates/switch-core/src/protocols/responses.rs`；`tests/gateway_server.rs::responses_upstream_passes_through_and_hides_the_upstream_id` |
| 8b | Chat Completions 适配标实验（应用前警告） | **已修复**（历史 P0-5 的「标实验」半条） | `crates/switch-core/src/application/apply.rs:286-303`（有模型走 CC 即加 `warning.chatAdapterExperimental`）；测试 `apply_service.rs:1158 chat_completions_models_are_flagged_as_experimental_before_applying`；文案 `src/locales/zh-CN.ts:955` |
| 8c | **工具调用门禁本身** | **未实现（PRD 已自行降级为产品决策）** | 批次 E §2：门禁产物必须来自真实上游，无凭据写出来是「恒真检查」，因此不补。`is_verified_adapter` 生产代码零调用 |
| 9 | 分阶段连接测试、脱敏日志、配置备份、托盘、关窗继续代理 | **已实现**：分阶段/脱敏/备份/托盘有覆盖；关窗继续代理**未验证** | 四阶段 `crates/switch-core/src/diagnostics/probe.rs:4-7`（connect/credential/model/generate）；关窗隐藏 `src-tauri/src/main.rs:377-379`（`prevent_close` + `hide`）——行为在代码，无自动化测试 |
| 10 | macOS Apple Silicon + Windows x64 签名安装包及相同核心功能 | **部分**：macOS 已交付（0.2.0）；Windows **未实现** | `docs/01-product-requirements.md:123` 自述 Windows helper 仍是桩、网关起不来、前置检查拦下；`src-tauri/src/commands.rs` 有 `update_install` 但网关前置检查未过，Windows 侧验收未完成 |

**P0 判定汇总**：已实现且有覆盖 6 条（1a/1c/3/5/7/8a）、已修复 3 条（1b/2/4）、部分或未验证 3 条（6/9 的关窗项/10）、未实现且待产品定性 3 条（1d/8c 及 Windows 包）。

---

## 2. 三条「已从 P0 承诺区滑走」的缺口（需产品总监定性，非需求侧缺陷认定）

章程 §四 规定「违反 PRD P0 承诺」即 P0。但这三条的 PRD 文本已被改动为「尚未实现 / 有意不做」，且批次 E 留了决策记录。**需求侧只做事实陈述与定性请求，不自行报成新缺陷。**

| 项 | 事实 | 需求侧定性请求 |
| --- | --- | --- |
| 供应商预设 | 无接口、无 UI、无消费方；`ProviderPreset` 类型仍在 | PRD §3 P0 文本自己写了「尚未实现」。是否把它正式移出 P0、或要求最小形态（用户自存地址模板，不预置厂商）？ |
| 工具调用门禁 | 不存在；`is_verified_adapter` 仅测试引用 | PRD §3 文本已写「门禁本身尚未建立」。是否提供测试凭据以关闭该 P0，或正式接受「文本可用、工具未验证」为发布口径？ |
| Windows x64 签名包 | 无产物、无真机验证；PRD §7 已自己承认未完成 | 章程 §四 例外条款已把「Windows 被前置检查拦下不写配置」定为正确行为。签名包是否仍算 0.3.0 的 P0 阻塞项？ |

---

## 3. AC-01~AC-17 覆盖三分类

口径：**有自动化覆盖**（给测试名）/ **只有人工**（给文档出处）/ **无覆盖**。

| AC | 场景 | 分类 | 测试 / 出处 |
| --- | --- | --- | --- |
| AC-01 | 无 `/models` 端点 | 有自动化覆盖 | `workspace_service.rs:407 discovery_leaves_models_it_did_not_see_untouched` |
| AC-02 | 不同供应商同名模型 | **部分**：路由/别名有覆盖；**原生选择器可区分只有人工** | `workspace_service.rs:359 default_display_name_carries_the_provider_prefix`；`apply_service.rs::applying_replaces_the_host_menu_with_exactly_the_published_aliases`；GUI 走 `docs/development/02-testing-and-release.md:40-41` |
| AC-03 | 同模型换 Key | 有自动化覆盖 | `workspace_service.rs::key_rotation_keeps_old_request_secret_and_rejects_stale_edit`；`gateway_server.rs::credential_version_change_blocks_the_request`；`workspace_service.rs::switching_the_active_key_puts_the_models_back_to_pending` |
| AC-04 | 覆盖 context 后刷新 | 有自动化覆盖（保留侧） | `workspace_service.rs:300 discovery_updates_the_discovered_layer_without_overwriting_user_values` |
| AC-05 | 输出限制 8192/4096；未知不发 0 | 有自动化覆盖 | `gateway_server.rs:848 the_declared_output_limit_reaches_the_upstream_request`；`protocols/chat.rs:1066 clamp_output_limit_has_three_distinct_outcomes`（未知 → `(None,false)`，不发 0） |
| AC-06 | 不支持某思考档位 | 有自动化覆盖 | `domain/reasoning.rs:133 unknown_support_cannot_declare_a_default`、`:142 unverified_mapping_is_not_host_selectable`；`codex/catalog.rs:492 toggle_only_reasoning_stays_gateway_side_with_warning` |
| AC-07 | 未知图片/PDF/video 能力 | 有自动化覆盖 | `gateway_server.rs:785 an_undeclared_modality_is_rejected_before_reaching_the_upstream`；`codex/catalog.rs:430 video_and_pdf_never_reach_input_modalities`、`:449 unknown_image_is_not_projected_as_native` |
| AC-08 | 已保存但宿主没重载 | 有自动化覆盖 | `apply_service.rs:491 unconfirmed_reload_stops_at_pending_instead_of_claiming_loaded`；`workspace_service.rs:246 model_save_recomputes_host_capabilities_and_never_claims_loaded` |
| AC-09 | 外部改模型/MCP | 有自动化覆盖 | `apply_service.rs:388 external_config_change_blocks_the_commit_and_keeps_the_foreign_content`；`config_golden.rs:152,208`（`[mcp_servers.docs]` 保留） |
| AC-10 | 同 revision 连点应用 | 有自动化覆盖 | `config_golden.rs:377 same_revision_applied_twice_is_idempotent`；`apply_service.rs:508 repeating_the_same_idempotency_key_reuses_the_same_operation` |
| AC-11 | 凭据库拒绝/锁定 | 有自动化覆盖 | `workspace_service.rs:183 locked_vault_leaves_no_metadata_and_missing_entry_cannot_be_selected`；`domain/credential.rs:275 keystore_locked_is_distinct_from_auth_failed` |
| AC-12 | 官方原生恢复 | 有自动化覆盖 | `apply_service.rs::restore_returns_to_native_when_the_recorded_baseline_was_our_own_write`、`restore_drops_managed_keys_but_keeps_an_externally_modified_field`、`restore_detects_external_modification_and_keeps_current_value` |
| AC-13 | 正在生成时关闭窗口 | **无覆盖** | 行为在 `src-tauri/src/main.rs:374-379`（`prevent_close` + `hide`，网关是独立生命周期）；**没有任何测试指向它**。章程侧的人工出处未见 |
| AC-14 | Codex 更新为未知版本 | 有自动化覆盖 | `codex/detect.rs:503 unknown_version_stays_unverified_and_known_version_is_stable` |
| AC-15 | 代理 200 返回 HTML | **部分**：发现路径有，**网关路由路径无针对性测试** | `diagnostics/discovery.rs:206 a_non_json_body_is_reported_as_unparsable`（`/models`）；`gateway_server.rs` 内无「上游 200 非 SSE/HTML」用例 |
| AC-16 | Key 无某模型权限 | 有自动化覆盖 | `gateway_server.rs:584 upstream_permission_error_is_classified`；`workspace_service`/`apply_service.rs:358 plan_refuses_a_disabled_provider` |
| AC-17 | 旧目录未重载、新目录已提交 | 有自动化覆盖 | `gateway_server.rs:383 unknown_catalog_revision_is_rejected`、`an_in_flight_request_holds_a_reference_to_its_revision`、`models_endpoint_lists_only_the_published_revision`；`routing.rs:5`（固定 routeRevision） |

### 当前没有任何测试指向的 AC

- **AC-13（正在生成时关闭窗口 → 网关继续，托盘可恢复窗口）**：完全无覆盖。属于「核心层没为此写用例」，坏掉不会被任何门禁拦住。
- **AC-15 的网关路由分支**：`/models` 发现路径有测试，但「上游对 generate 请求返回 200 + 非 SSE/HTML」在 `crates/switch-core/tests/gateway_server.rs` 无对应用例；仅 `mid_stream_error_frame_is_surfaced_instead_of_reported_as_completed`（`:495`）覆盖「流中途错误帧」这一相邻场景，**不等同**。
- **AC-02 的「原生选择器可区分」半条**：只能人工，见 §6。

---

## 4. 反向检查：做了但 PRD 没承诺

**纪律：以下一律标「越界待产品确认」，不写成缺陷。** PRD §3「当前不做」清单（账号轮换/OAuth/语音栏/GPT-Live/子智能体/会话迁移/远程控制/局域网/云同步/充值商城/内嵌浏览器）经检索**无一项被实现**——有意排除执行到位。

| 功能 | 证据 | 越界程度与请求 |
| --- | --- | --- |
| 应用内更新：检查 **+ 下载安装** | `src-tauri/src/commands.rs:988 update_check`、`:998 update_install`；`src/features/update/`（胶囊 + 弹窗） | 历史审计（09-20 §6.1）记为「只读不下载」。**现在有 `update_install`**，已从「检查」扩到「安装」，且章程 §2 记真实下载安装**未验证**。请求：确认 0.3.0 是否把「应用内自更新」纳入承诺；若纳入，则升级为 P1（未验证的安装路径）。 |
| 网关「暂停新请求」 | `src-tauri/src/main.rs:123`（托盘项「暂停新请求」）、`:148-155`（切换） | 语义克制（在途继续，`main.rs:122` 注释），PRD 未要求。越界待确认 |
| 扩展三页：工具管理 / 插件中心 / 内容中心 | `crates/switch-core/src/toolhub/`、`plugins/`、`content/`；`src/features/tools|plugins|content/` 各有测试 | PRD §3 明说该范围提案「**处于待确认状态，未确认前不作为需求生效**」（`docs/01-product-requirements.md:60`）。功能已落地但需求未生效 → 越界待产品确认 |
| 主题/语言切换、i18n 中英 | `src/features/settings/SettingsPage.tsx`、`src/i18n.test.ts` | PRD 未要求；历史已登记为「额外」。越界待确认 |
| 备份浏览/预览/恢复 UI | `src/features/settings/SettingsPage.tsx` | PRD P0 有「配置备份」，此处是合理展开，非越界 |

---

## 5. 与历史审计的对照

历史基线：`docs/audits/2026-09-20-requirements-parity.md`。

### 5.1 已修复（推翻历史结论的都是这一列）

| 历史条目 | 历史结论 | 本轮结论 | 证据 |
| --- | --- | --- | --- |
| P0-1 多 Key 不闭环 | 无法新增第 2+ 个 / 改名 / 删除 / 禁用 | **已修复** | `commands.rs:149/164/179/257`；`credential.rs:268`；`ProviderForm.tsx:140,277,314`；`workspace_service.rs:571,613` |
| P0-2 供应商搜索是死代码 | 有 state/过滤/空态，无输入框 | **已修复** | `src/app/App.tsx:354` |
| P0-4 模型编辑器缺协议字段 | `ModelDraft` 无 protocol | **已修复** | `application/mod.rs:55,413`；`ModelEditorPage.tsx:144-146`；`apply_service.rs:1104` |
| P0-5「工具调用门禁」不存在 | 半条满足（标实验） | **「标实验」半条已固化到应用前警告**（比历史更前移） | `apply.rs:286-303`；`apply_service.rs:1158`。**门禁本身仍不存在**（见 §2） |
| P0-3 供应商预设未实现 | 未实现 | **复核：仍缺失**，但 PRD 已自行降级、批次 E 有决策记录 | 全仓库无 `listPresets` |
| P0-6「已验证路由」无独立状态 | 无 | **复核：仍无独立状态** | 全仓库检索 `已验证路由/route_verified` 零命中；`probe.rs:4-7` 四阶段无工具调用、`generate` 直连上游不走网关 |
| P0-7 Windows 签名包不存在 | 不存在 | **复核：仍不存在** | `docs/01-product-requirements.md:123` 自述 |

### 5.2 本轮新增（历史未登记）

- **上表之外的 P0 已经清零**：0.2.0 时 P0 缺口 7 条，本轮「可自动化的部分」已全部补上并有测试名（§1、§3）。这是历史报告未预期的进度。
- **AC 覆盖首次成表**：历史未做 AC-01~AC-17 的逐条三分类；本轮定位出 **AC-13 无覆盖、AC-15 网关分支无针对性测试**两处空洞。
- **应用内更新从「检查」扩到「安装」**（`commands.rs:998 update_install`），历史记为「只读不下载」——这是一处**新出现的范围位移**，需产品确认（§4）。

### 5.3 复核仍存在（历史已认账，非新发现）

- 托盘行为与 `docs/appendix/01-source-index.md:265` 自述相反：代码仍 `prevent_close + hide`（`main.rs:374-379`）。历史 P2-1，**仍存在**（文档侧问题，非需求侧）。
- `OverviewPage` 的「模型调用」面仍不接真实数据、PRD §2 五面仍只呈现三类。历史 P2-5/P2-6，未在本轮重新核到行级（**未验证**）。
- 前端默认协议是否仍为 `chat_completions`（历史 P2-7）：**本轮未复核，标未验证**。

---

## 6. 未验证项（不得据此声称支持）

1. **真机 Codex GUI 原生模型选择器**（AC-02 的「可区分」、R03、P0 第 6 项）：本机环境无法自动化；所有证据停留在 app-server 层。**本轮未验证。**
2. **Windows**：安装、凭据 helper、网关、Codex 路由、NSIS/MSI、签名包——本机无 Windows，**全部未验证**。
3. **真实上游质量**：上游对上下文/输出/推理档位的实际遵从、`/models` 真实返回、Q05/Q07——本机无真实凭据，**未验证**。
4. **macOS Intel / Windows ARM64 产物**：无机器、无产物。
5. **PRD §6 非功能目标**（p95、冷启动、内存预算）：无基准脚本，**目标值无数据**（章程 §七 第 7 条）。
6. **AC-13 的运行时行为**、**AC-15 网关分支**：无测试，也未人工实测。
7. **本报告不含任何实测数字**：仅静态阅读 + 检索；「已实现」指代码路径与测试名存在，不代表本轮跑过。

---

## 7. 证据与复现命令

```bash
# P0 已修项
sed -n '354p' src/app/App.tsx                                 # 搜索输入框
sed -n '144p' src/features/models/ModelEditorPage.tsx         # 协议字段
sed -n '55p;413p' crates/switch-core/src/application/mod.rs   # protocol_override
sed -n '268p' crates/switch-core/src/domain/credential.rs     # Disabled 写入
grep -n "credentials_add\|credentials_delete" src-tauri/src/commands.rs

# 仍缺失 / 仍无状态
grep -rn "listPresets" src crates src-tauri                   # 无
grep -rn "已验证路由\|route_verified" src crates src-tauri    # 无
sed -n '4,7p' crates/switch-core/src/diagnostics/probe.rs      # 四阶段，无工具调用

# AC 覆盖定位
grep -n "fn " crates/switch-core/tests/gateway_server.rs
grep -n "fn " crates/switch-core/tests/workspace_service.rs

# 反向检查
sed -n '988p;998p' src-tauri/src/commands.rs                   # update_check / update_install
sed -n '123p' src-tauri/src/main.rs                            # 暂停新请求
sed -n '60p' docs/01-product-requirements.md                   # 扩展三页「待确认」
```

## 8. 审计边界

- 只读 + 检索；**未构建、未运行应用、未执行测试、未做性能/无障碍/签名实测**。
- 未改动任何业务代码；本报告是唯一新增文件。
- 文档自述一律按 C 级处理，除非有仓库内可复核的测试名或代码行。凡「未量过/未读过」一律写「未验证」，无一处编造数字。
