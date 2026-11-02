# Procshelf

Linux 上のコマンドを `config.toml` に登録し、PC・スマホの Web GUI から起動・停止・再起動・強制終了とログ閲覧を行う。ブラウザを閉じても管理サービスと配下のプロセスは動作を継続する。

設計と仕様は [docs/design.md](docs/design.md) にまとめている。初期版は、信頼できる利用者が localhost または VPN 内で使う、フォアグラウンドの非対話型コマンド向け。

## Guix でビルド・起動

開発ツールは [manifest.scm](manifest.scm) で管理する。`scripts/dev` はリポジトリを作業ディレクトリにし、ネットワークを共有する Guix コンテナ内で指定したコマンドを実行する。

```sh
./scripts/dev sh -c 'cd web && npm ci --ignore-scripts && npm run build'
./scripts/dev cargo build --locked --release
cp config.example.toml config.toml
./scripts/dev ./target/release/procshelf --config config.toml
```

`http://127.0.0.1:8787` を開き、設定ファイルと同じ場所の `config.procshelf.key` に保存されたアクセスキーでログインする。サンプルの Clock は自動起動し、Manual task は GUI から起動できる。`Ctrl-C` または `SIGTERM` は配下の停止・回収を待ってサービスを終了する。GUI の「停止」は選択した登録だけを停止する。

生成した GUI は Rust バイナリに埋め込まれるため、利用時に Node.js や別のフロントエンドサーバーは不要。GUI を変更した場合は `npm run build` の後に Rust バイナリも再ビルドする。

コンテナ外のコマンドを管理するときは、ビルド済みバイナリを通常のユーザー環境で実行する。

```sh
./target/release/procshelf --config /absolute/path/to/config.toml
```

開発用の Guix コンテナ内から実行すると、ホームディレクトリ以外のホストのパスは通常見えない。管理するコマンドの実行環境と PATH を意識して起動する。常駐時はユーザーのサービスマネージャー等からこのコマンドを実行し、標準エラーを保存する。systemd は必須ではない。

[channels.scm](channels.scm) は検証に使用した公式 Guix のリビジョンを記録している。同じパッケージ定義で環境を再現する場合:

```sh
guix time-machine -C channels.scm -- shell -m manifest.scm -CWNF -- sh
```

## 設定

```toml
[web]
listen = "127.0.0.1:8787"

[processes.api]
name = "API server"
program = "/absolute/path/to/server"
args = ["--port", "8000"]
cwd = "/home/user/project"
autostart = true
stop_timeout_seconds = 10

[processes.api.env]
APP_ENV = "development"
```

- `api` は登録 ID。英数字・`_`・`-` の 1〜128 文字。ID の変更は削除と新規登録として扱う。
- `name` は省略すると ID。`args` と `env` は省略可能。
- `program` と `cwd` の相対パスは設定ファイルのディレクトリ基準。`program` に暗黙の PATH 検索やシェル展開は行わない。シェルが必要なら `/bin/sh` と引数を明示する。
- `env` は管理サービスから継承した環境に追加・上書きする。環境変数の値を GUI の API へ返さないが、実行したコマンド自身が出力した内容はログに含まれる。
- `autostart = true` はサービス起動時と新規登録時に自動起動する。省略・`false` は手動起動。手動停止後や自然終了後は自動再起動しない。
- 既存登録の `autostart` 変更は次回サービス起動から適用する。実行ファイル・引数・環境・作業ディレクトリ・停止猶予の変更は次回のプロセス起動から適用する。
- 停止はプロセスグループへ `SIGTERM`、猶予後に `SIGKILL`。猶予は既定 10 秒、0〜3600 秒を指定できる。GUI の強制終了は即座に `SIGKILL` を送る。
- 不正な設定や読込失敗では直前の有効な設定と実行を維持し、GUI にエラーを表示する。修正すれば自動で追随を再開する。
- 稼働中の登録を削除してもログと停止操作を残し、配下の回収後に一覧から除外する。

ファイル更新は通知と 500ms ごとの再確認で検出し、置き換え保存にも対応する。設定は最大 1 MiB。Web 待ち受けの変更は管理サービスの再起動で適用する。同じ設定ファイルへの二重起動を防ぐため、設定と同じディレクトリにロックファイルを作成する。ディレクトリには書き込み権限が必要。稼働中にロックファイルを削除しないこと。

## スマホから利用

`listen` に VPN インターフェースの具体的な IP を設定して管理サービスを再起動し、スマホからその IP とポートで開く。

```toml
[web]
listen = "100.64.0.4:8787"
```

デフォルトは localhost 限定。全インターフェースを表す `0.0.0.0` / `::` とポート 0 は受け付けず、bind 失敗時に別アドレスへ切り替えない。VPN 経由でもアクセスキーによるログインが必要。Host・Origin・セッショントークンを検査するため、設定した IP（loopback 時は localhost も可）を直接利用する。リバースプロキシや独自ホスト名は初期版の対象外。

## 認証

初回起動時、設定ファイルの拡張子を `.procshelf.key` に置き換えたファイルへ、ランダムな 256 ビットのアクセスキーを保存する。たとえば `config.toml` なら `config.procshelf.key` となる。キーの保存先は起動ログに表示するが、キー自体はログや API に出力しない。

```sh
cat /absolute/path/to/config.procshelf.key
```

この内容をログイン画面のアクセスキー欄へ入力する。キーファイルは管理サービスと同じユーザーが所有する通常ファイルで、権限は `0600` が必要。読み取れないファイル、不正なキー、広すぎる権限ではサービスの起動を中止する。キーは設定や Git に含めず、必要な端末にだけ共有する。単一の共有キーによる個人利用向けの認証であり、ログインした端末はすべての登録を操作できる。

ログイン状態は HttpOnly・SameSite=Strict の Cookie で保持する。キーをブラウザの localStorage や URL に保存しない。セッションはログインから 12 時間で期限切れとなり、ログアウトするとそのセッションをサーバー側で失効させる。同じ Cookie を使うタブもログイン画面に戻る。管理サービスの再起動で全セッションが失効する。認証失効時は開いていたイベント配信も終了する。

キーを変更する場合は管理サービスを停止し、キーファイルを削除してから起動する。新しいキーが生成され、すべての端末で再ログインが必要になる。サービス停止は管理対象プロセスも停止するため、作業の区切りで実施する。キーのファイル変更はサービス再起動時に読み込む。

未認証では状態・ログ・操作用トークン・イベント配信・操作 API を利用できない。ログイン試行はサービス全体で 1 分あたり 20 回、同時セッションは 64 件まで。ログアウトは管理対象プロセスを停止しない。

通信は localhost または暗号化された VPN 内の HTTP を前提とする。HTTP で利用するため Cookie に Secure 属性は付けず、通信の秘匿性は VPN に依存する。平文の LAN や公開インターネットへ待ち受けを広げない。HTTPS・リバースプロキシへの対応は含まない。

## プロセス所有とログの範囲

実行ごとの監督プロセスが子・孫のプロセスグループを所有する。先頭のコマンドが終了したら残った配下も停止・回収し、再起動はその後に行う。管理サービスが異常終了しても制御パイプの切断を検出して停止し、回収が完了するまで設定のロックを保持する。

`setsid` 等でグループを離脱するプログラムは対象外。サーバーは daemonize を無効にして起動する。監督プロセス自体の強制終了、ホスト障害、シグナルで終了しないカーネル待ち状態までの復旧は保証しない。監督との接続が予期せず失われると「状態不明」として起動を禁止する。

ログは管理サービスのメモリに登録ごと最大 512 KiB / 2000 チャンクで保持する。保持上限と配信詰まりによる省略は表示する。ログのディスク保存・サービス再起動をまたぐ実行履歴・対話端末は初期版に含まれない。

## 開発と検証

```sh
./scripts/check
```

Guix 環境で npm のロック済み依存をインストールし、TypeScript 型検査・Prettier・フロントエンドの仕様テスト・GUI ビルド・Rust の fmt/clippy・単体/統合テスト・実サービスの HTTP/障害テストを実行する。画面は実サービスを起動し、PC とスマホ幅で確認する。

TDD の小さな仕様単位で赤・緑・リファクタリングを進める。状態遷移、設定の置き換え保存、子孫の回収、競合する操作、不正な API 要求を回帰テストの対象とする。

## CI とリリース

[GitHub Actions](https://github.com/ROCKTAKEY/procshelf/actions/workflows/ci.yml) は、PR と `main` の更新時に Ubuntu 26.04 上で Node.js と Rust を用意し、全検査とリリースビルドを実行する。手動実行も可能。ローカル開発環境は [manifest.scm](manifest.scm) で管理する Guix 環境を利用する。

CI は GHA の `parallel` でツールの準備と依存ロックの検証を並列実行する。GUI をビルドした後は、フロントエンド検査、整形・バージョン整合性、Rust・実サービスの検査、リリースビルドを並列実行する。Rust は GUI をバイナリに埋め込むため、GUI の生成完了後にコンパイルする。リリースビルドには別の出力ディレクトリを使い、Cargo のビルドロックによる待ち合わせを避ける。

`main` の検査に成功すると [tagpr](https://github.com/Songmu/tagpr) がリリース PR を作成・更新する。リリース PR を確認してマージすると、マージ後の検査を経て `vX.Y.Z` タグと GitHub Release を公開する。配布はソースコード形式。

リリース番号は `Cargo.toml` の `package.version` で管理し、[scripts/sync-versions.mjs](scripts/sync-versions.mjs) が Cargo のロックファイルと Web パッケージへ同期する。リリース PR で番号を手動変更するときは、変更後に次を実行して生成差分もコミットする。

```sh
./scripts/dev node scripts/sync-versions.mjs
```

tagpr は標準の `GITHUB_TOKEN` を利用する。リポジトリの Settings → Actions → General で「Allow GitHub Actions to create and approve pull requests」を有効にする必要がある。自動作成した PR では通常の PR イベントによる検査が起動しないため、ワークフローがそのブランチの CI を明示的に起動する。PR の自動承認・自動マージは行わない。

Actions の依存は [.github/workflows/actions.lock](.github/workflows/actions.lock) で固定し、CI で検証する。Dependabot が週次で更新 PR を作成する。ワークフローに Action を追加・変更したときは、[公式のロックツール](https://github.com/github/gh-actions-lock) で再生成し、差分を確認する。

```sh
gh extension install github/gh-actions-lock --pin v0.4.1
gh actions-lock
gh actions-lock --verify
```

## 依存の選定と更新

- Rust: Tokio（非同期 I/O・プロセス）、Axum（HTTP・SSE）、Serde/serde_json/TOML（境界での解析）、notify（ファイル監視）、nix（Linux 操作）、tokio-stream/futures-util（配信）、subtle（アクセスキーの定数時間比較）。
- GUI: TypeScript + Preact（画面と状態管理）、Zod（API 応答の検証）、Vite（ビルド）、Prettier（整形）。テストには Node.js 標準のテストランナーを利用する。
- 開発環境: Guix。アプリケーション依存: `Cargo.lock` と `web/package-lock.json`。

依存数の最小化を目的にせず、自前実装の複雑さも評価する。用途が重複する依存や不要な機能を避け、更新は変更内容・推移的依存・脆弱性情報と関連テストで確認する。

```sh
./scripts/dev cargo tree --locked
./scripts/dev cargo audit
./scripts/dev sh -c 'cd web && npm ls --all && npm audit'
```

`cargo audit` と `npm audit` は既知の脆弱性情報の確認であり、供給元の侵害や未知の問題がないことを保証するものではない。npm のインストールでは lifecycle scripts を実行しない構成を検証している。Rust の build scripts / proc macros はビルド時に実行されるため、依存更新時の確認対象に含める。

## ライセンス

Procshelf は GNU Affero General Public License version 3（AGPL-3.0-only）で提供します。全文は [LICENSE](LICENSE) を参照してください。
