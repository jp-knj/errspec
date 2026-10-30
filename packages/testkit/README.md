# @errspec/testkit（未公開）

Bun testとVitestで、失敗シナリオの観測をRust CLIへ渡す小さなESMパッケージです。
実装は通常のJavaScriptで、型定義はindex.d.mtsです。実行時の外部依存はありません。
同じ実装をTypeScriptで書いた`index.ts`も同梱しています。パッケージの入口は`index.mjs`のままです。
Node.jsの標準モジュールを使うため、ブラウザ内テストは対象外です。

このリポジトリでは相対パスでimportしています。別のプロジェクトではこのディレクトリをコピーし、ローカル依存として追加できます。
例えばプロジェクト直下の`vendor/errspec-testkit`へ置いた場合、package.jsonに
`"devDependencies": { "@errspec/testkit": "file:./vendor/errspec-testkit" }`
を追加し、利用中のパッケージマネージャーでインストールします。公開済みnpmパッケージを指す指定ではありません。

JavaScriptのファイルを置けないプロジェクトや、依存を追加せずテストの隣に置きたい場合は、`index.ts`だけを複製できます。
このファイル1つで型定義も含みます。テストからは`import { scenarioTest } from "./errspec-testkit"`のように読み込みます。
strictな型検査とBun/Vitestでそのまま動きます。`node:`モジュールの型には`@types/node`または`@types/bun`が必要です。
`index.ts`と`index.mjs`は同じテストを両方に対して実行し、挙動の一致を確認しています。複製した後に手で編集すると、上流の版との対応が分からなくなるため、取得元のcommitを記録してください。

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

### 単一障害で状態の原子性を検査する

`checkFailureAtomicity({ setup, faults, isFailure, only? })`は、正常系の依存呼び出しを観測し、各呼び出しへ障害を一回ずつ差し込みます。`setup`は実行ごとに独立したfixtureを返し、fixtureは`run(scope)`と`snapshot()`、任意の`dispose()`を持ちます。`scope.wrap(dependency, implementation)`で検査対象の非同期依存を包みます。ラッパーは引数とthisを保ち、選んだ呼び出しでは実装の代わりに障害関数を実行します。

`faults`は一意の`id`、`dependency`、有効な失敗値を返すか例外を投げる`inject`関数の配列です。`isFailure(result)`は操作の戻り値が失敗かをbooleanで返します。注入した例外はアプリケーションで処理して戻り値へ変換します。操作自身が例外を投げた場合は検査を失敗させます。

失敗値を返したら、複製した前後のスナップショットを比較します。参照が同じでも変更を検出し、ロールバック後の状態が同じなら成功します。保護対象の業務状態を返し、ログや時刻、秘密情報は含めないでください。

結果の`baseline.calls`は正常系の依存別呼び出し回数、`cases`は障害ごとの`fault`、`status`、`before`、`after`、`result`を持ちます。`preserved`は失敗時の状態維持、`recovered`は障害後の成功、`violated`は状態変更や未注入・例外・後処理の失敗です。違反があれば全ケースの実行後に`FailureAtomicityError`を投げ、その`report`から根拠を読めます。`only: { fault, call }`にケースの値を渡すと単一ケースを再実行します。

`recovered`は成功値を返したという観測で、正しく回復したという証明ではありません。エラーを握り潰して成功値を返すバグを見つけるには、成功時の出力・状態も既存テストで確認してください。

正常系の失敗、登録した依存を通らないケース、空・重複の障害、未完了の依存呼び出しは成功にしません。fixtureの`dispose()`は失敗時も呼びます。テスト対象のバックグラウンド処理を停止する機能はないため、操作は依存をawaitし、fixtureが外部状態を隔離してください。

型から任意の障害値や業務仕様を推測するAPIではありません。正常系の呼び出し回数に対する単一障害だけを探索します。回復成功の出力や、守る状態の選び方は既存テストで確認します。レポートはJavaScript値を含むため、JSONに変換できるとは限りません。

### シナリオの実行記録

- `scenarioTest(test, fileURL, { contract, id, tag, withoutFaults? }, body)` — トップレベルのテストを登録。bodyは必ず必要な非同期処理をawaitする。
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
