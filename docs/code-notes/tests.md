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

2026-11-14 の時間(テストを回す時間の合計、ビルドは別): 整理の前 117 秒 → 約 20 秒。アプリ 6.7 秒・ccad の結合テスト 7 秒・sim-core 3 秒・part-lib 2.4 秒。

2026-11-22: 51 秒に戻っていた → 約 26 秒。① ccad の結合テストは、ライブラリを読むのは 1 回でも、スクリプトごとに約 15 500 個を写していた (1 回 0.07 秒、スクリプトそのものの 70 倍。トランジスタのモデルの照合だけで 488 回)。`Session.lib` を `Arc<Vec<PartDef>>` にし、変えるコマンド (`fit write`・`lineup write`・`model`) だけ `Arc::make_mut` で写す。モデルの照合 5 本は 0.7〜7 秒 → 各 0.3 秒。② part-lib の `fit/tests.rs` は 4 つのテストがそれぞれ同梱の 63 個を順に当てはめ直していた (各 17 秒)。`bundled()` を `OnceLock` で 1 回にし `par_map` で並列に (4 本で 5.4 秒、いちばん遅い当てはめで決まる)。残りの 2 秒超え: アプリのレッスン・サンプルの過渡解析 (2〜4 秒)、`op_amps_with_an_earlier_bundled_model_get_the_data_sheet_model` (4 秒。古いモデルが 1 回だけ置き換わることを確かめるため `install_examples` を約 150 回呼ぶ)。

アプリのテストのエディタ(`SchEditor::default`)は、ユーザーの部品フォルダではなく、テスト用のフォルダ(`%TEMP%/circuitCad-app-test-lib`、実行の最初に空にして同梱部品だけ)を一度だけ読む(`schematic/mod.rs` の `startup_library`、`#[cfg(test)]`)。部品の一覧は `Arc<Vec<PartDef>>` で、エディタごとに写さない。前は 1 つ作るたびにユーザーのフォルダを読んで約 15 000 個を写し 0.1 秒、アプリのテストは 24 秒かかっていた。

## 足し方の決まり

1. **2 秒を超えるテストは直す。** ケースを並列に(`sim_core::par_map`。例: `every_lesson_loads_and_its_circuit_simulates`)、重い準備を共有する(例: ccad の結合テストはライブラリを一度だけ読む)、またはふだんは飛ばして(`#[ignore]`)理由と回し方を `///` に書く。
2. **教材は全部を回すテストが確かめる。** レッスン・チャレンジ・サンプルを足すと、`every_lesson_loads_and_its_circuit_simulates`・`every_answer_passes_and_every_start_sheet_is_bare`・`shipped_examples_simulate` などが自動で回す。専用のテストは、その教材に固有のこと(手順が進むか、数字が合うか)だけにする。
3. **同じ形の確かめは、表に 1 行足す。** 入力と期待だけが違うテストを新しく書かない。表のテストは、最初の失敗で止まらず全ケースを回し、失敗したケースの名前を全部出す(`bad` に集めて最後に `assert!(bad.is_empty(), …)`)。
4. **計算の正しさは下の層で確かめる。** シミュレーション・配線・DRC の答えは `sim-core`・`pcb-core` のテストで。アプリや ccad のテストは、つながっていること(画面・コマンドから呼べて、答えが届くこと)を数件にとどめる。ccad のテストは `tests/integration/common.rs` の `ccad(script, Flags)` で、実行ファイルを起動しない。
5. **調べもの用のテストは、使い終わったら消す。** 手元のファイルと環境変数が要る `#[ignore]` のテストには、何を調べるものかと回し方を `///` に書く。説明のない一時的なものは残さない(2026-11-14 に `print_finish_near` など 3 つを消した)。
6. **手で直す操作のテストは、自動の並べ方の結果を下地にしない。** 線を引く・消す・部品を動かす試験の基板は `strip_edit::plain_perfboard` (決まった置き方で線なし) から作り、要る線は試験の中で手で引く。自動の並べ方を下地にすると、並べ方や配線を変えるたびに「下の行は空いている」「端の列に線は来ない」のような前提が崩れて、関係の無い試験が落ちる (2026-11-26 に 3 つ落ちた)。自動の並べ方そのものを確かめる試験 (固定・区画の箱・揃える・パスコンの距離) は別。

## 見直し

月に 1 回ほど、`tools/test_times.py` で遅いテストを見る。同じ形のテストが増えた所は、表にまとめる。

表になっているもの(2026-11-14): `sim-core/src/tests_flat.rs` の `vendor_texts_give_their_voltages`(SPICE の文を読んで解き、1 つの電圧を見る 15 件。`Case`)と `vendor_texts_that_are_refused_say_why`(断る文と、エラーに要る言葉。`Refused`)、`pcb-core/src/drc/tests.rs` の `track_clearances`(配線と、間隔の違反の数 5 件)。どれも全ケースを回し、失敗したケースを全部名前で出す。`tests_flat.rs` のほかの 57 件は、1 つのテストで何通りも確かめるか過渡解析を見るもので、形がそろわないので残した。
