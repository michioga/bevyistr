# 0.2.0 公開準備（メンテナー向け）

コミット・push・マージ・タグ作成・crates.io公開はメンテナーが行います。
`publish_crates_io`で準備し、mainへマージするリリースを0.2.0とします。
この文書やCargoのバージョン変更だけでは、公開済みを意味しません。

## 構成と順序

ワークスペース構成とRustのライブラリ名は維持します。公開パッケージ名は
`bevyistr`および`bevyistr-*`にし、内部依存は同一バージョンを厳密指定します。
バージョン更新時はルートCargo.tomlのworkspace.packageとworkspace.dependenciesを
一緒に更新してください。タイトル・ヘッダー・Windowsファイル情報はCargoの番号を使います。

依存先から公開する順序（全て0.2.0）:

1. bevyistr-fem-core
2. bevyistr-interaction
3. bevyistr-camera
4. bevyistr-selection
5. bevyistr-box-select
6. bevyistr-hecmw
7. bevyistr-gmsh
8. bevyistr-picking
9. bevyistr-visualization
10. bevyistr-ui
11. bevyistr

公開名の利用可否・所有者は実際の公開前にcrates.ioで確認します。
0.2.0のAPIは発展途上であり、内部クレートを汎用の安定APIとしては保証しません。

## 同梱ファイル

- `assets/bevyistr.png`: ユーザー提供のアイコン原本。
- `app/assets/bevyistr.png`: crates.ioパッケージ内の同一コピー。
  原本を更新したらこちらも同期します。起動時に読み込む外部画像ではありません。
- WindowsのICOはビルド時に16/24/32/48/64/128/256pxで生成し、実行ファイルに埋め込みます。
  元画像の背景・余白は除去しません。ウィンドウとタスクバーにも埋め込みPNGを設定します。
- Windows MSVCはWindows SDKの`rc.exe`を自動探索します。`RC`で明示できます。
  Windows GNUは`windres`（`WINDRES`で指定可）が必要です。
  LinuxターゲットではWindows用のリソース処理は実行しません。
  Linuxデスクトップメニューへの登録やWaylandのアイコンテーマ配布は別途必要です。
- `materials.toml`: 外部編集用標準材料。
- `ui/assets/materials.toml`: 外部ファイルがない場合の内蔵版。標準値の更新時には同期します。
- 全11パッケージにREADME・MIT LICENSE・必要なテスト用fixtureを同梱します。
  FrontISTR/MPI/Gmsh本体やチュートリアルの解析結果は同梱しません。

## 検証

PowerShell（Windowsまたはpwshを導入したLinux）:

```powershell
./scripts/check-release.ps1 -Offline -AllowDirty
```

メタデータと同梱コピーの整合性を検査し、`cargo package --workspace`で全クレートの
パッケージ作成・展開後ビルドを検証します。公開はしません。
`-AllowDirty`は開発途中の検査用です。公開前にはコミット済みの状態で外します。
Offlineはキャッシュのみを使います。通常のネット接続でも検証してください。
ワークスペース一括パッケージ検証に対応したCargoを使用します。

```text
cargo test --workspace --locked
cargo build --package bevyistr --release --locked
mdbook build docs
```

WindowsとLinuxでビルド・起動、アイコン、材料の外部上書きとReload、Resultsの
PVTU/RES読み込み・フィールド選択・全フレーム再生・プローブ履歴を確認します。
Windows ExplorerのアイコンはOSキャッシュにより更新が遅れる場合があります。

## 公開前のゲート

- 全パッケージ名の空き／所有権確認、ソースとアイコンの公開権限確認。
- 依存ライブラリのライセンス・yank・脆弱性の確認。
- Windows/Linuxの検証、mainへのマージと最終版番号の一致。
- ネット接続可能な環境で`cargo publish --workspace --dry-run --locked`を実行。
  単体dry-runが未公開の内部依存で止まる場合、workspace全体をpackageで検証します。
- コミット・タグ・アップロードはメンテナーが明示的に実行。
  個別公開の場合は上記順序で、各依存の登録を確認してから次へ進みます。
  APIトークンをソースやチャットへ貼り付けないでください。
- 公開後にクリーンな環境で`cargo install bevyistr --version 0.2.0 --locked`を検証。

### この環境での検証上の問題

2026-09-19時点ではネット接続がSchannelの`SEC_E_NO_CREDENTIALS`で失敗し、
公開名の確認は未完了です。証明書検証を無効にして回避しないでください。
Cargo 1.98.1ではオフラインのworkspaceパッケージ作成は完了しましたが、展開後検証で
一時レジストリの`no hash listed for bevyistr-fem-core v0.2.0`というCargo内部エラーが
発生しました。`--no-verify`での作成成功だけを、公開可能の根拠にしないでください。
ネット接続・Cargo検証環境を確認し、通常のpackage/dry-runが通ってから公開します。

参考: [Cargo publishing](https://doc.rust-lang.org/cargo/reference/publishing.html)、
[cargo package](https://doc.rust-lang.org/cargo/commands/cargo-package.html)。
