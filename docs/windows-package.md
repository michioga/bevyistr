# Windows MSIの作成と検証

v0.6.0からのWindows x86_64配布手順です。

## ローカル候補

Windows、PowerShell 7、MSVC C++ビルドツール、WiX 7.0.1と同じ版のWixUI拡張を使います。WiXの利用条件を確認し、同意した環境でビルドしてください。PATHにWiXがない場合は、作成と検証の両方で `-WixTool <wix.exeのパス>` を指定できます。 [WiXの導入手順](https://docs.firegiant.com/wix/using-wix/) · [WiX Open Source Maintenance Fee EULA](https://docs.firegiant.com/wix/osmf/)

```powershell
rustup toolchain install 1.99.1 --profile minimal
cargo +1.99.1 install cargo-about --version 0.9.2 --features cli --locked
wix extension add -g WixToolset.UI.wixext/7.0.1
pwsh -File scripts/package-windows.ps1
$result = Get-Content target/windows-package-result.json -Raw | ConvertFrom-Json
pwsh -File scripts/test-windows-package.ps1 -Msi $result.msi -Checksums (Join-Path $result.artifact_directory 'SHA256SUMS.txt')
```

未コミットの変更をローカルで検証するときは `-AllowDirty` を付けます。その候補には `source_dirty=true` が記録されます。cargo-aboutを別の場所にインストールした場合は `-AboutTool <exeのパス>` で指定できます。配布ビルド用のシェルでは `RUSTFLAGS` と `CARGO_ENCODED_RUSTFLAGS` を解除してください。

出力先は `target/dist/<版番号・SHA・ランダムID>/` です。配布対象は `artifacts/` にあるMSIと `SHA256SUMS.txt` です。ライセンス収集に使う `licenses-private.json` はローカルパスを含み、同階層に留めます。

Rust 1.99.1で `x86_64-pc-windows-msvc` を対象に、release・locked依存・`-C target-feature=+crt-static` でビルドし、dumpbinで直接DLL参照を検査します。

## GitHub Actions

[Windows package](../.github/workflows/windows-package.yml)は `windows-2025` で実行します。メンテナーはWiXのEULAを確認し、同意後にリポジトリの **Settings → Secrets and variables → Actions → Variables** に `WIX7_EULA_ACCEPTED=true` を設定します。新規ランナーではこの設定を確認してからWiXを実行します。実際のランナーイメージ番号、ツールの版、ソースSHA、lockfileのハッシュは `BUILD-INFO.json` に記録されます。 [WiXの利用条件](https://docs.firegiant.com/wix/osmf/)

- **ブランチ候補**は **Actions → Windows package → Run workflow** で対象ブランチを選び、`release_tag` を空、`attach_draft` をOFFにして作成します。
- **タグ付き候補**は注釈付きの `vX.Y.Z` タグのpushで作成します。タグとCargoの版、およびコミットが `origin/main` に含まれることを検査します。
- **Release下書きへの添付**は手動実行で `release_tag` に注釈付きタグを指定し、`attach_draft` をONにします。対象版のリリースノート `distribution/release-notes-X.Y.Z.md` を同じタグに含めてください。検証済みMSIとチェックサムを下書きに添付します。正式公開はメンテナーが行います。

候補はActionsの `windows-x86_64` アーティファクトから取得できます。ダウンロードしたZIPを展開するとMSIと `SHA256SUMS.txt` が入っています。同じ候補SHAで **Release checks** も実施し、Windows／Linuxのテストと配布ソースを検証してください。

## パッケージの確認

検証スクリプトは、MSIをインストールせずに展開し、SHA-256、ユーザー単位のインストール先、デスクトップとスタートメニューのショートカット、「インストールされているアプリ」に登録するアイコン、製品の更新識別子、MSIとEXEの版番号、同梱ファイルを照合します。ウィザードが同梱のMITライセンス全文を表示し、同意後にインストールボタンが有効になる設定も検証します。収録するのは、EXE、標準 `materials.toml`、MIT LICENSE、README、第三者ライセンス、`BUILD-INFO.json`、`DLL-DEPENDENCIES.txt` です。 [ライセンスの収集範囲](../distribution/README.md)

正式候補はRust、FrontISTR、oneAPIのないWindows x86_64環境で次を確認します。

1. 標準ユーザーでMSIを開く。日本語のウィザードにMITライセンスの英文が表示され、同意欄を選ぶとインストールボタンが有効になることを確認する。インストール完了後、デスクトップとスタートメニューのショートカットにアプリのアイコンが表示され、両方から起動できることを確認する。版番号、メッシュと既存結果の読込も確認する。
2. ユーザー用の `materials.toml` を `%APPDATA%\bevyistr` に置く。**Materials → Reload materials.toml** で読込先を確認し、ユーザー設定とともに再起動後も使えることを確かめる。
3. 後続バージョンのMSIで更新し、デスクトップとスタートメニューのショートカット、および「インストールされているアプリ」の登録が各1件であることを確認する。ユーザー設定と編集した材料が引き継がれることを確かめる。
4. Windowsの設定からbevyistrをアンインストールする。インストール先のファイルと両方のショートカットが削除され、ユーザー設定、材料ファイル、インストール先の外に置いた解析データが残ることを確認する。

v0.5.0のZIPから移行する場合は、編集済みの材料ファイルを先にユーザー用の場所へコピーしてください。標準位置にある既存のユーザー設定は起動時に読み込まれます。 [インストールと移行](src/getting-started.md#windowsでmsiを使う)
