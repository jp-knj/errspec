# errspec

**エラー型と、失敗時に期待する振る舞いをつなぐ。**

TypeScriptのエラー型・失敗シナリオ・テスト結果を対応付け、検証漏れを検出するRust製CLIです。
型の解決には[oxc-project/tsrs](https://github.com/oxc-project/tsrs/tree/1bfd1e65be95131f9242a03687d37006214a96f6)を使用します。
既存のResult型を使い続けられます。エディタ拡張は不要です。npm/crates.ioには未公開です。

`scan`でプロジェクトからエラー候補と伝播経路を調べ、`init`で選んだ型の設定・テストを生成できます。
npm CLIとLinux/macOS向けバイナリは配布準備中です。[配布物の作成と導入](packages/cli/README.md)を参照してください。

```text
EC010 checkout.payment RateLimited rate-limit-retries:
Required assertion did not pass: attempts
```

## 最初に動かす

必要なものはRust 1.99.1とBun 1.2.19以降です。初回ビルドではGitHub/crates.ioへの接続が必要です。
Bun 1.2.18以前はJUnitレポートにテストファイルを出力しないため、`verify`はテストを起動する前に`EC_RUNNER`で停止します。
Rustはrust-toolchain.tomlで固定しています。Linux x86_64、Bun 1.4.4、Vitest 5.0.5、Node.js 24で検証しました。

```sh
cargo build --locked
./target/debug/errspec verify --runner bun \
  --config examples/scenarios-bun/errspec.json --format json
```

この例では、`Declined`の1ケースと`RateLimited`の2ケースを実行します。
初回で停止するケースと3回試行して停止するケースを別々に検証し、いずれも注文を作成しないことを確認します。
Bun版はnpm install不要です。

Vitest版も同じ仕組みです。

```sh
(cd examples/scenarios-vitest && npm ci --ignore-scripts --no-audit --no-fund)
./target/debug/errspec verify --runner vitest \
  --config examples/scenarios-vitest/errspec.json --format json
```

## 既存テストで確かめる

テストを書き換えずに、どのエラーが確かめられていないかを調べられます。設定ファイルも`scenarioTest`も不要です。

```sh
errspec coverage --project tsconfig.json
errspec coverage --project apps/api/tsconfig.json -- --project api   # vitest runへ引数を渡す
```

`scan`で見つけた有限タグ型ごとに、タグを3つに分けて示します。

| 状態 | 意味 |
|---|---|
| `asserted` | 成功したアサーション（`toEqual`、`toMatchObject`、`toBe`など）が、そのタグを持つ値を受け取った |
| `mentioned` | テストファイルにタグが書かれているが、値として確かめていない（モックの入力だけなど） |
| `missing` | どのテストにも現れない |

最も近い`vitest.config.*`と`node_modules/.bin/vitest`を親ディレクトリへ探します。利用者の設定とテストは変更せず、一時設定で記録用のsetup fileを追加して実行します。
`--fail-on-missing`で`missing`があれば終了コード1にします。現在はVitestだけに対応します。
障害が本当に呼ばれたか、必須の比較がそろっているかまで保証したい箇所は、次の`scenarioTest`と`verify`を使います。

## 設定とテスト

既存プロジェクトで一つの型から始める場合:

```sh
errspec scan --project tsconfig.json --format json
errspec init --project tsconfig.json --source src/errors.ts --export Failure --runner bun
errspec verify --runner bun --only-contract-tests
```

`init`はerrspec.jsonとerrspec.generated.test.tsを生成します。`--test`と`--config`で出力先を選べます。
`--source`はtsconfigのあるディレクトリから、出力先は呼出元から解決します。出力ディレクトリは先に作成してください。
既存ファイルは上書きしません。生成テストはTODOで失敗するため、依存を`s.fault`で呼び出し、期待動作を`s.check`で比較する処理を記入します。
別途、同じ版の`@errspec/testkit`と選んだランナーを導入してください。公開前は配布tarballまたはローカル依存を使用します。

`--only-contract-tests`は設定で参照したテストファイルだけをランナーへ渡します。省略時は従来どおりランナーが発見した全テストを実行します。
選択実行の成功は、選ばなかったテストの成功を意味しません。

設定のschemaVersion 2では、1つのタグに複数のシナリオを登録できます。
`id`は契約内で一意です。`observe`に必須の障害観測とアサーションのIDを登録します。
同じIDを故意に別のシナリオへ流用しないでください。

```json
{
  "schemaVersion": 2,
  "project": "tsconfig.json",
  "contracts": [{
    "id": "checkout.payment",
    "source": {
      "file": "src/errors.ts",
      "export": "PaymentError",
      "discriminant": "_tag"
    },
    "boundaries": ["POST /checkout"],
    "cases": [{
      "id": "rate-limit-first",
      "tag": "RateLimited",
      "expect": "Return 429 without creating an order",
      "test": { "file": "tests/checkout.test.ts" },
      "observe": {
        "faults": ["gateway"],
        "assertions": ["status", "orders"]
      }
    }]
  }]
}
```

ほかのタグが型にあれば、そのケースも登録が必要です。
`source.file`、`project`、`test.file`は設定ファイルのあるディレクトリから解決します。
`boundaries`は利用者の宣言であり、関数間の伝播を推論した結果ではありません。
`expect`は人がレビューする説明です。文章を実行可能な仕様に自動変換しません。

Bun/Vitestの`test`を補助APIへ渡します。以下は、このリポジトリのexample内に置く例です。

```ts
import { test } from "bun:test"; // Vitestでは "vitest"
import { scenarioTest } from "../../../packages/testkit/index.mjs";
import { checkout } from "../src/checkout";

scenarioTest(test, import.meta.url, {
  contract: "checkout.payment",
  id: "rate-limit-first",
  tag: "RateLimited",
}, async (s) => {
  let orders = 0;
  const charge = s.fault("gateway", async () => ({
    ok: false as const,
    error: { _tag: "RateLimited" as const, retryAfter: 30 },
  }));

  const response = await checkout({
    charge,
    createOrder: async () => { orders++; },
  });

  s.check("status", response.status, 429);
  s.check("orders", orders, 0);
});
```

- `s.fault(id, fn)`: `fn`が実際に呼ばれ、resolveまたはrejectするまでを数えます。定義しただけでは観測済みになりません。戻り値は常にPromiseです。
- `s.check(id, actual, expected)`: `node:assert/strict`のdeep equalityを実行し、結果を記録します。空のcallbackで検証済みにするAPIはありません。非同期の値は先にawaitします。
- 失敗したcheckをcatchしても、そのシナリオは失敗します。
- 障害が未呼び出し／処理中、checkが0件の場合は補助API自体も失敗します。
- 入力だけで結果が決まるシナリオ（入力チェックなど）は、設定で`"faults": []`を書き、`scenarioTest`の定義に`withoutFaults: true`を指定します。両方そろわないと失敗します。`assertions`は1件以上必要です。
- 設定に必須としたcheckが欠けていれば、テストが成功していてもRust側の照合が失敗します。
- テスト名は`errspec ["契約ID","シナリオID"]`として自動生成します。設定の`test.name`は省略します。
- 定義に`description`を渡すと、ランナーの表示名は`errspec ["checkout.payment","declined"] [異常系] 注文を作らない`のように説明付きになります。照合は先頭の固定部分で行うため、説明を変えてもシナリオの変更にはなりません。説明は1行の空でない文字列です。
- `scenarioTest`はトップレベルで登録してください。`describe`内の登録は現在未対応で、完全なテスト名が一致せず失敗します。テストがtsconfigのプロジェクトに含まれていれば、`check`が実行前に`EC017`で報告します。
- `test.concurrent`は対応済みです（BunではBun 1.3以降が提供します）。`test.skip`は未検証になります。retry/repeatで同一シナリオが複数回実行された場合は曖昧として拒否します。

依存を引数で受け取らず直接importしているコードには、Bunの`mock.module`やVitestの`vi.mock`で`s.fault`を差し込みます。
手順は[testkitのREADME](packages/testkit/README.md#直接importしている依存に障害を差し込む)を参照してください。

パッケージの移植方法とAPIは[packages/testkit/README.md](packages/testkit/README.md)を参照してください。

## 確認できること

### 実行前の静的照合

`check`と`verify`は、登録したテストファイルがtsconfigのプロジェクトに含まれていれば、テストを実行する前に設定と照合します。
tsrsの型情報で、`scenarioTest`と`s.fault`／`s.check`を名前ではなく宣言から特定します。IDやタグは各値の型から読むため、`const`の定数や`as const`の配列をループで回す書き方でも照合できます。

報告するのは、実行すれば必ず失敗する食い違いだけです。
- 未登録のシナリオ、登録と違うタグやテストファイル、`withoutFaults`と`"faults": []`の不一致（EC015）
- 設定にない障害IDや比較IDの使用（EC016）
- `describe`の中での`scenarioTest`の登録（EC017）

`string`型に広がった値、本体の外のヘルパーに渡した`s`、プロジェクト外のテストファイルは照合せず、実行時の観測の照合に任せます。
必須のIDがテストに書かれていないことも、ヘルパー経由で呼ばれる可能性があるため、静的には報告しません。

### 実行時の照合

`verify`は毎回、新規の一時ディレクトリを作り、ランナーのレポートと補助APIの観測記録を照合します。
以前の実行結果を読み込んで合格にするオプションはありません。
VitestはJSON、BunはJUnit XMLからテストの完全名・ファイル・状態を取得します。

| 項目 | 成功条件 |
|---|---|
| 型と登録 | 指定した型のすべての有限文字列タグにケースがあり、型から消えたタグの登録がない |
| テスト | 今回の実行で、指定ファイルと完全なテスト名に一致するテストが1件だけ成功 |
| 障害の観測 | 登録した障害用関数が1回以上呼ばれ、すべて完了 |
| 期待動作 | 必須のIDを持つ値の比較がすべて成功 |
| 観測の対応 | 今回のrun、契約ID、シナリオID、タグ、ファイル、テスト名が一致 |
| 実行全体 | ランナープロセスとランナーレポートの両方が成功 |

余分な観測ID、未登録シナリオ、重複実行、壊れたJSON、古いrun IDも拒否します。
JSON出力schemaVersionは2です。ケースには`testStatus`と`observationStatus`を別々に出します。
`observationStatus`は`not_required / not_run / missing / invalid / incomplete / failed / passed`です。
`check`はテストを実行しないため、成功しても`testsExecuted: false`、観測は`not_run`です。

**`passed`は、その実行で登録した観測・比較が成立したという意味です。アプリケーション全体の正しさの証明ではありません。**

## 保証の境界

- `fault`は開発者が指定した関数の実行を観測します。その戻り値が業務上のエラーであること、対象コードがそれを正しく消費したことまでは自動判定しません。
- 期待値と観測対象は開発者が選びます。意味のない比較や誤った期待値を自動で見破ることはできません。
- タグの種類は型から列挙します。payload、時間、並行実行、処理順序のすべての組合せは列挙しません。
- 補助APIとテストプロセスは信頼対象です。観測JSONは不正なテストコードに対する認証・証明ではありません。
- 未awaitの障害呼び出しは検出しますが、補助APIを通らないバックグラウンド処理や観測期間外の挙動を追跡しません。
- `scan`は明示的なthrow・Promise.reject・構造的なerror戻り値と呼出関係を調べます。全実行時エラーの完全発見、任意のResultライブラリの意味、タグが同じpayloadの型変更は保証しません。
- 実行中のソース書き換えや、残存した別プロセスによる書き換えに対する整合性保証はありません。

## ベースラインと互換性

```sh
./target/debug/errspec snapshot \
  --config examples/scenarios-bun/errspec.json --out baseline.json
./target/debug/errspec verify --runner bun \
  --config examples/scenarios-bun/errspec.json --baseline baseline.json
```

snapshot v2はタグに加え、シナリオID、タグ、期待動作の説明、テスト参照、必須観測IDを保存します。
タグが残っていても、以前あったシナリオを削除すると`EC013`で失敗します。
期待動作や必須観測の変更は`delta.changedScenarios`へ表示します。変更自体を禁止はしません。
CIで既存シナリオの変更を止めたい場合は、`check`または`verify`に`--strict-baseline`を指定します。

```sh
./target/debug/errspec verify --runner bun \
  --config examples/scenarios-bun/errspec.json \
  --baseline baseline.json --strict-baseline
```

このモードでは、期待動作・タグ・テスト参照・必須観測が変わったシナリオを`EC014`で報告し、テストが成功しても終了コード1になります。
検証を弱めたかどうかの意味判断はせず、観測IDの並び順の変更も対象です。既存シナリオを保った追加は許容し、削除は従来どおり失敗します。
設定v2とシナリオ履歴を持つsnapshot v2が必要です。baselineの未指定、履歴不足、設定v1、`snapshot`との併用はエラーになります。
意図した変更は差分をレビューし、新しいsnapshotへ更新してください。CIでは比較元の版を固定し、同じPRで書き換えたbaselineをそのまま比較元にしないようにします。errspecは人が承認した事実を証明しません。

削除が意図した変更ならレビュー後に別パスへ新しいsnapshotを作成します。既存ファイルは上書きしません。

設定v1とsnapshot v1は引き続き読み込めます。設定v1は以前どおり1タグ1テストの照合で、観測状態は`not_required`です。
旧snapshotにはシナリオ情報がないため、その履歴からシナリオ削除を検出することはできません。
旧例は`examples/checkout`、`examples/checkout-bun`に残しています。

初期試作の`error-contract`から名称を変更しました。既定の設定ファイルは`errspec.json`、
補助パッケージは`@errspec/testkit`です。以前の設定は`--config`で指定できます。
CLIと補助APIは同じ版を使ってください。実行環境変数は`ERRSPEC_OBSERVATIONS`／`ERRSPEC_RUN_ID`、
観測記録のkindは`errspec/observation`になりました。診断コード`EC…`は維持しています。
以前の`error-contract/snapshot`も読めますが、v2の自動テスト名は`errspec`へ変わるため、
初回の比較では`changedScenarios`に表示されます。

`source.export`は公開されたtype alias、interface、class（インスタンス型）を対象にします。
import、再export、paths、具体化したgeneric、union/intersectionをtsrsで解決します。
`any`、`unknown`、広いstring、optionalな識別子は解析不能として報告します。`never`は0件です。
型エラーや`noCheck: true`を無視して合格にはしません。

## CLIと診断

`scan`は検証とは独立した`errspec/scan` schemaVersion 1を返します。import/reexport、別名参照、オーバーロード、関数・arrow・メソッド、再帰、await/return、ローカル変数に保持したPromiseの使用とcatch境界を解析します。
`functions[].effects`は発生元IDと経路を持つ「外へ伝わる可能性」の集合です。同期throw、rejection、Resultを区別します。
`unknowns`は動的なsignature、外部実装、解決できないPromise使用などを示します。`--fail-on-unknown`で未解決箇所を終了コード1にできます。
`scanComplete`は解析処理の完了であり、`allRuntimeErrorsKnown`は常にfalseです。getter/proxy、eval、任意callback、暗黙の例外、厳密な条件分岐やfinallyの上書きは完全にはモデル化しません。
依存の型情報は使いますが、node_modulesとプロジェクトディレクトリ外の実装を走査対象に含めません。
候補の既定識別子は`_tag,kind,type,code`です。`--discriminant code`などで変更できます。タグ型が業務上のエラーかはレビューしてください。

| コマンド／オプション | 用途 |
|---|---|
| `scan --project PATH` | 型候補・明示的な失敗箇所と保守的な伝播を調べる。テスト実行なし |
| `init --source FILE --export NAME` | 選んだ型の設定と未完成のテストを生成 |
| `check` | 型とケース登録の照合。テスト実行なし |
| `verify --runner bun\|vitest` | テストと観測の照合。既定はVitest |
| `--only-contract-tests` | verifyで登録済みのテストファイルだけを選択 |
| `snapshot --out PATH` | 型とシナリオの一覧を保存 |
| `--baseline PATH` | 保存済み一覧との差分と削除を検出 |
| `--strict-baseline` | check/verifyで既存シナリオの変更を不合格にする。`--baseline`が必須 |
| `--format json` | Agent向けJSONをstdoutへ出力。子プロセスのログはstderr |
| `--vitest PATH` / `--bun PATH` | 実行ファイルの指定。指定パスは呼出元の作業ディレクトリから解決 |

Vitestの既定パスは設定ディレクトリの`node_modules/.bin/vitest`です。BunはPATHで解決します。
Bunは起動前に`--version`で版を確認し、1.2.19未満なら実行しません。版を取得できない場合は実行し、レポートの形式で判定します。
`--baseline`と`--out`も呼出元の作業ディレクトリから解決します。

| 診断 | 内容 |
|---|---|
| EC001 / EC002 | タグの登録漏れ／型の解決不能 |
| EC003 / EC004 | テストファイルがない／型から削除されたタグの登録 |
| EC005 / EC006 | テストが成功していない・曖昧／実行全体の失敗 |
| EC007 | 契約がbaselineから削除された |
| EC008 / EC009 | 今回のシナリオ観測がない／必須の障害が完了まで観測されていない |
| EC010 | 必須の値の比較が欠けている、または失敗 |
| EC011 / EC012 | 観測の識別情報が不一致・重複／シナリオが正常完了しなかった |
| EC013 | baselineにあったシナリオの削除 |
| EC014 | strictモードでbaselineにあったシナリオが変更された |
| EC015 | テストが未登録のシナリオを登録している、タグ・テストファイル・`withoutFaults`が設定と食い違う（実行前の静的照合） |
| EC016 | シナリオ本体が、設定にない障害IDや比較IDを使っている（実行前の静的照合） |
| EC017 | `scenarioTest`を`describe`の中で登録している（実行前の静的照合） |
| EC_EVIDENCE | 観測プロトコル・run ID・ファイルが不正 |
| EC_CONFIG / EC_BASELINE / EC_RUNNER / EC_SNAPSHOT / EC_INTERNAL | 設定・比較対象・実行器・保存・内部処理のエラー |
| TS… | tsrsによるコンパイラ診断 |

終了コードは0=成功、1=登録・テスト・観測の不足、2=設定・解析・レポート処理などのエラーです。
現行の`analysisComplete: false`は型解析失敗以外にワークフロー処理失敗も含みます。理由はdiagnosticsを参照してください。

## 開発と検証

仕様策定中は[sdev](https://github.com/azu/sdev)を使い、[Product・Requirements・Design](docs/README.md)をknjとAIで確認します。
仕様案は日本語で整理しています。内容が固まった後にsdevを外し、英語のドキュメントを残します。
sdevは開発中の作業用で、errspecの実行時依存やCIの必須条件には追加していません。

```sh
./bin/sdev help
./bin/sdev lint
```

通常のビルドと検証は次のとおりです。

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
node --test tests/testkit.test.mjs
TESTKIT_ENTRY=../packages/testkit/index.ts node --test tests/testkit.test.mjs
(cd examples/checkout && npm ci --ignore-scripts --no-audit --no-fund)
node scripts/demo.mjs
node scripts/demo-bun.mjs
node scripts/demo-scenarios.mjs
node scripts/demo-init.mjs
node scripts/demo-module-mock.mjs
```

`demo-scenarios.mjs`は実際のBun/Vitestを起動し、一時コピーだけに変更を加えます。
`demo-init.mjs`は初期生成したテストを両ランナーで実行し、未記入のTODOが通常実行でもverifyでも失敗することを確認します。
`demo-module-mock.mjs`は、依存を直接importするコードへモジュールのmockから障害を差し込めること、mockを外すと`EC009`になることを両ランナーで確認します。
必要なら`node scripts/demo-scenarios.mjs /path/to/errspec /path/to/bun`で実行ファイルを指定できます。
`demo-init.mjs`も同じ引数で実行ファイルを指定できます。
受け入れ結果は`artifacts/acceptance*.json`に保存します。GitHub Actionsでも同じ検証を実行します。
設計の詳細は[DESIGN.md](DESIGN.md)を参照してください。

## 着想と関連プロジェクト

[mizchiさんのmutator紹介記事](https://zenn.dev/mizchi/articles/introduce-mutator)を読んだことが開発のきっかけです。
errspecでは、エラー型から列挙したタグと、利用者が宣言した失敗シナリオ・必須の検証を対応付けます。
mutation testingとの併用を想定していますが、mutatorとの自動連携は未実装です。
