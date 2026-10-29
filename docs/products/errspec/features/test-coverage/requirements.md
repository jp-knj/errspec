# 既存テストによるエラーの網羅確認

## このドキュメントについて

[Product](../../product.md)に基づき、errspec専用のテストAPIを書かずに、既存のVitestテストがどのエラータグを確かめているかを示す機能を追加します。

### スコープ

`scan`で見つけた有限タグ型について、プロジェクトのVitestを実行し、成功したアサーションが受け取った値に含まれるタグを集計します。テストファイルに文字列として現れるだけのタグと、どのテストにも現れないタグを区別して示します。

### スコープ外

障害の呼出完了や必須比較の保証は`scenarioTest`と`verify`に残します。Bun、型の意味的な選別（業務上のエラーかどうか）、テストの自動生成は扱いません。

## ユーザーストーリー

### 設定なしで未確認のエラーを知る

開発者として、テストを書き換えずに、どのエラーがテストで確かめられていないかを知りたい。

**受け入れ条件:**

Scenario: 成功したアサーションがタグを受け取った場合、確認済みとする

- Given: `expect(result.error).toEqual({ type: "NOT_FOUND" })`が成功するテストがある。
- When: `errspec coverage`を実行する。
- Then: `NOT_FOUND`を`asserted`とし、確かめたテスト名を示す。

Scenario: タグがテストに現れるが値として確かめていない場合、区別する

- Given: モックの戻り値としてだけタグを書き、結果のHTTPステータスだけを比較するテストがある。
- When: `errspec coverage`を実行する。
- Then: そのタグを`mentioned`とし、`asserted`とは数えない。

Scenario: どのテストにも現れない場合、漏れとして示す

- Given: 実装が返すタグを、どのテストも扱っていない。
- When: `errspec coverage --fail-on-missing`を実行する。
- Then: そのタグを`missing`とし、終了コード1にする。

Scenario: 識別プロパティが違う同じ値のタグを、それぞれ集計する

- Given: `code: "Shared"`と`kind: "Shared"`の型があり、テストが前者だけを確かめている。テスト本文にはタグの値を直接書いていない。
- When: `errspec coverage --fail-on-missing`を実行する。
- Then: 前者は`asserted`、後者は`missing`として別々に集計し、終了コード1にする。

Scenario: テスト成功後に記録を検証できない場合、集計を成功としない

- Given: テストは成功したが、記録用ファイルに不正なJSONがある、またはファイルを取得できない。
- When: `errspec coverage`を実行する。
- Then: `EC_RUNNER`と終了コード2を返す。テストの実行済み状態とランナーの成否は実際の結果で報告する。

Scenario: 失敗・否定のアサーションは証拠にしない

- Given: 失敗したアサーション、または`.not`のアサーションがタグを含む値を受け取る。
- When: `errspec coverage`を実行する。
- Then: その値を`asserted`の根拠にしない。

## 機能要件

**入力:** tsconfig、識別プロパティ、Vitestの実行ファイル・設定（省略時は探索）、`vitest run`への追加引数。

**出力:** 型ごとのタグと状態（`asserted`・`mentioned`・`missing`）、確かめたテスト、集計、ランナーの成否。

アサーションの結果やテストの成否を変更しません。利用者のVitest設定とテストファイルを書き換えません。
