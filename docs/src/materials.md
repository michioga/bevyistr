# Materials：材料の選択と割り当て

**対象を選ぶ → 材料を選ぶ → Confirm**の順に操作します。

1. ビューポートまたは一覧から部品・要素グループを選びます。対象はシアンの枠で示されます。
2. プロジェクト内の材料、または`materials.toml`の材料を選びます。
3. ライブラリを使う場合はモデルの単位系を選び、数値を確認します。
4. **Confirm**で割り当てます。選んだだけでは解析設定を変更しません。

**Color: Material**では同名材料を同じ色で表示します。色が近い場合もあるため、材料名と割り当て情報で最終確認してください。

## 非圧縮ニュートン流体

`18_cavity_flow` を確認する場合は、**Model → Open Project** で例題の `hecmw_ctrl.dat` を開き、**Materials** のプロジェクト材料一覧で `FLUID` を選びます。**Viscosity mu** に `cavityflow.cnt` の `!FLUID`、**Density rho** に `!DENSITY` の値が表示されます。例題ではそれぞれ `0.001` と `1000` です。メッシュ側のゼロ値は節点4自由度の流体要素 `3414` 用の材料定義として保持し、粘度・密度にはCNT側の値を使います。[`!FLUID` の仕様](https://source-docs.frontistr.com/keyword_reference/cnt/fluid.html) · [`!DENSITY` の仕様](https://source-docs.frontistr.com/keyword_reference/cnt/density.html)

両方の値をモデルの単位系で入力します。Enterで確定し、Escで入力前の値に戻ります。対応範囲は、有限で正の一定の粘度と密度を持つ `INCOMP_NEWTONIAN` です。温度依存のテーブルや他の流体モデルが含まれる場合は、Solveの互換性表示で確認できます。

例題の `.msh` だけを開いた場合は、粘度と密度が未定義なので両欄を空欄で表示します。MSHの再出力では元の `!SECTION, TYPE=SOLID` の割り当てと、同名の `!MATERIAL, ITEM=3` のゼロ3項目を保持します。プロジェクト全体を書き出す手順は、[流体例題のMPI実行](solve/index.md#18_cavity_flow-をmpiで書き出し実行する)を参照してください。 [FrontISTRの `!MATERIAL` 仕様](https://source-docs.frontistr.com/keyword_reference/msh/material.html) · [`!SECTION` 仕様](https://source-docs.frontistr.com/keyword_reference/msh/section.html)

## materials.toml

起動時と **Reload materials.toml** では、次の順でファイルを探します。

1. 実行ファイルのフォルダーとは別の作業ディレクトリにある `materials.toml`
2. ユーザー用の `materials.toml`。Windowsでは `%APPDATA%\bevyistr\materials.toml`、Linuxでは `$XDG_CONFIG_HOME/bevyistr/materials.toml`（未指定なら `~/.config/bevyistr/materials.toml`）
3. 実行ファイルの隣にある標準の `materials.toml`

WindowsのMSI版は、標準ファイルをユーザー用の場所へコピーしてから編集します。Materialsに表示されるパスで読込先を確認できます。

いずれにもファイルがない場合は、アプリ内蔵の標準材料を使い、**Bundled defaults**と外部ファイルの配置先を表示します。ファイルは自動作成・上書きしません。表示された場所へ`materials.toml`を置いてReloadすると、外部の設定へ切り替わります。既存ファイルの記述ミスや読み込みエラーは、内蔵版へ黙って切り替えずエラーとして表示します。

外部ファイルを編集後、**Reload materials.toml**で再読み込みできます。保存値はPa、kg/m³で、割り当て時に選択したモデル単位系へ変換します。既存形状・荷重・読み込み済み材料をまとめて変換する機能ではありません。

同じプロジェクト材料の物性を変更すると、その材料を使うすべてのセクションに影響します。数値欄はEnterで確定、Escで未確定変更を取り消します。

付属材料は基本的な等方弾性の参考値です。対象の材種・温度・解析条件への適合性を確認してください。
