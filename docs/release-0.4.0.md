# 0.4.0 公開チェックリスト

## 公開候補の確定

- [x] Windowsで弾性棒のDirect／2ランクMPIを実行し、変位を照合した。4ランクMPIのヒンジでStop後の再実行とResultsの自動表示を確認した。
- [x] 2部品のグループ・接触・MPCをExportして再読込した。部品移動後の接触候補の再検出を自動テストで確認した。
- [x] 開発用Windows ZIPを作成し、チェックサム、同梱682ファイル、EXEのバージョン情報、展開後の起動を確認した。2026-11-17時点の作業ツリーから作成したため、表示は0.3.0、ビルド記録は`source_dirty=true`。正式な0.4.0のZIPは検証済みタグから作成する。
- [x] 開発用Windows ZIPを使用し、再起動後もTouchpad (Space)の選択が復元されることを確認した。
- [ ] 接触候補のハイライト更新とタッチパッドでの修飾ドラッグを実機で確認する。 [確認手順](verification.md)
- [ ] Resultsの連番再生とModelの選択を実機で試し、フレーム時間と連続再生前後のメモリを記録する。 [これまでの計測](performance.md)
- [x] 全11クレートの版番号と内部依存、`Cargo.lock`、README、Windows ZIPのREADME、mdBookの表記を0.4.0にそろえた。 [GitHub Releaseノート案](../distribution/release-notes-0.4.0.md)
- [x] Windowsで0.4.0候補のライブラリテスト429件が成功した。外部例題・手動計測などの11件は通常実行でスキップ。mdBookのビルドも成功した。
- [x] 版番号更新後にローカル候補ZIPを再作成した。ZIPのチェックサム、同梱682ファイル、EXEのバージョン情報`0.4.0`を照合し、展開後の起動を短時間確認した。ビルド記録は`source_dirty=true`。候補ZIPのSHA-256は`2c04a23f93f991434924be277708eab6875037e3b4d200bb56aed63f612057a6`。

## 検証から公開まで

1. 候補コミットをpushし、同じSHAで **Release checks** のWindows／Ubuntuジョブと **Dependency security** を通す。**Windows package** を `release_tag` 空欄、`attach_draft=false` で実行して候補ZIPを確認する。
2. メンテナーがmainへ統合する。mainの公開対象SHAと検証済みのSHAを照合し、ソースが変わった場合はそのSHAで再検証する。
3. 公開対象のコミットに注釈付きの `v0.4.0` を付けてpushする。タグpushでWindows ZIPのビルドが始まる。
4. タグのソースから依存順に、各クレートの `cargo publish -p <package> --dry-run --locked` と `cargo publish -p <package> --locked` を実行する。依存先の版がレジストリに反映されてから次へ進む。
5. **Windows package** を `release_tag=v0.4.0`、`attach_draft=true` で実行する。下書きに添付されたZIPを展開し、チェックサム、`BUILD-INFO.json`のSHA、起動・版番号・アイコンを確認してからメンテナーがReleaseを公開する。
6. **Documentation** をmainから `publish=true` で実行し、Pagesで0.4.0のマニュアルと画像を確認する。crates.ioからのインストール確認後、mainに取り込み済みの作業ブランチを整理する。

## crates.ioの公開順

1. `bevyistr-fem-core`
2. `bevyistr-interaction`
3. `bevyistr-camera`
4. `bevyistr-selection`
5. `bevyistr-box-select`
6. `bevyistr-hecmw`
7. `bevyistr-gmsh`
8. `bevyistr-picking`
9. `bevyistr-visualization`
10. `bevyistr-ui`
11. `bevyistr`

公開順はCargoのワークスペース依存に従う。公開時に全クレートの版番号とソースSHAを記録する。 [リリース管理方針](release-policy.md) · [Windows ZIPの手順](windows-package.md)
