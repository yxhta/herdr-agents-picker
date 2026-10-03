# プレビュー

選択したエージェントの端末を右側に表示し、ライブ接続が使えない場合はsnapshotに切り替える。

## Sub-features

- `live`: 選択中端末の更新を表示する。
- `switch`: 選択変更で対象を切り替える。
- `fallback`: stream失敗時もsnapshotを表示し診断を出す。
- `resize`: サイズ変更後も表示を維持する。

## How to get to it (user POV)

- ピッカーを開くと選択中のpreviewが表示される。
- 選択を移すとpreviewも変わる。
- ポップアップまたは端末のサイズを変更する。

## Driving it with tmux

Preconditions:

- features/README.mdの基準状態を満たす。
- 証拠ディレクトリは新規。実Herdrの場合は対象と元の表示先を控える。

- 自動recipe: `python3 .agents/skills/verify-agents-picker/scripts/verify.py run preview --evidence /private/tmp/agents-picker-evidence/preview-001`。
- AlphaのLIVEフレームが更新する。外部fixtureでstreamを不可にしてjでBetaへ移すと `SNAPSHOT verify-beta` と `live preview unavailable` が出る。qで終了0、focusなし。
- 実Herdrでは対象端末の通常操作で出力を更新し、previewの同じ内容を前後で採取する。
- resize入口は専用PTYのサイズを変更し、選択・検索語を維持したままpreviewが新サイズで表示されることを確認する。

## Gotchas

- snapshot初期表示だけではLIVE成功の証拠にならない。
- fallback/retry、5秒のsnapshot監視は実Herdrでも条件を用意して確認する。
- 自動recipeはresizeや本番ストリーム互換性を実行しない。
