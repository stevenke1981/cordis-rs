# ADR 0001：使用單一 SQLite Store

- 狀態：Accepted
- 日期：2026-08-14

## Context

Python v0.5 將 Core、Memory、Focus、Workflow 分散在 JSON 與 SQLite。Crash 或跨 Process Mutation 可能產生部分成功。

## Decision

Rust 版使用 `.cordis/cordis.db`，開啟 WAL、Foreign Key、Busy Timeout。所有 Domain State 以可查詢欄位加 Canonical JSON 保存。

## Consequences

優點：更一致的交易、復原、備份與查詢。代價：需要 Schema Migration，且不適合直接放在不可靠的網路檔案系統。
