# CORDIS Rust Roadmap

## v0.6 Alpha — Clean-room Alignment

- Rust Contracts／Policy／Store／Core／Memory／Runtime。
- Sequential Workflow FSM。
- Planner／Socrates Callable。
- Capability Registry。
- CLI／stdio MCP。
- Python v0.5 Migration。
- Trust-aware Memory。
- Cross-platform CI。

Release Gate：Workspace Build、Tests、Conformance、MCP Smoke、Migration Fixture 全通過。

## v0.7 Beta — Hardening

- SQLite Schema Migration Version Table。
- Redaction／Secret Detector。
- Idempotency Key 與 Command Journal。
- Stronger Artifact／Evidence Hashing。
- Full Approval Binding Record。
- FTS5 Retrieval 與可選 Tokenizer。
- Property Testing、Fuzz、Fault Injection。
- Structured Audit Export。
- Benchmark：With／Without CORDIS。

## v0.8 — Host Integration

- Codex Managed Lifecycle Adapter。
- Claude Code／OpenCode／Hermes Telemetry Adapter。
- Automatic Tool/Test Event Translation。
- Recovery UX：外部 Action 已執行但 Result 未提交。
- Model-specific Context Renderer，仍保持 Core Provider-neutral。

## v1.0 — Production Local Runtime

- 至少一個 Host 的完整 Managed Task Lifecycle。
- 可證明 Later-task Behavior Change。
- Stable DB Migration Policy。
- Security Review。
- Signed Cross-platform Binaries／SBOM。
- Observability Metrics：Route、Plan、Evidence Closure、Latency、Cost。

## v1.x — Extensibility

- Optional Remote API Transport。
- Workflow Preset Package。
- Embedding Adapter／Hybrid Retrieval。
- Capability Profiles。
- Policy Plugin Interface。
- Retention／Forget／Export API。

## v2.0 — Adaptive Multi-host Runtime

- 多 Host 共用經治理的 Cognitive State。
- 基於 Observed Evidence 的 Model／Tool Capability Routing。
- Controlled Parallel Scheduler。
- Benchmark 驗證更少重複失敗、更強 Evidence Closure 與更低不必要模型成本。
