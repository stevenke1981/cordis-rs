# CORDIS Rust 實作與交付計畫

## 目標

將 Python CORDIS v0.5.1 重寫為 Rust Workspace，先維持 Wire Contract 與 Cognitive Behavior，再修正 Authorization、Persistence、Memory Trust、MCP 與 CI 缺口。

## Phase 0 — Baseline 與相容規格

- [x] 鎖定上游 v0.5.1 Commit。
- [x] 整理八個 Python Distribution 與公開 API。
- [x] 固定 `cordis.*.v1` Schema 名稱。
- [x] 建立 valid／invalid Conformance Fixtures。
- [x] 定義不相容改善與 Migration Policy。

## Phase 1 — Rust Contracts 與 Policy

- [x] `cordis-contracts`。
- [x] TaskContract、Authorization、Difficulty、PlanIR、StepResult。
- [x] Evidence、Cognitive IR、Feedback 與 Memory Types。
- [x] 單一 Fail-closed `PolicyEngine`。
- [x] Allowed/Denied Action、Tool、Target、Network、Scope Enforcement。
- [x] Machine-readable `ExecutionPermit`。

## Phase 2 — Unified Store、Core 與 Memory

- [x] SQLite WAL／Foreign Key／Busy Timeout。
- [x] Task、Feedback、Domain、Strategy、Episode、Pattern。
- [x] Memory Source、Trust、Instruction Safety、Graph。
- [x] Atomic JSON Payload Storage。
- [x] Python `cognition.db` Import。
- [x] Strategy Promotion／Quarantine。
- [x] Evidence Attribution 與 Calibration。

## Phase 3 — Host Runtime 與 Workflow

- [x] Host Focus 與 Model Context。
- [x] Authorization-aware `execution_allowed`。
- [x] CJK-aware drift/destructive lexical signal。
- [x] Managed Session。
- [x] Durable Sequential Workflow FSM。
- [x] Plan Admission、Approval、Retry、Replan、Finish。
- [x] Acceptance ID 強制驗證。

## Phase 4 — Planner、Socrates、Capability

- [x] Provider-neutral Planner Callable。
- [x] No-model Fast Route。
- [x] Socrates Rule-only 與 Model-proposed Boundary Review。
- [x] Deterministic Hard Gates。
- [x] Local Capability Register／Detect／Require。

## Phase 5 — Transport 與操作工具

- [x] JSON CLI。
- [x] Native stdio MCP。
- [x] Legacy `initialize` Compatibility。
- [x] MCP `2026-07-28 server/discover`。
- [x] Structured Tool Results。
- [x] Codex／Claude Code／OpenCode／Hermes Setup。
- [x] Python v0.5 Migration Command。

## Phase 6 — 品質與發行

- [x] GitHub Actions Matrix。
- [x] Release Packaging Workflow。
- [x] MCP Smoke Script。
- [x] Static Audit Script。
- [x] Security、Operations、Testing 與 ADR 文件。
- [x] 在可用 Rust 1.97.1 環境完成 `cargo fmt`。
- [x] 在可用 Rust 1.97.1 環境完成 `cargo clippy`。
- [x] 在可用 Rust 1.97.1 環境完成 `cargo test --workspace`。
- [x] 執行 `cargo generate-lockfile`、審閱並提交 `Cargo.lock`。
- [ ] 建立第一次 GitHub Release Binary。

## Release Gate

正式 `v0.6.0` 前必須：

1. Linux、Windows、macOS CI 全綠。
2. Conformance Fixtures 全通過。
3. Pending／Denied／Expired Authorization 全部 Fail-closed。
4. MCP Legacy 與 Modern Discovery Smoke Test 通過。
5. Python v0.5 Migration Fixture 通過。
6. 重啟後 Workflow 可復原，且不能繞過 Approval。
7. Memory Prompt Injection Fixture 不得進入 Instruction Section。
8. 連續失敗策略會改變下一次 Preflight。
9. `Cargo.lock` 已由通過驗證的 Toolchain 產生並納入 Release Commit。
