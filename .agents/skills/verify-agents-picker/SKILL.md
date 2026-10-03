---
name: verify-agents-picker
description: Herdr Agents Pickerの実際のTUIを専用tmuxで操作し、検索、選択とフォーカス、プレビュー、再読み込み、起動と終了を検証する。Rustの変更後や操作不具合の再現、実Herdrとの連携確認に使う。
---

# Agents Pickerの動作検証

リポジトリのルートから実行する。最初に [feature map](features/README.md) を読み、変更した操作の全入口を確認する。ハーネスは本物のリリースバイナリへキーを送る。Herdrだけを本番のCLI境界 `HERDR_BIN_PATH` で置き換えるため、実Herdrのウィンドウ切り替えを検証したことにはならない。

## Launch

必要なものはCargo、Python 3、tmux。依存追加は不要。

```sh
cargo build --release --locked
python3 .agents/skills/verify-agents-picker/scripts/verify.py doctor
```

ビルド成功後、各 `run` が新しい専用tmuxサーバーと160列×32行のPTYを作り、TUIを起動する。`Agents`、`2/2`、`LIVE verify-alpha` が表示されれば準備完了。共有Herdrのインストール、リンク、設定変更は不要。各runは固有のソケット、設定ファイル、プラグイン状態ディレクトリを使い並行実行可能。`HOME` は変更しない。

実Herdrでの確認は以下の既存入口を使う。既存セッションを操作するため、ほかのエージェントと同じ画面を同時に操作しない。初回の `herdr plugin link "$PWD"` はユーザーがローカルリンクを求めた場合だけ行う。

```sh
herdr --version
herdr agent list
herdr workspace list
herdr tab list
herdr plugin pane open --plugin yxhta.agents-picker --entrypoint picker
```

Herdr 0.7.2以上とリンク済みプラグインが必要。実画面はHerdrアプリのUI操作ツールで観察する。エージェントがいない場合は空状態だけ確認できる。実Herdrの一覧や端末内容は機密を含み得るので、証拠として保存する範囲は検証対象に絞る。

## Doctor

```sh
python3 .agents/skills/verify-agents-picker/scripts/verify.py doctor
```

読み取り専用。バイナリの `--help`、絶対パス、SHA-256とtmuxバージョンを出力する。変更後は必ず再ビルドしてから実行する。怪しい場合はまずこれを再実行し、検証対象のバイナリを確認する。実Herdrでは上のversion/listコマンドが成功し、狙ったワークスペースとペインが存在することも確認する。CLI成功だけでは表示中クライアントの切り替え成功とは判断しない。

## Drive

証拠ディレクトリは毎回新しいパスを指定する。既存ディレクトリへの上書きは拒否する。

```sh
python3 .agents/skills/verify-agents-picker/scripts/verify.py run search --evidence /private/tmp/agents-picker-evidence/search-001
python3 .agents/skills/verify-agents-picker/scripts/verify.py run navigation --evidence /private/tmp/agents-picker-evidence/navigation-001
python3 .agents/skills/verify-agents-picker/scripts/verify.py run preview --evidence /private/tmp/agents-picker-evidence/preview-001
python3 .agents/skills/verify-agents-picker/scripts/verify.py run refresh --evidence /private/tmp/agents-picker-evidence/refresh-001
python3 .agents/skills/verify-agents-picker/scripts/verify.py run launch --evidence /private/tmp/agents-picker-evidence/launch-001
```

画面の期待文字列を最大8秒待つ。固定待ち時間だけで成功としない。`navigation` はEnterを押し、終了後にworkspace→tab→agentのフォーカス先と順序を呼び出し履歴で検証する。ほかの経路ではフォーカス呼び出しがないことを確認する。模擬CLIは未対応コマンドをエラーにする。ネットワークや実Herdrには接続しない。この隔離はdry-run名への信頼ではなく、専用CLI実装と `calls.jsonl` によって確認する。

tmuxソケットの作成がサンドボックスで拒否された場合は、同じ隔離したrunを適切な実行権限で再実行する。ユーザーのtmuxやHerdrへ接続する代替は使わない。失敗したrunもfinallyで片付け、失敗画面と呼び出し履歴を残す。

## Evidence

`--evidence` に指定した場所に以下を保存する。

- `doctor.json`: バイナリのパス、ハッシュ、ヘルプ、tmuxバージョン。
- `actions.jsonl`: feature ID、時刻、起動、キー入力、画面採取、後片付け。
- 各段階の `.txt` と `.ansi`: tmuxが再構成した画面。ANSI版は色・ハイライトも保持する。
- `calls.jsonl`: 模擬Herdrの全呼び出し。実際の副作用の代わりに、本番境界に渡した要求を証明する。
- `fixture.json` と `config.toml`: 検証対象の外部状態と設定。
- `exit.json`: プロセス終了コード。
- `result.json`: 成功したfeatureと検証境界。失敗時は生成しない。

実ユーザーと同じキー入力で到達し、操作前・操作後を対にして証明する。内部Appのsetterや既存ユニットテストだけを操作証拠にしない。実HerdrでEnterを確認する場合は、対象workspace/tab/paneの一覧と実際の表示画面を操作前後に採取する。実Herdrのライブ更新は対象エージェント自身の通常操作で出力を発生させ、プレビューにも反映されることを確認する。模擬フレームだけで本番ストリームの互換性を証明したと報告しない。

## Cleanup

runは成功・失敗を問わず、自分が起動したペインへCtrl+Cを送り、終了を待ち、自分のソケットだけに `tmux kill-server` を送る。プレビュー子プロセスはTUIが回収する。模擬ストリームにも8秒の上限がある。最後に実行専用のscratchを削除する。証拠は先に指定ディレクトリへコピーするため残る。

プロセス名によるkillは禁止。既存Herdrを終了しない。実Herdrのポップアップはnavigateモードの `q` またはCtrl+Cで閉じる。Enterで切り替えた場合は控えておいた元のworkspace/tab/paneへ戻す。既存設定やプラグインリンクを削除しない。

```sh
# runの後、証拠が残っていることを確認する
ls /private/tmp/agents-picker-evidence/search-001
cat /private/tmp/agents-picker-evidence/search-001/result.json
```

## Helpers

実行可能な [scripts/verify.py](scripts/verify.py) を同梱。上の `doctor` / `run` が公開入口。別ビルドを検証する場合は `--binary /absolute/path/agents-picker` を指定する。`child` と `fixture` はハーネス内部入口で、手動起動しない。
