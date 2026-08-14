# CORDIS Rust 測試策略

## 測試金字塔

### Unit Tests

每個 Crate 內：

- Contract Validation
- Policy Decision
- Store CRUD／Transaction
- Core Learning
- Memory Trust／Scope
- Difficulty Routing
- Managed Session
- Workflow Transition
- Planner／Socrates Hard Gate
- Capability Probe
- MCP Router

### Conformance Fixtures

`conformance/valid` 與 `conformance/invalid` 保存語言無關 JSON。目的是讓 Python、Rust、未來 Go／TypeScript Adapter 執行相同 Case。

### Integration Tests

建議 CI 追加：

- Direct Begin → Check Action → Observe → Finish → Next Begin
- Workflow Begin → Authorization → Plan → Approval → Result → Finish
- Restart between every lifecycle call
- Python v0.5 migration
- Prompt Injection Memory isolation

### MCP Smoke

`scripts/mcp_smoke.py`：

1. 啟動 `cordis-mcp`。
2. Legacy Initialize。
3. Tools List。
4. Modern Server Discover。
5. Status Tool Call。
6. Direct Task Lifecycle。
7. 驗證每一行都是 JSON-RPC Response。

## 本機完整檢查

第一次在可連線的 Rust Toolchain 環境驗證此 Alpha Source Tree 時，先產生並審閱鎖檔：

```bash
cargo generate-lockfile
git add Cargo.lock
```

Binary Workspace 的正式 Release Commit 必須包含 `Cargo.lock`。本次生成環境無
`cargo` 且無 Registry Metadata，因此交付包沒有偽造鎖檔。

```bash
./scripts/check.sh
```

等同：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features
cargo test --workspace
cargo build --workspace --release
python3 scripts/static_audit.py
python3 scripts/mcp_smoke.py target/release/cordis-mcp
```

Windows：

```powershell
.\scripts\check.ps1
```

## 必測負向案例

- Granted 無 Basis
- Expired Authorization
- Denied Tool
- Missing Target with Allowed Targets
- Network on Offline Profile
- Plan Scope Escape
- Parallel Plan Fork
- Wrong Plan Version
- Out-of-order Step
- Approval Missing
- Empty Evidence
- Success with Failed Evidence
- Success without Acceptance ID
- Duplicate Feedback
- Cross-project Memory Query
- Untrusted Prompt Injection as Instruction
- Duplicate Pattern Source
- Planner Prose Output
- Socrates Changed Goal
- Corrupt Legacy JSON／SQLite

## Property／Fuzz 建議

正式版前可加入：

- Arbitrary JSON Contract Deserialize Fuzz
- Workflow Transition Sequence Property Test
- Policy Deny Monotonicity：增加 Deny 不可讓 Permit 從 False 變 True
- Authorization Expiry Boundary
- SQLite Crash／Restart Fault Injection
- MCP Malformed Message Fuzz

## CI Matrix

- OS：Ubuntu、Windows、macOS
- Rust：Pinned Stable 1.97.1
- Optional：Latest Stable、Beta（allow failure only during Alpha）
- Features：All Features
- Release Build／Smoke

## 測試證據保存

CI 應上傳：

- JUnit／test output
- MCP smoke log
- Static audit report
- Binary checksums
- Migration report fixture
