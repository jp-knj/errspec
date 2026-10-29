# @errspec/testkit（未公開）

Bun testとVitestで、失敗シナリオの観測をRust CLIへ渡す小さなESMパッケージです。
実装は通常のJavaScriptで、型定義はindex.d.mtsです。実行時の外部依存はありません。
Node.jsの標準モジュールを使うため、ブラウザ内テストは対象外です。

このリポジトリでは相対パスでimportしています。別のプロジェクトではこのディレクトリをコピーし、ローカル依存として追加できます。
例えばプロジェクト直下の`vendor/errspec-testkit`へ置いた場合、package.jsonに
`"devDependencies": { "@errspec/testkit": "file:./vendor/errspec-testkit" }`
を追加し、利用中のパッケージマネージャーでインストールします。公開済みnpmパッケージを指す指定ではありません。

```ts
import { test } from "bun:test"; // または vitest
import { scenarioTest } from "@errspec/testkit";

scenarioTest(test, import.meta.url, {
  contract: "gateway", id: "offline", tag: "Offline",
}, async (s) => {
  const fetchUser = s.fault("network", async () => { throw new Error("offline"); });
  let message = "";
  try { await fetchUser(); } catch (error) { message = (error as Error).message; }
  s.check("message", message, "offline");
});
```

この短い例ではstubの動作を示しています。実用時は`fetchUser`をアプリケーションへ依存注入し、その応答や副作用を観測します。

## API

- `scenarioTest(test, fileURL, { contract, id, tag, withoutFaults? }, body)` — トップレベルのテストを登録。bodyは必ず必要な非同期処理をawaitする。
  `withoutFaults: true`は入力だけで結果が決まるシナリオの宣言です。設定の`observe.faults: []`と組み合わせます。指定すると`fault`の呼出が失敗になり、指定しないとfaultが0件のシナリオは失敗します。
- `scenarioTestName(contract, id)` — 設定照合に使うテスト名を生成する。
- `scope.fault(id, fn)` — 引数とthisを渡してfnを呼ぶasync wrapperを返す。fnの呼出回数と完了回数を記録。resolve/rejectとも完了と数える。
- `scope.check(id, actual, expected)` — deepStrictEqualによる実際の比較を実行。失敗は投げ直し、内部にも失敗を保持する。

各シナリオ内の障害IDと比較IDは、それぞれ一意な非空文字列です。
2種類のID空間は独立しているため、障害と比較に同じ名前を使うことはできます。
同じシナリオをretry/repeatで複数回実行した証拠は、初版ではRust側が曖昧として拒否します。
別々のシナリオIDを持つ`test.concurrent`は利用できます。

## 実行の記録

CLIから実行すると`ERRSPEC_OBSERVATIONS`と`ERRSPEC_RUN_ID`が子プロセスへ渡されます。
各シナリオはUUIDの異なるファイルに開始状態を記録し、終了時に完了状態・障害回数・比較結果を保存します。
一時ファイルからrenameするため、並列実行でJSON行が混ざりません。
片方の環境変数だけがある場合は設定エラーです。どちらもなければ通常の単体テストとして動き、ファイルを生成しません。

これらの環境変数を通常のプロジェクト設定に固定しないでください。古い記録をCLIへ渡す機能はありません。
テスト終了後のscope操作は失敗し、記録を未完了へ変更します。ただし、プロセス終了後やCLIが観測し終えた後の処理の追跡はできません。

## 観測の意味

faultの完了は「開発者が障害用に指定した関数が実行され、値を返すか例外を投げた」という事実です。
任意のResult型の意味や、戻り値が対象のエラー種別に該当することは、この汎用補助APIでは判定しません。
checkは実際に値を比較しますが、期待値と観測対象の妥当性はレビューする必要があります。
プロトコルは信頼するテストコードからの報告であり、悪意あるテストコードの改ざんに対する証明ではありません。
