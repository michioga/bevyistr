# Windows ZIPの作成と検証

0.3.0からの配布用手順です。commit・push・main統合・タグ作成はメンテナーが行います。
[公開チェックリスト](release-0.3.0.md)の確認を済ませてから正式公開します。

## ローカル候補

Windows、PowerShell 7、MSVC C++ビルドツールを使用します。

```powershell
rustup toolchain install 1.99.1 --profile minimal
cargo +1.99.1 install cargo-about --version 0.9.2 --features cli --locked
pwsh -File scripts/package-windows.ps1
$result = Get-Content target/windows-package-result.json -Raw | ConvertFrom-Json
pwsh -File scripts/test-windows-package.ps1 -Zip $result.zip -Checksums (Join-Path $result.artifact_directory 'SHA256SUMS.txt')
```

未コミットのスクリプトを検証するときだけ `-AllowDirty` を付けます。
そのZIPはBUILD-INFO.jsonの `source_dirty=true` で識別し、正式配布しません。
ローカルに入れたcargo-aboutは `-AboutTool <exeのパス>` で指定できます。
RUSTFLAGS / CARGO_ENCODED_RUSTFLAGSが設定されている場合は、配布ビルド用のシェルで解除してください。

出力先は `target/dist/<版番号・SHA・ランダムID>/` です。以前の候補は上書きしません。
`artifacts/` 内のZIPとSHA256SUMS.txtだけが配布対象です。
同階層のlicenses-private.jsonは収集用のローカルパスを含むため配布しません。

Rust 1.99.1、target x86_64-pc-windows-msvc、release、locked依存、
`-C target-feature=+crt-static` でビルドします。直接DLL参照をdumpbinで検査します。
GPUドライバーなど実行時ロードの依存や、別PCでの操作確認は別途必要です。

## GitHub Actions

[Windows package](../.github/workflows/windows-package.yml)はwindows-2025で実行します。
runnerの世代とRust版を指定していますが、runnerの更新まで固定するものではありません。
実際のrunner image番号・ツール版・ソースSHA・lockfileハッシュはBUILD-INFO.jsonへ記録します。

- ブランチの候補: Actions → Windows package → Run workflow。対象ブランチを選び、
  release_tagを空、attach_draftをOFFにして実行します。
- タグ: 既存の注釈付き `v0.3.0` をpushするとZIPを作成します。
  タグの版番号とCargoが一致し、コミットがorigin/mainに含まれることを検査します。
- Release下書き: 手動実行でrelease_tagに `v0.3.0`、attach_draftをONにします。
  同じタグから再ビルドしたZIPとチェックサムを下書きに添付します。
  既に公開したReleaseや同名添付ファイルは上書きしません。
  再ビルドしたZIPも改めて動作確認してください。別実行のZIPと同じハッシュとは限りません。

このworkflowはタグを作成せず、Releaseを正式公開せず、crates.ioへも公開しません。
通常ビルドはcontents: read、明示的な下書き添付ジョブだけcontents: writeです。
ダウンロードしたartifactの外側のZIPを展開すると、配布用ZIPとSHA256SUMS.txtが得られます。

既存のRelease checksで同じ候補SHAのWindows/Linuxテスト・配布ソースの検証も実施してください。
Windows packageの成功だけではそれらの代わりになりません。

## 同梱と確認

EXE、materials.toml、MIT LICENSE、README、第三者ライセンス、
BUILD-INFO.json、DLL-DEPENDENCIES.txtを同梱します。
ライセンス収集の出典・範囲は[distribution/README.md](../distribution/README.md)を参照してください。

検証スクリプトはZIPハッシュ、パスと重複、必須ファイル、各ファイルのハッシュ、
EXE版番号、個人設定・解析データが入っていないことを検査します。
チェックサムはコード署名ではありません。

正式候補では空白を含む別の展開先から起動し、版番号・アイコン・材料読込とReload・
設定保存/再起動・結果の読込/コンターを確認します。開発環境やoneAPIがないPCでも確認してください。
ローカルでPATHだけ制限した起動と、クリーンな別PCでの確認は区別します。
