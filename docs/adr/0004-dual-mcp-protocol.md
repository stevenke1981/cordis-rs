# ADR 0004：MCP Legacy 與 Modern Discovery 雙路相容

- 狀態：Accepted
- 日期：2026-08-14

## Context

現有 Agent Host 仍大量使用 `initialize`／`tools/list`／`tools/call`；新版 MCP 引入 Stateless Discovery。

## Decision

同一 stdio Server 支援 Legacy Lifecycle 與 `2026-07-28 server/discover`，Tool Result 同時提供 Text Content 與 Structured Content。

## Consequences

提高 Host 相容性；Transport Router 需要維持兩種協定測試。業務狀態仍在 SQLite，因此不依賴 Session RAM。
