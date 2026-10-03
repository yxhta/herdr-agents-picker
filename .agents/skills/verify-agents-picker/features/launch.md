# 起動と終了

HerdrのアクションやCLIからモーダルピッカーを開き、対象を切り替えずに閉じられる。

## Sub-features

- `open-popup`: --openからHerdrへpopup要求を出す。
- `plugin-entry`: plugin pane openとplugin_actionで開く。
- `cancel`: q/Esc/Ctrl+Cで閉じる。
- `empty`: エージェント不在を表示する。

## How to get to it (user POV)

- `herdr plugin pane open --plugin yxhta.agents-picker --entrypoint picker`。
- `target/release/agents-picker --open`。
- READMEの設定に従ったplugin_action `yxhta.agents-picker.open`。キー例はprefix+f。
- navigateモードのq/Esc、両モードのCtrl+C。

## Driving it with tmux

Preconditions:

- features/README.mdの基準状態を満たす。
- 証拠ディレクトリは新規。実Herdrの場合は対象と元の表示先を控える。

- 自動recipe: `python3 .agents/skills/verify-agents-picker/scripts/verify.py run launch --evidence /private/tmp/agents-picker-evidence/launch-001`。
- PTYでTUIが表示されqで終了0、focusなし。その後--openを実行し、plugin pane open --plugin yxhta.agents-picker --entrypoint picker --focusを記録する。
- 実HerdrではCLI入口と設定済みキー入口のそれぞれから開き、Agents見出しとpopup配置を採取する。Esc/Ctrl+Cで閉じ、元の表示先を維持することを確認する。
- エージェントがいない環境で `No agent panes found` を採取する。エラー時と空状態を混同しない。

## Gotchas

- --openの模擬要求は実ポップアップ表示を検証しない。
- 手動キー設定は環境依存。未設定なら未検証とする。
- 自動recipeはqだけを実行する。Esc/Ctrl+Cと空状態は追加検証が必要。
- --record-statusはmanifestのイベント入口であり、--helpには載っていない。
