---
name: sim-compare
description: シミュレータ(sim-core)の変更の前後で、答え・波形・速さを比べる。ニュートン法・刻みの制御・動作点の戦略・素子のモデルなど、過渡解析や DC の結果や速さが変わりうる変更をしたとき、「前後で比べて」「速さを測って」「ngspice と比べて」と言われたとき、コミットを勧める前に使う。
---

# シミュレータの前後比べ

`tools/sim_compare.py` がスナップショットを取り、2 つを比べる。中身は `crates/app/src/tests/sim_snapshot.rs`(無視されるテスト)と `tests_bench`(sim-core)。スナップショットは `target/sim-compare/<名前>/`(git の対象外)。

## 手順

1. **変更前**を取る。作業ツリーが変更前ならそのまま、もう変えてしまったならコミットを指定する(別の worktree で組み立てるので作業ツリーは触らない。初回は数分かかる):
   ```
   python tools/sim_compare.py snapshot before
   python tools/sim_compare.py snapshot before --rev HEAD
   ```
2. 変更する。
3. **変更後**を取って比べる:
   ```
   python tools/sim_compare.py snapshot after
   python tools/sim_compare.py compare before after
   ```
   どれも数分かかるので `run_in_background` で回す。

オプション: `--ngspice`(同じ設定で ngspice の過渡解析の時間も取る。起動の時間は除く)、`--only a,b`(そのサンプルだけ。デモとベンチは省く。調べている途中の速い確認用)、`--no-bench`、`--repeat N`(既定 3 回の最速)。比べる側は `--time-share 0.15 --time-floor 0.0005`(この割合とこの秒数の両方を超えた時間の変化だけ出す)。

## 結果の読み方

- `[bench]`: 時間以外の行の違い。動作点の戦略ごとの成否・反復回数、過渡の点数など。**違いが出たら理由を説明できるまで調べる**(答えが変わったのか、反復の数だけか)。
- `[demo waveforms]`: 総合デモ(配線なし・あり)とオペアンプの回路の全信号・全点の一致。`identical` でなければ、最初に違う時刻と最大の差が出る。
- `[examples]`: 全サンプルの過渡の合計時間、波形が変わったサンプル(値の指紋が違う)、遅く・速くなったサンプル、新しく失敗したサンプル。
- `[ngspice, …]`: 自前と ngspice の比(中央値・1.5 倍以上遅いもの・ngspice が失敗したもの)。

報告では、時間の数字と一緒に「波形が同じか」「ベンチの答えが同じか」を必ず書く。速くなっても波形が変わったなら、基準(刻みを細かく・許容を小さくした実行)と比べて、どちらが正しいかを確かめてから勧める。

## このスナップショットに無いもの

- **手元にしか無いモデル**(メーカーのモデル。再配布できないのでリポジトリに入れない。例: `eval-board/TC4421.zip`)は入らない。そのモデルに効く変更なら、一時的なテストを書いて前後で回し、終わったら消す。展開したファイルも消す。
- 合成テストの時間(`tests_blinker` など)は入らない。必要なら `cargo test --release -p sim-core --lib <テスト名>` の `finished in` を前後で比べる(`git stash` で前の版に戻すときは、戻し忘れに注意)。
- 全体のテストは別に回す(`cargo test --workspace` と `cargo clippy --workspace --all-targets`、バックグラウンドで 1 回)。

## 一時的な計測を入れるとき

原因を調べるための `eprintln!` やカウンタは、入れる前のファイルを scratchpad にコピーしておき、終わったらそのコピーか `git checkout` で戻す。戻したあと `grep -rn "TMP" crates/` で残りが無いか確かめてから、スナップショットを取る・コミットする。
