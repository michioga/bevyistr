# 高度なモデルの結果読み込み検証（2026-09-22）

対象ブランチは`test/advanced-results`、基準コミットは`58987c0`です。
既存のFrontISTRチュートリアル出力を読み取り専用で検証しました。
ソルバーの再実行や元ファイルの変更はしていません。
これは読み込み・数値対応の検証であり、解析設定の妥当性や全GUI操作の確認ではありません。

## 今回の結果

| チュートリアル | PVTUの節点／要素数 | ピース／フレーム | 確認内容 |
|---|---:|---:|---|
| `15_eigen_spring` | 78,771／46,454 | 1／5 | 全5モードのRESとVTKをMSHの節点順で照合。変位ベクトル・3成分が許容差内で一致。単独PVTUでも全フレームを読める。 |
| `16_heat_block` | 37,386／32,160 | 1／1 | 全節点のTEMPERATUREがRES／VTKで一致。値域20〜100。元のNode ID 2は約28.502321。単独PVTUは温度とMesh_Typeの2フィールド。 |
| `18_cavity_flow` | 35,863／178,142 | 8／1 | 13フィールド、節点／要素配列長、ピース共通の値域を確認。VELOCITY[1]は0〜0.001、[2]・[3]・PRESSUREは全ピースで0。変位なし。 |

複数ピースの節点数は各ピースの合計で、境界の重複節点を統合した数ではありません。
流体については今回、ネイティブMPI結果と全節点の対応比較までは行っていません。
RES／VTKの値比較には`abs(a-b) <= 2e-5 * max(abs(a), 1)`を用い、
成分ごとに確認します。これはASCII出力の桁数差を許容するためで、表示値への丸めは使いません。

参照ファイル：

- `15_eigen_spring/spring.msh`, `spring.res.0.1`〜`.5`, `spring_vis_psf.0001.pvtu`〜`.0005.pvtu`と参照ピース。
- `16_heat_block/block.msh`, `block.res.0.1`, `block_vis_psf.0001.pvtu`と参照ピース。
- `18_cavity_flow/cavityflow_vis_psf.0000.pvtu`と参照される8ピース。

## 当初発見した制限：固有値メタデータ（9月23日に対応）

`spring.cnt`には`!SOLUTION, TYPE=EIGEN`と5モードの指定があり、
RESのグローバルデータ・PVTUのFieldDataに`EIGENVALUE`があります。
最初のRESには`7.8306921036862833E+006`、PVTUには`7.830692e+006`が記録されています。

9月22日の基準実装では`StepResult`がこの値を保持しておらず、
全5フレームのTimeは既定値0、HISTORYはFrame軸でした。
固有値を時刻や節点コンターとして誤読しないテストを出発点としています。

9月23日、`feat/eigen-result-metadata`で次を実装しました。

- RESのグローバルデータとVTU／PVTUのFieldDataから、固有値を`f64`で保持。
- PVTU親ファイルにだけある固有値を各ピースへ引き継ぎ、ピース間・MPIランク間の不一致は拒否。
- Results、ホバー、PINNEDはMode／Eigenvalueを表示し、モードを時刻0と表示しない。
- HISTORYは読み込み順のMode/frame sequence。異なるモード系列のプローブ比較は拒否。
- CSV末尾に`frame_kind,mode,eigenvalue`を追加。モードの時刻欄は空欄、状態は`not_applicable_for_mode`。

全5モードでRES／VTKの固有値が`abs(a-b) <= 1e-6 * max(abs(a), abs(b), 1)`を満たし、
変位ベクトル・成分も従来の許容差で一致しました。RESに記録された64bit精度は保持し、
VTKに書かれていない桁を補いません。非有限・複数成分・重複した固有値は拒否します。
周波数換算と単一モードの往復振動は未対応のままです。Playはモードの切り替えです。

通常テストは354成功・0失敗・8スキップ。実チュートリアルの7テストは別途成功しました。
固有値表示は後述の提供画像で一部確認しました。残る操作確認は[チェックリスト](verification.md)を使用します。

## 提供CSVの照合（2026-09-23）

対象は`15_eigen_spring/probe_part1_node_30138.csv`です。
15列・5データ行、Part 1／Node 30138、Displacementのmagnitudeを確認しました。
直前の提供画像の固定対象Node 8254とは別の対象です。

- Frame／Step／Modeは全て1〜5。`frame_kind=mode`、値の状態は全行`available`。
- 時刻欄は全行空欄、`time_status=not_applicable_for_mode`。モードを時刻0として保存していません。
- 各`spring.res.0.1`〜`.5`のグローバルEIGENVALUEと、CSVの固有値は`f64`として全行一致。
- RES内のNode 30138の変位3成分から独立に大きさを計算し、CSVと照合。
  最大相対差は約`6.75e-8`で、アプリが保持する`f32`精度の範囲内です。
- 変位の大きさはモード順に`0.9274688, 1.0551693, 1.048943, 1.0918099, 1.0192614`。

元のCSV・RESは変更していません。今回の確認は1対象の保存結果であり、
キャンセル・上書き・保存中の表示切替などのGUI操作まで確認したものではありません。

## 再実行

ローカルデータを使うテストは通常のCIではスキップします。
`BEVYISTR_TUTORIAL_DIR`でチュートリアルのルートを指定してください。
絶対パスをコードに埋め込んでいません。PowerShellでの例：

```powershell
$env:BEVYISTR_TUTORIAL_DIR = 'D:\Work\FrontISTR\tutorial'
cargo test -p bevyistr-hecmw --locked --offline -j 1 tutorial_ -- --ignored --nocapture
```

既存の実結果テスト5件と、今回追加した`hecmw/tests/advanced_results.rs`の2件は成功しました。
既存テストにはhinge／conrodのRES・VTK対応、conrodの未使用節点、
dynamic_beamの連番検出・時刻順、単独VTK形状、流体8ピースの検証が含まれます。
外部データを使わない小さな固有値・温度の回帰テストも`hecmw/src/vtu.rs`に追加しています。

ワークスペース全体の通常テストは348成功・0失敗・8スキップでした。
スキップのうち7件は上記の実データ検証として別途実行し成功しています。
`mdbook build docs`も成功し、生成HTMLの画像リンクとコピー後のPNGのSHA-256一致を確認しました。

## 実画面の確認状況

- プローブ比較の提供画像で、2節点の色・ID・値と共通軸の曲線を確認済み。
  [マニュアル](src/results/probe.md)へ掲載しました。
- 静止画だけでは再生追従・Remove操作は確認できないため、確認済み扱いにはしていません。
- 2026-09-23提供の`results-eigen-mode.png`で、Frame 3/5、Mode 3、Eigenvalue 3.260034e7、時刻でない旨の説明と変位コンターを確認し、[マニュアル](src/results/animation.md)に掲載しました。All frames、Deformation ON、倍率4.50も画面で確認できます。
- 続いて提供されたRESの画面ではPart 1／Node 8254を固定し、Mode 3、Eigenvalue約3.260034264e7、変位の大きさ1.048088、HISTORYの5/5 samples、Mode/frame sequence (not time)、3番目の黄色い目印を確認しました。
- 別途Node 30138のCSVは上記のとおり照合済み。残る実画面の操作確認は次のチェック項目です。
- 固有値：全5フレームの連続切替、変形・成分切替、ホバーと固定対象の追従。静止画から連続操作の成功は判断しません。
- 熱：TEMPERATUREのコンター・プローブ、変位なし、1フレームの再生無効。
- 流体：VELOCITYの成分切替、ゼロ成分のConstant表示、ピースをまたぐIDの区別。
