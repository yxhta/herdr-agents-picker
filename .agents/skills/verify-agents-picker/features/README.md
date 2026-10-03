# Agents Pickerのfeature map

本物のTUIをユーザーと同じキー入力で操作するための検証索引。各ファイルの全入口を確認し、未実行の入口を他の入口の成功で代用しない。

## 基準状態

- リポジトリルートで `cargo build --release --locked` を完了する。
- `python3 .agents/skills/verify-agents-picker/scripts/verify.py doctor` が成功する。
- 各runは独立したtmuxサーバーで起動し、VerifyワークスペースのChecksタブにAlphaとBetaを表示する。
- 初期選択はAlpha。エージェントの状態はidle、設定はspaces順。
- 各runの証拠には新規ディレクトリを指定する。

## 検証の範囲

自動recipeは各機能の代表入口を実行する。追加キー、Herdrの実ポップアップ、実際のフォーカスとストリーム、表示順の設定やステータスイベントは個別に実行して証拠を採取する。未実行は未検証と記録する。既存ユーザーセッションを無断で増やしたり、他のエージェントと同時操作したりしない。

## 機能

- [検索](search.md): 絞り込み、該当なし、検索編集、キャンセル。
- [選択とフォーカス](navigation.md): 移動、Enter、初期選択。
- [プレビュー](preview.md): ライブ更新、選択変更、snapshot fallback。
- [一覧更新と表示順](refresh.md): 手動・自動更新、表示順と状態。
- [起動と終了](launch.md): プラグイン入口、CLI入口、終了。
