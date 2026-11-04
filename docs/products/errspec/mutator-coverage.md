# mutator 0.2.0との機能対応

[Product](product.md)の目標は、mutatorの全機能を包含し、errspecのエラー型・失敗シナリオ検証を併せて提供することです。この表は目標と実装済みの範囲を区別するために使います。Vitestによる生成・実行・判定を`mutate`で提供します。追加ワークフローの実装状況も下表に示します。全機能の条件は[追加のRequirements](features/mutation-workflows/requirements.md)に定めます。

比較対象は[mutatorのREADME](https://github.com/mizchi/mutator/blob/025870640d46e93e97285d67e4c265074cf8b295/README.md)と同じcommitの[CLI package.json](https://github.com/mizchi/mutator/blob/025870640d46e93e97285d67e4c265074cf8b295/packages/cli/package.json)で確認した0.2.0です。commitは`025870640d46e93e97285d67e4c265074cf8b295`に固定します。コマンドと設定は[options.ts](https://github.com/mizchi/mutator/blob/025870640d46e93e97285d67e4c265074cf8b295/packages/cli/src/options.ts)、組み込みmutatorは[mutators.ts](https://github.com/mizchi/mutator/blob/025870640d46e93e97285d67e4c265074cf8b295/packages/core/src/mutators.ts)も確認しています。

最初の提供単位を[Vitestによる生成・実行・判定のRequirements](features/mutation-testing/requirements.md)と[Design](features/mutation-testing/design.md)へ具体化しました。18種、基準実行、到達記録、状態別の判定、text / JSON、未検出を不合格にする任意の指定を対象にします。生成・実行・判定の範囲を実装し、下表へ反映しています。

## 包含の基準

利用者が利用できる機能の包含を基準にします。mutatorと同じCLI・終了コード・設定・JSON・プラグインAPIであることは目標に含めません。既存errspecのCLI・終了コード・設定・JSON・testkit APIは維持します。

失敗シナリオ検証のスコープ外にあるmutation testingとの連携は、その個別機能の範囲を表します。ミューテーション機能は追加機能としてRequirements・Designで具体化し、既存機能の約束を維持します。この表は実装の完了や、未作成の要件への合意を示すものではありません。

## ミューテーションの生成・実行・再利用

| mutator 0.2.0の機能 | errspecの実装状況・関連機能 |
| --- | --- |
| oxc ASTによるinstrumentation、mutation switching、書式の維持、sourcemap | `mutate`で提供。tsrs ASTとMagicStringを使用 |
| 組み込み18種のmutator | `mutate`で提供。規則と安全な配置の条件は[Design](features/mutation-testing/design.md)を参照 |
| dry run、テスト単位の到達記録、到達したテストだけの実行、モジュール初期化時のmutant | `mutate`で提供。concurrentや初期化時はファイル全体へ選択を広げる |
| Vitest/Vite内のメモリ上の変換とセッション再利用 | メモリ上の変換と`--reuse-session`によるinstrumented session再利用を提供。既定はfresh。globalSetupがある場合や必要な公開APIがない版ではfreshへ戻し、理由をengine.sessionReuseに示す |
| 位置に依存しないmutant ID、関数・テスト・依存・設定・環境・pluginの変更に応じた結果とcoverageの再利用 | 位置に依存しないIDと所有可能なAST scope情報を提供。結果・coverageキャッシュは未実装。CLIの`snapshot`はタグとシナリオの履歴 |
| `--since`によるGit差分の限定、`node`/`scope`単位の選択、検出テストの削除による再検査、未収集の変更テストの警告 | `--since`と`--diff-unit node/scope`を提供。テスト変更・削除やinventory外の変更では全候補を再検査し、未収集の変更テストを警告。検出テストを保存する履歴との照合は後続 |
| `--only`で指定したmutantだけの再検査 | `mutate --only`で提供。未知のIDはエラー |
| `--full-dry-run`による到達記録の再取得 | 未実装 |
| `--experimental-callgraph`による生存mutantの再検査範囲の限定 | 未実装。`scan`はエラーの伝播先を調査し、テストの省略には使わない |
| weak mutation、値の差によるテストの絞り込み、`not infected`の判定、無効化 | 比較・算術演算を`--weak-mutation`で提供。適用できない式と初期化・concurrentは全到達テストへ広げる |
| 利用先のTypeScript 6以前／7以降でのmutantの型検査、`CompileError`、無効化 | `--typecheck` / `--no-typecheck`で提供。TS 5.9.3・6.0.3・7.0.2で検証 |
| 型で同値と分かるoptional chaining mutantの除外と理由の提示 | 型検査で提供。strictNullChecksとnoUncheckedIndexedAccessを必要とする |
| aridなログ専用コードの除外、calleeパターンの追加、無効化 | 未実装 |
| disableコメントとStrykerコメントによる除外 | 未実装 |
| `accept`、理由付きの同値mutant承認、古くなった承認の案内 | `accept --reason`と`errspec.accepted.json`で提供 |
| custom mutator／ignorer plugin、モジュール・パッケージからの読取、mutator単位の除外、plugin変更時のキャッシュ無効化 | `--exclude-mutator`を提供。custom pluginとキャッシュは未実装 |
| `-j`による並列実行、長い実行の分割 | 未実装 |
| `--mutants-per-line`の決定的な標本選択、`--time-budget`、未実行の`Pending`表示 | 同名の指定と、理由付きの`Pending`で提供 |
| hit limit、stall／total timeout、予算を延長した再実行、RuntimeErrorの再確認 | `mutate`で提供。再試行は一候補につき一度まで |
| SIGINT／SIGTERM時の子プロセス終了、前回の孤立プロセスの検査と終了 | 今回所有するプロセス群の終了を提供。前回のプロセス探索は未実装 |

## 組み込みmutator

上記の固定commitにある18種を対象にします。各種の具体的な変換規則は、ミューテーション機能のRequirements・Designで定めます。

| 分類 | mutator | errspec |
| --- | --- | --- |
| 演算子 | `ArithmeticOperator`, `AssignmentOperator`, `EqualityOperator`, `LogicalOperator`, `UnaryOperator`, `UpdateOperator` | `mutate`で提供 |
| 条件・本体 | `ConditionalExpression`, `BlockStatement` | `mutate`で提供 |
| リテラル・配列・オブジェクト | `BooleanLiteral`, `StringLiteral`, `ArrayDeclaration`, `ObjectLiteral` | `mutate`で提供 |
| 関数の戻り値 | `ArrowFunction`, `FnValue` | `mutate`で提供 |
| 呼出・アクセス | `CallExpression`, `MethodExpression`, `OptionalChaining` | `mutate`で提供 |
| 正規表現 | `Regex` | `mutate`で提供 |

## ランナーとレポート

| mutator 0.2.0の機能 | errspecの実装状況・関連機能 |
| --- | --- |
| Vitest 5、Vite+、ランナーの自動選択と設定指定 | `mutate`はVitest 5系のNodeプロジェクトと設定指定に対応。Vite+固有の対応は未検証 |
| Jest 30のnative ESM adapter（実験的） | 未実装 |
| Playwright TestとVite dev serverのbrowser adapter、fixture、複数project（実験的） | 未実装 |
| text、mutation-testing-elements形式のJSON・HTML、保存ディレクトリの指定 | `mutate`独自のtext・JSONを提供。mutation-testing-elements形式、HTML、保存先指定は未実装 |
| `not detected`／`not infected`／`not reached`の分類、子プロセス内で実行された可能性の警告 | 未実装 |
| `--reporter agent`の不足・関数本文・到達テスト・修正案、stdoutとstderrの分離 | ミューテーション用は未実装。errspecのJSONは診断をstdout、ランナーのログをstderrへ出力 |
| `--reporter simplify`で削除してもテストが失敗しないコードを一覧化 | 未実装 |
| `select-tests`で変更に関係するテストを順位付けし、files／JSONで出力 | 未実装 |
| `--kill-matrix`による全検出テスト、検出なし・包含・重複テストの一覧、各実行方式での結果再利用 | 未実装 |
| high／low／breakの閾値、`--fail-on-survived`、baseline失敗とquality gate失敗の区別 | `--threshold-high/low/break`と`--fail-on-survived`を提供。quality gateは終了1、基準実行の異常は終了2 |
| critical領域のglob／コメント指定、全mutantの検出または承認の要求 | `--critical`と直前の`// errspec-critical` / `// mutator-critical`で提供 |
| GitHub Actionsの差分上のwarning annotation | 未実装 |

## 設定・配布・開発支援

| mutator 0.2.0の機能 | errspecの実装状況・関連機能 |
| --- | --- |
| `mutator.config.ts`／mts／mjs／js／json、`defineConfig`、JSON Schema、CLI優先順位と否定フラグ、root／include／exclude | `errspec.mutation.config.{ts,mts,mjs,js,json}`、`defineConfig`、JSON Schema、明示CLI/API優先、否定フラグを提供。受け入れ検証は`tests/mutation-options.test.mjs` |
| `runMutation`、report formatter、plugin定義などのprogrammatic APIとcore／runner／typecheck／CLIのpackage分離 | `@errspec/core`の`runMutation`と`defineConfig`を提供。共通の版付き要求をRustへ渡しCLIと同じ判定を使う。formatter・plugin API・runner/typecheck package分離は未実装 |
| npm CLIと各adapterの配布、Node.js 24以降での利用 | `mutate`のdriverをネイティブバイナリへ同梱。Node 24以降を要求。errspecの配布物は準備中、npm/crates.io未公開 |
| GitHub Action、差分のbase取得、失敗時も保存するsnapshot cache、別checkoutでの再利用 | ミューテーション用は未実装。errspecのCIはRust・testkit・Bun/Vitest受け入れ検証を実行 |
| Agent Skillsの同梱、skills-npm、`init-skill`、導入・反復修正・Playwrightの手順 | 未実装。既存のREADMEと導入例を提供 |

## errspecが追加する検証

| 機能 | 実装状況 |
| --- | --- |
| エラー型候補・throw／rejection／Result・伝播経路・未解決箇所の調査 | `scan`で提供 |
| 指定した型の設定と未完成テストの生成 | `init`で提供 |
| 有限タグ集合と契約登録の照合、テスト本文の確定的な食い違いの診断 | `check`で提供。`verify`も同じ照合を実行 |
| Bun/Vitestの実行結果と、障害用関数の完了・必須比較・今回の実行記録の照合 | `verify`とtestkitで提供 |
| シナリオ一覧の保存、追加・削除・変更、strict baseline | `snapshot`と`--baseline`で提供 |
| 既存Vitestテストが確かめたエラータグの集計 | `coverage`で提供 |

ミューテーション機能は個別のRequirements・Designで具体化し、実装と検証に合わせてこの表を更新します。

全仕様の実装・受け入れ検証の追跡は[implementation status](implementation-status.md)を参照してください。
