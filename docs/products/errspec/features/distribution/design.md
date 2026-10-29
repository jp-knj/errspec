# npm配布と初期生成の設計

## このドキュメントについて

[Requirements](requirements.md)と[Product](../../product.md)に基づき、既存Rust CLIをnpmから起動し、型を選んで最小設定を作ります。

### 対象読者

CLI・配布を保守する開発者とAI。

### 前提知識

npm optionalDependenciesとOS・CPUのパッケージ選択。

### このドキュメントで扱わないこと

エラー型の意味と期待動作の決定は利用者に委ねます。

## 実装方針

CLIの小さなNodeランチャーが、OS・CPUに合うoptional dependencyからバイナリを解決して起動します。install/postinstallスクリプトで取得やビルドを行いません。ビルド済みパッケージはLinux x64 glibc、macOS arm64/x64を対象とし、各環境のCIでpackと実行を確認します。

初期生成はtsrsで対象型を検証して全タグを取得し、設定とテストを組み立てます。出力先の衝突を先に確認し、create_newで競合も拒否します。書込失敗時はその呼出が新規作成したファイルだけを取り除きます。

## アーキテクチャ・シーケンス図

短い直列処理のため図は省略します。npmランチャー、ネイティブCLI、型解析、生成の順です。

## データモデル・ER 図

CLIと各native packageは同じversionを持ち、native packageのbinファイルをランチャーが解決します。生成設定は既存のschemaVersion 2を使います。

## API・インターフェース

`init --project tsconfig.json --source src/errors.ts --export Failure --discriminant _tag --runner bun`で、既定のerrspec.jsonとerrspec.generated.test.tsを生成します。`--test`でテスト先、`--contract`で契約ID、既存の`--config`で設定先を選びます。未完成のテストはTODOメッセージを伴う例外を投げ、誤って合格させません。

## 外部サービスとの連携方式

npmは通常のパッケージ解決に使います。GitHub Actionsでnative binaryとnpm tarballをartifactにします。公開用の認証情報はコードに埋め込みません。

## データの取り扱い設計

配布物にソース、ライセンス、必要なランチャー・型定義・バイナリだけを含めます。テストと一時実行結果は含めません。

## 認証認可に関する特記事項

packとartifact生成はpublish権限不要です。公開はnpm scope権限と認証を設定した後の操作です。

## セキュリティ上の留意事項

プラットフォームの照合と版の固定を行います。生成テストへ文字列を埋め込む場合はJSON文字列としてescapeします。

## デプロイに関する特記事項

未公開期間はtarballをローカルインストールして検証します。公開済みと同じnpm導入例を、公開前の即実行可能なコマンドとして案内しません。

## 検討した代替手段

### postinstallでバイナリをダウンロードする

追加のネットワーク経路とinstall scriptsの許可が必要になるため採用せず、npmの整合性検証が効くプラットフォーム別パッケージを使います。

## 未決事項

npm scopeの所有権と初回公開認証は公開時に確認します。Linux musl・Windowsなどの追加は、実際にビルドと実行を確認した後に対応一覧へ載せます。
