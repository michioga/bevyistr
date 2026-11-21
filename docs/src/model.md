# Model：モデルと部品配置

## 読み込む

- **Open Mesh**：新しいモデルとしてメッシュを開きます。
- **Add Mesh**：現在のアセンブリにメッシュを追加します。
- **Open Project**：`hecmw_ctrl.dat`から参照メッシュとCNTの設定を一緒に読み込みます。

MSHだけを開くことと、解析設定を含むプロジェクトを開くことは異なります。既存チュートリアルの設定も確認したい場合はOpen Projectを使用します。

FrontISTRの`hecmw_ctrl.dat`は、メッシュと解析制御データの入力先、結果の出力先を指定します。MPIでは`part_in`が分割前のメッシュ、`part_out`が分割時の出力先、`fstrMSH`がソルバーの入力です。`HECMW-DIST`の指定にはランク番号を除いたファイル名を使います。 [全体制御データの例](https://source-docs.frontistr.com/execution_guide/procedure/02_parallel.html) · [メッシュの指定 `!MESH`](https://source-docs.frontistr.com/keyword_reference/dat/mesh.html)

HEC-MWの`.msh`は、節点・要素の接続、節点／要素／面グループ、材料・断面などを定義します。接触ペアと多点拘束の書式も、公式サイトの「キーワード → メッシュデータ (msh)」で確認できます。 [FrontISTR公式ドキュメント](https://source-docs.frontistr.com/index.html) · [要素 `!ELEMENT`](https://source-docs.frontistr.com/keyword_reference/msh/element.html) · [接触ペア `!CONTACT PAIR`](https://source-docs.frontistr.com/keyword_reference/msh/contactpair.html) · [多点拘束 `!EQUATION`](https://source-docs.frontistr.com/keyword_reference/msh/equation.html)

## STEPからメッシュを作る

0.5.0向け開発版では、Gmshを使って単一ソリッドの `.step`／`.stp` から一次四面体メッシュを生成できます。Gmshをインストールし、`gmsh` を `PATH` から実行できるようにします。

1. Modelの **STEP max size (mm)** に要素サイズの上限を入力します。`0` はGmshの自動設定です。
2. **Open Mesh** または **Add Mesh** からSTEPファイルを選びます。
3. 生成後に表示されるX・Y・Zの寸法を確認し、材料・断面・拘束・荷重を設定します。メッシュ座標はミリメートルです。材料・荷重などの値も整合する単位系で指定してください。

生成に失敗した場合はModel欄に理由が表示されます。複数ソリッドはSTEPファイルを部品ごとに分け、**Add Mesh** で追加できます。 [GmshのSTEP読込](https://gmsh.info/doc/texinfo/#t20)

## 部品を配置する

1. PART POSITIONで対象部品を選びます。
2. MoveまたはRotateを選びます。
3. ビューポート編集を有効にし、矢印または回転リングをドラッグします。数値欄で厳密な値を確定できます。
4. ステップ量と軸別の`− | X | ＋`／`− | RX | ＋`などでも調整できます。軸の色はギズモと対応します。

部品の移動・回転は解析メッシュの座標を変更します。接触候補は移動後に再検出してください。Contactレビューの表示上の分離とは区別します。

**Check clearance to other parts**は選択部品と他部品の隙間・干渉を確認する読み取り専用の操作です。確認だけで部品を自動移動しません。
