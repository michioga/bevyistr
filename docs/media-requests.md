# マニュアル用の撮影依頼

## 今回：出力先と保存間隔（実機確認後）

- 保存先: `D:\Work\bevyistr\docs\media-inbox\solve-output-intervals.png`
- 掲載先: [出力先・保存間隔・物理量](src/solve/output-fields.md)
- SolveでStaticまたはNonlinearを選択し、SAVE RESULTSのRES: ON／Every 2 stepsとVTK: ON／Every 5 stepsを確定した状態。
- SAVE RESULTSと直下のOUTPUT FIELDSが読める画面を撮影してください。新しい解析の実行は不要です。

## 今回：固有値解析の設定（実機確認後）

- 保存先: `D:\Work\bevyistr\docs\media-inbox\solve-eigen-settings.png`
- 掲載先: [固有値解析の設定](src/solve/eigen.md)
- 公開可能なモデルでSolve → Eigenvalueを選択し、MUMPSと専用4項目が見える状態。
- モード数5・許容差1e-8・最大反復80・シフト0.01をEnterで確定し、文字が読めるウィンドウ全体を撮影してください。
- 実行ログや成功結果は不要です。この撮影は入力画面の説明用で、解析成功の証拠とは扱いません。
- 既存のチュートリアル入力や結果を上書きしないでください。

まずResultsの2点をお願いします。保存先はローカルの`D:\Work\bevyistr\docs\media-inbox\`です。このフォルダはGit管理・公開対象外です。素材を確認してから公開用フォルダへ移します。

## 1. コンターの物理量メニュー

- 保存名: `results-field-menu.png`
- 掲載先: [コンター・変形・色範囲](src/results/contours.md)
- データ: 公開可能な円柱などの結果。複数フィールドを持つVTU／PVTUまたはRES。
- 操作: Resultsで**Display field**を開いたまま撮影します。
- 写してほしいもの: モデル、物理量の一覧、現在項目の`[x]`、ボタン右端の開閉印、カラーバー。
- ウィンドウ全体が入り、文字が読める大きさにしてください。ファイル選択ダイアログは不要です。
- 公開用配置予定: `src/assets/images/results-field-menu.png`

## 2. フレーム間の色範囲の比較

- 保存名: `results-color-range.mp4`
- 掲載先: [コンター・変形・色範囲](src/results/contours.md)と[アニメーション](src/results/animation.md)
- データ: `03_hyperelastic_cylinder`など、複数フレームで値が変わる結果。時刻0しかないcavity_flowはこの撮影には使いません。
- 長さの目安: 20〜40秒、音声不要。1280×720以上、可能なら1920×1080。
- 操作:
  1. `NodalMISES`などの物理量を選びます。
  2. **Current frame**で先頭から再生し、カラーバーの数値も写します。
  3. 一時停止して**All frames**へ切り替えます。
  4. 先頭へ戻して再生し、カラーバーの範囲が固定される様子を写します。
  5. 最後に一時停止します。
- 撮影中はカメラを固定し、切り替え操作の前後で少し間を空けてください。
- 公開用配置予定: `src/assets/videos/results-color-range.mp4`

## 3. 固定プローブの履歴（動作確認後）

- 保存名: `results-probe-history.png`
- 掲載先: [ホバープローブ・固定プローブ](src/results/probe.md)
- データ: 円柱など、複数フレームで値が変わる公開可能な結果。
- 操作: 停止中に表面をクリックして固定し、`>`で途中のフレームへ進めます。
- 写してほしいもの: PINNED欄の部品・対象ID・物理量、HISTORYグラフの黄色い目印、モデル。
- 0.3.0向け開発版ではCSV保存の動作確認後に撮影し、**Export pinned history CSV...**と**Saved:**も含めてください。保存先は個人名・機密情報を含まないパスを使ってください。
- 保存先: `D:\Work\bevyistr\docs\media-inbox\results-probe-history.png`
- 公開用配置予定: `src/assets/images/results-probe-history.png`

## 4. 複数プローブ比較（受領・掲載済み）

- 2026-09-22受領：`media-inbox/results-probe-comparison.png`。
- 公開用に変更せずコピー：`src/assets/images/results-probe-comparison.png`。
- 掲載先：[複数対象の比較](src/results/probe.md)。Part 1のNode 103／949、NodalMISES、最終フレームの表示例。
- 静止画で確認した範囲は2曲線・色付きID・値の対応です。再生追従やRemove操作の証明とは区別します。

## 5. 固有値解析のモード表示（受領・掲載済み）

- 保存先: `D:\Work\bevyistr\docs\media-inbox\results-eigen-mode.png`
- データ: `15_eigen_spring/spring_vis_psf.0001.pvtu`の系列。
- 2026-09-23受領。変更せず`src/assets/images/results-eigen-mode.png`へコピーし、[連番とアニメーション](src/results/animation.md)に掲載しました。
- 確認範囲: Frame 3/5、Mode 3、Eigenvalue 3.260034e7、モード列の説明、変位コンター、All frames、変形倍率4.50。
- プローブは未固定です。HISTORY・CSV・連続操作の確認とは区別します。掲載画像の撮り直しは不要です。

## 6. 熱解析の温度・固定プローブ（受領・掲載済み）

- 保存先: `D:\Work\bevyistr\docs\media-inbox\results-heat-probe.png`
- データ: `16_heat_block/block_vis_psf.0001.pvtu`を直接開き、`TEMPERATURE`を選びます。
- 2026-09-23受領。変更せず`src/assets/images/results-heat-probe.png`へコピーし、[プローブ](src/results/probe.md)に掲載しました。
- 確認範囲: 温度コンター20〜100、PINNED Node 5845の100、Node 92の20との比較、Frame 1の2対象分の履歴、Deformation unavailable。
- 同時に受領した`probe_part1_node_5845.csv`は1行で、固定対象の温度・時刻・通常ステップ種別を元VTKと照合済み。
- 再生ボタンの無効化や比較解除の操作は静止画では未確認です。撮り直しは不要です。

## 7. 流体8ピースのフィールド・プローブ（画像受領・掲載保留）

2026-10-01、FrontISTRサポートへの問い合わせ中につき追加依頼を保留します。
以下は再開時の参考手順です。現時点では追加撮影・CSV提出は不要です。

- データ: `18_cavity_flow/cavityflow_vis_psf.0000.pvtu`。ソルバーの再実行は不要です。
- 保存先: `D:\Work\bevyistr\docs\media-inbox\results-flow-probe.png`
- 操作: `VELOCITY[1]`を選び、表面をクリックして固定。可能なら異なるPartからもう1対象を比較へ追加します。
- 写してほしいもの: モデル、8ピースの説明、選択成分、凡例0〜0.001、PINNED／COMPARISONのPartとNode。
- 固定対象を変えずに`VELOCITY[2]`へ切り替えた画面も`results-flow-constant.png`として同じフォルダに保存してください。Constant 0とPart／Nodeが分かる範囲で構いません。
- 各画面の状態でCSVを別名保存し、そのパスを教えてください。例: `flow_vx_partN_nodeM.csv`、`flow_vy_partN_nodeM.csv`。同じ既定名で上書きしないよう注意してください。
- `results-flow-probe.png`と`results-flow-constant.png`は受領済み・未掲載です。時刻0の表示を解析成功例として公開しません。比較解除・再生の操作確認も保留します。

## 後で依頼する候補（今回は不要）

- 左→右／右→左ドラッグによる選択の違い。
- オブジェクト→材料→決定と、材料色の確認。
- 接触候補のレビューと表示用分離。
- 正確な数値入力とビューポート操作の組み合わせ。

各章を詳しく整える段階で、改めて操作・ファイル名を指定します。撮影前に通知や個人パスを隠し、モデル・結果・画面内情報の公開可否を確認してください。
