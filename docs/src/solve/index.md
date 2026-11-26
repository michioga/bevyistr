# Solve：出力と実行

## 設定を確認する

解析方式と連立方程式ソルバーはメニューから選択します。MUMPS、MKL (DIRECTmkl)、CG、BiCGSTAB、GMRES、Directの選択と、実行方式のDirect／MPIは別の設定です。

0.3.0では、反復の種類を分けて表示します。

- **LINEAR ITERATION | !SOLVER**：CG／BiCGSTAB／GMRESの最大反復数と相対残差。MUMPS／MKL／Directでは非表示になり、直接法の案内を表示します。CG／BiCGSTAB／GMRESへ戻すと入力値は保持されています。
- **NONLINEAR STEP | !STEP**：Nonlinearでのみ表示する固定増分のサブステップ数、Newton反復上限、非線形残差の許容値です。線形反復とは独立しており、MUMPSでも設定できます。
- Enterで確定、Escで復元します。非線形の初期値は反復上限50・残差許容値`1e-3`です。変位増分など、ここで編集できない他の収束判定はFrontISTRの既定値を使用します。

MUMPSとMKLは別のMETHODとして読み込み・出力します。メニューにあることは実行環境での利用可否の保証ではなく、FrontISTRのビルドによって使用可能なソルバーが異なります。
線形反復は[!SOLVER](https://source-docs.frontistr.com/keyword_reference/cnt/solver.html)、非線形反復は[!STEP](https://source-docs.frontistr.com/keyword_reference/cnt/step.html)に対応します。
[出力先・保存間隔・物理量](output-fields.md)も実行前に設定してください。

Eigenvalueを選ぶと、独立した **EIGENVALUE | !EIGEN** 欄でモード数・許容差・最大反復数・シフト量を編集できます。[固有値解析の設定](eigen.md)を参照してください。

Solve の **Preconditioner** で SSOR（`PRECOND=1`／`2`）または対角スケーリング（`PRECOND=3`）を選べます。CG／BiCGSTAB／GMRESの数値欄は次のとおりです。

- **Precond. iterations**（`iterPREmax`）は前処理の反復回数です。**GMRES subspaces**（`NREST`）はGMRESで表示します。
- **Ordering colors**（`NCOLOR_IN`）は多色順序付けの色数で、FrontISTRではOpenMPが2スレッド以上のときに適用されます。**Precond. reuse**（`RECYCLEPRE`）は非線形静解析で表示し、`0` で再利用を無効にします。
- **Diagonal scale**（`SIGMA_DIAG`）は対角スケーリング、**SSOR relaxation**（`OMEGA`）はSSORで表示します。FrontISTRは `0 < OMEGA < 2` の範囲を使い、従来の `0` は `1` として扱います。
- 線形反復欄を空欄にしてEnterを押すと、FrontISTRの既定値を使います。`NITER`、`iterPREmax`、`RESID` の省略も、解析タイプや前処理を切り替えて表示されなくなった値も保持します。`RESID` は倍精度で扱います。 [FrontISTRの `!SOLVER` 仕様](https://source-docs.frontistr.com/en/keyword_reference/cnt/solver.html)

反復法の **Matrix scaling** は3次元解析用の `!SOLVER, SCALING` に対応します。**Default** はパラメーターを省略し、**On** は `YES`、**Off** は `NO` を出力します。直接法を選ぶと欄が非表示になり、反復法に戻すと選択値が復元されます。 [FrontISTRの `!SOLVER` 仕様](https://source-docs.frontistr.com/keyword_reference/cnt/solver.html)

### ソルバーログを設定する

- **Residual log** は `ITERLOG` に対応します。Onで、反復法では残差履歴、直接法では解の相対残差を出力します。
- **Solver timing log** は `TIMELOG` に対応します。Summaryで計算時間を出力し、Verboseでソルバーが対応する詳細を追加します。
- **Default** は対応するパラメーターを `.cnt` から省略し、使用するFrontISTRの既定動作を適用します。読み込んだファイルの明示的なOffもそのまま保持します。 [FrontISTRの `!SOLVER` 仕様](https://source-docs.frontistr.com/en/keyword_reference/cnt/solver.html)

### 係数行列と右辺を保存する

- **Matrix dump** で Matrix Market（`MM`）、`CSR`、`BSR`、または **Off**（`NONE`）を選びます。設定は `.cnt` の `!SOLVER, DUMPTYPE=...` に反映されます。
- **Dump action** が **Continue solve** の場合はダンプ後も計算を続けます。**Exit after dump**（`DUMPEXIT=YES`）ではダンプ後に終了するため、bevyistrは解析結果の自動読込を行わず、実行フォルダの確認を案内します。Matrix dump が Off のときは、ダンプによる終了は発生しません。
- 出力先は実行フォルダです。行列と右辺のファイル名には、FrontISTRがソルバー呼び出し番号とMPIランク番号を付けます。 [FrontISTRのソルバー・前処理](https://source-docs.frontistr.com/functions_reference/06_solver.html)

## 実行する

### 時刻歴解析の時間積分を確認・編集する

`18_cavity_flow` の `hecmw_ctrl.dat` を **Open Project** で開くと、**Solve → TIME INTEGRATION | !DYNAMIC (Newmark)** に時間積分設定が表示されます。**Fallback substeps**（`n_step`）、**Fallback increment**（`t_delta`）、**Newmark gamma**、**Newmark beta** をEnterで確定できます。例題で読み込む値は順に `10000`、`10`、`0.5`、`0.25` です。[`!DYNAMIC` の公式仕様](https://source-docs.frontistr.com/keyword_reference/cnt/dynamic.html)

`n_step` と `t_delta` は `!STEP` を省略した場合などの既定値です。明示的な `!STEP` の固定増分は、その `SUBSTEPS` と `DTIME, ETIME` で決まります。 [FrontISTRの `!STEP` 仕様](https://source-docs.frontistr.com/keyword_reference/cnt/step.html)

対応範囲は `TYPE=LINEAR`／`NONLINEAR`、先頭データ行が `1, 1` の陰解法Newmark時刻歴解析です。6行を読み込み、質量行列、Rayleigh減衰、監視対象と間隔、出力フラグも保持します。HHT、陽解法、周波数応答は互換性の指摘として表示します。動解析のプロジェクト全体の **Export／Run FrontISTR** は、`!STEP` と境界条件・荷重グループの対応後に有効になります。

### 読み込んだ設定に未対応項目がある場合

読み込んだCNT・MSH・DATで、保持できない設定や解決できない参照を検出すると、画面上部に **Input re-export blocked** と表示します。件数は再出力を止めた入力上の指摘数です。
Solveの最下部には元ファイル名・行番号・
キーワードと理由を表示し、Export／Run FrontISTRを停止します。
次の表はbevyistrが再出力できる範囲です。FrontISTRで使える構文とパラメータは、公式サイトの「キーワード → 解析制御データ (cnt)」で確認できます。 [FrontISTR公式ドキュメント](https://source-docs.frontistr.com/index.html)

| 項目 | 再出力できる代表例 | Export／Runが止まる代表例 |
| --- | --- | --- |
| 拘束・荷重の`GRPID` | `GRPID=1`。ヒンジ例題のような省略形は既定値1として扱います。 | `GRPID=2`など、グループ1以外の指定 |
| `!SOLVER` | 対応METHOD、反復法の`PRECOND=1`／`2`／`3`。標準の2データ行の `NITER`、`iterPREmax`、`NREST`、`NCOLOR_IN`、`RECYCLEPRE`、`RESID`、`SIGMA_DIAG`、`OMEGA` を保持します。反復法の `SCALING`、`ITERLOG`／`TIMELOG`、`DUMPTYPE`／`DUMPEXIT` にも対応します。 | その他の前処理、未知のMETHOD、追加のデータ行や列 |
| 解析方式と`!STEP` | 基本の静解析、単一の非線形静解析ステップ（固定増分、拘束・荷重のグループ1と全接触グループを有効化）、[固有値の4設定](eigen.md) | 動解析・熱解析の入力、複数ステップ・自動増分・個別の時間設定・部分グループの有効化 |
| 材料 | 等方弾性・密度。[非圧縮ニュートン流体の一定粘度・密度](../materials.md#非圧縮ニュートン流体)は読込・編集に対応。 | 流体プロジェクト全体のExport／Run、温度依存テーブル、異方弾性、弾塑性、超弾性 |
| 接触 | `NODE-SURF`／`SURF-SURF`、`ALAGRANGE`、`SSLID`／`FSLID`／`TIED`。同じ単一ステップで全ペアを有効にする入力。すべり接触では摩擦係数と第2行の摩擦ペナルティを保持 | 複数のSlave/Master行を持つ1枚の `!CONTACT PAIR`、`SLAGRANGE`、`NPENALTY`／`TPENALTY`、`!CONTACT_PARAM`、一部ペアだけの有効化 |
| 保存間隔・出力項目 | 基本の`!WRITE`。`!OUTPUT_RES`／`!OUTPUT_VIS`のGROUP／ACTIONなどの拡張カードは単体出力で内容を保持します。 | `!WRITE`の追加オプション、名前付き出力GROUPを含む複数部品の統合出力 |

FrontISTRの仕様を確認する際は、設定項目ごとのページを参照してください。 [拘束 `!BOUNDARY`](https://source-docs.frontistr.com/keyword_reference/cnt/boundary.html) · [集中荷重 `!CLOAD`](https://source-docs.frontistr.com/keyword_reference/cnt/cload.html) · [分布荷重 `!DLOAD`](https://source-docs.frontistr.com/keyword_reference/cnt/dload.html) · [線形ソルバー `!SOLVER`](https://source-docs.frontistr.com/keyword_reference/cnt/solver.html) · [ステップ `!STEP`](https://source-docs.frontistr.com/keyword_reference/cnt/step.html) · [固有値 `!EIGEN`](https://source-docs.frontistr.com/keyword_reference/cnt/eigen.html) · [弾性 `!ELASTIC`](https://source-docs.frontistr.com/keyword_reference/cnt/elastic.html) · [接触 `!CONTACT`](https://source-docs.frontistr.com/keyword_reference/cnt/contact.html)

`GRPID`を省略したときの既定値1は、FrontISTRソースの`fstr_setup.f90`にある`fstr_setup_BOUNDARY`、`fstr_setup_CLOAD`、`fstr_setup_DLOAD`の初期値で確認しています。

- Solverの選択欄は読み取れた部分のみであり、元入力全体を再現しているわけではありません。
  ドロップダウンの変更や別CNTの追加読込だけでは警告は解除されません。
- 元入力ファイルは変更しません。モデルの確認や **Open Results** での既存結果表示は可能です。
  未対応設定を含む解析は原本を維持して外部で実行してください。
- 新しいモデルをOpen Model／Open Projectで開き直すと、そのモデルの設定として再評価します。
  警告を消す目的で原本から必要な設定を削除しないでください。

新規作成でDynamicを選んだ場合も、動解析のプロジェクト全体のExport／Runは制限されます。
互換性検査は、対応する入力設定と参照を保持できるかを確認します。解析モデルの妥当性は別途確認してください。

`hecmw_ctrl.dat` の `fstrDYNA`、`result-in`、`fstrTEMP` は、既存プロジェクトを開いたときに再出力制限の理由として表示されます。 [結果ファイルの指定](https://source-docs.frontistr.com/en/keyword_reference/dat/result.html)

Open Projectで読み込んだ `fstrRES` と `vis_out` の出力ファイル名と相対フォルダを、ExportとRun FrontISTRで保持します。たとえば `results/hinge.res` は、選択した出力フォルダを基準に使われます。`!SUBDIR,ON` を使用する場合は、そのファイル名と相対フォルダがFrontISTRのステップ・ランク別のフォルダ配置に組み込まれます。テキスト結果の自動読込も `fstrRES` の指定に従います。絶対パスや `..` を含む出力指定は、Solveに再出力制限として表示されます。 [FrontISTRの結果ファイル指定](https://source-docs.frontistr.com/en/keyword_reference/dat/result.html) · [サブディレクトリ設定](https://source-docs.frontistr.com/en/keyword_reference/dat/subdir.html)

公式の`!OUTPUT_RES`と`!OUTPUT_VIS`は、物理量ごとのON／OFFを定義します。GROUP／ACTIONを含むカードや未知の項目を保持するのはbevyistrの再出力動作です。利用するFrontISTRの版で有効な指定は、その版の仕様を確認してください。 [RES出力の開始 `!WRITE, RESULT`](https://source-docs.frontistr.com/keyword_reference/cnt/write_result.html) · [RESの物理量 `!OUTPUT_RES`](https://source-docs.frontistr.com/keyword_reference/cnt/output_res.html) · [可視化の物理量 `!OUTPUT_VIS`](https://source-docs.frontistr.com/keyword_reference/cnt/output_vis.html)

`19_conrod`の`PRECOND=3`と、PSRの全外表面1面の可視化ブロックを保持します。`VTK`と`COMPLETE_MICROAVS`では、読み込んだ出力形式のまま再出力します。名前付き表面グループ、複数面、等値面、その他の出力形式を含む入力には再出力制限が適用されます。
保存済み結果は、対応形式であればResultsのコンター・変形・プローブで確認できます。

### 実行手順

1. 使用する`fistr1`と必要な実行環境を確認します。
2. DirectまたはMPIを選び、MPIの場合は分割数とOpenMPスレッド数を設定します。
3. **Run FrontISTR**を押します。最初の実行では出力フォルダを選びます。
4. 事前検査と入力ファイル出力後、実行ログをSolve内で確認します。

MPIでは`hecmw_part_ctrl.dat`を生成して`hecmw_part1`で領域分割し、`mpiexec -n N fistr1 -t T`で実行します。Nは領域分割数、TはOpenMPスレッド数で、独立した設定です。

**Solve → FRONTISTR EXECUTION → MPI** に実行ファイルの選択欄が表示されます。
**Choose hecmw_part1...** と **Choose mpiexec / mpirun...** で指定し、**Auto** で自動検索に戻せます。
分割ツールは`fistr1`と同じフォルダを先に調べ、その後PATHを検索します。ランチャーはPATHを検索します。
MPI ranksは新しい`hecmw_part_ctrl.dat`の`DOMAIN`に反映されます。新規作成時は`NODE-BASED / PMETIS`を使い、既存プロジェクトでは対応する分割方式・`DEPTH`・`UCD`・`CONTACT`・`CONTACT_OWNER`を保持します。
出力先フォルダに、分割と実行に必要な制御ファイルを生成します。

FrontISTRでは、`hecmw_ctrl.dat`の`part_in`／`part_out`で分割対象と出力先を指定し、`hecmw_part_ctrl.dat`の`!PARTITION`で分割数や手法を指定します。 [全体制御データと並列処理手順](https://source-docs.frontistr.com/execution_guide/procedure/02_parallel.html) · [領域分割 `!PARTITION`](https://source-docs.frontistr.com/keyword_reference/part/partition.html) · [4領域に分割するヒンジ例題](https://source-docs.frontistr.com/examples/tutorial_02.html)

実行ファイルなどのアプリ設定はTOMLに保持します。Windows／Linuxで実行ファイルとMPIランタイムをそれぞれの環境に合わせてください。設定例はリポジトリの`bevyistr.example.toml`を参照してください。

### Windowsで起動元の環境を使う

Windowsでは、Intel MPIがすでに設定されていれば、bevyistrは起動元の環境を引き継ぎます。未設定でoneAPIが見つかる場合は、FrontISTRの実行時に `setvars.bat` で初期化します。

普段の端末で `fistr1` と `hecmw_part1` が動く場合や、Windows TerminalのoneAPIプロファイルを使う場合は、その端末から環境の引き継ぎを明示できます。起動中のbevyistrを閉じ、同じPowerShellで実行します。

```powershell
$env:FRONTISTR_RUNTIME = 'inherit'
bevyistr
```

ZIP版では、展開先で `bevyistr` の代わりに `./bevyistr.exe` を実行します。MPIを使う場合は **Choose mpiexec / mpirun...** で、FrontISTRのビルドに対応するランチャーを選びます。Solveの **Runtime** が `inherited process environment` なら、起動元の環境が使われます。

`setvars.bat` が「入力行が長すぎます」で止まる場合にも、実行環境を用意した端末から上記の方法で起動できます。Windowsエラー623が出る場合は、同じ端末から `fistr1.exe` や `hecmw_part1.exe` を直接起動し、DLL読み込みの失敗が再現するか確認してください。

ログはアプリ内部に表示します。停止する場合はSolveのStopを使います。プロセスの正常終了だけで解析の収束やモデルの妥当性まで保証されるわけではありません。

成功した実行の結果はResultsへ引き継ぎます。既存結果を手動で開く操作と、その実行のMPI所有情報を使う自動引き継ぎは異なります。[結果形式の対応範囲](../results/limitations.md)も確認してください。
