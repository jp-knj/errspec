# 既存テストによるエラーの網羅確認

## このドキュメントについて

[Product](../../product.md)に基づき、errspec専用のテストAPIを書かずに、既存のVitestテストがどのエラータグを確かめているかを示す機能を追加します。

### スコープ

有限タグ型について、プロジェクトのVitestを実行し、成功したテストのアサーションが期待値として要求したタグを集計します。比較対象に含まれるだけのタグは確認済みにしません。テストファイルに文字列として現れるだけのタグと、どのテストにも現れないタグを区別して示します。

### スコープ外

障害の呼出完了や必須比較の保証は`scenarioTest`と`verify`に残します。Bun、型の意味的な選別（業務上のエラーかどうか）、テストの自動生成は扱いません。

## ユーザーストーリー

### 設定なしで未確認のエラーを知る

開発者として、テストを書き換えずに、どのエラーがテストで確かめられていないかを知りたい。

**受け入れ条件:**

Scenario: 成功したテストがタグの一致を要求した場合、確認済みとする

- Given: `expect(result.error).toEqual({ type: "NOT_FOUND" })`が成功するテストがある。
- When: `errspec coverage`を実行する。
- Then: `NOT_FOUND`を`asserted`とし、確かめたテスト名を示す。

Scenario: 比較対象にタグがあっても期待値が要求しない場合、確認済みにしない

- Given: タグを含む戻り値に対して、`toMatchObject({ ok: false })`やタグを指定しない`toHaveProperty`だけを実行する。
- When: 網羅確認を実行する。
- Then: そのタグを`asserted`には数えない。

Scenario: 比較の後にテストが失敗した場合、確認済みにしない

- Given: タグの比較が成功した後、同じテストが失敗する。
- When: 網羅確認を実行する。
- Then: 失敗したテストの比較を`asserted`の根拠にせず、ランナーの失敗も報告する。

Scenario: タグがテストに現れるが値として確かめていない場合、区別する

- Given: モックの戻り値としてだけタグを書き、結果のHTTPステータスだけを比較するテストがある。
- When: `errspec coverage`を実行する。
- Then: そのタグを`mentioned`とし、`asserted`とは数えない。

Scenario: どのテストにも現れない場合、漏れとして示す

- Given: 実装が返すタグを、どのテストも扱っていない。
- When: `errspec coverage --fail-on-missing`を実行する。
- Then: そのタグを`missing`とし、終了コード1にする。

Scenario: 失敗・否定のアサーションは証拠にしない

- Given: 失敗したアサーション、または`.not`のアサーションがタグを含む値を受け取る。
- When: `errspec coverage`を実行する。
- Then: その値を`asserted`の根拠にしない。

### 今ある漏れを認めたまま、後退だけを止める

開発者として、既存の漏れをすぐには直せなくても、新しい漏れや確認の後退をCIで止めたい。

**受け入れ条件:**

Scenario: 保存した状態から後退した場合、失敗にする

- Given: `errspec coverage --save-baseline coverage-baseline.json`で現在のタグの状態を保存した。
- When: テストの変更で、`asserted`だったタグが`mentioned`か`missing`に、または`mentioned`だったタグが`missing`になり、`--baseline coverage-baseline.json`で実行する。
- Then: 後退したタグを示し、終了コード1にする。

Scenario: 保存時になかったタグが未確認で現れた場合、失敗にする

- Given: 型に新しいタグが加わり、どのテストにも現れない。
- When: `--baseline`付きで実行する。
- Then: 新しい`missing`として示し、終了コード1にする。

Scenario: 改善や変わらない漏れは通す

- Given: 保存時から漏れが残っている、または確認が強くなった。
- When: `--baseline`付きで実行する。
- Then: 改善を示し、終了コードは後退がなければ0にする。既存のbaselineファイルは上書きしない。

## 機能要件

**入力:** tsconfig、識別プロパティ、Vitestの実行ファイル・設定（省略時は探索）、`vitest run`への追加引数。

**出力:** 型ごとのタグと状態（`asserted`・`mentioned`・`missing`）、確かめたテスト、集計、ランナーの成否。

アサーションの結果やテストの成否を変更しません。利用者のVitest設定とテストファイルを書き換えません。
