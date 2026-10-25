# 0.3.0 公開前の依存監査（2026-10-25）

**結果: 公開前に解決・評価が必要。監査は合格していません。**

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

## 次の対処

1. anyhow/event-listener/memmap2とwayland-scannerについて、互換範囲内の更新と再監査を検討する。
   crates.ioメタデータではwayland-scanner 0.31.11のquick-xml要求は^0.41。
2. bevy_image 0.19.1のruzstd要求は^0.8.0なので、0.9.1へlockfileだけで差し替えない。
   使用するBevy機能・展開バックエンドの見直し、または上流更新が必要。勝手な互換性偽装や監査除外はしない。
   2026-10-25のレジストリでBevy 0.19系の最新は0.19.1。0.20への移行は単なるパッチ更新ではない。
3. 保守終了警告について親依存・利用箇所・代替を確認し、対応または残存理由を記録する。
4. 変更した場合はWindows/Linuxのビルド・回帰・実機確認と監査をやり直す。

今回の作業では上記依存の更新、監査除外、タグ作成、crates.io/GitHub Release公開は行っていません。
監査ツール・DB・ビルド生成物はgit管理外のtarget内にあり、ユーザーの恒久PATHや認証設定は変更していません。
