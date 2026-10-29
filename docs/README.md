# errspecの仕様

この文書群は、knjとAIがerrspecの目的・振る舞い・設計を共有するための仕様です。
初版はこれまでの議論と試作から整理したレビュー用の案です。文書が存在することやテストの成功を、knjによる仕様の確認と同じ意味には扱いません。

読む順序は次のとおりです。

1. [Product](products/errspec/product.md): 誰の、どんな問題を解くか。
2. [Requirements](products/errspec/features/failure-verification/requirements.md): 利用者から見える振る舞いと受け入れ条件。
3. [Design](products/errspec/features/failure-verification/design.md): その条件を実現する仕組みと判断理由。

現在はDesignに実装・リファクタリング方針の案を追加しています。CLIの責任分離、検証済み入力の型、判定とレポートの分離の順で進め、既存の入出力と合否を維持する提案です。
機能追加や出力の意味の変更は、別の要件として判断します。

[仕様を共有する進め方](engineering/guidelines/specification-workflow.md)に従って更新します。
仕様の正本は日本語です。変更の経緯と確認結果はGitとPRに残します。

日本語は仕様を話し合う間の言語です。内容をknjと確認した後に英語のドキュメントへ整理し、sdevの起動スクリプト・設定・専用の参照指示を外します。
sdevを恒久的な依存やCIの必須条件にする予定はありません。

## 二つの仕様の役割

この文書群は、errspec自体を開発するための仕様です。
利用者が`errspec.json`に登録する失敗シナリオは、errspecで検証する対象アプリケーションの設定です。
sdevは前者の作成とレビューを支援し、errspecのCLIとtestkitは後者を照合します。

`./bin/sdev lint`はリンク切れと文書の参照方向を検査します。
文章の妥当性、人との合意、実装の適合性を自動で証明するものではありません。
