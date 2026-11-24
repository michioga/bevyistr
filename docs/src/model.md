# Model：モデルと部品配置

## 読み込む

- **Open Model**：STEPまたはメッシュのファイルを開き、現在のモデルを置き換えます。
- **Add Part**：STEPまたはメッシュのファイルを、現在のアセンブリに部品として追加します。
- **Open Project**：`hecmw_ctrl.dat`から参照メッシュとCNTの設定を一緒に読み込みます。

MSHだけを開くことと、解析設定を含むプロジェクトを開くことは異なります。既存チュートリアルの設定も確認したい場合はOpen Projectを使用します。

FrontISTRの`hecmw_ctrl.dat`は、メッシュと解析制御データの入力先、結果の出力先を指定します。MPIでは`part_in`が分割前のメッシュ、`part_out`が分割時の出力先、`fstrMSH`がソルバーの入力です。`HECMW-DIST`の指定にはランク番号を除いたファイル名を使います。 [全体制御データの例](https://source-docs.frontistr.com/execution_guide/procedure/02_parallel.html) · [メッシュの指定 `!MESH`](https://source-docs.frontistr.com/keyword_reference/dat/mesh.html)

HEC-MWの`.msh`は、節点・要素の接続、節点／要素／面グループ、材料・断面などを定義します。接触ペアと多点拘束の書式も、公式サイトの「キーワード → メッシュデータ (msh)」で確認できます。 [FrontISTR公式ドキュメント](https://source-docs.frontistr.com/index.html) · [要素 `!ELEMENT`](https://source-docs.frontistr.com/keyword_reference/msh/element.html) · [接触ペア `!CONTACT PAIR`](https://source-docs.frontistr.com/keyword_reference/msh/contactpair.html) · [多点拘束 `!EQUATION`](https://source-docs.frontistr.com/keyword_reference/msh/equation.html)

## STEPからメッシュを作る

Gmshを使って単一ソリッドの `.step`／`.stp` から一次四面体メッシュを生成できます。Gmshをインストールし、`gmsh` を `PATH` から実行できるようにします。

1. Modelの **STEP max size (mm)** で目標要素サイズの上限を設定します。`0` ならGmshがモデル寸法から決めます。**STEP curve resolution** は曲率に応じた細分化の強さです。円一周あたりの目標分割数で、初期値は `20`、`0` で曲率による細分化を無効にします。値を上げると穴や丸みの周辺が細かくなり、離れた部分に向けて段階的に粗くなります。設定は次のSTEP読み込みやプレビューの再生成で反映されます。 [Gmshの要素サイズ設定](https://www.gmsh.info/doc/texinfo/gmsh.html#Specifying-mesh-element-sizes)
2. **Open Model** または **Add Part** からSTEPファイルを選びます。生成中は進捗バーと経過時間が表示されます。曲線・面の分割でGmshから進捗率が得られた場合、バーはその工程内の進捗を示します。進捗率が得られない工程ではバーが往復し、3D分割で報告された追加節点数も表示されます。
3. 生成後はプレビューを回転・拡大して、形状やメッシュの粗密を確認します。設定を調整したら **Regenerate Preview** で作り直します。**Use Mesh** でモデルの置き換えまたは部品の追加を確定し、**Discard** で元の表示に戻ります。
4. **Sets** に、ソリッド全体の要素グループ `STEP_SOLID` と、CAD面ごとの節点グループ `[N] STEP_FACE_*`・面グループ `[S] STEP_FACE_*` が表示されます。番号はGmshによる生成時の面番号です。
5. メッシュとグループを単体で保存するには、**PART POSITION** でSTEP由来の部品を選び、**Save FrontISTR Mesh (.msh)...** を使います。保存したファイルは **Open Model** で読み込めます。
6. 解析するには、**Materials** で材料とソリッド断面を設定し、**BC / Loads** で面の節点や要素面を使って拘束と荷重を設定します。座標はミリメートルです。材料と荷重にも整合する単位系を使用してください。
7. **Solve** の **Export** は `.msh`・`.cnt`・`hecmw_ctrl.dat` を出力します。**Run FrontISTR** は入力を作成して解析を開始します。MPI実行では領域分割用の入力も生成します。

生成に失敗した場合はModel欄に理由が表示されます。複数ソリッドはSTEPファイルを部品ごとに分け、**Add Part** で追加できます。 [GmshのSTEP読込](https://gmsh.info/doc/texinfo/#t20)

### 接触予定面を細かく分割する

1. **Use Mesh** を押す前に、プレビューで接触予定のCAD面を左クリックします。面全体がオレンジ色に変わります。同じ面をもう一度クリックすると解除できます。
2. **Contact size (mm)** に接触面の目標要素サイズを入力し、**Regenerate Preview** を押します。Gmshが指定した面を細かく分割し、周囲の要素サイズを段階的に変えます。全体のサイズ上限と曲率による細分化も有効です。`0` で接触予定面へのサイズ指定を解除できます。 [Gmshのサイズフィールド](https://www.gmsh.info/doc/texinfo/gmsh.html#Gmsh-mesh-size-fields)
3. 面の番号とメッシュを確認し、**Use Mesh** で確定します。相手部品にも局所サイズを指定する場合は、その部品を **Add Part** で開き、同じ手順を行います。
4. 部品を配置した後、**Model → Sets** で該当する `STEP_FACE_<番号>` の面グループ `[S]` を選び、**Contact** で **Capture Master** または **Capture Slave** を使います。NODE-SURF の Slave には同じ番号の節点グループ `[N]` を選びます。両側を取り込んだら **Create Contact Pair** で登録します。 [接触の定義と確認](contact.md)

局所サイズの指定はSTEPプレビュー中に行います。接触ペアは、メッシュを確定して部品を配置した後に登録します。

## 部品を配置する

1. PART POSITIONで対象部品を選びます。
2. MoveまたはRotateを選びます。
3. ビューポート編集を有効にし、矢印または回転リングをドラッグします。数値欄で厳密な値を確定できます。
4. ステップ量と軸別の`− | X | ＋`／`− | RX | ＋`などでも調整できます。軸の色はギズモと対応します。

部品の移動・回転は解析メッシュの座標を変更します。接触候補は移動後に再検出してください。Contactレビューの表示上の分離とは区別します。

**Check clearance to other parts**は選択部品と他部品の隙間・干渉を確認する読み取り専用の操作です。確認だけで部品を自動移動しません。
