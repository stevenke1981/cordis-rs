# CORDIS Rust Rewrite — Top-level Specification

此檔案是 `spec/SPEC.md` 的入口。完整規格、驗收、不變條件與相容邊界分別位於：

- `spec/SPEC.md`
- `spec/ACCEPTANCE.md`
- `spec/INVARIANTS.md`
- `docs/COMPATIBILITY.md`

核心要求：

1. 保留上游 CORDIS v0.5.1 的公開 `cordis.*.v1` Contract family。
2. Authorization 不得只存在 Prompt；必須在 Action/Step 層由單一 Policy Engine 執行。
3. Task、Feedback、Workflow、Memory、Focus 與 Capability 使用同一 SQLite Database。
4. Success 必須證明所有 Required Acceptance IDs。
5. 外部 Memory 預設不是指令。
6. Planner、Socrates、MCP 與 CLI 不擁有工具執行權。
7. 每一個非法狀態轉移必須回傳結構化錯誤，而不是自動猜測。
