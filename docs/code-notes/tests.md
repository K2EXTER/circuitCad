# テストの回し方と足し方

テストは 1 か月で 822 → 2,169 件(2026-10-15 → 11-14)と、1 日に約 45 件ずつ増えている。数より、全体の時間が延びないことと、同じ形のテストが並ばないことを大事にする。

## 回し方

| いつ | どうする |
|---|---|
| 作業中 | 変えた所のテストだけ: `cargo test -p <クレート> <名前の一部>` |
| コミットの前 | `python tools/test_changed.py`: 変わったファイルからクレートを選び、それを使うクレートも足して `cargo test -p …` と clippy。`--list` で選んだ理由だけ。比べる先は `--base`(既定は origin/main) |
| push の前・CI | 全体: `cargo test --workspace` と `cargo clippy --workspace --all-targets` |
| ときどき(テストを足した後など) | `python tools/test_times.py`: 1 件ずつの時間、実行ファイルごとの時間、2 秒を超えたテスト。`--serial` で 1 件ずつ順に(ほかのテストに邪魔されない時間)、`-p` でクレートを絞る |

`test_changed.py` の選び方: `crates/<名前>/` のファイルはそのクレートと、それに依存するクレート全部(`cargo metadata`)。それ以外の場所のファイルは、ソースの文字列(`include_str!`・`.join("../../docs/examples")` など。コメントは数えない)がその名前か、2 段以上のフォルダ(`docs/examples`)を書いているクレート。`Cargo.toml`・`Cargo.lock`・`.cargo/`・`.github/` は全部。ほかの docs・tools は何も選ばない。

`test_times.py` は libtest の `--report-time`(安定版では使えない)を、テストの実行ファイルにだけ `RUSTC_BOOTSTRAP=1` を渡して使う(作り直しは起きない)。実行ファイルは cargo と同じく、クレートのフォルダで動かす。

2026-11-14 の時間(テストを回す時間の合計、ビルドは別): 整理の前 117 秒 → 38 秒。アプリ 24 秒・ccad の結合テスト 7 秒・sim-core 3 秒・part-lib 2.4 秒。

## 足し方の決まり

1. **2 秒を超えるテストは直す。** ケースを並列に(`sim_core::par_map`。例: `every_lesson_loads_and_its_circuit_simulates`)、重い準備を共有する(例: ccad の結合テストはライブラリを一度だけ読む)、またはふだんは飛ばして(`#[ignore]`)理由と回し方を `///` に書く。
2. **教材は全部を回すテストが確かめる。** レッスン・チャレンジ・サンプルを足すと、`every_lesson_loads_and_its_circuit_simulates`・`every_answer_passes_and_every_start_sheet_is_bare`・`shipped_examples_simulate` などが自動で回す。専用のテストは、その教材に固有のこと(手順が進むか、数字が合うか)だけにする。
3. **同じ形の確かめは、表に 1 行足す。** 入力と期待だけが違うテストを新しく書かない。表のテストは、最初の失敗で止まらず全ケースを回し、失敗したケースの名前を全部出す(`bad` に集めて最後に `assert!(bad.is_empty(), …)`)。
4. **計算の正しさは下の層で確かめる。** シミュレーション・配線・DRC の答えは `sim-core`・`pcb-core` のテストで。アプリや ccad のテストは、つながっていること(画面・コマンドから呼べて、答えが届くこと)を数件にとどめる。ccad のテストは `tests/integration/common.rs` の `ccad(script, Flags)` で、実行ファイルを起動しない。
5. **調べもの用のテストは、使い終わったら消す。** 手元のファイルと環境変数が要る `#[ignore]` のテストには、何を調べるものかと回し方を `///` に書く。説明のない一時的なものは残さない(2026-11-14 に `print_finish_near` など 3 つを消した)。

## 見直し

月に 1 回ほど、`tools/test_times.py` で遅いテストを見る。同じ形のテストが増えた所(例: `sim-core/src/tests_flat.rs` の SPICE の読み込み 77 件)は、表にまとめる候補。
