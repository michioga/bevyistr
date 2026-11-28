# Contact：接触の定義と確認

接触面や節点を選び、NODE-SURF／SURF-SURFなどの対応関係を確認します。Small、Finite、Tiedの挙動、摩擦係数、必要に応じたペナルティ値を設定します。

## STEPの面グループから接触を定義する

1. STEPから生成したメッシュを確定し、**PART POSITION** で部品を配置します。接触予定面の細分化は、メッシュ確定前のプレビューで行います。 [メッシュと局所細分化の操作](model.md#接触予定面を細かく分割する)
2. **Contact → CONTACT DEFINITION** の一覧からSlave側の `STEP_FACE_*` を選び、**Capture Slave** を押します。複数部品では `[1]`、`[2]` などが部品番号です。**NODE-SURF** は節点グループ `[N]`、**SURF-SURF** は面グループ `[S]` を使います。
3. Master側の面グループ `[S]` を選んで **Capture Master** を押し、青（Master）とオレンジ（Slave）のプレビューを確認して **Create Contact Pair** を押します。 [FrontISTRの接触ペア仕様](https://source-docs.frontistr.com/keyword_reference/msh/contactpair.html)
4. **Materials** と **BC / Loads** で両部品の物性・断面・拘束・荷重を設定します。**Solve** で **Nonlinear** を選び、**Run FrontISTR** で計算します。

## 候補を検出する

1. 部品の実際の位置関係を確認します。微調整はContact内のPART POSITIONからも行えます。
2. 検出距離（**Search gap**）と法線角度の許容値を確認して **Detect Contact Candidates** を実行します。
3. 候補のMaster／Slave面を確認し、必要な組を **Accept as …** で登録します。不要な組は **Reject** で候補一覧から外します。

候補が0件なら、検出条件と部品間の距離を確認してください。候補が存在することだけでは、解析上適切な接触定義であることは保証されません。

**PART POSITION** で部品を動かすと、直前の候補一覧が消えます。変更後の位置で再検出し、面を確認してから登録します。登録済みの接触は定義一覧で確認できます。

Ghost表示やレビュー用の分離は隠れた面を確認するための表示上の補助です。解析座標とエクスポートは変更しません。PART POSITIONによる実際の移動とは異なります。

MPCや剛体スパイダーは接触とは別の拘束です。定義した式の対象・自由度・係数を確認してから出力します。詳細な操作例は今後追加します。

## 接触を含むプロジェクトを開き直す

bevyistrで出力した非線形静解析の接触設定を **Open Project** で再読込できます。`ALAGRANGE`／`SLAGRANGE` の接触形式、摩擦係数、第2行の摩擦ペナルティ、固定増分ステップごとの接触グループ参照を保持します。FrontISTRのヘルツ接触例題もこの範囲に入ります。 [接触条件](https://source-docs.frontistr.com/keyword_reference/cnt/contact.html) · [ヘルツ接触例題](https://source-docs.frontistr.com/examples/tutorial_09.html)

同名のSTEP部品などを統合出力するときは、グループ名の衝突を防ぐため `PART1_`、`PART2_` などの接頭辞を使います。生成されたCNTの拘束・荷重とMSHの接触ペアは同じ接頭辞を参照します。
