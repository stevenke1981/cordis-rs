# CORDIS Rust 安全模型

## 資產

- Task Goal、Scope、Constraint 與 Acceptance Criteria
- Authorization、Approval 與 Execution Permit
- Tool／Test／Error Evidence
- Project Memory 與 Learned Strategy
- Capability Path／Version
- Host MCP Configuration
- SQLite Database 與 Backup

## 信任邊界

| 來源 | 預設信任 |
|---|---|
| User 明確授權 | 只在 Authorization Envelope 的範圍內 |
| Host Tool Telemetry | Observed，但仍需 Contract Validation |
| Test Runner Result | Observed |
| Planner／Socrates 模型輸出 | Untrusted Proposal |
| 網頁／Issue／文件／工具 stdout | Untrusted Reference |
| Python Legacy Memory | Untrusted，Event 可標 Observed |
| Reviewed Principle | Reviewed + Instruction-safe 後才可作指令 |

## 主要威脅與控制

### 1. Authorization Bypass

威脅：Host 忽略 Prompt 中的「不可執行」，或從不同 Boolean 推導為可執行。

控制：

- 只有 `PolicyEngine` 產生 `ExecutionPermit`。
- Deny 優先。
- Pending／Denied／Expired Fail-closed。
- MCP／CLI 完整傳遞 Policy 欄位。
- Workflow 在 Plan Admission 與 Current Step Execution 都重新檢查。

### 2. Approval Replay

威脅：將舊 Approval 套用到新 Plan 或不同 Step。

控制：Approval Record 必須綁定 Workflow ID、Plan ID／Version、Step ID、Approver 與 Timestamp；Current Step 改變後舊 Approval 不成立。

### 3. Prompt Injection Persistence

威脅：外部內容保存進 Memory，之後跨任務成為指令。

控制：

- 外部內容 `untrusted`。
- Instruction Section 僅允許 Reviewed、Instruction-safe Principle。
- Reference Section 明確標示非指令。
- Migration 不保留 Instruction-safe Claim。
- Source ID 與 Provenance 保留供審查。

### 4. Evidence Forgery

威脅：模型聲稱測試通過或偽造多個 Source ID。

控制：

- Managed Session 期待 Host Telemetry，不接受自然語言宣稱作為 Observed。
- Required Acceptance 使用明確 ID。
- Pattern 需要不同 Source；正式 Host 應使用 Artifact Hash／CI Run ID，而不是任意模型字串。
- 高風險場景可要求 Reviewed Evidence。

### 5. Scope Escape

威脅：Plan Step 或 Action Target 超出 Task Scope。

控制：Plan Admission 與 Permit 都檢查 Scope In／Out。Scope Out 永遠優先。

### 6. Tool／Target Substitution

威脅：核准了 Git Read，執行時改用 Shell Delete 或不同 Repository。

控制：Permit 同時檢查 Action Class、Action Name、Tool、Target、Network Profile、Scope；Host 必須把真正執行參數映射到 Action Proposal。

### 7. Database Corruption／Race

控制：SQLite WAL、Foreign Key、Busy Timeout、Transaction、Durable Snapshot、Backup。不可將 DB 放在不支援可靠 File Lock 的共享網路目錄。

### 8. Secret Leakage

目前 Alpha 不保存專用 Secret 欄位。Host 必須：

- 不把 Token、Cookie、Authorization Header 寫進 Evidence Summary。
- URI 移除 Query Secret。
- Tool stdout 在 Observe 前做 Redaction。
- Backup 使用 OS Disk Encryption 或加密封裝。

未來版本應加入可配置 Redactor 與 Secret Detector。

### 9. Command Probe

Capability Detect 只執行指定 Executable 的 Version Flag，設 Timeout，不使用 Shell Expansion。仍應避免對不可信使用者開放任意 Probe Spec。

### 10. MCP Local Attack Surface

stdio MCP 沒有遠端 Listener，但繼承啟動 Host 的檔案權限。使用者應：

- 每個信任邊界使用不同 Data Dir。
- 不以 Administrator／root 執行一般 Agent。
- 限制 `.cordis` 權限。
- Setup Helper 只管理有 Marker 的區塊。

## 安全非保證

- Lexical Drift Check 不是完整語意分析。
- CORDIS 無法阻止惡意 Host 故意忽略 Permit。
- 本機 Database Access 等同可讀取保存的 Task／Memory。
- Alpha 不提供遠端身分驗證、Tenant Isolation 或 Distributed Lock。

## 回報漏洞

請勿在公開 Issue 貼出 Token、使用者資料或可直接利用的生產環境細節。報告至少包含：

- 受影響版本／Commit
- 最小重現
- 預期與實際 Policy／State
- 影響範圍
- 是否可跨 Project／跨 Host
