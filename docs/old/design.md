# remote — リモート同期/ビルド CLI ツール 設計書

`docs/memo.md` の最終形（`./remote sync|build|run|test|logs ...`）を、**Rust 製の単一 exe** として実装するための設計。
Windows ローカルから WSL/rsync に依存せず動作させることを目標とする。

---

## 1. 方針 / 確定事項

| 項目 | 決定 | 理由 |
|---|---|---|
| 実装言語 | **Rust** | 単一静的バイナリ、クロスプラットフォーム |
| 同期方式 | **SFTP 差分同期を自前実装** | rsync 非依存・完全自己完結 |
| 配布形態 | `remote.exe` 1個 + 設定ファイル 1枚 | memo の発想を踏襲 |
| 設定形式 | `remote.toml` | serde で型安全に読める |

設計の役割分担（memo の運用ルールを継承）:

```text
ローカル: 正のコード置き場（ここだけ編集する）
リモート: ビルド・実行用のコピー（直接編集しない）
```

---

## 2. クレート選定

| 用途 | クレート | 備考 |
|---|---|---|
| SSH / SFTP | **`russh` + `russh-sftp`** | **ピュア Rust**。OpenSSL/libssh2 等の C ネイティブ依存が無く、Windows 単一 exe ビルドが楽。tokio 非同期。 |
| 非同期ランタイム | `tokio` | russh が要求 |
| CLI | `clap`(derive) | サブコマンド + グローバルフラグ |
| 設定 | `serde` + `toml` | `remote.toml` のデシリアライズ |
| 除外パターン | `ignore` または `globset` | gitignore 風のマッチング |
| ハッシュ(任意) | `blake3` | `--checksum` モード用 |
| ログ表示 | `tracing` + `tracing-subscriber` | `-v/-vv` で詳細度切替 |
| エラー | `anyhow`(アプリ) / `thiserror`(内部) | |

> 補足: `ssh2` クレート（libssh2 バインディング）はコードは短く書けるが、Windows で libssh2/OpenSSL のネイティブビルドが必要になりがち。**単一 exe 配布を優先して russh（ピュア Rust）を採用**する。

---

## 3. ディレクトリ構成

### ツール本体（このリポジトリ）

```text
local-sync-command/
  Cargo.toml
  remote.toml.example      # 設定ひな形（コミットする）
  remote.toml              # 実設定（.gitignore 推奨／秘密情報を含み得る）
  docs/
    memo.md
    design.md              # 本書
  src/
    main.rs                # clap エントリ、サブコマンド振り分け
    config.rs              # remote.toml ロード + 検証
    ssh.rs                 # russh セッション確立 / exec / SFTP ハンドル取得
    sync.rs                # 差分同期エンジン（walk + diff + transfer）
    walk.rs                # ローカル走査（exclude 適用）
    commands/
      mod.rs
      sync.rs              # sync サブコマンド
      build.rs             # sync → リモート build コマンド実行
      run.rs               # リモート run 実行
      test.rs              # sync → リモート test 実行
      logs.rs              # リモート→ローカルの逆同期（fetch）
      shell.rs             # 対話 ssh
      clean.rs             # リモート生成物削除
```

### 同期されるプロジェクト側（利用者の構成例）

```text
project/
  src/
  assets/
  tests/
  build.sh  run.sh  test.sh   # リモートで実行されるスクリプト
  remote.toml                 # 接続設定（ホスト変更時はここだけ直す）
  remote.exe                  # 配置 or PATH 上
```

---

## 4. 設定スキーマ（remote.toml）

```toml
# 接続先
host          = "192.0.2.10"
port          = 22
user          = "user"
remote_dir    = "/home/user/work/project"
local_dir     = "."                          # 省略時はカレント

# 認証（いずれか。未指定なら ssh-agent を試す）
identity_file = "~/.ssh/id_ed25519"          # 秘密鍵パス
# password    = "..."                        # 非推奨（平文）

# 同期除外（gitignore 風）
exclude = [
  ".git", "target", "build", "dist",
  "bin", "obj", "logs", "artifacts",
  "node_modules", "Library", "Temp", ".vs",
]

# リモートで実行するコマンド（サブコマンドに対応）
[commands]
build = "./build.sh"
run   = "./run.sh"
test  = "./test.sh"
clean = "rm -rf build logs artifacts"

# logs サブコマンドで「リモート→ローカル」に取得する対象
[[fetch]]
remote = "logs/"
local  = "logs/"
[[fetch]]
remote = "artifacts/"
local  = "artifacts/"
```

検証ルール（`config.rs`）:
- `host` / `user` / `remote_dir` は必須。
- `identity_file` の `~` を展開。指定が無ければ ssh-agent → 既定鍵パスの順に探索。
- `commands.*` 未定義のサブコマンドを呼んだらエラーメッセージを出す。

---

## 5. コマンド仕様

```text
remote sync            ローカル → リモートへ差分同期
remote build           sync 後にリモートで commands.build を実行
remote run             リモートで commands.run を実行（sync しない）
remote test            sync 後にリモートで commands.test を実行
remote logs            [[fetch]] 対象をリモート → ローカルへ取得
remote shell           対話 ssh（cd remote_dir 済み）
remote clean           リモートで commands.clean を実行
remote status          dry-run。送信/削除予定の一覧だけ表示（転送しない）
```

グローバルフラグ:

| フラグ | 意味 |
|---|---|
| `--config <path>` | 設定ファイルを明示（既定: `./remote.toml`） |
| `-n, --dry-run` | 実転送せず差分のみ表示 |
| `--delete` | リモートにあってローカルに無いファイルを削除（sync 時） |
| `--checksum` | mtime/サイズでなく blake3 ハッシュで差分判定 |
| `-v, -vv` | 詳細ログ |

> 安全側の既定: `--delete` は**明示時のみ**。memo の `rsync --delete` をデフォルトにしないことで事故を防ぐ。

---

## 6. SFTP 差分同期アルゴリズム（sync.rs の中核）

```text
1. ローカル走査
   - local_dir を再帰走査。exclude パターンに一致するものは除外。
   - マップ L: 相対パス -> (size, mtime[, hash])

2. リモート走査
   - SFTP で remote_dir を再帰走査。
   - マップ R: 相対パス -> (size, mtime)

3. 差分判定
   - アップロード対象 = L のうち、次のいずれか:
       ・R に存在しない
       ・size が異なる
       ・(既定) local.mtime > remote.mtime
       ・(--checksum 時) hash が異なる
   - 削除対象(--delete 時) = R に存在し L に無いもの

4. ディレクトリ生成
   - アップロード対象の親ディレクトリを mkdir -p 相当で先に作成。

5. 転送
   - 変更ファイルを SFTP で書き込み（ストリーム）。
   - 転送後、リモート側 mtime をローカルに合わせて setstat。
     → 次回の mtime 比較を安定させるため必須。

6. 削除
   - --delete 指定時のみ削除対象を remove。

7. サマリ出力
   - 送信 N 件 / 削除 M 件 / スキップ K 件 を表示。
```

判定基準の整理:

| モード | 比較キー | 速度 | 精度 |
|---|---|---|---|
| 既定 | size + mtime | 速い | 通常十分（rsync 既定と同等） |
| `--checksum` | blake3 ハッシュ | 遅い | 内容一致を厳密判定 |

実装上の注意:
- **mtime の往復**: SFTP の `setstat` で転送後に mtime を合わせないと、毎回「ローカルの方が新しい」と誤判定して全送信になる。
- **走査の並行化**: 初期実装は逐次でよい。後で `tokio` のセマフォで N 並列アップロードに拡張可能（構造は分離しておく）。
- **シンボリックリンク / パーミッション**: 初期スコープ外（必要になったら追加）。
- **パス区切り**: ローカルは Windows(`\`)、リモートは POSIX(`/`)。相対パスは内部で `/` 正規化して扱う。

---

## 7. SSH 実行（ssh.rs）

- `build/run/test/clean` は `ssh exec` 相当:
  ```text
  cd <remote_dir> && <commands.xxx>
  ```
  を 1 チャネルで実行し、stdout/stderr をローカルにストリーム表示。終了コードを伝播。
- `shell` は対話 PTY を割り当て（russh の PTY 要求）。初期は「`cd` 済みで bash 起動」を満たせれば十分。
- 認証順序: `identity_file` → ssh-agent → 既定鍵（`~/.ssh/id_ed25519`, `id_rsa`）。
- ホスト鍵検証: 初期は known_hosts を読み TOFU（未知なら警告して保存）。**検証無効化はしない**。

---

## 8. 典型フロー

```text
# 開発中（試行錯誤）
remote build      # 差分同期 → リモートビルド
remote run        # リモート実行
remote logs       # ログ/成果物を取得

# 確認だけ
remote status     # 何が送られるか dry-run で確認
```

memo のハイブリッド運用（普段 rsync 相当の sync、区切りで git push）はそのまま成立する。

---

## 9. 段階的な実装順序（着手時の想定）

```text
M1: 設定ロード + ssh 接続 + exec（build/run/test/shell）
M2: SFTP 差分同期（sync, status, --dry-run, --delete）
M3: logs（逆同期 fetch）, clean
M4: --checksum, 並列転送, known_hosts/TOFU 整備
```

M1 だけでも「ローカル正・リモートでビルド実行」の最小価値は出る（同期は一旦 scp 代替でも可）。M2 で本命の差分同期が入る。

---

## 10. 未確定 / 要検討事項

- 認証方式の優先度（agent 必須にするか、鍵パス必須にするか）。
- ホスト鍵検証ポリシー（TOFU で十分か、known_hosts 必須か）。
- リモート Windows 対応（`cd && ./build.sh` が通らない。PowerShell 経路を別途用意するか）。memo では「リモート Windows は WSL 導入推奨」としており、初期は **リモート=POSIX 前提**で割り切る案。
- `remote.toml` を同期対象に含めるか（含めると秘密情報がリモートへ行く → **exclude 既定に入れる**べき）。
```