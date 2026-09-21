# 测试审核报告（2026-09-22）

角色：测试审核员 · 仓库：`/Users/example/Code/ProjDev/gptswitch-macapp` · 工作区版本：**0.3.0**
依据：`docs/audits/2026-09-22-product-director-charter.md` §5.5 任务书
本报告只读，未改任何源码，未改 `evidence-manifest.json`。所有 `文件:行` 均已当场核过。

> 引用约定：**「主控实跑」**= 由主控在本机跑过的门禁结果，本报告引用时不重复跑；**「本轮核」**= 本次审核自己读出来的结论与 grep/文件证据。

---

## 0. 结论先行

1. **自动层在「纯逻辑」上很扎实，问题不在覆盖率数字，在于「夹具是自造的」与「一次真实宿主契约的固化」。** `gateway_server.rs` 断言的是**手写**的请求体夹具（`crates/switch-core/tests/gateway_server.rs:321-329` `request_body()`），协议 SSE 夹具也是手写字面量（同文件 `:332-343`），仓库里**没有任何一份从真实 Codex 抓下来的脱敏请求/响应 fixture**（`tests/fixtures/` 下只有 `config/*.toml` 11 份，无协议夹具——本轮 `find tests -type f` 核对）。→ 一旦 Codex 的真实请求形状漂移（字段改名、`input` 项结构变化），**所有测试继续全绿，生产静默发错参数**。这是本报告认定的**第一位静默缺口**。

2. **故障注入只做到一半。** 章程与 `docs/development/02-testing-and-release.md:80-82` 要求「在 journal 各阶段杀掉测试进程；磁盘满、只读目录、Keychain 失败、文件占用、DB commit 失败、目录 hash 不匹配分别注入」。实测：**只读目录有**（`apply_service.rs:900`）、**hash 不匹配/篡改有**（`:553`）、**DB commit 失败有**（`workspace_service.rs:216`）、**Keychain 失败只有内存凭据库模拟**（`workspace_service.rs:183`；真 Keychain 在 `tests/system_vault.rs:4` 是 `#[ignore]`）；**「在 journal 各阶段杀进程」与「磁盘满」两条没有任何实现**（见 §3.1 的 grep 证据）。

3. **AC-13（关窗后网关继续、托盘可恢复）与 AC-15 的网关生成路径在自动化层是空的**，AC-01「发现失败不删旧记录」、AC-16「单模型权限失败不牵连整个供应商」两条的关键半边没有断言（详见 §2）。

4. **CI 七个 job 拦得住「代码写错」，拦不住「真实宿主不再接受我们的产物」。** 端到端探针不能进公开 CI 是**环境限制**（真 Codex 二进制随 ChatGPT.app 分发，runner 装不上），但 `probes` job 目前只做 `node --check`（语义级零执行）——**探针里的断言可以在无人察觉中失效**，这部分是**可补的**（见 §4）。

5. **人工层「有痕迹，但不在指定落点，且停在 0.2.0」。** `docs/appendix/evidence-manifest.json` 里**没有任何探针执行痕迹**（`status` = `documentation_only`，`last_review_date` = `2026-09-18`，全部 key 均与探针无关）；但 0.2.0 时期三个探针**确实在本机跑过**并留下了原始输出（`.local/g0-*.json` + `docs/audits/2026-09-20-implementation-verification.md:516-585`）。**0.3.0 没有任何探针执行痕迹；`coexist-check.mjs`（共存人工层）没有任何实跑痕迹**（见 §5）。

---

## 1. 测试层次盘点（本轮核）

| 层 | 位置 | 规模（本轮核） |
| --- | --- | --- |
| Rust 单元（内联） | `crates/switch-core/src/**`、`crates/bridge/src/**`、`src-tauri/src/**` | `grep '#\[test\]'` 命中 **446** 处 |
| Rust 集成 | `crates/switch-core/tests/*.rs`（7 文件 / 4582 行）+ `crates/bridge/tests/multiplex.rs`（440 行） | 见下表 |
| 前端 | `src/**/*.test.ts(x)` **20 文件** | `grep -cE '^\s*(it|test)\('` 合计 **187**（与主控实跑一致） |

集成测试各文件规模与用例（本轮核，行号为文件内 `fn` 起始行）：

- `apply_service.rs`（1352 行）：计划 → CAS → 原子替换 → 恢复 → 还原 → 回执 → 共存，**31 个** `#[test]`。
- `gateway_server.rs`（1172 行）：真实 TCP 网关 + mock 上游，**28 个** `#[test]`（含 `every_failure_is_answerable` 子模块）。
- `config_golden.rs`（538 行）：TOML 逐字节保真，**24 个**。
- `workspace_service.rs`（684 行）：发现/凭据/模型生命周期，**19 个**。
- `sqlite_repository.rs`（260 行）：**8 个**。
- `restart_host.rs`（109 行）：**2 个，均 `#[ignore]`**（`:27`、`:72`）。
- `system_vault.rs`（27 行）：**1 个，`#[ignore]`**（`:4`）。
- `crates/bridge/tests/multiplex.rs`：**10 个**（`:209,229,258,306,322,353,376,396,413,428`），与主控实跑「10 passed」一致。

**引用主控实跑（非本轮）**：`pnpm typecheck` 过；`pnpm test` 20 文件 / 187 用例全过；`pnpm build` 成功（index js 553.08 kB / gzip 161.43 kB，有 >500 kB chunk 告警）；`cargo fmt --all --check` = 0；`cargo clippy --workspace --all-targets -- -D warnings` = 0；`cargo test -p switch-core` = 556 passed / 0 failed / 3 ignored；`cargo test -p gptswitch-bridge` = 10 passed；`scripts/check-publish-safety.sh` 退出 0。

---

## 2. AC-01~AC-17 的自动化覆盖（测试侧证据）

判定口径：**自动化** = 有断言指向该行为的测试文件与用例名；**部分** = 关键半边有、另半边无；**人工** = 只有 `scripts/g0` 探针或界面走查；**无** = 自动化层没有指向它的用例。
（需求侧口径由需求审核员给，本节只给测试侧证据。）

| AC | 场景 | 测试侧判定 | 证据（文件:行 用例名） |
| --- | --- | --- | --- |
| AC-01 | 无 `/models`，手动模型可保存，**发现失败不删旧记录** | **部分** | 手动保存：`src/features/models/ModelFormDialog.test.tsx`、`src/app/App.test.tsx:344`（`vendor/manual`）。**「发现失败不删旧记录」无直接用例**：本轮 `grep -rn 'discoveryUnparsable\|discoveryUnreachable' crates/switch-core/tests/` **零命中**；最近的是 `workspace_service.rs:407 discovery_leaves_models_it_did_not_see_untouched`（部分发现，非失败）与 `diagnostics/discovery.rs:206 a_non_json_body_is_reported_as_unparsable`（只测错误分类）。行为安全靠「发现与保存是两个调用」的结构，**无断言**。 |
| AC-02 | 同名模型可区分、路由正确 | **部分** | 区分：`workspace_service.rs:359 default_display_name_carries_the_provider_prefix`、`qualifying_*` 系列单元。路由：`gateway/routing.rs` 单元、`crates/bridge/tests/multiplex.rs:259 routes_by_model_choice`。**「原生选择器里能一眼区分」= 人工**。 |
| AC-03 | 换 Key 后新请求用新 Key、在途/绑定续接维持旧 Key | **自动化** | `workspace_service.rs:140 key_rotation_keeps_old_request_secret_and_rejects_stale_edit`（旧 secret 仍可 `vault.load`、新版本=2、陈旧编辑 409）；`gateway_server.rs:709 credential_version_change_blocks_the_request`（版本不符 → 409 `CONTINUATION_BOUND`，且**不触达上游**）；`gateway/routing.rs:617 request_route_identity_ignores_display_but_tracks_credentials`。**但无「换 Key 时在途流用旧 Key 跑完」的端到端**——现有断言是「旧 Key 仍可取 + 跨版本被拦」。 |
| AC-04 | 用户覆盖 context 后刷新模型，保留覆盖与来源 | **自动化** | `workspace_service.rs:300 discovery_updates_the_discovered_layer_without_overwriting_user_values`、`:339 discovery_follows_the_upstream_name_when_the_user_never_renamed_it`。 |
| AC-05 | 输出限制 8192/4096，上游参数正确，未知不发 0 | **自动化（强）** | `gateway_server.rs:848 the_declared_output_limit_reaches_the_upstream_request`（宿主 32000 → 上游 `max_tokens: 2048`）；`protocols/chat.rs` `the_declared_output_limit_wins_over_a_larger_host_request` / `a_smaller_host_request_is_kept_and_not_raised_to_the_declared_limit` / `the_declared_limit_applies_even_when_the_host_asks_for_nothing` / `clamp_output_limit_has_three_distinct_outcomes`；`protocols/responses.rs the_declared_output_limit_is_applied_to_passthrough_too`。 |
| AC-06 | 不支持的思考档位不能应用、不悄悄降档 | **自动化** | `domain/reasoning.rs:121-176`（`default_must_belong_to_allowed_values`、`unknown_support_cannot_declare_a_default`、`unverified_mapping_is_not_host_selectable`、`non_effort_control_is_not_projected_to_levels`、`budget_above_output_limit_is_blocked`、`toggle_rejects_more_than_two_values`）；`protocols/chat.rs an_undeclared_effort_is_dropped_and_reported_instead_of_guessed`；`protocols/responses.rs an_undeclared_effort_is_removed_rather_than_forwarded`。**「宿主拒绝非标准档位」只有人工**（历史已登记）。 |
| AC-07 | 未知图片/PDF/video 不虚报、不静默丢 | **自动化（强）** | `gateway_server.rs:785 an_undeclared_modality_is_rejected_before_reaching_the_upstream`、`:821 a_declared_modality_is_forwarded`；单元 `unknown_image_is_not_reported_as_native`、`video_and_pdf_never_reach_input_modalities`、`video_and_pdf_are_never_host_projectable`、`requested_modalities_are_detected_from_content_parts`。**真实宿主附件流转无端到端**。 |
| AC-08 | 已保存但宿主没重载 → 显示待加载，不显示成功 | **自动化（强）** | `apply_service.rs:491 unconfirmed_reload_stops_at_pending_instead_of_claiming_loaded`、`:477 host_receipt_promotes_to_verified_only_after_confirmation`、`:1204/1232/1252` 三个回执用例；前端 `src/features/codex/CodexConfigPage.test.tsx`（提交后不得出现成功文案）、`src/app/App.test.tsx` 负面断言「不宣称已加载」。 |
| AC-09 | 外部改模型/MCP → 冲突显式处理、MCP 保留 | **自动化（强）** | `config_golden.rs:49 preserves_comments_unknown_fields_and_sections`（含 `[mcp_servers.docs]`）、`:354 restore_detects_external_modification_and_keeps_current_value`、`:471 foreign_manager_detection_does_not_touch_other_providers`；`apply_service.rs:617 recovery_enters_conflict_when_an_external_change_diverges_from_the_plan`；`codex/detect.rs:572-585` 外来 `mcp_servers` 识别。 |
| AC-10 | 同 revision 连点应用 → 只执行一次 | **部分** | 后端：`apply_service.rs:508 repeating_the_same_idempotency_key_reuses_the_same_operation`（复用同一事务、不再写盘）。**前端无「连点」用例**：本轮 `grep -n 'idempot\|连点\|只执行一次' src/features/codex/CodexConfigPage.test.tsx` **零命中**。 |
| AC-11 | 凭据库拒绝/锁定 → 清楚报错，无明文回退 | **部分** | 内存凭据库：`workspace_service.rs:183 locked_vault_leaves_no_metadata_and_missing_entry_cannot_be_selected`、`:216 failed_metadata_commit_removes_only_the_new_vault_entry`；单元 `locked_keystore_reports_dedicated_code`、`keystore_locked_is_distinct_from_auth_failed`、`missing_keystore_entry_is_distinct_from_auth_failure`。**真 Keychain 无自动化**（`tests/system_vault.rs:4` `#[ignore]`）。 |
| AC-12 | 官方原生恢复：删自有字段、登录与历史保留 | **自动化（强）** | `apply_service.rs:649/740/796/840/858`（删受管键、保留被外部改过的字段、恢复回原生、拒绝在未写前恢复、恢复计划后配置变则阻断）；`config_golden.rs:122/165/216`。**「登录与历史不动」无常驻断言**，靠「只写 config.toml 的受管段」结构保证。 |
| AC-13 | 正在生成时关窗 → 网关继续，托盘可恢复 | **无** | 本轮 `grep -rni 'tray\|托盘\|close.*window\|关窗\|minimize' src/ src-tauri/src/` 的**测试命中为零**；无 `src-tauri` 测试文件。最接近的是 `gateway_server.rs:874 a_client_disconnect_stops_the_upstream_stream`，语义**相反**（客户端断开=取消上游）。 |
| AC-14 | Codex 更新为未知版本 → 显示未验证，不自动覆盖适配 | **自动化** | `codex/detect.rs:503 unknown_version_stays_unverified_and_known_version_is_stable`、`unknown_version_is_unverified`、`matching_cli_version_is_stable_and_schema_only_is_experimental`。 |
| AC-15 | 代理 200 返回 HTML → 判协议失败，不标正常 | **部分** | 仅 `/models` 发现路径：`diagnostics/discovery.rs:206 a_non_json_body_is_reported_as_unparsable`（200 + `<html>nope</html>` → `CapabilityUnsupported` / `error.discoveryUnparsable`）、`:215 a_payload_without_data_is_rejected_instead_of_returning_nothing`。**网关生成路径无对应用例**：本轮 `grep -n 'html\|HTML' crates/switch-core/tests/gateway_server.rs` **零命中**；生成路径对非 JSON 的 `data:` 行在 `gateway/server.rs:625` 是 `let Ok(data) = ... else { 跳过 }`，非 SSE 正文会走到 `Incomplete`——**但没有任何断言固定这一行为**。 |
| AC-16 | Key 无某模型权限而其他模型可用 → 仅相关范围失败 | **部分** | `gateway_server.rs:584 upstream_permission_error_is_classified`（403 → 分类）；`diagnostics/discovery.rs:72`（403 → `ModelPermissionDenied`）；`probe_status_mapping_separates_401_from_missing_record`。**无「同一供应商多模型、一个 403 另一个 200」的隔离性用例**。 |
| AC-17 | 旧宿主目录未重载、新目录已提交 → 按 URL 中的目录版本服务 | **自动化（强）** | `gateway_server.rs:383 unknown_catalog_revision_is_rejected`、`:1007 an_in_flight_request_holds_a_reference_to_its_revision`、`:1083 models_refuses_an_instance_that_does_not_own_the_revision`、`:1127 a_deleted_provider_is_reported_as_an_error_not_a_dropped_connection`；`storage/snapshot.rs old_prefix_keeps_serving_its_own_snapshot`；`apply_service.rs:936/983`（超保留代数回收、被引用版本存活）。 |

**AC 无自动化覆盖清单**：**AC-13**（唯一整条无覆盖）。
**AC 关键半边无断言**：AC-01（失败不删旧记录）、AC-10（前端连点）、AC-15（网关生成路径的协议失败）、AC-16（失败隔离）、AC-12（登录/历史不动）。
**AC 只有人工**：AC-02 的「原生选择器可见」、AC-06 的「宿主拒绝档位」。

---

## 3. 缺口细节（定位到具体行为）

### 3.1 故障注入

| 注入项（测试文档 `:80-82` 要求） | 覆盖到哪一步 | 证据 |
| --- | --- | --- |
| **journal 各阶段杀掉测试进程** | **未实现** | 本轮 `grep -rn 'kill\|SIGKILL\|SIGTERM\|catch_unwind' crates/switch-core/src/application crates/switch-core/tests/apply_service.rs` → **零命中**。恢复用例是**手工改写事务记录**来伪造崩溃点，不是真杀进程：`apply_service.rs:594-599`（把 `stage` 改成 `Committing` 并写入 `written_hash` 后再跑 `startup_recovery`）、`:622-626`。→ **坏了会怎样**：只在「文件已替换、DB 未记账」与「外部改过文件」两个崩溃点被验证；进程在**其它阶段**（如 CAS 校验后、发布后、回收中）被杀的真实恢复路径没有测试。**为什么门禁拦不住**：恢复逻辑的分支由测试自己摆出来，摆错/漏摆不会有任何信号。 |
| **磁盘满** | **未实现** | 本轮 `grep -rn 'ENOSPC\|磁盘满' crates/switch-core/src crates/switch-core/tests` → 唯一命中 `storage/migration.rs:105`（迁移错误的**字符串 mock**），与配置写盘无关。 |
| **只读目录** | **有** | `apply_service.rs:900 a_failed_write_takes_back_the_publication_it_just_made`（chmod 目录只读 → 写入失败必须报错、已发布的目录版本必须被收回、配置保持原样）。注意：该用例在 **root** 下会假绿（runner 非 root，当前有效）。 |
| **Keychain 失败** | **半** | 只有内存凭据库模拟：`workspace_service.rs:183 locked_vault_...`、`:216 failed_metadata_commit_...`，以及单元 `locked_keystore_reports_dedicated_code`。真 Keychain：`tests/system_vault.rs:4` `#[ignore]`。 |
| **文件占用 / DB commit 失败** | 半（DB 有，文件占用无） | `workspace_service.rs:216`（元数据提交失败只回滚新条目）；「文件被占用」本轮 `grep -rn 'flock\|file lock\|占用' crates/switch-core` 无对应用例。 |
| **目录 hash 不匹配** | **有** | `apply_service.rs:553 a_tampered_plan_hash_is_rejected_before_any_write`（篡改哈希 → `error.planHashMismatch`，不写盘）、`:589 recovery_records_commit_when_the_file_matches_the_written_hash`、`config_golden.rs:482 content_hash_changes_and_is_stable`。 |
| **多进程同实例串行/冲突** | 半 | `sqlite_repository.rs:71 concurrent_connections_cannot_overwrite_a_winning_edit`（多连接 CAS）、`storage/journal.rs` `memory_journal_rejects_duplicate_operations`。**无真多进程**用例（都是同进程多连接/内存仓储）。 |

### 3.2 协议一致性 fixture

测试文档 `:86` 列了 13 类，逐条核对（`crates/switch-core/src/gateway/sse.rs` 的解析器用例 + `gateway_server.rs` 的端到端用例）：

| 类别 | 状态 | 证据 |
| --- | --- | --- |
| UTF-8 被拆 | **有** | `sse.rs:286 decodes_utf8_character_split_across_chunks`（含 `saw_invalid_utf8` 反向断言） |
| 单次 chunk 多事件 | **有** | `sse.rs:269 parses_multiple_events_in_one_chunk` |
| 多行 data | **有** | `sse.rs:278 joins_multi_line_data_with_newline` |
| 空注释 / keepalive | **有** | `sse.rs:318 ignores_comment_and_keepalive_lines` |
| 无结束事件（截断） | **有** | `sse.rs:344 truncated_stream_reports_incomplete`、`:358 finish_without_pending_data_returns_none` |
| CRLF / 非法 retry / 空事件 / 冒号值 | **有** | `sse.rs:309`（CRLF）、`:335`（非法 retry）、`:365`（无 data 的事件）、`:374`（纯空行）、`:389`（值里的冒号）、`:326`（id/retry） |
| 上游中途错误帧 | **有** | `gateway_server.rs:495 mid_stream_error_frame_is_surfaced_instead_of_reported_as_completed`（不打扮成 completed、不泄漏 Key） |
| **取消** | **有** | `gateway_server.rs:874 a_client_disconnect_stops_the_upstream_stream`（客户端断开 → 停止读上游 + `result.clientDisconnected` 留痕） |
| **超时** | **半** | 只有**判定函数**的单元测试：`gateway/timeouts.rs:99-150`（四条预算、只有 connect 可重试、零值被拒）。**无「上游真的卡住 → 流超时收尾」的集成用例**：`MockReply::SseDelayed` 只在 `gateway_server.rs:1010`（`an_in_flight_request_holds_a_reference_to_its_revision`）用过，不是超时用例。 |
| **上游断流（硬断开）** | **无** | 本轮 `grep -rn '断流\|disconnect\|reset\|EOF' crates/switch-core/tests/gateway_server.rs` 只命中客户端断开那条；mock 上游的读线程在 `read_line` 返回 0 时只是 `break`（`:79`），**没有模拟「响应头已发、正文中途断连」**。 |
| **背压** | **无** | 本轮 `grep -rn '背压\|backpressure' crates/switch-core/src/gateway/sse.rs` 只命中**文档注释**（`:5`），无测试。 |
| **多个并行 tool** | **无** | `protocols/chat.rs` 只在请求侧透传 `parallel_tool_calls`（`:58-59`）；流式侧 `stream_accumulates_tool_call_argument_fragments` 只覆盖**单个** tool 的参数分片。 |
| **工具 output 错序** | **无** | 本轮无任何用例名/断言涉及工具结果乱序（`grep -rn '错序\|out.of.order' crates/switch-core` 零命中）。 |
| 工具参数逐字（fragments 拼接） | **有** | `chat.rs stream_accumulates_tool_call_argument_fragments`、`prepare_merges_consecutive_tool_calls_and_passes_results_back` |

**「多个并行 tool / output 错序 / 上游断流 / 背压」四条坏了会怎样**：并行工具调用在上游（尤其 Chat Completions 中转）是常态；`chat.rs` 的合并逻辑若对**多 tool 交错**处理错，用户的工具调用会**静默丢参数或错配结果**，而单 tool 用例全绿——因为 `stream_accumulates_tool_call_argument_fragments` 只喂了一个 tool。**为什么门禁拦不住**：现有 SSE 夹具（`gateway_server.rs:332-343` 的 `CHAT_SSE`/`RESPONSES_SSE`）里**没有多 tool、没有断流、没有任何非 data 行**，测试永远走不到这些分支。

### 3.3 并发写损坏

- **有回归**：`sqlite_repository.rs:71 concurrent_connections_cannot_overwrite_a_winning_edit`（同库多连接，赢家编辑不被覆盖）；`apply_service.rs:508` 幂等键复用；`a_failed_write_takes_back_the_publication_it_just_made`（`:900`）；`config_golden.rs:491 atomic_write_leaves_no_partial_file`、`:377 same_revision_applied_twice_is_idempotent`。
- **`gateway_server.rs` 里的并发相关**：`:938 pausing_the_gateway_rejects_new_requests_without_affecting_in_flight_ones`（在途不受暂停影响）、`:1007/1037`（在途请求持有/释放目录版本引用）。
- **缺**：真**多进程**同时申请同实例操作（测试文档要求「必须串行/冲突」）没有用例——现有多连接是同一测试进程内。

---

## 4. CI 七个 job：各自拦得住什么、拦不住什么

`.github/workflows/ci.yml`（行号为 job 定义与关键 step）：

| job | 行号 | 跑什么 | 拦得住 | 拦不住 |
| --- | --- | --- | --- | --- |
| `frontend` | `:24-43` | `pnpm typecheck`（`:39`）、`pnpm test`（`:41`）、`pnpm build`（`:43`） | TS 类型错误、187 前端用例回归、构建失败 | **>500 kB chunk 警告不失败**（主控实跑已见告警，`build` 仍 exit 0）；真实 WKWebView 运行时差异；运行期 a11y/焦点 |
| `core` | `:45-64` | `cargo test -p switch-core`（`:60`）、`cargo test -p gptswitch-bridge`（`:64`），Ubuntu | 核心逻辑 + 网关集成 + 桥接 10 项的回归 | **3 个 `#[ignore]` 永不在 CI 跑**（真 Keychain、真宿主重启 ×2）；依赖 chmod 的只读用例在 root 下会假绿（runner 非 root，当前有效）；macOS 专有路径 |
| `lint` | `:66-85` | `cargo fmt --all --check`（`:83`）、`cargo clippy --workspace --all-targets -- -D warnings`（`:85`），**macOS runner**（编译 `src-tauri`） | 格式、lint、未用变量/错误模式 | 语义错误（lint 过≠对） |
| `pipeline` | `:87-117` | `cargo run -q -p switch-core --example g0_apply_pipeline`（`:105`）+ 产物断言（`:107-117`） | 计划 → CAS → 原子写入链路的回归；产出物缺失、配置未写目录、事务未停在 `awaitingReload` | **宿主是否真的接受自建目录与 helper**（无真 Codex） |
| `probes` | `:119-139` | `for f in scripts/g0/*.mjs; do node --check`（`:130`）+ 依赖自检（`:132-138`） | 探针脚本**语法错、引用已删除模块** | **探针的断言逻辑从不在 CI 执行**——`mock-provider.mjs` 的 SSE 契约、分页、目录编译改了，`node --check` 照样过 |
| `privacy` | `:141-149` | `scripts/check-publish-safety.sh`（`:149`） | 已跟踪 + 未跟踪文件的六类规则（本机路径、私网、密钥形态、凭据库引用、邮箱、私有清单） | **git 历史**（已删除的秘密）、**构建产物内部**（`dist-release/` 被 gitignore，脚本不开包）、⑥签名身份是 **warn**（`:163` 历史记录），私有清单未配置时域名规则空转 |
| `workflows` | `:151-158` | `raven-actions/actionlint@v2`（`:158`） | 工作流 YAML 的结构问题（重复 job 名、缺 `runs-on`、引用不存在的输出） | 工作流**运行期**行为 |

### 端到端探针不进 CI：环境限制还是可补？

**判定：主体是环境限制，但 `probes` job 目前的做法是可以补强的。**

- **不可补的部分（环境限制）**：三个探针都需要真实 Codex 可执行文件（`/Applications/ChatGPT.app/Contents/Resources/codex`，或 `GPTSWITCH_CODEX_BINARY` 覆盖）。它是 ChatGPT 桌面端的一部分，**GitHub runner 上装不上**（`docs/development/02-testing-and-release.md:143-145` 已论证；CI 注释 `.github/workflows/ci.yml:92-94` 同样写明）。所以「宿主是否接受自建目录 + helper」这一层，**在公开 runner 上无法自动化**——这是真实的。
- **可补的部分**：`probes` job 现在**只做语法级检查**，探针里的断言从不执行。`scripts/g0/mock-provider.mjs`、`catalog.mjs`、`rpc.mjs` 是纯 Node、不依赖 Codex 的模块；把它们承载的契约（mock 上游的 SSE 形状、目录编译输出、分页）抽成**不需要 Codex 的 Node 测试**并在 `probes` job 里跑，就能在 CI 拦住「探针断言静默失效」。**这是可做且成本低的一步**。
- 历史审计 `docs/audits/2026-09-20-audit-synthesis.md:284` 已建议「端到端探针进 CI（至少 macOS runner 上跑 probe-catalog 与 probe-apply-pipeline）」。**复核结论：仍存在（未做）**，且该建议中「macOS runner 上跑」的前提（runner 装不上 Codex）不成立——需要在自托管 runner 预装 ChatGPT.app 才可行，属成本取舍。

---

## 5. 人工层有没有真实痕迹

### 5.1 章程指定的落点：`docs/appendix/evidence-manifest.json` —— **没有任何探针痕迹**

本轮逐条核对（未改动该文件）：

- 顶层 key 全部为：`research_date / status / repositories / official_sources / local_observations / screenshots / last_review_date / user_reports / desktop_shell_sources`（`python3 -c "json.load(...)"` 读出）。
- `status` = **`documentation_only`**（`:3`）；`last_review_date` = **`2026-09-18`**（`:503`）。
- 本轮 `grep -c 'g0\|probe\|coexist\|0\.2\.0\|0\.3\.0\|observedAt'` = **0 命中**。
- 内容全是**调研证据**（第三方仓库 commit/sha256、官方 schema sha256、本机 app info.plist hash、九张截图 hash），**没有一条 0.2.0 / 0.3.0 的探针执行记录，没有时间戳，没有命令原文**。

→ **按章程口径：`evidence-manifest.json` 里的人工层无痕迹。** `docs/development/02-testing-and-release.md:153` 要求的「发布前必须人工跑一遍第三层并把输出贴进对应版本发布说明或 `evidence-manifest.json`」，在 **0.3.0 上完全没有兑现**；0.2.0 也没有落在该文件（见下）。

### 5.2 但人工层**确实跑过**——在别的落点，且停在 0.2.0

本轮否证了「人工层从未跑过」这一更强结论：

- **`.local/g0-catalog.json`**：`observedAt` = **`2026-09-20T11:03:17.733Z`**，`binaryVersion` = **`codex-cli 0.155.0-alpha.9.2`**，`result` = 「app-server custom catalog and pagination passed; Desktop UI and routing still unverified」（文件 mtime `Sep 20 19:03`，与 UTC 时间自洽）。
- **`.local/g0-apply-pipeline.json`**：`observedAt` = **`2026-09-20T11:03:20.327Z`**，同 `binaryVersion`，`result` = 「passed: 真实管线产物在真实 Codex 中列出并被路由（app-server 层）；Desktop UI 仍未验证」。
- **`docs/audits/2026-09-20-implementation-verification.md:516-585`**（**已跟踪**）贴出了三个探针的原始输出与解读：5.1 `probe-catalog.mjs`（`:527`）、5.2 `probe-apply-pipeline.mjs`（`:545`）、5.3 `probe-full-loop.mjs`（`:563`），并记录审计时间 **2026-09-20 19:00–19:20（UTC+8）**、工作区版本 **0.2.0**、HEAD `2fda2a5`、宿主 Codex Desktop `26.915.31945` + `codex-cli 0.155.0-alpha.9.2`。5.3 里 `upstreamRequests` 显示真实上游收到 `max_tokens: 4096` 与 `reasoning_effort: low`，与 AC-05 / AC-06 相关。

**所以准确的结论是**：

| 项 | 结论 |
| --- | --- |
| `evidence-manifest.json` 里 0.2.0 / 0.3.0 的探针痕迹 | **无**（无时间戳、无命令原文） |
| 0.2.0 是否有真实执行痕迹 | **有**——`.local/`（gitignored）两份 JSON + `implementation-verification.md` 的原始输出。**但不在章程指定落点**，且 `.local/` 被 gitignore、不进仓库/发布物 |
| 0.3.0 是否有真实执行痕迹 | **无**（本轮 `grep -rln '0\.3\.0' docs/` 命中 4 份文档，均非探针证据） |
| `coexist-check.mjs`（共存人工层）是否有实跑痕迹 | **无**（`grep -rn 'coexist-check' docs/ README.md` 只命中命令行说明，无任何输出记录） |
| release 正文里有没有探针输出 | **无**（`grep -n 'probe\|g0\|evidence' .github/workflows/release.yml` 零命中） |

**「坏了会怎样」**：探测记录停在 0.2.0、且落在 gitignored 目录，等于**发布流程在事实上没有人工验收记录**——0.3.0 新增的共存 Bridge（`coexist-check.mjs`）与界面层从未被任何人跑过，「宿主 GUI 菜单可见性」这一产品的全部价值点，在 0.3.0 上**连 0.2.0 那种一次性证据都没有**。

---

## 6. 与历史审计的对照

基线：`docs/audits/2026-09-20-test-and-privacy.md`（0.2.0，HEAD `2fda2a5`）。逐条对应：

| 历史条目 | 本轮结论 | 证据 |
| --- | --- | --- |
| A4 端到端探针**不在任何 CI** | **仍存在** | `.github/workflows/ci.yml` 只有 `pipeline`（example，非探针）与 `probes`（`node --check`）；真探针仍全在人工层。 |
| A4 建议「把探针的纯本地部分接进 CI」 | **仍未做** | `probes` job（`:119-139`）只做语法与依赖自检，断言零执行。 |
| A2 断言强度抽查（强，无 snapshot 兜底） | **复核：仍成立** | 本轮抽查 `apply_service.rs`、`gateway_server.rs`、`sse.rs`、`bridge/multiplex.rs`，均为行为/参数断言；`bridge` 断言还含「子进程错误不被打扮成成功」（`multiplex.rs:377 surfaces_child_errors_instead_of_faking_success`）。 |
| A1 三个 `#[ignore]`（真 Keychain、真宿主重启 ×2）合理但**无 CI 内替代** | **仍存在** | `tests/system_vault.rs:4`、`tests/restart_host.rs:27/72`；`cargo test -p switch-core` 的 3 ignored 与主控实跑一致。 |
| A5 「测试目录残留 `zz_audit_probe.rs` 应清理」 | **已修复** | 本轮 `ls crates/switch-core/examples/` 未再见到该文件（现有 `examples/g0_apply_pipeline.rs`）。 |
| A1 `pnpm test` 112 用例 / 15 文件（0.2.0） | **已扩大** | 0.3.0 为 **187 用例 / 20 文件**（本轮核）。 |
| A1 `cargo test -p switch-core` 410 通过（0.2.0） | **已扩大** | 0.3.0 为 **556 passed / 0 failed / 3 ignored**（主控实跑）。 |
| A5 `act(...)` 警告 17 条 | **本轮未复现核对**（主控未提供该输出） | 标「未验证」。 |
| B2-② 真实第三方上游域名进入被跟踪代码与 git 历史 | **本轮未核**（属隐私审核员口径） | 只记录「脚本退出 0」为**主控实跑**，未自行复扫。 |
| B3 `looks_like_secret` 20–39 位无前缀密钥泄露 | **本轮未核**（属后端/隐私口径） | 未做结论。 |

**本轮新增**（历史基线没有的）：§3.2 逐条核出「多个并行 tool / 工具 output 错序 / 上游断流 / 背压」**四条无 fixture**；§5 核出「人工层痕迹存在但落在 gitignored 的 `.local/` 且停在 0.2.0，`evidence-manifest.json` 为空」。

---

## 7. 未验证项（本报告未取得直接证据，不做结论）

1. **主控实跑的全部数字**（187 / 556 / 10 / build 体积 / check-publish-safety 退出 0）：本报告**引用为主控结果**，未自行复跑（章程未授权我跑全套门禁）。行号与文件内容为本轮亲核。
2. **真实 Codex / 真实上游凭据下的层**：端到端探针由另一位专人执行，我未跑 `probe-catalog.mjs` / `probe-apply-pipeline.mjs` / `probe-full-loop.mjs` / `coexist-check.mjs`，只审「有没有被跑过、有没有留证据」。
3. **`act(...)` 警告当前条数**：历史为 17 条，本轮未复现核对。
4. **CI 实际运行结果**：本报告只审 `.github/workflows/ci.yml` 的**定义**；没有 CI 运行记录（无 run URL / 日志）可证「七个 job 在最近一次提交上确实都绿」，主控实跑是**本地等价复现**，不等于 CI 已绿。
5. **`.local/g0-*.json` 的完整内容与是否可复现**：只读了 `observedAt`/`binaryVersion`/`result` 三个字段与两份文件的顶层 key；未逐字段比对，也未重跑生成。
6. **Windows 分支、Intel Mac、真 Keychain 在他人机器上的行为**：无机器/无产物，全部未验证。
7. **`plugins` 对真实 `~/.codex/skills` 的一次真实安装、真实 RSS/GitHub 抓取的限流表现**：沿用 `docs/development/02-testing-and-release.md:12-14` 的未验证状态，本轮未改变。

---

**报告结束。** 本文件为只读审核产物，未改任何源码，未改 `evidence-manifest.json`。
