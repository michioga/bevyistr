# Windows配布素材とライセンス

作成方法は[Windows ZIP手順](../docs/windows-package.md)を参照してください。

- about-windows.toml: Windows targetの依存をcargo-about 0.9.2で収集します。
  build依存を含め、dev依存を除外します。受け入れ対象外のライセンスや収集失敗は停止します。
- licenses.hbs: 依存一覧とライセンス本文をプレーンテキストで出力します。
- package-windows.ps1はレジストリ内の各依存からLICENSE/LICENCE/COPYING/COPYRIGHT/NOTICE/OFLも収集し、
  同梱Cコード等の通知を保持します。Bevy shaderの冒頭copyrightコメントも保持します。
- Rust toolchainのCOPYRIGHT-library.htmlとlicensesを追加します。
- licenses/FiraMono-LICENSE.txtはBevy埋め込みフォントのSIL Open Font License全文です。
  出典: [Bevy v0.19.1 FiraMono-LICENSE](https://raw.githubusercontent.com/bevyengine/bevy/v0.19.1/assets/fonts/FiraMono-LICENSE)。
  このコピーのSHA-256: `587dd9548304fd421cd254f7550bd101c1250ffc95417f2f7f92d55b44ecbe53`。

収集対象は実行時に使う機能より広い場合があります。依存の追加や埋め込み素材の変更時には、
自動収集結果だけでなく追加の通知が必要かも確認してください。
FrontISTR/MPI/Gmsh本体は配布物に含めません。
