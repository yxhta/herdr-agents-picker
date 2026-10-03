# 選択とフォーカス

ユーザーは対象エージェントを選び、Enterでそのworkspace、tab、paneへ移動する。

## Sub-features

- `move`: navigate/searchの各キーで選択を移す。
- `focus`: Enterで選択エージェントを開く。
- `initial-selection`: 現在フォーカス中のエージェントを初期選択する。
- `self-exclusion`: ピッカー自身のpaneを一覧から除く。

## How to get to it (user POV)

- navigateモードでj/k、上下矢印、Ctrl+P/Ctrl+N。
- searchモードで上下矢印、Ctrl+K/Ctrl+J、Ctrl+P/Ctrl+N。
- どちらのモードでもEnter。
- エージェント内からポップアップを開く。

## Driving it with tmux

Preconditions:

- features/README.mdの基準状態を満たす。
- 証拠ディレクトリは新規。実Herdrの場合は対象と元の表示先を控える。

- 自動recipe: `python3 .agents/skills/verify-agents-picker/scripts/verify.py run navigation --evidence /private/tmp/agents-picker-evidence/navigation-001`。
- jでAlpha→Betaとなり `LIVE verify-beta` が出る。EnterでTUIが終了0。`calls.jsonl` のfocus要求はworkspace verify-w→tab verify-t→agent verify-betaの順。
- 追加キーは専用PTYで入力し、右側の選択対象も確認する。
- 実HerdrのEnterは元のworkspace/tab/paneを控え、別workspaceの対象を選ぶ。一覧と実表示の両方が移ることを確認し、元へ戻る。
- 初期選択と自己除外は、実Herdrでエージェント内から開き、現在エージェントのpreviewと一覧を確認する。

## Gotchas

- 模擬CLIのfocus成功は本番の表示中クライアントの移動を証明しない。
- pane_idを優先する。terminal_idだけを期待する検証にしない。
- Enterは該当なしでは終了しない。
- 自動recipeはjとnavigateのEnterだけを実行する。
