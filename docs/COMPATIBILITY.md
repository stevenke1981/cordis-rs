# Python CORDIS v0.5.1 對齊矩陣

## Package Mapping

| Python Distribution | Rust Crate | 對齊狀態 | 改善 |
|---|---|---:|---|
| `cordis-core` | `cordis-core` | 完成 | 統一 Store、Fail-closed Auth、CJK Retrieval |
| `cordis-memory` | `cordis-memory` | 完成 | Trust、Instruction Safety、來源隔離 |
| `cordis-runtime` | `cordis-runtime` | 完成 | Machine Permit、Managed Session、FSM |
| `cordis-planner` | `cordis-planner` | 完成 | Provider-neutral Callable、Strict Plan Validation |
| `cordis-socrates` | `cordis-socrates` | 完成 | Rule-only Mode、Model Gate 收緊 |
| `cordis-bridge` | `cordis-mcp` + `cordis-cli` | 完成 | Native Binary、雙 MCP Protocol、Structured Content |
| `cordis-capability` | `cordis-capability` | 完成 | Timeout Probe、同一 DB |
| `cordis-ai` | `cordis-sdk` + Workspace binaries | 完成 | Embeddable Composition Root |

## Contract Mapping

| Schema | Rust Type | 相容性 |
|---|---|---|
| `cordis.task.v1` | `TaskContract` | 名稱與主要欄位對齊；Rust Authorization 欄位更完整 |
| `cordis.authorization.v1` | `AuthorizationEnvelope` | 對齊並新增 Tool/Target/Expiry Enforcement |
| `cordis.difficulty.v1` | `DifficultyProfile` | 六軸與 Control Mode 對齊 |
| `cordis.plan.v1` | `PlanIr` | 對齊；Alpha Runtime 仍限制單一路徑 |
| `cordis.step-result.v1` | `StepResult` | 對齊；Evidence 規則更嚴格 |
| `cordis.cognitive.v1` | `CognitiveIr` | 對齊核心欄位，加入明確 Permit／Trust Context |
| `cordis.feedback-result.v1` | `FeedbackResult` | 對齊核心 Learning Result |

## Public Lifecycle Mapping

| Python API／Tool | Rust API／Tool | 備註 |
|---|---|---|
| `preflight` / `cordis_begin` | `CordisCore::preflight` / `cordis_begin` | 對齊 |
| `feedback` / `cordis_finish` | `CordisCore::feedback` / `cordis_finish` | Success Evidence 更嚴格 |
| `query` | `CordisMemory::query` / `cordis_query` | 加入 Trust Segmentation |
| `observe` | `CordisHostRuntime::observe` | 對齊 |
| `check_action` | `CordisHostRuntime::check_action` | 從 Advisory 擴充為 Permit + Drift Signal |
| `workflow_begin` | `CordisWorkflowRuntime::begin` | Authorization 完整傳遞 |
| `workflow_submit_plan` | `submit_plan` | 對齊；Action Policy 真正驗證 |
| `workflow_approve_step` | `approve_step` | Approval 綁定 Current Step |
| `workflow_submit_step_result` | `submit_step_result` | 對齊 |
| `workflow_replan` | `replan` | 對齊 |
| `workflow_finish` | `finish` | Acceptance ID 明確驗證 |
| `status` | `CordisEngine::status` | 聚合所有子系統 |

## 刻意更嚴格的差異

### Authorization

Python v0.5 可在 Core 標示 Authorization Required，但 MCP／Workflow 入口可能漏傳 Authorization，且某些 Host Control 仍可能回傳 `execution_allowed=true`。Rust 版：

- 所有入口完整接受 Authorization Envelope。
- Policy Engine 是唯一 Permit 來源。
- Pending／Denied／Expired 都是機器可判斷的 Block。
- Allowed／Denied Action、Tool、Target 與 Network Profile 真正執行。

### Acceptance Evidence

Python Workflow 可能用 Criterion Description 是否出現在 Evidence Summary 中推定通過。Rust 版拒絕推定；必須明確提供 `acceptance_id`。

### Persistence

Python 使用 `state.json`、`cognition.db`、`focus.json`、`workflow.json`。Rust 使用單一 `cordis.db`，降低跨檔案 Crash 不一致。

### Memory Trust

Python Memory Content 可被直接組進 Model Context。Rust 版加入：

- Trust Level
- Instruction-safe Flag
- Principle-only Instruction Rule
- Migrated Content 預設 Untrusted

### Multi-writer

Python 文件明確要求一個 State Directory 一個 Mutating Host。Rust 版用 SQLite WAL 改善本機多 Process 協調，但仍不宣稱 Distributed Multi-writer。

## 暫未對齊或延後

- 任意 Parallel DAG Scheduler。
- HTTP／SSE Remote MCP Transport。
- Embedding／Vector Database Adapter。
- 完整多租戶身分、RBAC 與遠端 Secret Store。
- 自動模型能力路由與成本最佳化。
- Stable v1.0 Benchmark 數據。

## Migration Compatibility

`cordis migrate-python` 讀取舊 `.cordis`：

- 可保留已有 Learning 與 Memory。
- Active Focus 不自動續跑。
- Legacy Memory 不自動變成 Trusted Principle。
- 原資料不被修改。
- 新資料寫入指定 Rust `--data-dir`。
