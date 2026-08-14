# ADR 0003：Memory Trust 與 Instruction Safety 分離

- 狀態：Accepted
- 日期：2026-08-14

## Context

外部內容或舊模型 Lesson 可能含 Prompt Injection，跨任務保存後風險更高。

## Decision

Memory 擁有 `trust` 與 `instruction_safe`。只有 Reviewed、Instruction-safe Principle 可以進 Instruction Section；其他一律是 Reference Data。

## Consequences

降低持久化注入風險，但需要 Host／人工流程審閱 Principle。舊資料遷移後不會自動保留指令權。
