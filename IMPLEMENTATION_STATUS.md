# Implementation Status

## 已完成

- 12-crate Rust Workspace。
- Contracts、Policy、Store、Core、Memory、Runtime、Workflow、Planner、Socrates、Capability、SDK、MCP、CLI。
- 單一 SQLite DB 與 Legacy Migration。
- Spec、Plan、Acceptance、ADR、Schemas、Fixtures、Examples、CI、Release Workflow。
- `Cargo.lock` 已由 Rust 1.97.1 產生並納入版本控制。

## 真實驗證狀態（Rust 1.97.1 / Windows x86_64-msvc）

2026-08-14 在具備完整 Toolchain 的環境完成第一次端到端驗證：

```text
cargo fmt --all -- --check                              PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings   PASS
cargo test --workspace --all-features --locked         PASS (38 tests, 0 failed)
cargo build --workspace --release --all-features --locked   PASS
python scripts/static_audit.py                         PASS
python scripts/mcp_smoke.py target/release/cordis-mcp  PASS (Windows: .exe)
```

驗證期間的修正：

- `cargo fmt` 套用全 workspace 格式。
- 移除未使用 import（`PlannerBoundary`）與多餘 `mut`。
- `[workspace.lints.clippy]` 的 `all` 群組優先權修正為 `priority = -1`。
- Clippy `field-reassign-with-default`、`filter-map-bool-then` 修正。
- `scripts/mcp_smoke.py` 斷言對齊實際契約：
  - `cordis_finish` 回傳 `FeedbackResult`（`event` 於頂層，無 `learning` 包裝）。
  - `cordis_status` 的 task 計數位於 `runtime.core.counts.task_records`。

## 後續驗證環境

正式 Release 前仍需在 CI Matrix（Linux、macOS）完成同一組指令：

```bash
./scripts/check.sh
```

或 Windows：

```powershell
.\scripts\check.ps1
```
