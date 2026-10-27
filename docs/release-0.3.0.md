# 0.3.0 リリース候補チェックリスト

2026-10-27更新。公開前の作業記録です。ソースの版番号を0.3.0へ更新しましたが、未公開です。
固有値入力・RES/VTK保存間隔は6e8ab53でcommit/push済みです。
メンテナーの再確認によりcommit、push、マージ、タグ作成は引き続きメンテナーが行います。
メンテナーから「RES=2／VTK=5のExport・再読込」「固有値入力4項目の保存・再読込」の両方を確認できたとの回答を受領しました（2026-10-27記録）。
方針は[release-policy.md](release-policy.md)、操作確認は[verification.md](verification.md)、
公開手順は[releasing.md](releasing.md)を参照してください。

**公開前の残作業（2026-10-27）:** [依存監査](dependency-audit-0.3.0.md)の脆弱性3件と
unsound警告3件は解消しました。保守終了警告5件は依存経路と対応方針を記録しています。
Windows ZIPとライセンス同梱をローカルで検証済みです。その他の実機確認・正式候補CI・配布物確認が済むまで正式公開しません。

## 今回の範囲

固定プローブCSV・最大4対象比較、固有値メタデータ・シークバー、未対応設定の欠落防止、
線形/非線形反復の分離、固有値入力4項目、RES/VTK保存ON/OFFと独立した間隔を対象とします。
マニュアルを整備し、同じソースからcrates.ioとWindows ZIPを提供します。

SolidCraft風カメラ操作・パネル折りたたみ・比較全体CSVは今回追加しません。
流体例題はFrontISTRサポート回答待ちを継続し、再解析や解析成功の宣言を行いません。

## 現在の変更を確定

- [ ] 固有値入力・保存間隔の変更と自動テストをレビューする。
- [x] RES=2、VTK=5のExport・再読込を確認する（メンテナー確認報告）。
- [ ] Esc、Undo/Redo、OFF/ON、不正値、固有値時の間隔編集不可を実機確認する。
- [x] 固有値入力4項目の保存・再読込を確認する（メンテナー確認報告）。
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
- [x] Windows配布Actionsを実装する。構文検証済み、GitHubでの実行はcommit/push後。既存Release checksとは別。
- [ ] 固定runner/toolchainでZIP候補を作り、SHA-256とビルド情報を保存する。
- [ ] 開発ツール・oneAPIに依存しない環境でZIP起動、版番号、材料・設定保存、結果表示を確認する。

AllowDirtyでの検証は開発途中の確認専用です。正式候補はcleanなコミットから再検証します。
内部依存を展開アーカイブへ置き換えた検証は、crates.ioインストールやregistry向けdry-runとは別です。

## 統合・版番号・公開

- [ ] リモートを更新し、mainとの差分・祖先関係・未統合コミットを確認する。
- [ ] mainへ統合する。積み重ねた旧枝を重複マージしない。
- [x] 全11クレートの版番号・厳密内部依存・Cargo.lockを0.3.0へ揃える（d75e6fcでコミット済み）。
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

この節は版番号更新前の記録です。更新後の検証は末尾の2026-10-25節を参照してください。

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

## 2026-10-25 公開準備と停止事項

- 6e8ab53のcommit/pushを確認。fetch後も開発枝とoriginが一致し、mainは未統合。
- 全11クレート、厳密内部依存、Cargo.lockを0.3.0へ更新。外部依存は変更していない。
- Windowsで`cargo build -p bevyistr --locked --offline --jobs 2`成功。
  EXEのFileVersion/ProductVersionも0.3.0であることを確認した。配布用releaseビルドとは別。
- `cargo test --workspace --locked --offline --jobs 1`で401成功・0失敗・8スキップ。
  前回と同様、プロセス内TEMP/TMPをtarget配下へ、_LINK_へ/DEBUG:NONEを指定。
- `cargo package --workspace --locked --offline --allow-dirty --no-verify`で全11アーカイブ作成成功。
  0.3.0アーカイブの展開後ビルド、registry向けdry-run、クリーンな最終候補のCIは別途必要。
- mdBookビルド、メタデータの11クレート版番号/内部依存一致、diff空白検査に成功。
- cargo-audit 0.22.2で脆弱性3件・警告8件を検出。内容と経路は[監査記録](dependency-audit-0.3.0.md)。
- メンテナーは保存間隔・固有値設定の実機確認が未実施と回答。公開条件として残す。
- GitHub CLI認証は無効と表示された。自動公開を行う場合は再認証が必要。認証情報は変更していない。
- commit/push/main統合/タグはメンテナーが担当。公開操作は未実施。

今回のコミット案: `chore: prepare 0.3.0 release metadata and record audit blockers`
このコミットだけではリリース完了ではないため、まだv0.3.0タグを付けない。

## 2026-10-26 依存の脆弱性修正

- 基点はd75e6fc。Bevy 0.19.1の機能を維持し、Zstandardを公式のzstd_cへ切り替え。
  wayland-scanner/quick-xmlとanyhow/event-listener/memmap2を修正版へ更新した。
- パッケージ検証で分かったclipboard-winのyanked版参照も5.4.3への更新で解消した。
- 最終Cargo.lockの監査は終了コード0、脆弱性0件、unsound警告0件。
  保守終了警告5件の依存経路と今後の対応は[監査記録](dependency-audit-0.3.0.md)に記載。
- Windows開発ビルド、全11クレートのパッケージ作成、展開ソースからのビルドとUI単独checkが成功。
  最終lockfileでworkspaceテスト401成功・0失敗・8スキップ。TEMP/TMPとMSVC設定は前節と同じ。
- mdBookビルドとdiff空白検査成功。Dependency security workflowを追加したが、GitHub上では未実行。
- 未コミット作業ツリーでの検証であり、Linux CI・GUI・配布ZIPの確認を代替しない。
  保存間隔と固有値入力の実機確認、ライセンス収集など、上記チェックリストの残作業を継続する。

コミット案: `fix: resolve vulnerable dependencies and add security audit CI`

## 2026-10-27 Windows配布準備

- 基点5e50fdd177d1a7211f33a7d3b6c8c3fa054face1と未コミットの配布スクリプト。
  アプリのRustソースとCargo.lockは基点から変更なし。コミット・タグ・公開操作は行っていない。
- メンテナーがRES=2／VTK=5のExport・再読込、および固有値入力4項目の保存・再読込を確認済みと回答。
  この報告を他の未確認操作や実ソルバーの間隔検証まで拡張しない。
- 基点SHAの[Dependency security](https://github.com/michioga/bevyistr/actions/runs/40288819495)と
  [Documentation](https://github.com/michioga/bevyistr/actions/runs/40288819405)の成功を確認。
  同じSHAのWindows/Linux Release checksは未確認。
- Rust 1.99.1、x86_64-pc-windows-msvc、release、locked、CRT静的リンクでビルド成功。
  EXE ProductVersionは0.3.0。直接DLL参照20件にVCRUNTIME/MSVCP/oneAPI/zstd DLLは含まれなかった。
- cargo-about 0.9.2でWindows向け390クレート、153ライセンス本文を収集。
  FiraMono、Bevy shader、同梱nativeコード、Rust標準ライブラリの追加通知も同梱。
- ローカルZIPは36,899,072 bytes、展開後694ファイル。
  SHA-256: `8749a2371d290ac86a3e36df66e52de2dbf120e47a8ca38e8fb138d7f20a5f4d`。
  BUILD-INFO.jsonのsource_dirty=trueであり、正式配布用ではない。
- ZIP全体・収録ファイルのハッシュ、必須ファイル、版番号・内容検査が成功。
  不正なZIPチェックサムと、ZIPハッシュだけ再計算したREADME改変の両方を拒否した。
- 空白を含む展開先、別設定パス、Windows標準フォルダーだけのPATHで起動。
  15秒間プロセスが存続し、試験で起動したプロセスだけを終了した。
  GPU描画・材料Reload・設定保存・結果操作・別PCでの動作を確認したという意味ではない。
- PowerShell構文検証、YAMLパース、actionlint 1.7.12による新workflow検証が成功。
  Windows packageはタグからビルドし、手動指定時だけRelease下書きに添付する。
  正式公開や既存添付の上書きは自動で行わない。
- ローカルZIP・検証方法と正式タグの手順は[windows-package.md](windows-package.md)に記録。
  最終候補はcleanなコミットからCIで再作成し、そのZIPを操作確認する。
