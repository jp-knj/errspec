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

## 直接importしている依存に障害を差し込む

依存を引数で受け取らず、モジュールから直接importしているコードもあります。
製品コードを依存注入の形へ書き換えなくても、テストランナーのモジュールmockで差し込めます。
mockは差替え可能な参照へ転送するだけにします。各シナリオがその参照へ`s.fault`の戻り値を代入します。

Bun:

```ts
import { mock, test } from "bun:test";
import { scenarioTest } from "@errspec/testkit";
import { notify } from "../src/notify"; // 内部で "./mailer" の deliver をimportしている

const mailer = { deliver: async (_to: string) => ({ accepted: true }) };
mock.module("../src/mailer", () => ({ deliver: (to: string) => mailer.deliver(to) }));

scenarioTest(test, import.meta.url, { contract: "notify", id: "offline", tag: "retry_later" }, async (s) => {
  mailer.deliver = s.fault("mailer", async () => { throw new Error("offline"); });
  s.check("result", await notify("user@example.com"), { status: "retry_later" });
});
```

Vitest:

```ts
import { test, vi } from "vitest";
import { scenarioTest } from "@errspec/testkit";
import { notify } from "../src/notify";

// vi.mockはimportより前へ巻き上げられるため、参照はvi.hoistedで作ります。
const mailer = vi.hoisted(() => ({ deliver: async (_to: string) => ({ accepted: true }) }));
vi.mock("../src/mailer", () => ({ deliver: (to: string) => mailer.deliver(to) }));
// scenarioTest の中身は Bun と同じ
```

- mockの戻り値に`s.fault`を直接書くことはできません。mockはシナリオより先に評価され、`s`がまだないためです。
- 各シナリオの先頭で参照へ代入し直します。前のシナリオの障害が残ったまま次のシナリオを実行しないためです。
- mockが効いていないと実際の依存が呼ばれ、障害は未呼出の扱いになります。CLIは`EC009`で失敗します。
- 動作する一式は[examples/module-mock](../../examples/module-mock)です。両ランナーで実行するには`node scripts/demo-module-mock.mjs`を使います。

## API

- `scenarioTest(test, fileURL, { contract, id, tag, withoutFaults? }, body)` — テストを登録する。トップレベルでも`describe`の中でもよい。bodyは必ず必要な非同期処理をawaitする。
  `withoutFaults: true`は入力だけで結果が決まるシナリオの宣言です。設定の`observe.faults: []`と組み合わせます。指定すると`fault`の呼出が失敗になり、指定しないとfaultが0件のシナリオは失敗します。
- `scenarioTestName(contract, id)` — 設定照合に使うテスト名を生成する。
- 定義の`description`は、テスト名の後ろに空白1つを挟んで付く表示用の説明です。1行の空でない文字列に限ります。CLIは説明を除いた固定部分で照合し、観測にはランナーと同じ完全な名前を記録します。
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
