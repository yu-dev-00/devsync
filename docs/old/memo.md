構成としては、**ローカルに本体リポジトリを置き、リモートPCには同期された作業コピーを置く**形がよいです。

```text
[ローカルPC]
  project/
    src/
    assets/
    scripts/
      remote-sync.sh
      remote-build.sh
      remote-run.sh
      remote-test.sh
      remote-fetch.sh

        ↓ rsync / git push / ssh

[リモートPC]
  ~/remote-work/project/
    src/
    assets/
    build/
    logs/
    artifacts/
```

基本の流れはこれです。

```text
1. ローカルPCでコードを書く
2. rsyncでリモートPCへ差分同期
3. sshでリモートPC上のbuild.shを実行
4. sshでテスト・実行
5. ログや成果物だけローカルに取得
```

---

## まず作るべき最小構成

### ローカル側

```text
project/
  src/
  assets/
  build.sh
  run.sh
  test.sh
  scripts/
    remote-sync.sh
    remote-build.sh
    remote-run.sh
    remote-test.sh
    remote-fetch-logs.sh
```

### リモート側

```text
~/work/project/
  src/
  assets/
  build.sh
  run.sh
  test.sh
  build/
  logs/
  artifacts/
```

ローカルとリモートで同じプロジェクト構成にしておくと、スクリプトが単純になります。

---

## 1. 同期スクリプト

ローカルの `scripts/remote-sync.sh` です。

```bash
#!/usr/bin/env bash
set -e

REMOTE_USER="user"
REMOTE_HOST="192.0.2.10"
REMOTE_DIR="/home/user/work/project"

rsync -av --delete \
  --exclude ".git" \
  --exclude ".vs" \
  --exclude ".vscode" \
  --exclude "bin" \
  --exclude "obj" \
  --exclude "build" \
  --exclude "dist" \
  --exclude "logs" \
  --exclude "artifacts" \
  --exclude "Library" \
  --exclude "Temp" \
  --exclude "node_modules" \
  ./ "${REMOTE_USER}@${REMOTE_HOST}:${REMOTE_DIR}/"
```

重要なのは、**生成物を同期しない**ことです。

```text
同期する:
  src/
  assets/
  設定ファイル
  build.sh
  run.sh
  test.sh

同期しない:
  build/
  bin/
  obj/
  logs/
  artifacts/
  UnityのLibrary/
  node_modules/
  .git/
```

---

## 2. リモートビルドスクリプト

ローカルの `scripts/remote-build.sh` です。

```bash
#!/usr/bin/env bash
set -e

REMOTE_USER="user"
REMOTE_HOST="192.0.2.10"
REMOTE_DIR="/home/user/work/project"

bash scripts/remote-sync.sh

ssh "${REMOTE_USER}@${REMOTE_HOST}" "
  cd ${REMOTE_DIR}
  chmod +x build.sh
  ./build.sh
"
```

これでローカルから、

```bash
./scripts/remote-build.sh
```

だけで、

```text
同期 → リモートビルド
```

ができます。

---

## 3. リモート実行スクリプト

```bash
#!/usr/bin/env bash
set -e

REMOTE_USER="user"
REMOTE_HOST="192.0.2.10"
REMOTE_DIR="/home/user/work/project"

ssh "${REMOTE_USER}@${REMOTE_HOST}" "
  cd ${REMOTE_DIR}
  chmod +x run.sh
  ./run.sh
"
```

---

## 4. リモートテストスクリプト

```bash
#!/usr/bin/env bash
set -e

REMOTE_USER="user"
REMOTE_HOST="192.0.2.10"
REMOTE_DIR="/home/user/work/project"

bash scripts/remote-sync.sh

ssh "${REMOTE_USER}@${REMOTE_HOST}" "
  cd ${REMOTE_DIR}
  chmod +x test.sh
  ./test.sh
"
```

---

## 5. ログ取得スクリプト

```bash
#!/usr/bin/env bash
set -e

REMOTE_USER="user"
REMOTE_HOST="192.0.2.10"
REMOTE_DIR="/home/user/work/project"

mkdir -p ./logs

rsync -av \
  "${REMOTE_USER}@${REMOTE_HOST}:${REMOTE_DIR}/logs/" \
  ./logs/
```

これで、リモート実行後にログだけローカルへ持ってこれます。

```bash
./scripts/remote-fetch-logs.sh
```

---

## 全体の操作イメージ

普段はこうです。

```bash
# コードを書く

# リモートでビルド
./scripts/remote-build.sh

# リモートで実行
./scripts/remote-run.sh

# リモートでテスト
./scripts/remote-test.sh

# ログ取得
./scripts/remote-fetch-logs.sh
```

もっと楽にするなら、1つにまとめます。

```bash
./scripts/remote-build-run.sh
```

中身はこうです。

```bash
#!/usr/bin/env bash
set -e

bash scripts/remote-sync.sh
bash scripts/remote-build.sh
bash scripts/remote-run.sh
```

ただし、この書き方だと `remote-build.sh` の中で再度同期するので、実際には共通化したほうがよいです。

---

## もう少しきれいな構成

おすすめは、設定を1ファイルにまとめることです。

```text
project/
  scripts/
    remote.env
    remote-sync.sh
    remote-build.sh
    remote-run.sh
    remote-test.sh
    remote-fetch-logs.sh
```

`remote.env`

```bash
REMOTE_USER="user"
REMOTE_HOST="192.0.2.10"
REMOTE_DIR="/home/user/work/project"
```

各スクリプトの先頭で読み込みます。

```bash
source scripts/remote.env
```

例：

```bash
#!/usr/bin/env bash
set -e
source scripts/remote.env

rsync -av --delete \
  --exclude ".git" \
  --exclude "build" \
  --exclude "logs" \
  ./ "${REMOTE_USER}@${REMOTE_HOST}:${REMOTE_DIR}/"
```

こうしておくと、リモートPCが変わったときに `remote.env` だけ変えればよくなります。

---

# Git push方式との違い

## rsync方式

```text
ローカル編集
  ↓
rsync
  ↓
リモートビルド
```

メリット：

```text
・未コミットの変更もすぐ送れる
・試行錯誤が速い
・個人開発に向いている
・毎回commitしなくていい
```

デメリット：

```text
・同期除外設定をちゃんとしないと重い
・リモート側だけ変更するとズレる
```

普段の開発はこちらがおすすめです。

---

## git push方式

```text
ローカル編集
  ↓
git commit
  ↓
git push
  ↓
リモートでgit pull
  ↓
ビルド
```

メリット：

```text
・履歴がきれい
・チーム開発に強い
・CIに近い
・リモート側の状態が再現しやすい
```

デメリット：

```text
・試行錯誤のたびにcommit/pushが必要
・開発中は少し面倒
```

リリース前や安定ビルド確認はこちらがよいです。

---

## おすすめはハイブリッド

普段は `rsync`、区切りがついたら `git push` がよいです。

```text
開発中:
  ローカル編集 → rsync → リモートビルド

区切り:
  git commit → git push

リリース確認:
  リモートでgit pull → クリーンビルド
```

これが一番現実的です。

---

# リモート側の build.sh 例

## .NET / C# の場合

```bash
#!/usr/bin/env bash
set -e

mkdir -p logs artifacts

dotnet restore
dotnet build -c Debug | tee logs/build.log
```

リリースビルドなら：

```bash
#!/usr/bin/env bash
set -e

mkdir -p logs artifacts

dotnet publish ./src/MyApp/MyApp.csproj \
  -c Release \
  -o artifacts/MyApp \
  | tee logs/build.log
```

---

## C++ / CMake の場合

```bash
#!/usr/bin/env bash
set -e

mkdir -p build logs

cmake -S . -B build
cmake --build build -j8 | tee logs/build.log
```

---

## Python / AI系の場合

```bash
#!/usr/bin/env bash
set -e

mkdir -p logs artifacts

python -m compileall src | tee logs/build.log
python -m pytest tests | tee logs/test.log
```

GPU実行なら：

```bash
#!/usr/bin/env bash
set -e

mkdir -p logs

python scripts/check_cuda.py | tee logs/cuda.log
python train.py | tee logs/train.log
```

---

## Unityの場合

Unityは生成物が大きいので、rsyncの除外が重要です。

除外するもの：

```text
Library/
Temp/
Obj/
Build/
Builds/
Logs/
UserSettings/
.vs/
```

`build.sh`

```bash
#!/usr/bin/env bash
set -e

mkdir -p logs Builds

/opt/Unity/Editor/Unity \
  -batchmode \
  -quit \
  -projectPath "$(pwd)" \
  -executeMethod BuildScript.BuildWindows \
  -logFile logs/unity-build.log
```

Windowsリモートなら PowerShell で：

```powershell
& "C:\Program Files\Unity\Hub\Editor\2022.3.XXf1\Editor\Unity.exe" `
  -batchmode `
  -quit `
  -projectPath "C:\work\project" `
  -executeMethod BuildScript.BuildWindows `
  -logFile "C:\work\project\logs\unity-build.log"
```

---

# Windowsローカルの場合

Windowsローカルなら、個人的には **WSLからrsync/sshを使う**のが楽です。

```text
Windows
  - VS Code / Cursor / Visual Studio
  - WSL
    - rsync
    - ssh
    - bash scripts
```

構成はこうです。

```text
C:\Users\you\project
  ↓ VS Codeで編集

WSLから /mnt/c/Users/you/project に移動
  ↓ rsync

リモートPC
  ~/work/project
```

WSL側で：

```bash
cd /mnt/c/Users/you/project
./scripts/remote-build.sh
```

PowerShellだけでやることもできますが、`rsync` が使いにくいので、WSLのほうが安定です。

---

# リモートPCがWindowsの場合

リモートPCがWindowsなら、2パターンあります。

## A. Windows OpenSSHを使う

```text
ローカル WSL
  ↓ ssh / rsync
リモート Windows OpenSSH
```

ただし、Windows側でrsyncを受けるのは少し面倒です。
この場合は `scp` や `robocopy`、Git pull方式のほうが楽なことがあります。

## B. リモートWindowsにWSLを入れる

こちらのほうが扱いやすいです。

```text
ローカル WSL
  ↓ rsync / ssh
リモート Windows + WSL
  ↓
WSL内でビルド
```

ただし、WPFやWinFormsなどWindows GUIアプリをビルド・実行する場合は、Windowsネイティブ側でビルドしたほうがよいです。

---

# 便利なコマンド構成

最終的には、こういうコマンドにしておくと楽です。

```bash
./remote sync
./remote build
./remote run
./remote test
./remote logs
./remote clean
./remote shell
```

`remote` という1つのスクリプトにまとめる例です。

```bash
#!/usr/bin/env bash
set -e
source scripts/remote.env

case "$1" in
  sync)
    rsync -av --delete \
      --exclude ".git" \
      --exclude "build" \
      --exclude "logs" \
      --exclude "artifacts" \
      ./ "${REMOTE_USER}@${REMOTE_HOST}:${REMOTE_DIR}/"
    ;;

  build)
    "$0" sync
    ssh "${REMOTE_USER}@${REMOTE_HOST}" "cd ${REMOTE_DIR} && ./build.sh"
    ;;

  run)
    ssh "${REMOTE_USER}@${REMOTE_HOST}" "cd ${REMOTE_DIR} && ./run.sh"
    ;;

  test)
    "$0" sync
    ssh "${REMOTE_USER}@${REMOTE_HOST}" "cd ${REMOTE_DIR} && ./test.sh"
    ;;

  logs)
    mkdir -p logs
    rsync -av "${REMOTE_USER}@${REMOTE_HOST}:${REMOTE_DIR}/logs/" ./logs/
    ;;

  shell)
    ssh "${REMOTE_USER}@${REMOTE_HOST}" "cd ${REMOTE_DIR} && bash"
    ;;

  clean)
    ssh "${REMOTE_USER}@${REMOTE_HOST}" "cd ${REMOTE_DIR} && rm -rf build logs artifacts"
    ;;

  *)
    echo "Usage: ./remote {sync|build|run|test|logs|shell|clean}"
    exit 1
    ;;
esac
```

使い方：

```bash
./remote build
./remote run
./remote test
./remote logs
./remote shell
```

これはかなりおすすめです。

---

# 構成の完成形

最終的にはこういう形がよいです。

```text
project/
  src/
  assets/
  tests/

  build.sh
  run.sh
  test.sh

  scripts/
    remote.env

  remote
```

`remote.env`

```bash
REMOTE_USER="user"
REMOTE_HOST="192.0.2.10"
REMOTE_DIR="/home/user/work/project"
```

`remote`

```text
sync / build / run / test / logs / shell / clean を実行する入口
```

---

## 運用ルール

大事なのは、**リモート側では基本的にコードを直接編集しない**ことです。

```text
ローカル:
  正のコード置き場

リモート:
  ビルド・実行用のコピー
```

こうしておくと、どちらが正しいコードなのか迷いません。

リモートで直接直したくなった場合は、原則ローカルに戻して直します。

```text
NG:
  リモートで直接コード修正

OK:
  ローカルで修正 → rsync → リモートビルド
```

---

## おすすめ結論

最初はこの構成で十分です。

```text
ローカル:
  VS Code / Cursor / Helix
  Git
  ./remote スクリプト

リモート:
  ビルド環境
  実行環境
  logs/
  artifacts/
```

操作はこれだけにします。

```bash
./remote build
./remote run
./remote test
./remote logs
```

これにしておくと、あとから

```text
・Unityビルド
・C#アプリビルド
・Python AI実験
・Docker実行
・組み込みボードへの書き込み
・CI化
```

にも広げやすいです。
