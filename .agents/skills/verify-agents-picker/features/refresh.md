# 一覧更新と表示順

エージェント一覧は定期更新され、手動更新もできる。Herdrの表示順設定と状態を反映する。

## Sub-features

- `manual-reload`: r/Ctrl+Rで更新する。
- `automatic-reload`: 約2秒ごとに変更を反映する。
- `selection-preservation`: 更新後も同じ対象を選ぶ。
- `order-status`: spaces/priority、状態アイコン、状態変更順を反映する。

## How to get to it (user POV)

- navigateモードのr。
- searchモードのCtrl+R。
- ピッカーを開いたままエージェントの増減を待つ。
- Herdrのui.agent_panel_sort設定、およびpane.agent_status_changedイベント。

## Driving it with tmux

Preconditions:

- features/README.mdの基準状態を満たす。
- 証拠ディレクトリは新規。実Herdrの場合は対象と元の表示先を控える。

- 自動recipe: `python3 .agents/skills/verify-agents-picker/scripts/verify.py run refresh --evidence /private/tmp/agents-picker-evidence/refresh-001`。
- 外部一覧をAlphaだけにしrで `1/1`。Betaを戻しキー入力なしで `2/2`。Alpha previewを維持し、qで終了0、focusなし。
- searchモードでは検索語を入力してCtrl+Rを送り、検索語と対象を維持した更新を確認する。
- priority設定は実ユーザーの設定変更、または検証専用設定で確認する。blocked→done→working→idle→unknownの順、同状態は最新変更優先。workspace/tab名はHerdr sidebarと照合する。
- 状態イベント経路を検証する場合は専用HERDR_PLUGIN_STATE_DIRで --record-statusを実行し、agent-status-order内の対象paneファイルと表示順の変化を両方採取する。実際のHerdrイベント配送は別途確認する。

## Gotchas

- 自動recipeはnavigateのrと自動更新だけを実行する。
- spacesが基準。priorityや状態イベントの検証にユーザーの設定・stateを流用しない。
- 自動更新待ちは期待状態まで待ち、2秒sleepだけで成功判定しない。
