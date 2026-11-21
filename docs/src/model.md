# Model：モデルと部品配置

## 読み込む

- **Open Model**：STEPまたはメッシュのファイルを開き、現在のモデルを置き換えます。
- **Add Part**：STEPまたはメッシュのファイルを、現在のアセンブリに部品として追加します。
- **Open Project**：`hecmw_ctrl.dat`から参照メッシュとCNTの設定を一緒に読み込みます。

MSHだけを開くことと、解析設定を含むプロジェクトを開くことは異なります。既存チュートリアルの設定も確認したい場合はOpen Projectを使用します。

FrontISTRの`hecmw_ctrl.dat`は、メッシュと解析制御データの入力先、結果の出力先を指定します。MPIでは`part_in`が分割前のメッシュ、`part_out`が分割時の出力先、`fstrMSH`がソルバーの入力です。`HECMW-DIST`の指定にはランク番号を除いたファイル名を使います。 [全体制御データの例](https://source-docs.frontistr.com/execution_guide/procedure/02_parallel.html) · [メッシュの指定 `!MESH`](https://source-docs.frontistr.com/keyword_reference/dat/mesh.html)

HEC-MWの`.msh`は、節点・要素の接続、節点／要素／面グループ、材料・断面などを定義します。接触ペアと多点拘束の書式も、公式サイトの「キーワード → メッシュデータ (msh)」で確認できます。 [FrontISTR公式ドキュメント](https://source-docs.frontistr.com/index.html) · [要素 `!ELEMENT`](https://source-docs.frontistr.com/keyword_reference/msh/element.html) · [接触ペア `!CONTACT PAIR`](https://source-docs.frontistr.com/keyword_reference/msh/contactpair.html) · [多点拘束 `!EQUATION`](https://source-docs.frontistr.com/keyword_reference/msh/equation.html)

## STEPからメッシュを作る

0.5.0向け開発版では、Gmshを使って単一ソリッドの `.step`／`.stp` から一次四面体メッシュを生成できます。Gmshをインストールし、`gmsh` を `PATH` から実行できるようにします。

1. Modelの **STEP max size (mm)** に要素サイズの上限を入力します。`0` はGmshの自動設定です。
2. **Open Model** または **Add Part** からSTEPファイルを選び、生成後に表示されるX・Y・Zの寸法を確認します。
3. **Sets** に、ソリッド全体の要素グループ `STEP_SOLID` と、CAD面ごとの節点グループ `[N] STEP_FACE_*`・面グループ `[S] STEP_FACE_*` が表示されます。番号はGmshによる生成時の面番号です。
4. メッシュとグループを単体で保存するには、**PART POSITION** でSTEP由来の部品を選び、**Save FrontISTR Mesh (.msh)...** を使います。保存したファイルは **Open Model** で読み込めます。
5. 解析するには、**Materials** で材料とソリッド断面を設定し、**BC / Loads** で面の節点や要素面を使って拘束と荷重を設定します。座標はミリメートルです。材料と荷重にも整合する単位系を使用してください。
6. **Solve** の **Export** は `.msh`・`.cnt`・`hecmw_ctrl.dat` を出力します。**Run FrontISTR** は入力を作成して解析を開始します。MPI実行では領域分割用の入力も生成します。

生成に失敗した場合はModel欄に理由が表示されます。複数ソリッドはSTEPファイルを部品ごとに分け、**Add Part** で追加できます。 [GmshのSTEP読込](https://gmsh.info/doc/texinfo/#t20)

## 部品を配置する

1. PART POSITIONで対象部品を選びます。
2. MoveまたはRotateを選びます。
3. ビューポート編集を有効にし、矢印または回転リングをドラッグします。数値欄で厳密な値を確定できます。
4. ステップ量と軸別の`− | X | ＋`／`− | RX | ＋`などでも調整できます。軸の色はギズモと対応します。

部品の移動・回転は解析メッシュの座標を変更します。接触候補は移動後に再検出してください。Contactレビューの表示上の分離とは区別します。

**Check clearance to other parts**は選択部品と他部品の隙間・干渉を確認する読み取り専用の操作です。確認だけで部品を自動移動しません。
