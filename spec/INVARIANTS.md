# CORDIS Rust 不變條件

這些條件不是建議，而是 Runtime 必須持續成立的系統不變條件。

## Contract

- **INV-C01**：Task ID 非空、不可重複開啟。
- **INV-C02**：Plan 的 Task ID、Workflow ID 與 Goal 必須與 Active Workflow 完全一致。
- **INV-C03**：Step Result 必須指向 Active Plan ID、Plan Version 與 Current Step ID。
- **INV-C04**：Acceptance ID 在同一 Task 內唯一。
- **INV-C05**：Allowed 與 Denied Policy Set 不得重疊。

## Authorization 與 Policy

- **INV-P01**：`AuthorizationStatus::Denied` 永遠不得產生 Allowed Permit。
- **INV-P02**：Expired Grant 等同無效 Grant。
- **INV-P03**：需要 Authorization 時，Pending 不得執行。
- **INV-P04**：Denied Action／Tool／Target 永遠優先於 Allowed。
- **INV-P05**：Allowed Set 非空時，未宣告項目不得通過。
- **INV-P06**：Offline Profile 不得執行 Network Action。
- **INV-P07**：Scope Out Target 永遠不得通過。
- **INV-P08**：Approval 只對指定 Workflow、Plan Version、Step 有效。
- **INV-P09**：Takeover Mode 不得執行 Change Action。
- **INV-P10**：Host 不得自行計算 `execution_allowed`。

## Workflow

- **INV-W01**：只有 `awaiting_plan` 可 Submit First Plan。
- **INV-W02**：只有 `awaiting_approval` 的 Current Step 可被 Approve。
- **INV-W03**：只有 `active` 可提交 Step Result。
- **INV-W04**：未完成 Dependency 的 Step 不可執行。
- **INV-W05**：Replacement Plan Version 必須遞增。
- **INV-W06**：未進入 `awaiting_replan` 不可替換 Plan。
- **INV-W07**：Closed Workflow 不得再 Mutate 或 Finish。
- **INV-W08**：Alpha Runtime 不接受 Parallel Success Path。

## Evidence 與 Learning

- **INV-E01**：Success 不得含 Failed Evidence。
- **INV-E02**：Failure 必須含 Failed Evidence。
- **INV-E03**：Task Success 必須證明每個 Required Acceptance ID。
- **INV-E04**：Step Success 必須證明每個 Required Step Evidence Summary。
- **INV-E05**：同一 Task 只接受一次 Terminal Feedback。
- **INV-E06**：Attribution Hint 不可與 Evidence Kind 衝突。
- **INV-E07**：單一成功不得把 Seed Strategy Promotion 為 Active。
- **INV-E08**：同一 Source ID 的重複 Pattern Evidence 只算一次。

## Memory

- **INV-M01**：Project Memory 不得回傳給其他 Project。
- **INV-M02**：Conversation／Workflow Memory 不得在沒有對應 Context 時查出。
- **INV-M03**：Untrusted Memory 不得進入 Instruction Section。
- **INV-M04**：只有 Reviewed、Instruction-safe Principle 可作為指令。
- **INV-M05**：Migrated External Memory 預設 Untrusted。
- **INV-M06**：Candidate Pattern 不得被 Host 當成已證實 Principle。

## Persistence

- **INV-S01**：SQLite Foreign Key 必須開啟。
- **INV-S02**：SQLite 使用 WAL 與 Busy Timeout。
- **INV-S03**：每個 Workflow Mutation 寫入 Durable Snapshot。
- **INV-S04**：Focus 可跨 MCP／CLI Process 復原。
- **INV-S05**：Migration 不自動恢復 Active Execution Permit。
- **INV-S06**：Mutating API 失敗時不得回傳成功狀態。

## Model Boundary

- **INV-A01**：Planner 不執行工具、不授權、不核准。
- **INV-A02**：Socrates 不執行工具、不授權、不核准。
- **INV-A03**：模型輸出不得修改 Task Goal 或 ID。
- **INV-A04**：模型輸出不得放寬 Deterministic Hard Gate。
- **INV-A05**：模型宣稱的 Evidence 不等於 Observed Evidence。
