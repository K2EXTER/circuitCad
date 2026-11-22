# app: デジタル回路を動かしながら見る (`schematic/live_logic.rs`、候補 B8、2026-11-18)

回路図の上に重ねる学習のモード。設計のエディタ(パネル・解析)には混ぜない(`docs/candidates.md` の B 節の方針「画面と入口は分け、回路のデータとシミュレータは 1 つ」)。

## 入口
- 学ぶメニュー「デジタル回路を動かしながら見る」(`main.rs`。動かしている間は「やめる」に変わる)。
- レッスン 87〜89(`lessons/digital.rs` の `HALF_ADDER`・`SR_LATCH`・`COUNTER_LIVE`。`setup` が `start_live_logic` を呼ぶ)。学習パス「部品の素顔」の段階 7「デジタル回路を動かす」(`learn/paths.rs`)。
- サンプル一覧の 3 つ(`samples.rs` の `sample!(live …)`。`Sample::live` が真だと開いたあとにモードに入る)。
- 起動引数 `--live-logic [SW1,SW2]`(名前の部品のスイッチを切り替えた状態で始める)、`--live-slow`(ゆっくり再生)。撮影用(`args.rs`)。
- 設計の画面のパネルにはボタンを置かない。「試作」のタブの左の「デジタル回路を回路図の上で動かす」(`BreadboardView::want_live_logic` を `main.rs` が見て、回路図のタブに移って始める)。

## 仕組み
- **動かす部分は `sim-core/src/live_logic.rs`**(2026-11-22 に app から移した。ブラウザ版 `crates/web` と共有)。入力・クロック・解く・LED・7 セグメント・記憶・真理値表・グリッチと競合(`races` は `TimingViolation` のまま。文は画面が作る: app の `race_text`)。High / Low の判定は `sim-core/src/logic_level.rs`(`app/logic_plot.rs` は `pub use`)。時間は呼ぶ側が測って `tick(dt)` で渡す。app の `schematic/live_logic.rs` は表示の状態(ロジックアナライザの `PlotState`・開いたバス・前のフレームの時刻)だけを持つ包みで、中身へは `Deref` で届く(レッスン・試作のタブ・`for_bench.rs` の書き方は前のまま)。回路の正しさの試験(半加算器・SR ラッチ・カウンタ・7 セグメント・非同期クリアのグリッチ・シフトレジスタ・信号機)は sim-core に移し、app には編集画面から始めてスイッチが LED に届く 1 本だけを残した。
- **エンジンは過渡解析と同じ**。`sim_core::TranRun`(`tran.rs`。`transient` はこれを 1 回 `advance_to(tstop)` したもの)を開いたままにし、毎フレーム「表示の時刻」まで続きを解く(`LiveLogic::tick`)。学習用の別の計算は無い。
- 始めるとき(`LiveLogic::new`): `sim_circuit()` の回路で、**PULSE の電源はクロック**として `Dc(v1)` に置き換え、帯のつまみ・ボタンが `set_source` で `v1`/`v2` を切り替える。**スイッチの部品**は、モデルの中の電源 `Vpress`(`SW_PUSH`)・`Vpos`(`SW_SLIDE`)を持つ部品(`switch_of`)。クリックで `set_source("SW1.Vpos", Dc(0/1))`。押しボタンは押している間だけ(Shift + クリックで押したまま)。
- 時間: 普通は画面の 1 秒 = シミュレーションの 1 秒(刻みの上限 20 ms。コンデンサの無い回路なので刻みは答えに効かない、デジタルの出来事は刻みと関係なく正確)。**ゆっくり**は画面の 1 秒 = 10 ns(`SLOW_RATE`)。ゆっくりの間は、クロックは前の変化が伝わりきってから次を出す(`settled`)。
- **1 イベント**: `TranRun::next_digital_event`(どれかのセルの出力が遅れの後に変わる時刻、`Compiled::dig_next_any_event`)の 0.2 ns 後まで解く(アナログ節点 = LED の抵抗の付いた出力は、その時刻の次の解で式に入るため)。待っている出来事が無いとき(スイッチを変えた直後)は 1 ns 進めて、スイッチの線を解く。
- **1 クロック**: 上がって下がる 2 回の変化。普通の速さではすぐ、ゆっくりでは 1 回ずつ伝わりきってから(`burst`)。そのあと一時停止。一時停止中(普通の速さ)のスイッチはすぐ効く。ゆっくりで一時停止中は「1 イベント」で進める。
- 回路を変えたら(`signature` が変わったら)モードを終える。動かしている間、キャンバスは移動・拡大とスイッチのクリックだけ(`canvas.rs` で `live_logic_input` のあと `return`)。

## 表示
- 色は `canvas_theme.rs` の役 `LogicHigh`・`LogicLow`・`LogicX`・`LedOn`(設定の窓で変えられる)。High / Low / X は `logic_plot::thresholds`(電源の 30 % / 70 %)。電源はセルの電源の最大(`TranRun::logic_supply`)。
- 色を塗るのは**ロジックのネット**だけ: ロジック部品(モデルに `Y` 行のある部品)のピン・スイッチの線・クロックの線。LED と抵抗の間のネットは LED の順電圧で High でも Low でもないので塗らない(黄色の X にすると紛らわしかった)。電源のネット(直流の電源・電源記号・GND が付くネット)はピンの 0 / 1 も出さない。
- LED は アノード − カソード > 1.2 V で光る印だけ(明るさは出さない)。
- **7 セグメント表示器**(glyph `seg7` の部品、`Seg7`): ピン名 a〜g・dp と COM のネットを覚え、|V(セグメント) − V(COM)| > 1.2 V のセグメントを、記号の 8 の字(`glyphs::seg7_segments`)の上に `LedOn` の色で太く描く(向きを問わないので、アノードコモンも光る)。テストは `Seg7::shown` で数字を読む(CD4511B の 6・9 の書き方も)。
- **グリッチ**(2026-11-19): `solve_to` は、フレームの時刻まで一気に解かず、デジタルの出来事ごと(1 フレームに `EVENTS_PER_FRAME` = 400 まで)に 0.2 ns 過ぎまで解いて、ロジックのネット(クロックのネットは除く。「1 クロック」は短いパルスをわざと作る)のレベルを見る。スイッチ・クロックを変えた直後は待っている出来事が無いので、最初に 1 ns 解く。前の変化から `GLITCH_MAX`(100 ns)以内に元のレベルに戻ったら、その幅のグリッチ(`Glitch`)。線を赤く太く光らせ(`GLITCH_SHOW` = 4 秒で薄れる)、帯の 2 行目に「グリッチ: CLR_N が 7 ns だけ Low」。
- **競合**: `TranRun::timing_violations`(sim-core に足した。`finish` と同じ点検を、続いている途中で)の違反を、帯に「競合: U1.Y1: セットアップ時間の違反 …」と 3 つまで。
- **中間の電圧**: ロジックの部品のピンで、High でも Low でもないネットは「X」の代わりに電圧(`2.4V`)。
- **中の記憶**: `TranRun::cell_states`(`U1.YFa` のように部品.セル)を部品ごとにまとめ、セルの名前の順に並べて、部品の箱の上に「記憶 0101 (= 5)」(最初のセルが右端。3 ビット以上は数も)。キャンバスの地の色の小さな下敷きの上に描く。スライドスイッチはレバー、押しボタンは丸(押していると塗る)を、線の色で描く。
- ロジックアナライザ: `logic_plot::draw` をそのまま使う。見るネットは入力・クロック・ラベルのあるロジックのネット(12 本まで)。D0〜D3 のような名前はバスにまとまり 16 進。時間の軸は画面の秒(ゆっくりのときもそのまま。だから伝搬遅延が幅として見える)、直近 10 秒が右へ流れる。
- 真理値表: クロックの無い回路で、入力 1〜4 本と LED が 1 つ以上のとき。落ち着いている(`settled`)フレームごとに、入力のネットのレベルを鍵に、LED の点灯を記録する。同じ鍵で違う値を見たら「記憶」(前の状態で決まる)。列の名前は入力のネット、出力は LED を抵抗の向こうで駆動するネットの名前(無ければ部品番号)。

## 部品
- `SW_PUSH`(タクトスイッチ 6 mm、ピン 1・2、足は 4 本)と `SW_SLIDE`(1 回路 2 接点、ピン 1 = A・2 = COM・3 = B、2.54 mm)。`part-lib/examples/`。モデルは `S` 素子 + 位置を持つ電源。ふつうの解析では離した / A 側のまま。記号は `glyphs.rs` の `switch_push`・`switch_spdt`(`part_lib::BODY_GLYPHS` と部品作成フォームの記号の選択肢 `part_form/glyph_pick.rs` にもある)。
- `SEG7_CC_0.56`(7 セグメント 1 桁、0.56 インチ、赤、カソードコモン。DIP 10 ピン・列の間 15.24 mm、ピンは 5161AS 形: 1=e 2=d 3=COM 4=c 5=dp 6=b 7=a 8=COM 9=f 10=g)。モデルはセグメントごとに赤色 LED(WP7113ID の値)を COM へ、COM の 2 本は中で 0.01 Ω。記号は箱の中に 8 の字(`seg7`、重ねる記号 `overlay_glyph`)。

## サンプル(`tests/examples.rs` の `write_live_logic_examples` で作る。`seg7_counter` は `write_seg7_counter_example`、`shift_register`・`traffic_light` は `write_live_logic_more_examples`)
- `shift_register`: SW1(スライド)で DATA、VCK → 74HC164(B・CLR は VCC)、QA〜QH に LED(間隔 16。9 だと抵抗の線が隣とつながった)。テスト `the_shift_register_moves_its_pattern_one_place_per_clock`。
- `traffic_light`: VCK → 74HC4017(CE・MR は GND)、Q0〜Q3 をダイオードで GREEN、Q4 は黄、Q5〜Q9 をダイオードで RED、それぞれ 330 Ω + LED。テスト `the_traffic_light_goes_green_yellow_red`(4・1・5 クロック)。
- 9 → 0 のグリッチのテスト `the_asynchronous_clear_is_seen_as_a_glitch`(`seg7_counter`、CLR_N の Low のパルス、競合なし)。
- `seg7_counter`(2026-11-19): VCK(1 Hz)→ 74HC161、74HC00 の NAND(Q1, Q3)で 10 になったら CLR(0〜9)、CD4511B(LT・BL は VCC、LE は GND)→ 470 Ω × 7 → SEG7_CC_0.56(COM と dp は GND)。テスト `the_seven_segment_counter_shows_0_to_9_and_its_memory`(11 クロックで 1 ずつ・9 の次は 0、74HC161 の記憶が表示と同じ)。
- `half_adder`: SW1・SW2(スライド、A=GND・B=VCC)→ 74HC86(S)・74HC08(C)→ 330 Ω + LED。
- `sr_latch`: 10 kΩ のプルアップと押しボタン(GND へ)で S_N・R_N、74HC00 の 2 つでラッチ(Q・QB)、残りの 2 つをインバータにして LED_Q・LED_QB を駆動。**ラッチの線に LED を直接つながない**: 出力がアナログ節点にあるゲートのラッチは、DC の動作点で Q = QB = 0 になり、過渡解析でゲートの遅れの 2 倍の周期で鳴り続ける(エンジンの既存の不具合。下)。
- `live_counter`: 74HC161 と VCK(PULSE 1 Hz。動かしている間はつまみのクロック)、Q0〜Q3 に LED。
- 生成の `side_label`: `label` は中心からの長い方向に線を出すので、縦長の IC の横のピンの線が隣のピンを通ってショートした(A と B、CLK と VCC がつながった)。横のピンは横に出す。

## 見つけた不具合(直していない)
- **出力がアナログ節点にあるゲートで組んだラッチの DC 動作点**: 74HC00 の 2 つの NAND を交差につなぎ、Q と QB に抵抗(LED)を付けると、`dc_op` が Q = QB = 0(NAND(1,0) = 1 と矛盾)を返し、`transient` の開始(`dig_begin_run`)から 2 × td の周期で Q と QB が同時に振れ続ける(1 µs で 121 回)。デジタル網(セルだけがつながる節点)なら、ゲートを 1 つずつ落ち着かせるので片方の状態に落ちる。アナログ節点の出力は、静的な落ち着かせでも次の解まで電圧が変わらないので、両方のゲートが同じ古い値を読むのが原因と見ている。直すなら `dc/digital.rs` の静的な `settle` で、アナログ節点に 1 つのセルだけが出す出力を、次のゲートが読む値にすぐ反映する。

## 試験(`live_logic.rs` の `tests`)
- 半加算器: 4 通りで LED が S = A xor B・C = A and B、真理値表が埋まる。
- SR ラッチ: 始めに落ち着いている(鳴らない)・Q と QB が反対、セット・リセットのあと「記憶」の行。
- カウンタ: 「1 クロック」で 1 ずつ増える、ゆっくりでは 1 クロックの変化が何回もの「1 イベント」に分かれる。
- 続きから解いた結果と頭から解いた結果の一致は sim-core の `tests_tranrun.rs`。

## 残り(`docs/candidates.md` の B8)
電子サイコロのサンプル、LED の明るさ(電流)とロジック部品の外のネットの電圧の表示。
