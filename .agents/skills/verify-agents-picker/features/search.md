# 検索

検索はエージェントの種類、名前、workspace/tab、cwdを入力に応じて絞り込む。

## Sub-features

- `search-match`: 一致するエージェントだけ表示する。
- `search-empty`: 該当なしとエージェント不在を区別する。
- `search-edit`: Backspace、Ctrl+U、Ctrl+Wで検索語を編集する。
- `search-cancel`: Escでフィルターを消しnavigateへ戻る。

## How to get to it (user POV)

- ピッカーで `/` を押す。
- searchモードで文字入力、Backspace、Ctrl+U、Ctrl+Wを使う。
- searchモードのEscで検索を取り消す。

## Driving it with tmux

Preconditions:

- features/README.mdの基準状態を満たす。
- 証拠ディレクトリは新規。実Herdrの場合は対象と元の表示先を控える。

- 自動recipe: `python3 .agents/skills/verify-agents-picker/scripts/verify.py run search --evidence /private/tmp/agents-picker-evidence/search-001`。
- `/`→`Beta`で `1/2` とBetaのプレビュー。Ctrl+U→`zznomatchzz`で `No matches for "zznomatchzz"`。Escで `/ to filter` と `2/2`。qで終了0、フォーカス要求なし。
- 追加入口は専用PTYで実際のキーを送り、入力欄と件数を前後で採取する。Backspaceは末尾1文字、Ctrl+Wは最後の単語を削除する。種類・workspace/tab・cwdのそれぞれを検索して一致行を確認する。

## Gotchas

- searchモードのqは検索文字。閉じるにはCtrl+Cか、Esc→q。
- 検索中のEscはTUIを終了しない。
- 自動recipeはBackspace/Ctrl+Wや検索対象全項目までは実行しない。
