# 0.3.0 リリース候補チェックリスト

2026-10-24。公開前の作業記録です。現在の版番号は0.2.0です。
固有値入力・RES/VTK保存間隔は未コミットです。commit、push、マージ、タグ作成、公開はメンテナーが行います。
方針は[release-policy.md](release-policy.md)、操作確認は[verification.md](verification.md)、
公開手順は[releasing.md](releasing.md)を参照してください。

## 今回の範囲

固定プローブCSV・最大4対象比較、固有値メタデータ・シークバー、未対応設定の欠落防止、
線形/非線形反復の分離、固有値入力4項目、RES/VTK保存ON/OFFと独立した間隔を対象とします。
マニュアルを整備し、同じソースからcrates.ioとWindows ZIPを提供します。

SolidCraft風カメラ操作・パネル折りたたみ・比較全体CSVは今回追加しません。
流体例題はFrontISTRサポート回答待ちを継続し、再解析や解析成功の宣言を行いません。

## 現在の変更を確定

- [ ] 固有値入力・保存間隔の変更と自動テストをレビューする。
- [ ] 対応する新規モデルでRES=2、VTK=5を確定し、ExportしたCNTと再読込値を照合する。
- [ ] Esc、Undo/Redo、OFF/ON、不正値、固有値時の間隔編集不可を実機確認する。
- [ ] 固有値入力を5、1e-8、80、0.01にし、CNTと再読込で一致することを確認する。
- [ ] 非流体の検証用コピーで実ソルバーの保存間隔とRES OFF後の自動読込抑止を確認する。
- [ ] 確認した変更をcommit/pushし、候補SHAを記録する。

提供されたSolve画像は配置を示しますが、RES/VTKはいずれも1です。
またSetup incompleteによりExport/Runは停止対象です。2/5の保存・解析実行の証拠とは扱いません。
未対応設定を削除して検査を回避せず、対応する新規モデルまたは検証用コピーを使用します。

## 配布候補を検証

- [ ] Windowsのworkspaceテスト、展開アーカイブからのビルド・テストを完了する。
- [ ] 同じSHAでLinuxのRelease checksを完了する。Linux GUI実機確認とは区別する。
- [ ] スキップした実データ・実機テストを必要な手動確認と突き合わせる。
- [ ] mdBookをビルドし、新設定と既知の制限の説明を確認する。
- [ ] 依存ライセンス・脆弱性を確認し、ZIP用の第三者ライセンスを用意する。
- [ ] Windows配布Actionsを実装する。既存Release checksはZIP作成用ではない。
- [ ] 固定runner/toolchainでZIP候補を作り、SHA-256とビルド情報を保存する。
- [ ] 開発ツール・oneAPIに依存しない環境でZIP起動、版番号、材料・設定保存、結果表示を確認する。

AllowDirtyでの検証は開発途中の確認専用です。正式候補はcleanなコミットから再検証します。
内部依存を展開アーカイブへ置き換えた検証は、crates.ioインストールやregistry向けdry-runとは別です。

## 統合・版番号・公開

- [ ] リモートを更新し、mainとの差分・祖先関係・未統合コミットを確認する。
- [ ] mainへ統合する。積み重ねた旧枝を重複マージしない。
- [ ] 全11クレートの版番号・厳密内部依存・Cargo.lockを0.3.0へ揃える。
- [ ] README・マニュアル・リリースノートを更新する。公開前は公開済みと書かない。
- [ ] 版番号更新後の同一SHAでCI、パッケージ検証、Windows ZIP実機確認を再実施する。
- [ ] 検証済みSHAに注釈付きタグv0.3.0を付ける。公開後は移動しない。
- [ ] 依存順にdry-runとcrates.io公開を行い、各クレートの反映を確認する。
- [ ] 同じタグのZIPとSHA-256をRelease下書きに添付し、確認後に公開する。
- [ ] 公開版インストール・ZIP起動・Pages内容を確認する。
- [ ] Pages公開元とenvironment許可をmainへ移行し、包含確認済みの旧作業枝だけ整理する。

公開順: fem-core → interaction → camera → selection → box-select → hecmw → gmsh
→ picking → visualization → ui → bevyistr。
途中で止まったら成功済みの名前・版番号・SHAを記録し、未公開分だけ再開します。

## 2026-10-24 検証記録

- 基点はf094705と未コミット変更。版番号0.2.0。
- 11クレートのメタデータ・LICENSE・埋め込み素材チェックと.crate作成は成功。
- Windows、Rust 1.99.1で`cargo test --workspace --lib --jobs 1 --locked --offline`成功。
  合計398成功、0失敗、6スキップ。box-select 3、camera 5、fem-core 23、gmsh 8、hecmw 100、
  interaction 1、picking 4、selection 8、ui 213、visualization 33。
  TEMP/TMPはプロセス内で`target/compat-test-temp`へ指定した。
- スキップはHECMWの実チュートリアル依存5件とUIの実機MPI依存1件。
  流体例題を含む実データテストと実ソルバーはこの区切りでは実行していない。
- `mdbook build docs`と`git diff --check`成功。
- `scripts/check-release.ps1 -Offline -AllowDirty -ArchiveBuild`で展開アーカイブからのアプリビルド成功。
  続く全workspaceテストのexample inspectリンクがMSVC LNK1140（PDB容量制限）で停止した。
  同じ展開済みCargo.tomlに対して、プロセス内の`_LINK_`へ`/DEBUG:NONE`を追加し、
  `cargo test --workspace --jobs 1 --locked --offline --manifest-path ... --target-dir target`
  を再実行して401成功・0失敗・8スキップ。通常の398件にappの3件を加えたもの。
  追加のスキップ2件は実チュートリアル依存のintegration test。例を検査対象から除外していない。
  同じ展開先のUI単独`cargo check -p bevyistr-ui --locked --offline`も成功。
  標準設定でのスクリプト一括成功とは区別する。
- 正式候補SHAでのWindows/Linux CI、配布ZIP、依存監査、registry向けdry-runは未確認。
- ローカル参照はmainと開発枝の2本。fetchしていないためリモート整理完了とは断定しない。

公開直前には版番号更新後の候補SHA、OS、Rust版、コマンド、成否、スキップ理由、CIリンクを記録します。

リンカー設定の参考: [MSVC LINK環境変数](https://learn.microsoft.com/en-us/cpp/build/reference/linking?view=msvc-170)、
[/DEBUG設定](https://learn.microsoft.com/en-us/cpp/build/reference/debug-generate-debug-info?view=msvc-170)。
ローカルの永続環境変数・Cargoプロファイル・公開ソースはこの対処では変更していません。
既存CIはDEV/TESTのdebug情報を無効化していますが、今回の候補SHAでのCI実行確認は別途必要です。
