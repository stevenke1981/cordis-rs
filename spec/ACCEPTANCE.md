# CORDIS Rust 驗收標準

每個驗收項目都應由 Unit Test、Conformance Fixture、Integration Test 或 MCP Smoke Test 提供可重複證據。

## A. Workspace 與建置

- **AC-A01**：12 個 Workspace Member 均存在且 `cargo metadata` 可解析。
- **AC-A02**：Rust 1.97.1 在 Linux、Windows、macOS 可編譯。
- **AC-A03**：`cargo fmt --all -- --check` 通過。
- **AC-A04**：`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` 通過。
- **AC-A05**：`cargo test --workspace --all-features --locked` 通過。
- **AC-A06**：Release Build 產生 `cordis` 與 `cordis-mcp`。
- **AC-A07**：正式 Release Commit 包含經審閱的 `Cargo.lock`。

## B. Contract 對齊

- **AC-B01**：可解析有效 `cordis.task.v1`。
- **AC-B02**：拒絕 Granted 但無 Basis 的 Authorization。
- **AC-B03**：拒絕 Allowed／Denied 重疊。
- **AC-B04**：可解析有效 `cordis.plan.v1`。
- **AC-B05**：拒絕 DAG Cycle、未知 Dependency、自我 Dependency。
- **AC-B06**：可解析有效 `cordis.step-result.v1`。
- **AC-B07**：拒絕空 Evidence 的 Step Result。

## C. Authorization 與 Policy

- **AC-C01**：High/Critical Pending Task 的 `execution_allowed=false`。
- **AC-C02**：Explicit Denied 永遠拒絕。
- **AC-C03**：Expired Grant 永遠拒絕。
- **AC-C04**：Granted、在 Scope、Allowed Tool／Target／Action 可通過。
- **AC-C05**：Denied Tool 即使 Action Allowed 仍拒絕。
- **AC-C06**：Offline Network Action 拒絕。
- **AC-C07**：Read-only Network Change 拒絕。
- **AC-C08**：Scope Out Target 拒絕。
- **AC-C09**：Required Approval 未取得時拒絕。
- **AC-C10**：Takeover Change 拒絕。

## D. Core Learning

- **AC-D01**：Strategy Failure 改變下一次 Preflight Prefer／Avoid。
- **AC-D02**：兩個獨立 Source 才 Promotion World Pattern。
- **AC-D03**：Strategy Seed 一次成功不 Promotion。
- **AC-D04**：符合 3 Uses、2 Success、0 Failure 才 Active。
- **AC-D05**：兩次 Failure 進入 Quarantined。
- **AC-D06**：Prediction Calibration 在 Failure 後改變。
- **AC-D07**：Restart 後 Learning State 保留。
- **AC-D08**：Duplicate Terminal Feedback 拒絕。

## E. Memory

- **AC-E01**：Project A Memory 不出現在 Project B。
- **AC-E02**：Reviewed Instruction-safe Principle 進入 Instruction Section。
- **AC-E03**：Untrusted Knowledge 只進入 Reference Data。
- **AC-E04**：Prompt Injection Fixture 不可成為 Instruction。
- **AC-E05**：Query 排除已看過 ID。
- **AC-E06**：兩個不同 Source 才 Activate Pattern。
- **AC-E07**：Migrated Memory 預設 Untrusted。

## F. Workflow

- **AC-F01**：Begin 可持久化並在新 Process 復原。
- **AC-F02**：Authorization Missing 進入 `awaiting_authorization`。
- **AC-F03**：補上 Granted Authorization 後可進入 `awaiting_plan`。
- **AC-F04**：Plan 越過 Scope 被拒絕。
- **AC-F05**：Parallel Path 被拒絕。
- **AC-F06**：Approval-gated Step 在 Approve 前拒絕 Result。
- **AC-F07**：Approve 後可取得 Allowed Permit。
- **AC-F08**：Out-of-order Step Result 被拒絕。
- **AC-F09**：Retry Limit 保持同一 Step，超限依 Policy 轉移。
- **AC-F10**：只有 Evidence 觸發 Replan 後可替換 Plan。
- **AC-F11**：Terminal Success 必須明確證明 Required Acceptance ID。
- **AC-F12**：Finish 後 Workflow Closed 且 Core Learning 更新。

## G. Planner 與 Socrates

- **AC-G01**：Planner 接受 Callable，不需要 Provider SDK。
- **AC-G02**：Planner 拒絕 Prose、Wrong Task、Wrong Goal、Wrong Version。
- **AC-G03**：Fast Route 的 Simple Authorized Task 可 Direct。
- **AC-G04**：High Impact 未授權回傳 Authorization Required。
- **AC-G05**：Socrates 無法放寬 Complexity／Impact／Unknown Hard Gate。
- **AC-G06**：Required Planner Failure 不得 Direct Fallback。

## H. Capability

- **AC-H01**：Register／Status／Require 正常。
- **AC-H02**：Missing Tool Require 失敗。
- **AC-H03**：Version Probe 有 Timeout。
- **AC-H04**：Detect 不安裝 Tool、不改 PATH。

## I. MCP 與 CLI

- **AC-I01**：Legacy `initialize` 成功。
- **AC-I02**：`tools/list` 列出 Direct、Workflow、Memory、Capability 工具。
- **AC-I03**：`tools/call` 回傳 `content` 與 `structuredContent`。
- **AC-I04**：Modern `server/discover` 成功。
- **AC-I05**：stdio 一行一個 JSON Response。
- **AC-I06**：Notification 不回 Response。
- **AC-I07**：跨 Process Begin／Observe／Finish 保留 Focus。
- **AC-I08**：MCP 完整傳遞 Authorization、Allowed/Denied Action、Tool、Target。
- **AC-I09**：CLI JSON Input／Output 與 MCP Tool Result 對齊。
- **AC-I10**：Setup 不覆蓋非 Managed 同名設定。

## J. Migration 與營運

- **AC-J01**：Python `state.json` 可匯入 Task／Strategy／Episode／Pattern。
- **AC-J02**：Python `cognition.db` 可匯入 Memory／Source／Graph。
- **AC-J03**：Active Focus 不自動續跑。
- **AC-J04**：Backup／Restore 文件可操作。
- **AC-J05**：Corrupt Legacy Input 回傳錯誤，不改成成功。
- **AC-J06**：Static Audit 驗證 JSON、TOML、Module、Placeholder 與 Git 狀態。
