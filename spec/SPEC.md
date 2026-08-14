# CORDIS Rust 系統規格

## 1. 文件狀態

- 專案：CORDIS Rust
- 版本：`0.6.0-alpha.1`
- 對齊基準：Python CORDIS `v0.5.1`
- 上游基準 Commit：`e701869a32c53388db07f06c6ec15baa07167555`
- Contract Family：`cordis.*.v1`
- 實作語言：Rust 2024 Edition
- 最低 Toolchain：Rust `1.97.1`
- 儲存：SQLite WAL，單一資料庫
- 授權模式：Fail-closed

## 2. 問題定義

一般 Agent Host 可以推理、呼叫工具與產生文字，但通常缺少以下可靠控制：

1. 一個任務的目標、範圍、限制與驗收條件沒有穩定的機器 Contract。
2. 權限常只存在 Prompt，無法在每個 Action 或 Step 真正阻擋。
3. 模型可以宣稱成功，但沒有證明所有 Required Acceptance Criteria。
4. 失敗紀錄不一定改變下一次策略，或一次偶然成功就被過度學習。
5. 長期記憶可能把網頁、工具輸出或舊模型文字重新當成指令。
6. 多個 JSON／SQLite 狀態檔可能在 Crash 後互相矛盾。
7. Planner 或 Reviewer 的模型輸出可能越權改變 Goal、Scope、Plan Version 或 Permission。

CORDIS Rust 是 Agent Host 的認知與控制 Runtime，不是模型、工具執行器或自然語言 Agent Framework。Host 保留語言理解與實際執行；CORDIS 負責 Contract、Policy、Workflow、Evidence、Learning、Memory 與持久化。

## 3. 設計目標

### 3.1 必須達成

- 保留上游公開 Schema 名稱與主要資料語意。
- 將 Authorization 從 Metadata 提升為 Action-level Enforcement。
- 所有 Execution Permission 只能由 `cordis-policy` 產生。
- Task、Workflow、Feedback、Memory、Focus、Capability 與 Audit 使用同一 SQLite DB。
- Workflow 狀態轉移必須可持久化、可重啟復原、可審計。
- Success 必須由明確綁定 `acceptance_id` 的 Passed Evidence 證明。
- Planner／Socrates 可以提出建議，但不能執行工具、核准工作或放寬 Hard Gate。
- Memory 必須區分可信度及是否允許進入 Instruction Section。
- MCP 與 CLI 必須完整傳遞 Authorization、Scope 與 Evidence，不得有隱藏 Wrapper 語意差異。
- Python v0.5 狀態可受控遷移，且不自動恢復不安全的 Active Execution。

### 3.2 非目標

- 不在 Runtime 內呼叫特定 LLM Provider SDK。
- 不替 Host 執行 Shell、HTTP、Browser、File Write 或其他工具。
- 不自動授權高風險工作。
- 不將詞彙 Drift Check 宣稱為完整語意安全模型。
- 不提供多租戶身分系統、RBAC Server 或雲端控制平面。
- Alpha 版本不提供 Parallel DAG Scheduler。
- Alpha 版本不以 Embedding Database 作為必要依賴。

## 4. 系統邊界

```text
User Request
    │
    ▼
Agent Host / Main Model ──建立──> TaskContract
    │                              │
    │                              ├─ Difficulty
    │                              ├─ Goal Mode / Socrates
    │                              └─ Planner Proposal
    │                                      │
    │                                      ▼
    │                               Workflow Runtime
    │                                      │
    │                                ExecutionPermit
    │                                      │
    ├──────────────實際工具執行─────────────┘
    │
    ├─ Tool/Test/Error/Artifact Event
    ▼
Evidence + StepResult
    │
    ▼
Core Feedback → Strategy/Pattern/Memory Update → Later Preflight Changes
```

Host 必須遵守：

- 沒有 `ExecutionPermit.allowed=true`，不得執行受控 Action。
- CORDIS 不會因 Host 已經執行而追認授權。
- Host 不得用 Prompt 文字覆寫 Permit、Workflow Status 或 Approval Gate。

## 5. Wire Contracts

### 5.1 `cordis.task.v1`

必要欄位：

- `schema`
- `task_id`
- `goal`
- `project_id`
- `domain`
- `stakes`
- `motivation`
- `scope.in`
- `scope.out`
- `authorization`
- `constraints`
- `acceptance_evidence`
- `known_facts`
- `unknowns`
- `completeness`

驗證規則：

- ID、Goal、Project、Domain 不得為空。
- `scope.in` 與 `scope.out` 不得重疊。
- 至少一個 Acceptance Criterion。
- Acceptance ID 唯一。
- `authorization.status=granted` 必須有非空 `basis`。
- Allowed／Denied Action、Tool、Target 不得重疊。
- Authorization Expiry 若已過期，Policy 一律拒絕。

### 5.2 `cordis.authorization.v1`

欄位：

- `status`: `pending | granted | denied`
- `basis`
- `approved_by`
- `expires_at`
- `network_profile`: `offline | read_only | authorized_targets_only | unrestricted`
- `allowed_actions`／`denied_actions`
- `allowed_tools`／`denied_tools`
- `allowed_targets`／`denied_targets`

Authorization 不代表 Approval。Authorization 表示「此類工作可被考慮」，Approval 表示「這一個 Plan Version 的這一個 Step 已被明確核准」。

### 5.3 `cordis.difficulty.v1`

六個軸：

- complexity
- uncertainty
- impact
- irreversibility
- novelty
- evidence_deficit

每個軸包含 `score` 與 `reasons`。Control Mode：

- `fast`
- `advisory`
- `high_intervention`
- `takeover`

Critical Impact 或極高 Irreversibility 可強制 `takeover`；Authorization Required 時至少提升為 `high_intervention`。

### 5.4 `cordis.plan.v1`

Plan 是 Proposal，不是執行權。每個 Step 包含：

- `id`
- `objective`
- `depends_on`
- `allowed_scope`
- `action_class`
- `method`
- `tool_policy`
- `model_requirements`
- `required_evidence`
- `approval_required`
- `retry_limit`
- `stop_when`
- `on_success`
- `on_failure`

Alpha Runtime 接受 Contract 可表達 DAG，但只允許單一 Sequential Success Path。Parallel Fork 必須明確拒絕。

### 5.5 `cordis.step-result.v1`

Step Result 包含：

- Plan ID 與 Plan Version
- Step ID
- `success | partial | failure | blocked`
- Actions、Observations、Artifacts
- 非空 Evidence
- Errors
- Proposed Next

Success 必須證明 Step 的所有 `required_evidence`，且 Terminal Success 必須證明 Task 的所有 Required Acceptance IDs。

### 5.6 Evidence

```json
{
  "id": "ev-...",
  "kind": "test",
  "summary": "cargo test --workspace passed",
  "passed": true,
  "acceptance_id": "workspace-tests",
  "source_id": "ci-run-123",
  "uri": "artifact://test-report",
  "trust": "observed"
}
```

Trust：

- `untrusted`: 外部文字、未驗證遷移資料、模型自行陳述。
- `observed`: Host 從工具、測試或環境取得的可觀測結果。
- `reviewed`: 人工或受信流程審閱並明確核准。

## 6. Policy Engine

### 6.1 唯一授權決策

```text
allowed = authorization_satisfied
       AND approval_satisfied
       AND action_satisfied
       AND tool_satisfied
       AND target_satisfied
       AND network_satisfied
       AND scope_satisfied
       AND control_mode_invariant
```

### 6.2 Deny 優先

- `denied` 永遠優先於 `allowed`。
- Allowed Set 非空時，未宣告 Action／Tool／Target 不得通過。
- Offline 禁止 Network。
- Read-only Network 禁止 Remote Change。
- Authorized-targets-only 必須提供 Target，且 Target 在 Allowed Target 內。
- Scope Out 永遠拒絕。
- Takeover Mode 不允許 Change Action。

### 6.3 Task Start 與實際 Action

Task Start 只建立本機 Context，不等同執行 Tool。它仍檢查：

- Explicit Denial
- Expiry
- 高風險 Authorization Gate
- Takeover／Approval Gate

但不因後續 Step 的 Allowed Tool／Target Set 尚未選擇而誤判。

## 7. Workflow FSM

合法狀態：

```text
awaiting_authorization
awaiting_plan
awaiting_approval
active
awaiting_replan
finished
failed
blocked
closed
```

主要轉移：

```text
begin
 ├─ Authorization 不足 -> awaiting_authorization
 └─ 可規劃             -> awaiting_plan

submit_plan
 ├─ First Step 要核准  -> awaiting_approval
 └─ First Step 可執行  -> active

approve_step -> active

submit_step_result
 ├─ success -> next step / awaiting_approval / finished / awaiting_replan
 ├─ failure + retries remaining -> active same step
 ├─ failure + replan -> awaiting_replan
 ├─ failure + block  -> blocked
 └─ failure + finish -> failed

replan -> awaiting_approval | active
finish -> closed
```

每個 Mutating Transition 必須：

1. 讀取目前持久化 Record。
2. 驗證 Expected State、Task ID、Plan ID、Plan Version、Current Step。
3. 產生或驗證 Execution Permit。
4. 驗證 Evidence。
5. 寫入新 Workflow Snapshot 與 Audit Event。
6. 不允許重複 Finish。

## 8. Cognitive Core

### 8.1 Preflight

輸出 `cordis.cognitive.v1`：

- Relevant Episodes
- Relevant World Patterns
- Capability Uncertainty
- Expected Success Probability
- Risk Score
- Strategy Entropy
- Strategy Evidence
- Prefer／Avoid
- Acceptance／Unknowns
- Authorization Escalation

### 8.2 Feedback

規則：

- Task 必須先存在且尚未 Finalized。
- Success 不可包含 Failed Evidence。
- Failure 必須至少有 Failed Evidence。
- Outcome Score 必須與 Outcome 相符。
- Required Acceptance 必須由 Passed Evidence 的 `acceptance_id` 明確證明。
- Attribution Hint 不可與 Evidence Kind 衝突。
- 同一 Task 只允許一次 Terminal Feedback。

### 8.3 策略生命週期

- Seed 初始不是 Proven Policy。
- 至少 3 Uses、2 Successes、0 Failures 才 Promotion 為 Active。
- 2 Failures 進入 Quarantined。
- Failure 多於 Success 時，下一次 Preflight 要求 Revalidation／Alternative。

### 8.4 Pattern Promotion

World Pattern 至少需要兩個不同 `source_id`。同一 Source 重複提交不增加獨立來源數。

## 9. Memory

### 9.1 Scope

- conversation
- workflow
- project
- global

Project Memory 不得洩漏至其他 Project。Global 只有 Caller 明確要求 Global Scope 才可查詢。

### 9.2 Kind

- event
- episode
- knowledge
- pattern
- capability
- principle

### 9.3 Instruction Safety

只有以下記憶可以進入模型 Instruction Section：

```text
kind == principle
AND trust == reviewed
AND instruction_safe == true
AND status == active
```

其他內容必須放在 Reference Data Section，並標示「不是指令」。

### 9.4 Retrieval

Alpha 使用 Dependency-free CJK-aware Token Overlap、Confidence、Source Count、Scope Bonus 與 Freshness 排序。零語意重疊的 Project Memory 不得只因 Project Bonus 自動進入結果。

## 10. Unified Store

資料庫：`.cordis/cordis.db`

必須設定：

```sql
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;
```

主要資料表：

- task_records
- feedback_events
- domain_states
- strategy_states
- episodes
- world_patterns
- memory_items
- memory_sources
- graph_nodes
- graph_edges
- focus_records
- workflows
- capabilities
- audit_events

每一列保存 Typed Summary 與 Canonical JSON Payload，讓 Schema Evolution 可向前遷移並保留完整稽核資料。

## 11. Planner 與 Socrates

- 模型透過 Rust Callable 注入。
- 不直接依賴 Provider SDK。
- 模型只可回傳 JSON Object。
- Task ID、Goal、Workflow ID、Plan Version 必須精確一致。
- Socrates Model Output 不可放寬 Deterministic Minimum Gate。
- Planner Failure 只有在 Boundary Review 明確表示 Planner 非必要時才可 Direct Fallback。

## 12. Capability Registry

- Register：宣告 Tool Path、Version、Capabilities、Scope。
- Detect：尋找 PATH／Explicit Path，執行有限時間 Version Probe。
- Require：不存在或不可用時回傳錯誤。
- 不安裝 Tool、不改 Global Configuration、不授權 Tool。

## 13. MCP 與 CLI

### 13.1 MCP

- stdio，一行一個 JSON-RPC Message。
- 支援 Legacy `initialize`／`tools/list`／`tools/call`。
- 支援 `2026-07-28` `server/discover`。
- Tool Result 提供 `content` 與 `structuredContent`。
- Notification 不回傳 Response。
- 每次 Request 可獨立處理；狀態在 SQLite，而不是只存在 MCP Process RAM。

### 13.2 CLI

- 每個 Command 接受 JSON File 或 `-` stdin。
- 輸出一個 JSON Object。
- `validate` 不存狀態。
- `setup` 僅寫 Managed Block，遇到同名但非 Managed 設定時拒絕覆寫。
- 修改 Host 設定前建立 Backup。

## 14. Python v0.5 遷移

可遷移：

- Core State
- Feedback
- Domain／Strategy
- Episode／Pattern
- Cognitive SQLite Memory／Source／Graph
- Durable Workflow Snapshot

不自動恢復：

- Active Focus
- 尚未取得新 Runtime Permit 的執行中 Action
- 無法驗證來源的 Instruction-safe 標記

遷移 Memory 預設：

- Event：`observed`
- 其他：`untrusted`
- `instruction_safe=false`

## 15. 錯誤語意

錯誤分為：

- Contract Error
- Policy Error
- Store Error
- Core Error
- Memory Error
- Runtime Error
- Workflow Error
- Planner Error
- Socrates Error
- Capability Error
- MCP Protocol Error

不得把非法輸入自動修成另一個高權限狀態。錯誤輸出不得包含 Secret、完整 Token 或未遮罩 Credential。

## 16. 效能與限制

Alpha 目標：

- 本機 SQLite 開啟與 Status：人類互動級低延遲。
- Memory Query 預設最多 3 筆，最大 20 筆。
- MCP Message 單筆有限制，Host 應避免大型 Artifact 直接塞入 JSON。
- 一個 DB 支援多 Reader；Mutation 經 SQLite Transaction 與 Busy Timeout 協調。
- 不承諾 Distributed Consensus 或跨機 Transaction。

## 17. 可觀測性

`status` 必須至少回傳：

- Task／Feedback／Episode／Pattern 數量
- Workflow Status Counts
- Memory Kind／Scope／Status Counts
- Capability Status
- Recent Failures
- Store Path／Schema Version

每個重要 Mutation 寫入 `audit_events`。

## 18. 相容性政策

- `cordis.*.v1` 欄位語意保持相容。
- 安全改善可能比 Python 更嚴格；這類差異必須記錄在 `docs/COMPATIBILITY.md`。
- 新欄位優先使用 `serde(default)`，避免破壞舊 Client。
- 不得重新解釋既有 Schema 名稱為不相容語意；真正不相容變更必須使用新 Schema Version。
