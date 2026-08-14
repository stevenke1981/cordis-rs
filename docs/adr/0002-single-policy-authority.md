# ADR 0002：Execution Permit 只有一個 Authority

- 狀態：Accepted
- 日期：2026-08-14

## Context

Prompt、Control Mode、Authorization Flag 與 Approval 若各自判斷，可能出現文字說不可執行、機器欄位卻允許的矛盾。

## Decision

只有 `cordis-policy::PolicyEngine` 可以產生 `ExecutionPermit`。所有 Host／Workflow 只消費 Permit，不重新推導。

## Consequences

安全語意一致；新增 Policy 欄位必須集中更新。Host 若故意忽略 Permit，仍超出 Runtime 可防止範圍。
