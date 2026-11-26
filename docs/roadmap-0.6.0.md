# v0.6.0 開発計画

## 解析入力と実行

- FrontISTR 6.0の接触設定、複数の`!STEP`と`GRPID`参照、弾塑性材料、STEPから接触解析へ進むワークフローを進める。`.cnt`、`.msh`、`hecmw_ctrl.dat`、`hecmw_part_ctrl.dat`は[公式仕様](https://source-docs.frontistr.com/)と対象版の例題で照合し、保存と再読込を確認する。
- 流体の基準例題は`18_cavity_flow`とする。WindowsのIntel oneAPI MPIで計算済みの8ランク・101フレームを使い、PVTUとMPI RESの全フレームを自動テストで確認し、利用者からも結果表示の確認を得た。流体用四面体`3414`はメッシュ単体の書き出しで要素番号を保持する。[検証記録](advanced-results-verification.md)
- 流体の入力側は`!SOLVER`の`BiCGSTAB`と`SCALING`の読み込み・編集・再出力に対応した。[非圧縮ニュートン流体の粘度・密度](src/materials.md#非圧縮ニュートン流体)の読込・編集と材料カード単体の往復も確認した。`!DYNAMIC`は陰解法Newmark時刻歴の6行を保持し、時間積分の4項目をSolveで編集できる。固定増分の動解析`!STEP`を1件読み込み、`DTIME`／`ETIME`、反復設定、拘束・荷重の`GRPID`と有効化順を保持する。例題では`BOUNDARY, 1`と`BOUNDARY, 2`を読み込み、Solveで確認できる。流体用MSHの材料・断面に対応した後、例題を別の出力先に書き出し、Windows MPIで分割・実行・結果表示まで確認する。
- MPI設定でRCB／PMETIS／KMETISを選択できる。RCBでは2の累乗のランク数と分割軸を指定し、`hecmw_part_ctrl.dat` に再出力する。`18_cavity_flow` は8ランクのKMETISを使用する。[公式仕様](https://source-docs.frontistr.com/keyword_reference/part/partition.html)
- 次版へ回す候補はAMG／DIRECTmfの詳細設定、複数ソリッドSTEPの一括メッシュ化、高度な材料・連成・周波数応答。

## Resultsの任意断面

- `Results`で断面を1枚設定し、X／Y／Zの軸プリセットと任意方向を選べるようにする。断面をドラッグで移動でき、位置と法線は数値でも指定できるようにする。断面を有効にした状態で元の外形を参照できるようにする。
- 最初は一次四面体の体積結果を対象とする。平面と要素の交差から断面ポリゴンを作り、節点の圧力と速度を補間して、断面そのものをコンター表示する。速度の大きさは補間した速度ベクトルから求める。元の結果と解析モデルは編集しない。
- `18_cavity_flow`のPVTUとMSH付きMPI RESで、中央断面と斜め断面を確認する。同じ断面で圧力・速度の大きさ・速度成分を切り替え、シークしても位置・向き・色範囲の選択が保たれることを確認する。分割境界で隙間や重複描画が出ないこと、一定値と線形分布を断面上で再現できることを検証する。外表面だけのデータでは、断面の補間に体積要素が必要であることを表示する。

## WindowsのMSI配布

- v0.6.0から、GitHub ReleaseのWindows x86_64バイナリを`bevyistr-v0.6.0-windows-x86_64.msi`と`SHA256SUMS.txt`で配布する。既存のWindowsパッケージ作成を利用し、タグ・Cargo・EXE・MSIの版番号、ライセンス、DLL依存、ビルド情報の確認をMSIに引き継ぐ。
- WiXでユーザー単位のインストーラーを作成する方針。スタートメニューから起動し、Windowsの「インストールされているアプリ」から削除できるようにする。後続版で同じ製品を更新できるようにし、編集用の材料ファイルはユーザー側の保存場所を案内し、ユーザー設定・編集した材料データ・解析ファイルを更新や削除から保護する。FrontISTR、Intel oneAPI／MPI、Gmshは既存の外部ツール設定を使う。[WiXのPackage仕様](https://docs.firegiant.com/wix/schema/wxs/package/) · [Windows Installerのインストール範囲](https://learn.microsoft.com/en-us/windows/win32/msi/installation-context)
- リリース候補では、開発環境やoneAPIを入れていないWindowsでインストール、スタートメニューからの起動、メッシュと結果の読込、後続バージョンへの更新、アンインストールを確認する。v0.5.0のZIP利用者向けには、既存の設定を引き継ぐ手順を記載する。MSI検証後に、注釈付きタグを基準としてGitHub Release、crates.io、マニュアルの公開を進める。
