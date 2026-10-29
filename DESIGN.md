# 基本設計：型・シナリオ・実行証拠の対応

## コンセプト

エラー型の変更で増えた失敗種別と、登録された失敗時の仕様・テスト・実行証拠を照合する。
Agent JSONは診断の受渡し手段であり、型・仕様・実行の対応に価値を置く。
障害注入やミューテーションそのものに独自性を主張しない。

## Rustを使う境界

| モジュール | 責任 |
|---|---|
| tsrs_adapter | tsconfig、診断、exportのsymbol解決、有限タグ集合の取得 |
| model | コンパイラのハンドルを含まない所有可能なデータとenum |
| contract | 型タグとケース登録の比較、snapshot、シナリオの追加・削除・変更差分 |
| evidence | ランナー非依存のテストファイル＋完全名による照合 |
| vitest / bun | JSON/JUnitを共通のRunEvidenceへ変換 |
| observation | 補助APIの記録の検証と、必須観測との照合 |
| CLI | プロセス、一時ディレクトリ、環境変数、表示、終了コード |
| packages/testkit | JSランナー内で障害用関数の実行と値の比較を記録 |

型解析はRust＋tsrs、対象コードとテストの実行はBun/Vitestが担当する。
Oxcエコシステムのtsrsを使用しているが、追加のoxc_parserは入れていない。
同じ型システムを再実装せず、tsrs内部APIへの依存はadapterに閉じ込める。
Git revisionとCargo.lockを固定する。arenaは単発CLIのプロセス寿命で管理し、常駐APIはまだ提供しない。

今回、並列化や高速キャッシュは追加していない。将来は所有可能なIRへ変換した後の処理や独立したプロジェクトの解析を候補にする。
Rust採用だけで高速性や健全性が保証されるとは主張しない。

## 型・シナリオ・業務仕様を分ける

Eを型から取得したタグ集合、Cをケースに登録されたタグ集合とすると、E−Cは登録漏れ、C−Eは不要な登録。
シナリオは(contract id, scenario id)で識別する。複数シナリオが同じタグを持てる。
この集合の一致は、payloadや時間的な状態を含む全失敗経路の網羅性を意味しない。

schemaVersion 2の各ケースには、必須のfault IDとassertion IDを明示する。
それぞれ1件以上を必要とする。説明文expectは実行可能な式として扱わない。
指定したエラー型を公開して登録する方式を維持し、Resultの型引数を名前だけで推定しない。

## 識別子とテストの結び付け

補助APIがトップレベルに登録する名前は`errspec `とJSON配列[contract id, scenario id]の連結。
区切り文字を含むIDでも衝突しない。ファイルはimport.meta.urlから取得してRust側でcanonicalizeする。
テストの成功はランナーレポートで確認し、補助APIの自己申告だけを成功にしない。
ファイル・名前が重複したテスト、同じシナリオの複数実行は曖昧として拒否する。

describeによる名前の接頭辞、retry/repeat、ブラウザ内実行は現時点の対応範囲外。
distinct IDのconcurrent実行は各callbackに独立したscopeと観測ファイルを与える。

## 観測のライフサイクル

1. CLIが新しい一時ディレクトリとrun IDを用意する。
2. ランナーが補助APIのテストを開始すると、runningの記録を作る。
3. fault wrapperが呼ばれるとcallsを増やす。resolve/rejectでcompletedを増やす。
4. checkがdeepStrictEqualを実行し、passed/failedを記録する。catchされても失敗の印は消さない。
5. body終了時に、障害の呼出・完了と比較の存在を確認し、最終状態を書き込む。
6. ランナー終了後、Rustがレポートと観測を照合する。

ファイルはシナリオ実行ごとのUUIDを使い、一時ファイルからrenameする。
途中終了時のrunning記録、残った一時ファイル、壊れたJSONは合格にならない。
読取上限は1ファイル1 MiB、合計32 MiB、10000ファイル。超過はEC_EVIDENCE。
run IDは偶発的な古い記録の混入を検出するためで、認証用の秘密ではない。

## 成功条件

必要なタグがすべて登録され、対象テストが今回1件だけ成功し、以下をすべて満たすこと。

- run ID、契約ID、シナリオID、タグ、テストファイル、テスト名が一致。
- シナリオの記録が1件だけあり、stateがpassed。
- 必須のfaultがそれぞれ1回以上呼ばれ、全呼出が完了。
- 必須のassertionがすべてpassed。
- 未登録のfault/assertion IDやシナリオがない。
- ランナーレポートとプロセスの両方が成功。

faultは開発者が選んだ関数の観測であり、その戻り値が意味的に正しい障害かどうかは証明しない。
assertionの期待値、観測対象、仕様の十分性もレビュー対象。
プロセス・補助API・テストコードを信頼する。任意のJS全体や観測期間外の処理を検証したとは扱わない。

## 状態と出力

JSONレポートはv2。v1設定では観測を要求せずnot_requiredとし、旧ワークフローを維持する。
checkは型と登録だけを確認し、観測はnot_runのまま。verifyで初めてmissing等へ遷移する。
ObservationStatus、ScenarioState、AssertionStatusにはRust enumを使用する。
必須の比較が抜けていてもランナーはpassedになり得るため、testStatusとobservationStatusは分ける。

設定、snapshot、出力JSON、観測プロトコルのschemaVersionは、それぞれ独立したバージョン。
観測プロトコルは新設のv1。snapshotはv2を生成し、旧v1も読み込む。
既存のanalysisCompleteはワークフロー失敗も含むため、厳密な静的解析の完了指標にはまだ分離していない。

## ベースライン

v2 snapshotはタグとシナリオのID・タグ・expect・テスト参照・必須観測を保存する。
シナリオの削除はEC013。タグが別シナリオによって残っていても検出する。
同じシナリオIDで期待動作や必須観測が変わればchangedScenariosへ表示する。
旧snapshotはシナリオ履歴を持たないため、移行前の削除は判断しない。
ソース定義の参照先が変わった場合は、以前と同じ型だと推定せずbaselineを拒否する。

## 今後の順序

1. 明示したexported functionのsignatureと対象型の利用関係を、根拠位置付きで取得。
2. 明示したResultライブラリごとの型抽出・実行時のエラー観測アダプター。
3. 直接呼び出し、return、awaitから始める限定した伝播解析。動的dispatchやanyはunknownとして残す。
4. ミューテーション結果との連携で、比較が実際の処理変更を検出する証拠を追加。
5. 依存関係と設定・ランナー・バージョンを含むキャッシュキーを設計し、性能を実測。

関係が分からない場合にテストを省くことはしない。誤った成功を減らすことを優先する。
