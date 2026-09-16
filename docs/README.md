# マニュアルの執筆・確認・公開

利用者向けの入口は[はじめに](src/introduction.md)です。このファイルは執筆者向けです。

## 構成

- `book.toml`: mdBook設定。
- `src/SUMMARY.md`: 目次。章の追加・順序変更はここに反映します。
- `src/`: 公開する日本語マニュアル。実装済みの操作と、未実装・制限を区別します。
- `theme/manual.css`: 画像・動画の表示調整。
- `verification.md`: アプリの手動確認項目。確認記録は対象コミット・データ・環境を添えます。
- `media-requests.md`: 撮影依頼と素材の受け渡し方法。
- `media-inbox/`: 未確認素材の受け渡し場所。Git管理・公開対象外です。
- `book/`: 自動生成されるHTML。Git管理対象外で、直接編集しません。

既存の`results-guide.ja.md`には、新しい各章への案内を残しています。

## ローカル確認

mdBook **0.5.4**を使用します。CIも同じバージョンに固定します。インストール方法は[mdBook公式のCIガイド](https://rust-lang.github.io/mdBook/continuous-integration.html)を参照してください。

リポジトリのルートで実行します。Windows／Linuxで同じコマンドです。

```text
mdbook --version
mdbook build docs
mdbook serve docs --open
```

`serve`は編集内容を反映するローカルプレビューです。終了はCtrl+Cです。`build`は章や構成の生成確認であり、説明内容の正しさやリンク先サイトの存在まで検証するものではありません。追加したリンクと図版はプレビューでも確認してください。

## スクリーンショットと動画

必要な場面ごとに、[撮影依頼](media-requests.md)へファイル名・操作・掲載先を記録します。受け渡し用の`docs/media-inbox/`に保存し、内容確認後に公開用素材を`src/assets/images/`または`src/assets/videos/`へ配置します。元動画や不要なファイルを`src/`へまとめて置かないでください。mdBookの生成物に含まれます。

- 画像はPNG、動画はブラウザで再生できるMP4（H.264）を基本とします。
- 個人名・ユーザーディレクトリ・機密モデル・通知を写さず、データの公開可否も確認します。
- 画像には代替テキストと説明を付けます。動画を見なくても操作できる本文を残します。
- 動画は短い操作単位とし、自動再生せず、操作コントロールを表示します。
- 素材が届くまでは本文の「画面・動画は準備中」という案内に留めます。存在しないファイルへの埋め込みは作りません。

例えば`src/results/contours.md`からの動画埋め込みは、素材を配置してから次のように記述します。

```html
<video controls preload="metadata" playsinline>
  <source src="../assets/videos/results-color-range.mp4" type="video/mp4">
  お使いのブラウザでは動画を再生できません。
</video>
```

## GitHub Pages

公開予定URLは[bevyistrマニュアル](https://michioga.github.io/bevyistr/)です。これは公開先の予定であり、この構成を追加するだけでは公開しません。

`.github/workflows/docs.yml`は、ドキュメント変更のpush／pull requestでHTMLをビルドします。公開は次の手動手順に限定します。

1. 内容を確認してmainへマージします。
2. GitHubのSettings → Pages → Build and deploymentでSourceを**GitHub Actions**に設定します。
3. Actions → Documentation → Run workflowで**main**を選び、`publish`を有効にして実行します。
4. 実行完了後に公開ページの目次・画像・動画・検索を確認します。

main以外、または`publish`が無効の場合はビルドだけです。Pagesの設定や環境保護ルールによっては管理者による許可が必要です。手順は[GitHub公式のカスタムPagesワークフロー](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)に基づきます。

`site-url`はプロジェクトサイト用の`/bevyistr/`です。公開先の変更時は`book.toml`も更新してください。[mdBook公式ガイド](https://rust-lang.github.io/mdBook/continuous-integration.html)も参照できます。

アプリのバージョンはこのドキュメント整理では変更しません。mainへのマージ時にリリース内容と合わせて更新します。
