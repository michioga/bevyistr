# リリース管理方針（0.3.0以降）

2026-10-31更新。Windows配布用Actionsとローカル検証スクリプトを使用し、0.3.0を公開しました。
実行手順は[Windows ZIP](windows-package.md)を参照してください。
コミット、push、マージ、タグ作成、crates.io公開、GitHub Release公開はメンテナーが行います。

0.3.0の具体的な残作業・検証記録は[リリース候補チェックリスト](release-0.3.0.md)で管理します。

## タグとブランチの役割

- 公開版は注釈付きタグ `v0.3.0` のように固定する。公開後はタグを移動・再利用しない。
- `main` は統合・次期開発の基点。`feat/*` は作業単位で使う。
- `release/0.3.0` は必要なら公開準備用に一時的に作り、mainへの統合確認後に整理する。
- `maintenance/0.3` は0.4開発中に0.3系へ修正を提供する必要が生じた場合だけ作る。
  旧版を保存するためだけのブランチは不要。修正版は新しいタグ `v0.3.1` にする。
- Cargo.tomlの版番号・内部依存、Cargo.lock、画面表示、タグ、Release名、ZIP名を一致させる。
  タグからビルド時に版番号を書き換えない。変更はタグを付ける前にコミットする。

## 同じソースからの公開

1. 0.3.0の対象機能・既知の制限を確定し、必要な変更をmainへ統合する。
2. 全11クレートの版番号と厳密内部依存、lockfile、リリースノートを更新する。
3. Windows/Linuxのテスト、パッケージ展開後ビルド、依存ライセンス・脆弱性確認を通す。
   既存の `Release checks` は検証専用で、バイナリの配布ワークフローではない。
4. 候補コミットからWindows配布ZIPを作り、実機で動作確認する。修正したら再検証する。
5. 検証済みのmain上のコミットに `v0.3.0` を付ける。
   タグのコミットと検証したSHAが一致することを確認する。
6. そのコミットから依存順に各クレートのdry-runとcrates.io公開を行う。
   内部依存が未公開の状態では後続クレートのレジストリ向けdry-runは完了できないため、
   事前のローカルアーカイブ検証と区別する。依存先のレジストリ反映を確認してから次へ進む。
7. 同じタグをcheckoutするActionsでWindowsバイナリを作成し、GitHub Releaseの下書きへ添付する。
   ZIPの動作、チェックサム、全クレートの公開を確認後にReleaseを正式公開する。
8. 公開ページ、crates.ioからのインストール、ZIPからの起動、バージョンとアイコンを確認する。

公開済みクレートの同じバージョンは上書きできない。一部だけ公開された場合、
成功済みの名前・版番号・ソースSHAを記録し、未公開分だけ再開する。
ソース修正が必要になった場合は版番号・依存関係を再検討し、公開済み版を差し替えない。
タグpushだけでcrates.ioへ自動公開する運用は、今回は採用しない。

## Windowsバイナリ配布の設計

初回はインストーラーではなく `bevyistr-v0.3.0-windows-x86_64.zip` を推奨する。

- 対象: `x86_64-pc-windows-msvc`。`cargo build --release --locked -p bevyistr --target ...`。
- 同梱: `bevyistr.exe`、`materials.toml`、MIT LICENSE、簡潔な起動説明、必要な第三者ライセンス。
- 同梱しない: ユーザーの `bevyistr.toml`、個人パス、解析データ、FrontISTR/MPI/Gmsh本体。
- 実行ファイルのアイコンは埋め込み済み。作業ディレクトリに依存した素材参照がないか確認する。
  外部materials.tomlの探索場所・編集方法も、実装に合わせて起動説明に記す。
- ZIPのSHA-256、ソースSHA、ターゲット、Rustバージョンをビルド記録へ残す。
- runnerは`windows-2025`、Rustは`1.99.1`を指定する。runner内の更新はあるため実image番号も記録する。
- 配布前にDLL依存・VC++ランタイム要否を調べ、開発ツールやoneAPIのPATHに依存せず起動するか検証する。
- 空白を含む展開先、通常ユーザー、設定保存、材料読込、結果表示を確認する。
  FrontISTR未導入でも結果ビューアとして起動でき、Solveでは未設定を明示すること。
- 未署名配布ではSmartScreen等の警告が出る可能性がある。チェックサムはコード署名の代わりではない。
  署名証明書の取得や警告回避のためのセキュリティ設定変更はこの作業に含めない。

Actionsは「ビルド・検証」と「Releaseへの添付」を分ける。
通常のPRビルドはread-only、Release添付ジョブだけ `contents: write` を与える。
タグpushまたは明示的な手動実行を入口とし、任意ブランチのHEADを公開しない。
タグ名とCargoの版番号の不一致は失敗にする。crates.ioの認証情報をバイナリ作成へ渡さない。
実装は`.github/workflows/windows-package.yml`。タグpushはビルドのみ、手動のattach_draft指定で下書きへ添付する。

## ブランチ整理

公開前に全ブランチの先端・祖先関係・未統合コミットを確認する。
積み重ねたブランチでは、最新の先端に古い枝の変更が含まれている場合、重ねてマージしない。
mainへの包含を確認した作業枝だけ、公開後にローカル・リモートを整理する。
未統合コミットを持つ枝は残す。タグと必要な保守ブランチは削除しない。
Pagesの公開元とenvironmentの許可ブランチもmainへ移行して動作確認してから古い枝を整理する。

## 参照

- [Cargo: Publishing on crates.io](https://doc.rust-lang.org/cargo/reference/publishing.html)
- [GitHub: About releases](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases)
- [このプロジェクトの公開手順・0.2.0記録](releasing.md)
