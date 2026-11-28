# v0.6.0 開発計画

## 解析入力と実行

- FrontISTR 6.0の接触設定、複数の`!STEP`と`GRPID`参照、弾塑性材料、STEPから接触解析へ進むワークフローを進める。`.cnt`、`.msh`、`hecmw_ctrl.dat`、`hecmw_part_ctrl.dat`は[公式仕様](https://source-docs.frontistr.com/)と対象版の例題で照合し、保存と再読込を確認する。
- 流体の基準例題は`18_cavity_flow`とする。WindowsのIntel oneAPI MPIで計算済みの8ランク・101フレームを使い、PVTUとMPI RESの全フレームを自動テストで確認し、利用者からも結果表示の確認を得た。流体用四面体`3414`はメッシュ単体の書き出しで要素番号を保持する。[検証記録](advanced-results-verification.md)
- 流体の入力側は`!SOLVER`の`BiCGSTAB`と`SCALING`の読み込み・編集・再出力に対応した。[非圧縮ニュートン流体の粘度・密度](src/materials.md#非圧縮ニュートン流体)の読込・編集と材料カード単体の往復も確認した。`!DYNAMIC`は陰解法Newmark時刻歴の6行を保持し、時間積分の4項目をSolveで編集できる。固定増分の動解析`!STEP`を1件読み込み、`DTIME`／`ETIME`、反復設定、拘束・荷重の`GRPID`と有効化順を保持する。例題では`BOUNDARY, 1`と`BOUNDARY, 2`を読み込み、Solveで確認できる。流体用MSHは要素 `3414` のSOLID断面と同名材料のゼロ3項目を保持し、例題のMSH単体で再出力・再読込を確認した。例題からMPI用の `.msh`、`.cnt`、2個の `.dat` を生成し、再読込時に設定が一致することを確認した。Windows MPIで再出力した入力の分割・実行・結果表示を確認するテストを追加した。再出力した入力でのWindows MPI実行を利用者の環境で確認した。
- 非線形静解析の接触設定に `!CONTACT_ALGO` の `ALAGRANGE`／`SLAGRANGE` と、`ALAGRANGE` の `AUGITER` を追加した。Solveから変更できる。Hertz接触例題（408節点、168要素）で `ALAGRANGE, AUGITER=4` と `SLAGRANGE` の読み込み・一式の書き出し・再読込を確認した。再配置情報を含むWindowsの検証用OpenMP版で実行し、両方式とも最終サブステップのRESを読めた。 [検証記録](verification.md#060向け-hertz接触例題とwindowsエラー623) · [FrontISTRの仕様](https://source-docs.frontistr.com/keyword_reference/cnt/contact_algo.html)
- 非線形静解析の固定増分 `!STEP` を複数保持し、Solveでステップごとの時間設定と有効な拘束・荷重・接触グループを確認できる。Hertz例題を2ステップに分け、書き出し・再読込・WindowsでのFrontISTR実行とRES読込を確認した。 [検証記録](verification.md#060向け-hertz接触例題とwindowsエラー623) · [FrontISTRの `!STEP` 仕様](https://source-docs.frontistr.com/keyword_reference/cnt/step.html)
- 非線形静解析の温度非依存のvon Mises弾塑性は、二直線・多直線硬化をCNTに再出力し、Materialsで値を確認できる。円柱例題と二直線硬化に変更した入力の両方で、元のCNTと再出力後の最終RESを節点単位で比較した。 [操作手順](src/materials.md#von-misesの弾塑性) · [検証記録](verification.md#060向け-円柱例題のvon-mises弾塑性)
- MPI設定でRCB／PMETIS／KMETISを選択できる。RCBでは2の累乗のランク数と分割軸を指定し、`hecmw_part_ctrl.dat` に再出力する。`18_cavity_flow` は8ランクのKMETISを使用する。[公式仕様](https://source-docs.frontistr.com/keyword_reference/part/partition.html)
- 次版へ回す候補はAMG／DIRECTmfの詳細設定、複数ソリッドSTEPの一括メッシュ化、高度な材料・連成・周波数応答。

## Resultsの任意断面

- Resultsで断面を1枚設定できる。X／Y／Zプリセット、位置のスライダーと数値欄、方位角と仰角、外形のメッシュ線に対応する。[操作手順](src/results/contours.md#一次四面体の内部を断面で見る)
- 一次四面体の体積結果を対象とし、変形前の座標で断面と交わる節点値を補間する。速度の大きさは交点の補間ベクトルから求める。未対応の体積要素や節点値のないフィールドは状態を表示する。
- 実例題の最終フレームの圧力断面をPVTUとMPI RESで比較した。中央断面の面積は両方とも1.00000000、斜め断面は1.34638188と1.34638187。線形分布の補間と、重複セル・共有面の描画を自動テストで確認する。
- 利用者の環境で、MPI RESの59／101フレーム目を開き、Z断面の圧力と外形メッシュ線を表示できることを確認した。

## WindowsのMSI配布

- v0.6.0から、GitHub ReleaseのWindows x86_64バイナリを`bevyistr-v0.6.0-windows-x86_64.msi`と`SHA256SUMS.txt`で配布する。既存のWindowsパッケージ作成を利用し、タグ・Cargo・EXE・MSIの版番号、ライセンス、DLL依存、ビルド情報の確認をMSIに引き継ぐ。
- WiXでユーザー単位のインストーラーを作る。スタートメニューにショートカットを登録し、後続版で同じ製品を更新できるよう製品の識別子を固定する。設定と編集用の材料ファイルはインストール先と別のユーザー用フォルダーに置く。FrontISTR、Intel oneAPI／MPI、Gmshは既存の外部ツール設定を使う。 [作成と検証](windows-package.md) ·[WiXのPackage仕様](https://docs.firegiant.com/wix/schema/wxs/package/) · [Windows Installerのインストール範囲](https://learn.microsoft.com/en-us/windows/win32/msi/installation-context)
- v0.5.0の公開済みバイナリを使ったMSIで、721ファイル、版番号、SHA-256、インストール先とショートカットの設定を、インストールせずに展開して照合した。
- 利用者のWindows環境で、「インストールされているアプリ」のbevyistrアイコン、日本語のMITライセンスウィザード、デスクトップショートカットの作成を確認した。
- リリース候補では、開発環境やoneAPIを入れていないWindowsでインストール、スタートメニューからの起動、メッシュと結果の読込、後続バージョンへの更新、アンインストールを確認する。v0.5.0のZIP利用者向けには、既存の設定を引き継ぐ手順を記載する。MSI検証後に、注釈付きタグを基準としてGitHub Release、crates.io、マニュアルの公開を進める。
