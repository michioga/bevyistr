# 0.5.0 公開チェックリスト

## 公開候補の検証

- [x] 全11クレート、内部依存、`Cargo.lock`、README、Windows配布文書、mdBookを0.5.0にそろえる。 [GitHub Release用ノート](../distribution/release-notes-0.5.0.md)
- [x] Windowsの通常テストと、配布用アーカイブを展開したワークスペースのテストはそれぞれ484件成功、18件スキップ。展開したアプリとUI単体のビルド検証に成功した。全11件の配布ロックファイルが `cargo audit --deny unsound` を通過した。アプリのロックファイルではメンテナンス終了の警告4件が残る（RustSec DB `52384ab`）。
- [x] Gmsh 4.13.1の実機テスト4件が成功した。STEPからのメッシュ生成、複数ソリッドの拒否、穴・接触予定面の局所細分化、面グループと拘束・荷重のFrontISTR入力への引き継ぎを確認した。
- [ ] Windowsの通常PowerShellでFrontISTRのDirect、MPI、行列ダンプ後の通常解析と結果表示、および公式ヒンジ例題のMPIを確認する。Codexの実行環境では`fistr1.exe`と`hecmw_part1.exe`の単体起動でもWindowsエラー623が出たため、解析開始前に止まった。再確認の手順は下記を参照。
- [x] mdBook 0.5.4でビルドし、0.5.0の表記とSTEP手順へのリンクを確認した。
- [x] crates.ioを参照した `bevyistr-fem-core` 0.5.0単体の `cargo publish --dry-run --locked` が成功した。一括dry-runはCargo 1.99.1の `no hash listed for bevyistr-fem-core v0.5.0` で停止したため、全11クレートは展開ビルドで検証した。 [パッケージ検証の手順](releasing.md) · [Cargo #14396](https://github.com/rust-lang/cargo/issues/14396)
- [ ] GitHub ActionsでWindows／Linuxの **Release checks**、依存監査、候補Windows ZIPの作成を確認する。

### FrontISTRを通常PowerShellで再確認する

リポジトリのルートで実行します。Intel MPIのパスと例題ディレクトリは、環境に合わせて選んでください。

```powershell
$env:FRONTISTR_MPI_LAUNCHER = 'C:\Program Files (x86)\Intel\oneAPI\2026.1\bin\mpiexec.exe'
$env:BEVYISTR_FRONTISTR_SMOKE_INPUT = 'D:\Work\FrontISTR\tutorial\02_elastic_hinge_parallel'
cargo test -p bevyistr-ui --lib --locked installed_frontistr -- --ignored --nocapture --test-threads=1
```

`fistr1`と`hecmw_part1`は`PATH`から検出されます。別のインストールを試す場合は`FRONTISTR_EXECUTABLE`と`FRONTISTR_PARTITIONER`に実行ファイルのパスを指定できます。

## 公開

- [ ] mainへ統合し、公開対象のコミットに注釈付きの `v0.5.0` を付ける。
- [ ] crates.ioへ依存順に公開し、クリーンな環境で `cargo install bevyistr --version 0.5.0 --locked` を確認する。
- [ ] タグとWindows ZIPのソースSHAを照合し、チェックサム、起動、版番号、アイコンを確認してGitHub Releaseを公開する。
- [ ] mainからDocumentationを `publish=true` で実行し、GitHub Pagesで0.5.0のマニュアルを確認する。 [公開マニュアル](https://michioga.github.io/bevyistr/)
