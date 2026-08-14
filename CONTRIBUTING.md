# Contributing

## 原則

- 保持 Core、Planner、Socrates Provider-neutral。
- 不在 Transport 層複製業務邏輯。
- Execution Permission 必須經過 `cordis-policy`。
- 新 Contract 欄位附 Schema、Valid／Invalid Fixture 與 Negative Test。
- 安全改善不得只寫在 Prompt。

## 開發

```bash
rustup override set 1.97.1
./scripts/check.sh
```

Commit 前確認：

- Format、Clippy、Tests、Release Build。
- Static Audit。
- 新功能更新 Spec／Compatibility／Acceptance。
- 不提交 `.cordis`、Token、私人 Evidence 或 Build Artifact。

## Pull Request

說明：問題、設計、Contract 變更、Migration、測試證據、安全影響與相容性。
