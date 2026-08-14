# CORDIS Rust 操作手冊

## 安裝

```bash
rustup toolchain install 1.97.1 --component rustfmt clippy
cargo generate-lockfile
cargo build --workspace --release
install -m 0755 target/release/cordis ~/.local/bin/cordis
install -m 0755 target/release/cordis-mcp ~/.local/bin/cordis-mcp
```

第一次驗證 Alpha Source Tree 時，請審閱並提交產生的 `Cargo.lock`。正式 Binary
Release 必須使用已納入版本控制的鎖檔。

Windows 可將 `target\release\cordis.exe` 與 `cordis-mcp.exe` 放入 PATH 目錄。

## 初始化

```bash
cordis --data-dir /path/to/project/.cordis init
cordis --data-dir /path/to/project/.cordis status
```

預設資料庫：

```text
.cordis/cordis.db
.cordis/cordis.db-wal
.cordis/cordis.db-shm
```

## Host 設定

```bash
cordis setup codex
cordis setup claude-code
cordis setup opencode
cordis setup hermes
```

Setup Helper：

- 使用目前 `cordis-mcp` Binary 絕對路徑。
- 每個 Host 預設獨立 Data Dir。
- 寫入 Managed Marker。
- 修改前建立 Backup。
- 偵測到非 Managed 同名設定時拒絕。

## 備份

停止該 Data Dir 的 Mutating Host，或使用 SQLite Backup Tool：

```bash
sqlite3 .cordis/cordis.db ".backup '.cordis/backup-$(date +%Y%m%d-%H%M%S).db'"
```

最簡單的離線備份：

```bash
cordis --data-dir .cordis status
# 停止 MCP Host
cp .cordis/cordis.db .cordis/cordis-backup.db
```

不要只複製 Main DB 而讓 Writer 持續運作；WAL 中可能還有未 Checkpoint 資料。

## Restore

1. 停止所有使用該 Data Dir 的 Host。
2. 保存目前 DB 供鑑識。
3. 替換 Main DB。
4. 移除屬於舊 DB 的 `-wal`／`-shm`。
5. 執行 `cordis status`。
6. 查詢 Active Workflow，確認是否有外部 Tool 已執行但 Evidence 未提交。

## Python v0.5 遷移

```bash
cordis --data-dir .cordis-rs migrate-python .cordis-python
```

遷移前：

- 備份舊目錄。
- 停止舊 MCP Server。
- 記錄任何尚在執行的工具。

遷移後：

- 檢查 Migration Report。
- 檢查 Strategy／Pattern Counts。
- 審閱重要 Memory Trust。
- Active Focus 不會自動續跑；重新建立新 Task。

## Concurrency

- 同一台機器上的多 Process 依賴 SQLite Lock。
- 不要在 SMB／NFS 等無可靠 Lock 的路徑共享 DB。
- 高 Mutation 負載應使用每個 Project／Host 分離的 Data Dir。
- 遇到 Busy Timeout，Host 應退避並重新讀取 Workflow State，而不是盲目重送外部 Action。

## 清理與保留

Alpha 版沒有自動 TTL Worker。建議定期：

- 匯出／審閱 Old Evidence。
- Retire 過期 Knowledge／Pattern。
- 移除不再需要的 URI／Artifact Reference。
- Vacuum 前先備份。

```bash
sqlite3 .cordis/cordis.db "PRAGMA wal_checkpoint(TRUNCATE); VACUUM;"
```

## 診斷

```bash
cordis status
cordis capability-status
cordis workflow-get examples/workflow-id.json
```

資料庫完整性：

```bash
sqlite3 .cordis/cordis.db "PRAGMA integrity_check;"
```

## 升級

1. 備份 DB。
2. 升級 Binary。
3. 執行 `cordis status` 觸發／驗證 Schema Migration。
4. 執行 Smoke Task。
5. 重啟 Host。

不應直接降級到不認識新 Schema 的 Binary。需要降級時用升級前 Backup。
