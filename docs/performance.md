# 性能の計測（開発者向け）

対応できる節点数の上限や「快適」の保証値はまだ定めていません。
CPU処理、GPU描画、メモリ、ファイル読込は別々に測り、同じ規模でも要素型・表面数・
結果フィールド数・フレーム数・区画数で負荷が変わるものとして扱います。

## プローブ探索のCPU基準

リポジトリのルートから実行します。Windows／Linux共通で、FrontISTR・外部メッシュ・
ウィンドウ・GPU初期化を必要としません（コンパイルには通常のBevy依存環境が必要です）。

```text
cargo run -p bevyistr-visualization --release --example probe_benchmark --locked -- 8 16 32
```

末尾は各軸のHex8要素数です。32なら35,937節点・32,768要素です。
引数なしも8／16／32です。1～128を指定できますが、メモリは概ね立方で増えます。
最初から最大値を試さず、小さい規模から増やしてください。

CSV形式の標準出力（`#`行は説明）は次を区別します。

- `mesh_ms`: 節点・要素の生成とトポロジー構築。結果配列の確保は含まない。
- `build_*_ms`: 変形した境界面のプローブBVH構築。1回の予熱後、5回の中央値・最小・最大。
  節点／要素ID対応表や三角形生成も含む。破棄時間は含まない。
- `pick_mean_us`: キャッシュ済みBVHで4096本の固定レイを照会する平均時間。
  値・距離の確認とループの負荷を含む。全レイが変形面に当たり、変位値0.25を保つことも検査する。
- 節点・要素・境界面数。全て単一区画の規則格子で、複雑な実モデルやMPI区画の代用ではない。

キャッシュ改善の回帰はUIのテストで再構築回数と取得値を確認します。
このCPU例題だけでは、その改善によるアプリ全体の高速化率を示せません。
性能の閾値を自動テストへ固定せず、数値の正しさの検証と計測を分けます。

## 記録する情報

計測日・ソースSHA（未コミット変更があればその旨）・OS／CPU／RAM・Rust版・
ビルドprofile・入力サイズ・コマンド・出力を残します。描画を測る場合はGPU・ドライバ・
ウィンドウサイズ・描画モードも必要です。繰返しのばらつきと初回／再使用を分けます。

次に、実例題で読込時間・コンターと変形の更新・選択・MPI結果のメモリ使用量を測ります。
現在の計測例は読み込み、選択、FPS、最大メモリ使用量、長時間解析の安定性を測りません。

## 初回計測: 2026-11-01

ソースはmain `cb4c1b2aab8bccab7745ba8ac560df0d8bd91d33`から作成した
`perf/result-probe-cache`の未コミット変更を含みます。Rust 1.99.1、通常のrelease profile、
Windows（実行環境が報告したバージョン: NT 10.0.26300.0）、論理CPU数16。
CPU識別子は `AMD64 Family 25 Model 97 Stepping 2, AuthenticAMD`。
CPU商品名・RAM容量は今回の制限環境では取得していません。GPU描画は使用しません。
他アプリの負荷を制御していないローカル測定なので、他環境への保証値にしません。

```text
cargo +1.99.1 run -p bevyistr-visualization --release --example probe_benchmark --locked --offline -- 8 16 32
target/release/examples/probe_benchmark.exe 64
```

後者はWindowsで生成済み実行ファイルを直接実行した記録です。
Linuxで規模64を再現する場合も、上のcargoコマンドの末尾を64にすれば同じ処理になります。

| 各軸の要素数 | 節点 | 要素 | 境界面 | モデル準備 ms | BVH構築中央値 ms（最小～最大） | 照会平均 µs |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 8 | 729 | 512 | 384 | 1.743 | 0.156（0.135～0.230） | 0.379 |
| 16 | 4,913 | 4,096 | 1,536 | 11.733 | 0.757（0.703～0.871） | 0.390 |
| 32 | 35,937 | 32,768 | 6,144 | 91.699 | 4.483（4.252～4.632） | 0.510 |
| 64 | 274,625 | 262,144 | 24,576 | 798.568 | 28.506（28.340～29.871） | 0.587 |

全ケースで4096／4096レイのヒット・値・距離の検査に成功しました。
この例ではキャッシュ破棄後の構築コストが通常の照会より大きく、
表示モード切替だけでBVHを作り直す経路を除く対象として妥当です。
修正前後のアプリ全体の高速化率、実メッシュの応答性、快適に扱える上限は未計測です。

## 実データの読込・コンター生成（2026-11-01）

`visualization`の手動計測テストを追加しました。ASCII VTU／PVTUの**固定形状のソリッド系列**を
対象にします。外部データの指定が必要なので通常のテストではスキップします。
外部ファイルへの書込みやFrontISTRの実行は行いません。

PowerShellでは、リポジトリのルートから次を実行します。パスは手元のデータへ置き換えます。

```powershell
$env:BEVYISTR_BENCH_RESULT = 'D:\Work\FrontISTR\tutorial\19_conrod\vis_out\conrod_psf.0000.pvtu'
$env:BEVYISTR_BENCH_FIELD = 'NodalMISES'
cargo +1.99.1 test -p bevyistr-visualization --release --lib real_result_pipeline_baseline --locked -- --ignored --nocapture
```

Linuxのシェルでは同じ環境変数をコマンドに渡せます。

```sh
BEVYISTR_BENCH_RESULT=/path/to/result.0000.pvtu BEVYISTR_BENCH_FIELD=NodalMISES \
  cargo +1.99.1 test -p bevyistr-visualization --release --lib real_result_pipeline_baseline --locked -- --ignored --nocapture
```

`BEVYISTR_BENCH_FIELD`を指定しない場合は、最初の節点スカラー、なければ先頭のフィールドを使います。
指定した物理量がなければエラーにします。通常は実ファイルの物理量名を明示してください。
計測時は他のコンパイル・重いアプリを並行実行せず、入力ファイルを固定します。

測る範囲は次のとおりです。

- `LOAD`: 指定ファイルから検出した**系列全体**の読込・検証・トポロジー構築・結果モデルの登録。
  初回の観測値と、その後5回の中央値。OSキャッシュは消していないため、コールドディスク測定ではありません。
- `FRAME`: 各区画の先頭／最終フレームを対象に、変形OFF／倍率1／倍率5で面・ワイヤー・
  プローブ探索データを個別に構築します。各5回の中央値で、破棄・GPU転送・描画は含みません。
  変位のない熱解析では、倍率を指定しても`deformation=false`になります。
- 面・ワイヤーの頂点数を出し、全ワイヤー頂点がコンター面に存在することを計測の外で検査します。
  ビームを含む混合モデルではこの前提が成り立たないため、この計測例の対象外です。
- `SELECTION`: 元形状の境界面BVHへ256本の固定レイを照会する候補取得だけを測ります。
  三角形との厳密な交差、修飾キー、トポロジー成長、ハイライト等のUI全体の時間ではありません。

### 読み込みの重複処理を削減

conrodの読込がコンター生成より重かったため、VTKの結果モデル登録経路を点検しました。
VTU解析／ゴースト除去で作成済みのトポロジーを、`FemModel::single_mesh`／`add_mesh`で
再構築していました。VTKローダー内では検証済みメッシュを直接移し、二重構築を除きました。
一般の編集可能なメッシュ登録の再構築処理は維持しています。
後続フレームの形状／接続の照合、区画ローカルID、ゴースト除去、共通色範囲の仕様は維持します。

ソースは`87ed246`からの未コミット変更。環境は上記初回計測と同じWindows・Rust 1.99.1・
release profileです。実際のコマンドには`--offline`と、プロセス内のTEMP／TMP・PDB無効化を追加しました。

| データ | 区画 | フレーム | 節点／要素（各区画） | 修正前の再読込中央値 ms | 修正後の再読込中央値 ms |
| --- | ---: | ---: | --- | ---: | ---: |
| `19_conrod/vis_out/conrod_psf.0000.pvtu` | 1 | 2 | 94,047／56,115 | 855.835 | 673.305 |
| `01_elastic_hinge/hinge_vis_psf.0000.pvtu` | 1 | 2 | 84,056／49,871 | 未計測 | 549.112 |
| `16_heat_block/block_vis_psf.0001.pvtu` | 1 | 1 | 37,386／32,160 | 未計測 | 117.514 |

conrodで観測した再読込中央値は約21%減りました。単一環境・一組の比較であり、アプリ全体の
高速化率ではありません。hinge／heatは修正前を測っていないため、改善率は算出しません。
修正後のコンター面構築はconrod／hingeで約6～11ms、heatで約4.2～4.3ms、
ワイヤーは約2.3～5.3msでした。測定のばらつきもあり、FPSへの換算は行いません。
3例題の面／ワイヤーの座標整合検査に成功しました。

生ログはローカルの`target/conrod-performance-before.txt`、`target/conrod-performance-after.txt`、
`target/hinge-performance-after.txt`、`target/heat-performance-after.txt`にあります（Git管理対象外）。
RES読み込み、完全な選択操作、複数区画の実データ、GPU描画、最大メモリは今回未計測です。
流体例題の再解析は保留を維持しています。

## 連番の後続フレームのトポロジー構築削減（2026-11-01）

`f927d79`を基準に、VTKの座標・要素・結果配列の読込と、探索用トポロジーの構築を分離しました。
各フレームでゴースト除去と座標・要素接続・ローカルIDの照合を行い、
先頭フレームの確定形状だけでトポロジー／BVHを構築します。
ゴースト除去前に構築していた一時的なトポロジーも不要になりました。
未構築の形状は専用の内部型で扱い、空キャッシュのFemMeshを公開モデルへ渡しません。

比較は前節と同じCPU／Windows／Rust 1.99.1・release profileです。
修正前の読込コードを含む既存Release計測実行ファイルを`target/series-benchmark-before.exe`に保存して再測定し、
修正後を同じ入力・フィールドでビルドして比較しました。
各実行は最初の読込のあと、5回の再読込の中央値を使用します。OSキャッシュは消していません。
修正後は`f927d79`からの未コミット変更を含みます。

| 例題 | 区画 | フレーム／モード | 節点／要素 | 修正前 ms | 修正後 ms | 観測した短縮率 |
| --- | ---: | ---: | --- | ---: | ---: | ---: |
| conrod | 1 | 2 | 94,047／56,115 | 682.309 | 511.370 | 約25% |
| hyperelastic cylinder | 1 | 6 | 629／432 | 17.962 | 9.183 | 約49% |
| eigen spring | 1 | 5 | 78,771／46,454 | 1110.947 | 504.826 | 約55% |

入力はそれぞれ`19_conrod/vis_out/conrod_psf.0000.pvtu`、
`03_hyperelastic_cylinder/cylinder_vis_psf.0000.pvtu`、
`15_eigen_spring/spring_vis_psf.0001.pvtu`です。
フィールドはconrod／cylinderが`NodalMISES`、springが既定の先頭節点スカラー`DISPLACEMENT[1]`でした。
ファイル数が多い系列で効果が大きい結果ですが、単一環境の測定であり、アプリ全体の高速化率ではありません。
先頭／最終フレームのコンター面・ワイヤーの座標整合も3例題で成功しました。

生ログは`target/series-{conrod,cylinder,spring}-{before,after}.txt`です（Git管理対象外）。
再現には前節の手動計測コマンドで入力とフィールドを変更します。
ばねはTimeの系列とは扱わず、読込対象の5モードを数えています。
複数区画の形状保持は合成fixtureで検証しました。複数区画の実データの性能、RES、GPU、
ピークメモリ、GUIでの操作感は今回の比較の対象外です。

## 並列弾性ヒンジのPVTUとメモリ（2026-11-16）

`1bdb9ec`のWindows・Rust 1.99.1・Releaseで、既存の`real_result_pipeline_baseline`を
`02_elastic_hinge_parallel/hinge_vis_psf.0000.pvtu`に対して実行しました。物理量は`NodalMISES`です。
Windows NT 10.0.26300.0、論理CPU 16、AMD64 Family 25 Model 97 Stepping 2。
試験には保存済みファイルを使いました。

| 区画 | 節点 | 要素 | 境界面 |
| ---: | ---: | ---: | ---: |
| 1 | 21,943 | 12,726 | 4,960 |
| 2 | 22,469 | 13,236 | 4,832 |
| 3 | 21,961 | 12,751 | 4,942 |
| 4 | 22,168 | 12,924 | 4,936 |
| 合計 | 88,541 | 51,637 | 19,670 |

節点・要素数は読込後の区画別ローカルIDの合計です。共有境界を含むので、全体の一意ID数とは異なります。
2フレームの初回読込は618.880 ms、続く5回の中央値は398.458 msでした。
区画ごとの構築中央値は面1.120～2.241 ms、ワイヤー0.711～1.320 ms、プローブ探索用データ1.240～2.292 msです。
先頭／末尾、変形なし／倍率1／倍率5の全組合せで、ワイヤー頂点がコンター面に含まれることを確認しました。
生ログはローカルの`target/hinge-parallel-performance.txt`にあります。

別に、上記テストの実行ファイルだけを例題ごとに新規プロセスで3回起動し、取得できた
Windowsの`PeakWorkingSet64`の最大を各回で記録しました。これはその時点までにプロセスが使用した
物理メモリのピークです。最初の結果と、読み直し時の一時データ、面／ワイヤー／プローブの生成と検証も含みます。
OSキャッシュは消していません。

| PVTU | 区画／フレーム | 再読込中央値の範囲 ms | 観測ピークの中央値 MiB（最小～最大） |
| --- | ---: | ---: | ---: |
| `01_elastic_hinge/hinge_vis_psf.0000.pvtu` | 1／2 | 411.019～415.295 | 116.28（116.12～116.40） |
| `02_elastic_hinge_parallel/hinge_vis_psf.0000.pvtu` | 4／2 | 391.267～393.021 | 101.94（101.93～102.17） |

入力ファイル・区画境界・要素数が異なります。この表から分割自体でメモリや読込時間が減るとは判断できません。
GPUメモリとGUI全体の常駐メモリは未計測です。

再測定時は上記の手動計測コマンドでビルドした実行ファイルを使います。
`$env:BEVYISTR_BENCH_RESULT`と`$env:BEVYISTR_BENCH_FIELD`を設定し、`$exe`を
Cargo出力の`Running unittests`に表示された実行ファイルの絶対パスに置き換えてください。

```powershell
$proc = Start-Process -FilePath $exe -ArgumentList @(
    'real_result_pipeline_baseline', '--ignored', '--nocapture'
) -WindowStyle Hidden -PassThru
$peak = 0L
while (!$proc.WaitForExit(20)) {
    $proc.Refresh()
    try { $peak = [Math]::Max($peak, $proc.PeakWorkingSet64) }
    catch [System.InvalidOperationException] { break }
}
$proc.WaitForExit()
if ($proc.ExitCode -ne 0 -or $peak -eq 0) { throw 'Measurement failed' }
$peak / 1MB
```

サンプリングしたWindowsのピークカウンターなので、終了間際の更新は取り逃す場合があります。
Linuxで同じ試験を行う場合はテスト実行ファイルをGNU `time -v`で起動し、
`Maximum resident set size`を記録できます。OSの異なる値は直接比較しません。

## 実メッシュでの選択操作のCPU計測（2026-11-16）

`real_selection_pipeline_baseline`は、FrontISTRのソリッドMSHを使って、通常のホバー、
選択プレビュー、クリック確定をBevyのスケジュールで実行します。固定のカメラと
800×600の仮想ウィンドウを使うため、GPUやOSのウィンドウは不要です。元ファイルは読み取り専用です。

```powershell
$env:BEVYISTR_BENCH_MESH = 'D:\Work\FrontISTR\tutorial\02_elastic_hinge_parallel\hinge.msh'
cargo +1.99.1 test -p bevyistr-ui --release --lib real_selection_pipeline_baseline --locked -- --ignored --nocapture
```

LinuxではMSHパスを変更し、`BEVYISTR_BENCH_MESH=... cargo ...`の形式で実行できます。
`HOVER`は各選択レベルに対し同じ256座標を走査し、命中と外れを別々に記録します。
`Face Smooth`の既定角度は15度です。`DOUBLE_CLICK`はFaceの同じ命中位置で、
二度目のクリックにかかる時間と確定した面数を25回記録します。
中央値と95パーセンタイルの単位はµsです。命中時に選択レベルとプレビューを検査し、
ダブルクリックでは毎回同じ面数が選択されることを確かめます。

測るのは最小構成の`App::update`のCPU時間です。選択範囲の描画、GPU転送、ウィンドウ入力の処理、
ドラッグによる範囲選択、アプリ全体のフレーム時間は未計測です。

`02_elastic_hinge_parallel/hinge.msh`は84,056節点、49,871要素、境界17,866面です。
計測コードを加えた`1bdb9ec`の実行ファイルを保存し、選択確定の修正前後を同じ入力・Release設定で
交互に3回ずつ実行しました。表は各実行で得た中央値の範囲です。

| 操作 | 修正前 ms | 修正後 ms |
| --- | ---: | ---: |
| Face Singleのホバー（命中195点） | 0.192～0.199 | 0.189～0.197 |
| Face Smoothのホバー（命中195点） | 10.169～10.839 | 10.337～10.880 |
| Faceのダブルクリック（17,866面、各25回） | 228.511～232.851 | 11.133～12.193 |

ダブルクリック時、選択とハイライトの一覧を対象ごとに走査して重複を除いていました。
グループ内のIDをハッシュ集合で照合し、並び順は入力順を使うよう変更しました。
ダブルクリックの3回分の中央値を比較すると231.680 msから11.312 msとなり、選択面数は全試行で同じでした。
選択クレート9件とUI229件の通常テストが成功しました。追加テストでは重複した入力、
既存選択との重複、同じローカルIDを持つ別部品、追加と解除での並び順を確認します。

生ログはローカルの`target/hinge-selection-{before,after}-{1,2,3}.txt`にあります。

### Coplanar／Smoothのプレビュー

`7ec67d5`と、面法線と共有辺の対応表を再利用する修正版を、同じヒンジMSH・Release設定で
交互に3回ずつ実行しました。表は各実行で得た中央値の範囲です。

| 操作 | 修正前 ms | 修正後 ms |
| --- | ---: | ---: |
| Face Singleのホバー（命中195点） | 0.193～0.207 | 0.196～0.204 |
| Face Smoothのホバー（命中195点） | 10.338～10.603 | 0.604～0.612 |
| Faceのダブルクリック（17,866面、各25回） | 11.629～11.825 | 11.535～11.874 |

Face Smoothの実行ごとの中央値をさらに比較すると、10.388 msから0.605 msになりました。
面法線・共有辺の対応表は各メッシュの最初のCoplanar／Smooth照会で構築し、
座標や接続を編集してトポロジーを再構築した際に破棄します。
角度は照会ごとに比較し、Coplanarの起点面との比較とSmoothの隣り合う面同士の比較を維持します。

代表値は初回構築を含む計測の中央値で、初回照会の待ち時間やキャッシュの常駐メモリは個別に測っていません。
GPU描画と実画面のフレーム時間も未計測です。
回帰テストでは、座標を変更した場合と面の接続を外した場合の選択要素IDを確認しました。
生ログはローカルの`target/hinge-smooth-{before,after}-{1,2,3}.txt`にあります。

## ネイティブASCII RESの解析（2026-11-01）

`e237f56`を基準に、`NativeResult`の通常数値の文字列確保と、本文／セクションのコピーを削減しました。
E指数・通常の数値は直接解析し、FortranのD／d指数がある場合だけ文字列を正規化します。
セクションは元文字列を参照し、LF／CRLF・末尾改行の省略にも対応を維持します。
数値の精度、欠損節点のNaN表現、IDの照合・重複検査、非有限値の拒否は変更しません。

専用の読取専用例を追加しました。Windows／Linuxで同じ引数を使えます。

```text
cargo +1.99.1 run -p bevyistr-hecmw --release --example benchmark_results --locked -- <matching.msh> <result.res.rank.step>
```

例えばconrodでは次の組合せです。

```text
cargo +1.99.1 run -p bevyistr-hecmw --release --example benchmark_results --locked -- D:/Work/FrontISTR/tutorial/19_conrod/conrod.msh D:/Work/FrontISTR/tutorial/19_conrod/fstrRES/STEP1/conrod.res.0.1
```

測定範囲を分けて出力します。

- `MESH first_ms`: MSH読込・トポロジー構築の初回観測値。
- `RES read_ms`: 指定した単一RESのUTF-8テキスト読込。
- `RES parse_ms`: メモリ上の単一RESからネイティブレコードを解析する処理。ファイルI/Oは含まない。
- `RES map_ms`: 解析済みレコードをMSHのID順へ配置し、未使用節点の欠損を展開する処理。
- `SERIES repeat_median_ms`: 読込済みMSHに対する系列全体のファイルI/O・解析・ID対応・欠損展開。
  MSHの読込、GUIダイアログ、モデル登録、描画は含まない。

各中央値は予熱後5回です。生成物の破棄を計測の外に置き、OSキャッシュは消していません。
今回の3例題は完全な単一メッシュ結果です。MPIのランク統合を測る例ではありません。
VTKの`LOAD`は形状・トポロジー構築も含むため、このRESの`SERIES`と直接比較できません。

### 測定結果

Windows／CPU／Rust 1.99.1・release profileは前節と同じです。
修正前は`e237f56`に計測例だけを追加して構築し、`target/res-benchmark-before.exe`へ保存。
修正後は`e237f56`からの未コミット変更を含みます。検証プロセス内のTEMP／TMP・PDB無効化と
`--offline`を使用しました。

| 例題 | フレーム／モード | 単一RES解析 前→後 ms | 系列全体 前→後 ms | 系列で観測した短縮率 |
| --- | ---: | ---: | ---: | ---: |
| conrod | 2 | 111.569→69.657 | 311.635→230.601 | 約26% |
| eigen spring | 5 | 34.794→19.163 | 273.681→193.807 | 約29% |
| heat block | 1 | 8.738→4.913 | 15.304→11.357 | 約26% |

ばねの入力は`15_eigen_spring/spring.msh`と`spring.res.0.1`、熱は
`16_heat_block/block.msh`と`block.res.0.1`です。結果のデータ量・出力物理量は例題ごとに異なります。
生ログは`target/res-{conrod,spring,heat}-{before,after}.txt`にあります（Git管理対象外）。

系列全体のID順序、各フレームのStep／Time／Eigenvalue、全フィールドの名前・種別・範囲・値のビット列から
計算する比較用signatureも出力します。非暗号用途の64bitハッシュで、正式なファイル同一性保証ではありません。
今回の比較では3例題とも変更前後で一致しました。

- conrod: `e1a25731951a0067`
- eigen spring: `3a560eac6b7ac41c`
- heat block: `d593f497086b68dd`

これらは特定入力・単一環境でのCPU計測です。アプリ全体の高速化率、GPU描画、ピークメモリ、
実機MPIの速度、GUIでの操作感の測定ではありません。
