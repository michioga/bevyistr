# 0.3.0 公開前の依存監査

**2026-10-26再監査: 脆弱性0件、unsound警告0件。保守終了警告5件は残存。**

## 修正と再監査（2026-10-26）

Bevy 0.19.1と既存の描画機能を維持し、次の変更を行いました。

| 依存 | 変更 |
| --- | --- |
| wayland-scanner / quick-xml | 0.31.10 / 0.39.4 → 0.31.11 / 0.41.0 |
| ruzstd | 0.8.3を依存から除去。Bevy公式のzstd_cバックエンドへ変更 |
| anyhow | 1.0.102 → 1.0.103 |
| event-listener | 5.4.1 → 5.4.2 |
| memmap2 | 0.9.10 → 0.9.11 |
| clipboard-win | 5.4.1（yanked）→ 5.4.3。パッケージ検証時に判明した取り下げ済み版の参照も解消 |

Bevyの設定をworkspaceへ集約し、全11クレートで共有します。標準の`3d` / `3d_api`は
`zstd_rust`を再度有効にするため、3D機能を展開して`zstd_c`を選びました。
Cargoメタデータで、元のdefault機能をバックエンドと集約用feature名以外は維持していることを確認しました。
Bevy 0.20への移行、上流ソースの改変、監査除外は行っていません。

新たな依存はzstd 0.13.3、zstd-safe 7.3.0、zstd-sys 2.1.2+zstd.1.5.7です。
通常のビルドでは同梱Cソースをコンパイルして静的リンクします。WindowsはMSVC、Linuxは
Cコンパイラが必要です。既存のRelease checksのビルド環境で用意する構成です。
この変更だけで別途zstd DLLをインストールする必要はありません。

```powershell
./target/release-tools/bin/cargo-audit.exe audit --db target/advisory-db --file Cargo.lock --deny unsound --json
```

終了コード0。対象653パッケージ、脆弱性0件、unsound警告0件、unmaintained警告5件。
RustSec DBは1352件、commit `21d909e4c8bd4e14c7aea58bf980c881a0053ab1`。
OS/architectureの絞り込みやignore指定はありません。新しいDependency security workflowも
全lockfileを監査し、脆弱性またはunsound警告を検出すると失敗します。

### 更新後の検証

- Windows / Rust 1.99.1でアプリの開発ビルド成功。
- 全11クレートのパッケージ作成成功。最終lockfileではyanked警告なし。
- 展開した配布ソースでアプリビルド・workspaceテスト・UI単独check成功。
  テストは401成功、0失敗、8スキップ（実チュートリアル7件、実機MPI1件）。
  clipboard-win更新後にパッケージを作り直し、展開先とlockfileを更新して再確認した。
  内部クレートのみ展開パスへpatchする既存の検証方式で、第三者依存のchecksumは変更していない。
- TEMP/TMPはプロセス内でtarget配下、MSVCのPDB容量制限対策は`_LINK_`の`/DEBUG:NONE`。
  永続的な環境設定・Cargoプロファイルは変更していない。
- mdBookビルドと`git diff --check`成功。

上記は修正時の未コミット作業ツリーでの確認です。
2026-10-27追記: コミット5e50fddの[Dependency security](https://github.com/michioga/bevyistr/actions/runs/40288819495)成功を確認しました。
Windows ZIPとライセンス同梱のローカル検証も完了し、[リリース記録](release-0.3.0.md)へ追記しています。
Linuxビルド、registry向けdry-run、最終候補ZIPのGUI実機操作は別途必要です。

### 残る保守終了警告

以下は脆弱性の検出とは区別します。現在の依存を維持したまま置換できる修正版は
RustSecに記載されておらず、親ライブラリ側の移行を追跡します。警告は隠していません。

| 依存 | 確認した経路・今後の対応 |
| --- | --- |
| paste 1.0.15 | parry3d → simba → paste。数値演算・物理依存の上流移行を追跡 |
| ttf-parser 0.25.1 | Linux winit → sctk-adwaita → ab_glyph → owned_ttf_parser。ウィンドウ装飾の依存更新を追跡 |
| zbus_names 4.3.2 | Linux rfd → ashpd → zbus。親依存のzbus 6対応を追跡 |
| zvariant 5.12.0 | 同上。zbus 6への統合に伴う移行が必要 |
| zvariant_derive 5.12.0 | 同上 |

これらの解消のためだけに描画・物理・ファイルダイアログを削除したり、互換性のない版へ
強制置換したりはしていません。第三者ライセンスの収集、実機確認、正式候補CIは別の公開前確認です。

## 初回監査（2026-10-25、修正前の記録）

以下は修正前の結果です。現在の状態は上の再監査を参照してください。

対象は6e8ab53から版番号のみ0.3.0へ更新したCargo.lockです。外部依存は更新していません。
`cargo-audit 0.22.2`をリポジトリの`target/release-tools`へインストールし、次の監査を実行しました。

```powershell
./target/release-tools/bin/cargo-audit.exe audit --db target/advisory-db --file Cargo.lock --json
```

終了コード1、対象652パッケージ。RustSec DBは1352件、commit
`21d909e4c8bd4e14c7aea58bf980c881a0053ab1`（取得時の最終更新2026-10-24）。
ignore指定・target絞り込みは行っていません。これはライセンス監査ではありません。

## 検出された脆弱性3件

| 依存 | Advisory | 修正済み範囲 | 確認した経路 |
| --- | --- | --- | --- |
| quick-xml 0.39.4 | [RUSTSEC-2026-0194](https://rustsec.org/advisories/RUSTSEC-2026-0194.html) | >=0.41.0 | Cargo.lockのwayland-scanner 0.31.10からの依存 |
| quick-xml 0.39.4 | [RUSTSEC-2026-0195](https://rustsec.org/advisories/RUSTSEC-2026-0195.html) | >=0.41.0 | 同上 |
| ruzstd 0.8.3 | [RUSTSEC-2026-0344](https://rustsec.org/advisories/RUSTSEC-2026-0344.html) | >=0.9.1 | Windows向け依存ツリーでbevy_image 0.19.1からの依存を確認 |

quick-xmlの2件はXML処理のCPU/メモリ消費、ruzstdは圧縮データ展開時のメモリ枯渇に関する報告です。
bevyistrへの入力から該当処理が実際に到達可能かは未評価です。
この一覧だけでbevyistrの結果ファイル読込がそのまま悪用可能とは断定しません。
Windows向け`cargo tree -i quick-xml`には表示されませんでした。Linuxを含む配布方針なので無視はしません。
全targetのoffline treeは未取得のab_glyph依存で停止したため、全環境の経路確認完了とは扱いません。

## その他の警告8件

unsound（健全性に関する警告）:

- anyhow 1.0.102: RUSTSEC-2026-0190、修正>=1.0.103。
- event-listener 5.4.1: RUSTSEC-2026-0221、修正>=5.4.2。
- memmap2 0.9.10: RUSTSEC-2026-0186、修正>=0.9.11。

unmaintained（保守状況の警告。脆弱性の検出とは区別）:

- paste 1.0.15: RUSTSEC-2024-0436。
- ttf-parser 0.25.1: RUSTSEC-2026-0192。
- zbus_names 4.3.2: RUSTSEC-2026-0371。
- zvariant 5.12.0: RUSTSEC-2026-0370。
- zvariant_derive 5.12.0: RUSTSEC-2026-0372。

## 初回時点の対処方針

1. anyhow/event-listener/memmap2とwayland-scannerについて、互換範囲内の更新と再監査を検討する。
   crates.ioメタデータではwayland-scanner 0.31.11のquick-xml要求は^0.41。
2. bevy_image 0.19.1のruzstd要求は^0.8.0なので、0.9.1へlockfileだけで差し替えない。
   使用するBevy機能・展開バックエンドの見直し、または上流更新が必要。勝手な互換性偽装や監査除外はしない。
   2026-10-25のレジストリでBevy 0.19系の最新は0.19.1。0.20への移行は単なるパッチ更新ではない。
3. 保守終了警告について親依存・利用箇所・代替を確認し、対応または残存理由を記録する。
4. 変更した場合はWindows/Linuxのビルド・回帰・実機確認と監査をやり直す。

初回監査時点では上記依存の更新、監査除外、タグ作成、crates.io/GitHub Release公開は行っていません。
監査ツール・DB・ビルド生成物はgit管理外のtarget内にあり、ユーザーの恒久PATHや認証設定は変更していません。
