# Packaging

## Source Package

```bash
./scripts/package.sh
```

產生：

- `cordis-rs-0.6.0-alpha.1-source.tar.gz`
- `cordis-rs-0.6.0-alpha.1.bundle`
- `SHA256SUMS`

## Git Bundle Restore

```bash
git clone cordis-rs-0.6.0-alpha.1.bundle cordis-rs
git -C cordis-rs log --oneline
```

## Binary Release

GitHub Release Workflow 應在 Linux、Windows、macOS 建立原生 Binary、產生 Checksums，並保存 SBOM／Test Evidence。Alpha 原始碼包不包含 `target/`。
