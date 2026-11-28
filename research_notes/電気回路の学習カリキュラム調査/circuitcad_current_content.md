# circuitCad が今持っている学習の中身 (ソースコードからの棚卸し、2026-11-28 時点の main)

調べた範囲: ローカルのリポジトリ E:/circuitCad のみ (Web は見ていない)。出典はすべてリポジトリ内のファイル。数はスクリプトでソースを数えた値。

主な出典:
- 学習パス: [crates/app/src/learn/paths.rs](E:/circuitCad/crates/app/src/learn/paths.rs)
- レッスン一覧: [crates/app/src/schematic/lessons/mod.rs](E:/circuitCad/crates/app/src/schematic/lessons/mod.rs) の `LESSONS` と各ファイル (basics・devices・analog・supply・micro・models・signals・debug・repair・parts・hierarchy・digital)
- チャレンジ: [crates/app/src/schematic/challenge/mod.rs](E:/circuitCad/crates/app/src/schematic/challenge/mod.rs)
- 基板レッスン: [crates/app/src/board_editor/lesson.rs](E:/circuitCad/crates/app/src/board_editor/lesson.rs)
- 実機の作業ガイド: [crates/app/src/bench_guide.rs](E:/circuitCad/crates/app/src/bench_guide.rs)、[crates/app/src/ds_reading.rs](E:/circuitCad/crates/app/src/ds_reading.rs)、[crates/app/src/measure_guide.rs](E:/circuitCad/crates/app/src/measure_guide.rs)、[crates/app/src/power_on_guide.rs](E:/circuitCad/crates/app/src/power_on_guide.rs)、[crates/app/src/bench_tab.rs](E:/circuitCad/crates/app/src/bench_tab.rs)
- 用語集: [crates/app/src/glossary.rs](E:/circuitCad/crates/app/src/glossary.rs)、豆知識: [crates/app/src/tips.rs](E:/circuitCad/crates/app/src/tips.rs)、サンプル: [crates/app/src/samples.rs](E:/circuitCad/crates/app/src/samples.rs)
- 機能一覧: [docs/features.md](E:/circuitCad/docs/features.md)、学習パスのメモ: [docs/learning-path.md](E:/circuitCad/docs/learning-path.md)、候補: [docs/candidates.md](E:/circuitCad/docs/candidates.md)

## 1. 学習パス 9 本の順・項目・前提 (`requires`)

### Takeaway
パスは `PATHS` の順に「安定化電源 → Arduino 互換 → Mega 互換 → 基板づくり → スイッチ → 電池 → 信号 → 部品の素顔 → 修理」の 9 本。各項目は `requires` で前提を持つ樹形図で、同じ id の項目は複数のパスで共有され、1 か所で終えると他のパスでも済みになる。項目の種類は レッスン (Lesson)・チャレンジ (Build)・基板レッスン (Board)・手の作業のチェックリスト (Hands)・組み立てて測る (Measure)。

### Cited Findings
- `PATHS = [POWER_PATH, ARDUINO_PATH, MEGA_PATH, BOARD_PATH, SWITCH_PATH, BATTERY_PATH, SIGNAL_PATH, PARTS_PATH, REPAIR_PATH]` — [paths.rs 54 行](E:/circuitCad/crates/app/src/learn/paths.rs)
- 「レッスンの記録は項目の id で持つので、あるパスで終えたレッスンは他のパスでも終わり。前提 (`requires`) はパスごとに決める」(コメント) — [paths.rs 16 行](E:/circuitCad/crates/app/src/learn/paths.rs)
- 準備中 (`planned`/`plan`) の項目は今は 0 件 (「none is planned right now」) — [paths.rs 9 行・145 行](E:/circuitCad/crates/app/src/learn/paths.rs)、[docs/learning-path.md](E:/circuitCad/docs/learning-path.md)「準備中 (`plan`): なし」

以下、項目 id「題名」← 前提。前提が `[]` のものは入口。(B) は回路を自分で作る印 `build: true` の付いたもの。

#### パス 1: `power_supply`「安定化電源を作ろう」(28 項目) — [paths.rs 85–143 行](E:/circuitCad/crates/app/src/learn/paths.rs)
目標: AC アダプタや電池から負荷が変わっても一定の 5V を作る電源を原理から理解し、回路図・基板・実機まで。実験は 12V 以下。
1. 入門: 電圧・電流と測り方
   - `divider`「電圧を分ける (分圧回路)」← [] (B)
   - `led`「LED と電流制限抵抗」← [] (B)
   - `coin_cell`「電池の内部抵抗」← divider
2. 基礎: 電源をつくる部品
   - `diode_curve`「ダイオードの特性」← led
   - `rectifier`「交流を直流にする (整流・平滑・リップル)」← diode_curve
   - `electrolytic_life`「電解コンデンサの寿命」← rectifier
   - `zener`「ツェナーで電圧を一定にする」← diode_curve, divider (B)
   - `bjt_switch`「トランジスタのスイッチ」← led
   - `emitter_follower`「エミッタフォロワ」← zener, bjt_switch (B)
   - `debug_circuit`「動かない回路を直す (デバッグ)」← zener
3. 応用: 安定化のしくみ
   - `opamp_noninv`「非反転増幅」← divider / `opamp_inv`「反転増幅」← opamp_noninv (B)
   - `series_regulator`「基準電圧と帰還で安定化する」← opamp_inv, emitter_follower (B)
   - `regulator_78xx`「三端子レギュレータ (78xx / LDO)」← emitter_follower (B)
   - `tl431_ref`「TL431」← zener
   - `datasheet_pdf`「データシートの PDF を読む」(Hands) ← regulator_78xx
   - `ldo` ← regulator_78xx / `heat`「熱と定格」← regulator_78xx / `protection`「電源を守る」← diode_curve, regulator_78xx
   - `coil_layer_short`「コイルのレアショート」← protection
   - `variable_supply`「ダイヤルで電圧を変える電源 (トランス + LM317)」← rectifier, regulator_78xx
   - `buck`「降圧コンバータ」← regulator_78xx, heat
4. 製作: 回路図から基板、実機まで
   - `draw_psu`「安定化電源の回路図を自分で設計する (最終課題)」(Build) ← series_regulator, regulator_78xx, protection
   - `board` ← draw_psu / `board_copper` ← board / `board_fix` ← board / `board_repair` ← board_fix
   - `build_psu`「組み立てて、性能を測る」(Measure) ← board_fix, board_copper

#### パス 2: `arduino_board`「Arduino 互換ボードを作ろう」(25 項目) — [paths.rs 153–220 行](E:/circuitCad/crates/app/src/learn/paths.rs)
目標: ATmega328P に水晶・リセット・電源をつけた互換ボードを回路図・基板・実機まで。ゴール文に「マイコンの中身はシミュレーションできないので、周りの回路 (電源・水晶・リセット・LED) を確かめます」とある (試作のタブでスケッチが動く今の機能より前に書かれた文)。
1. 土台: `datasheet`「データシートの読み方」← [] / `pullup` ← divider / `decoupling`「バイパスコンデンサ」← divider
2. クロックとリセット: `crystal` ← datasheet / `reset` ← pullup
3. 入出力と通信: `pin_led` ← led, datasheet / `temp_sensors` ← datasheet, divider / `current_sensors` ← temp_sensors / `light_sensors` ← temp_sensors / `debounce` ← pullup / `flyback` ← pin_led / `equiv_model`「ULN2803A を等価回路で」← flyback, datasheet / `pwm` ← pin_led / `levels`「3.3V と 5V」← pullup / `serial` ← levels, reset / `eeprom_basics`「EEPROM (I²C)」← pullup
3b. 発展 USB: `usb_power` ← decoupling / `usb_speed` ← pullup / `usb_signal` ← usb_speed / `board_usb` ← usb_signal, board / `board_pairs` ← board_usb
4. 設計して基板に: `draw_arduino`(Build、最終課題) ← decoupling, crystal, reset, pin_led, serial / `board_arduino` ← draw_arduino, board
5. 作って動かす: `solder`「表面実装のはんだ付け」(Hands) ← board_arduino / `bootload`「ブートローダを書き込んで LED を点滅」(Hands) ← solder, serial

#### パス 3: `arduino_mega`「Arduino Mega 互換ボードを作ろう」(6 項目) — [paths.rs 224–250 行](E:/circuitCad/crates/app/src/learn/paths.rs)
1. `adc_input`「ADC の入力 (信号源 10kΩ 以下)」← pin_led, datasheet / `mega_power`「電源ピンが 5 組」← decoupling / `crystal_load`「水晶の負荷容量」← crystal / `ch340g`「CH340G と USB の差動」← usb_signal, serial
2. `board_mega`「100 ピンを手で仕上げる」← board_arduino
3. `solder_tqfp100`「100 ピン (0.5mm) のはんだ付け」(Hands) ← board_mega, solder

#### パス 4: `board_craft`「基板づくりの道」(8 項目) — [paths.rs 252–284 行](E:/circuitCad/crates/app/src/learn/paths.rs)
1. `board` ← [] / `board_copper` ← board / `board_fix` ← board
2. `board_arduino` ← board_fix / `board_usb` ← board_fix / `board_pairs` ← board_usb
3. `board_mega` ← board_arduino / `board_repair` ← board_fix

#### パス 5: `switching`「スイッチで負荷を動かそう」(10 項目) — [paths.rs 286–324 行](E:/circuitCad/crates/app/src/learn/paths.rs)
1. `bjt_switch` ← [] / `mosfet` ← bjt_switch / `pnp_highside` ← bjt_switch
2. `flyback` ← bjt_switch / `pwm` ← mosfet
3. `buck` ← mosfet, flyback / `switching_loss` ← buck / `diode_recovery` ← buck / `ldo_vs_buck` ← buck
4. `board_copper` ← buck

#### パス 6: `battery`「電池で動かそう」(6 項目) — [paths.rs 326–351 行](E:/circuitCad/crates/app/src/learn/paths.rs)
1. `coin_cell` ← [] / `tp4056`「リチウムイオン電池の充電」← coin_cell
2. `protection` ← tp4056 / `ldo` ← coin_cell / `heat` ← ldo
3. `buck` ← ldo, heat

#### パス 7: `signals`「信号を扱おう: フィルタ・増幅・発振」(10 項目) — [paths.rs 441–473 行](E:/circuitCad/crates/app/src/learn/paths.rs)
1. フィルタ: `rc_lowpass` ← [] (B) / `rlc` ← rc_lowpass / `hierarchy`「同じ回路を何回も使う (階層)」← rc_lowpass / `poles`「極と零」← rlc
2. 増幅と安定: `opamp_noninv` ← rc_lowpass / `opamp_inv` ← opamp_noninv (B) / `phase_margin` ← opamp_inv, poles / `slew_rate` ← opamp_noninv / `noise` ← opamp_noninv
3. 発振: `ne555`「555 で LED を点滅」← rc_lowpass

#### パス 8: `parts`「部品の素顔: 動作と特性と計算式」(32 項目) — [paths.rs 353–439 行](E:/circuitCad/crates/app/src/learn/paths.rs)
0. はじめに: `intro_ohm`「オームの法則」← [] / `intro_read`「回路図の読み方と値の書き方」← intro_ohm
1. 受動部品: `resistor` ← intro_read / `capacitor` ← resistor / `inductor` ← capacitor
2. ダイオード: `diode_basics` ← diode_curve / `led_basics` ← diode_basics, led / `segment_basics`「7 セグメント」← led_basics / `zener_basics` ← diode_basics, zener
3. 増やす部品: `transistor` ← [] / `mosfet_basics` ← transistor / `opamp_basics` ← opamp_noninv
4. センサと光: `thermistor_basics` ← temp_sensors / `phototransistor_basics` ← light_sensors / `photocoupler_basics` ← led_basics / `hall_basics` ← current_sensors
5. 電源と保護: `battery_basics` ← coin_cell / `regulator_basics` ← regulator_78xx / `transformer_basics` ← rectifier, inductor / `bridge_basics` ← transformer_basics / `fuse_basics` ← protection / `varistor_basics` ← fuse_basics
6. スイッチ・時計・音・ロジック: `switch_basics` ← pullup / `analog_switch_basics` ← switch_basics / `relay_basics` ← inductor, flyback / `crystal_basics` ← crystal, rlc / `speaker_basics` ← inductor / `logic_basics` ← levels
7. デジタル回路を動かす: `half_adder` ← logic_basics / `sr_latch` ← half_adder / `counter_live` ← sr_latch / `ring_oscillator`「MOSFET でロジックを組む」← counter_live

#### パス 9: `repair`「動かない回路を直そう (修理の道)」(5 項目) — [paths.rs 56–83 行](E:/circuitCad/crates/app/src/learn/paths.rs)
1. `repair_divider`「テスターの当て方 (分圧回路)」← divider
2. `repair_zener` ← zener, repair_divider / `repair_led`「トランジスタで点ける LED」← bjt_switch, repair_divider
3. `repair_regulator`「帰還のある電源」← series_regulator, repair_zener / `board_repair` ← board_fix, repair_zener

### Inferences
- 9 本のうち入口 (前提 `[]`) を持つのは power_supply (divider・led)・arduino_board (datasheet)・board_craft (board)・switching (bjt_switch)・battery (coin_cell)・signals (rc_lowpass)・parts (intro_ohm・transistor)。Mega と修理のパスは他のパスの項目を前提にしていて、単独では始められない (Mega は crystal・pin_led・usb_signal など、修理は divider・zener など)。
- 「部品の素顔」パスの 0 段目 (`intro_ohm`・`intro_read`) が一番やさしい入口だが、パスの並び (`PATHS`) では 8 番目にある。パス 1 の最初の divider はオームの法則の説明を前提にしている形になる。
- 項目の数の合計は 130 (同じ項目を複数のパスで数えた延べ)。

### Gaps
- 画面上で各パスの段階の説明 (`about`) と「実機で確かめよう」の文 (`real`) は今回は省いた (paths.rs に全部ある)。
- 実際の画面の並び (学習パスの地図と樹形図) は目で見ていない。

## 2. レッスン (94 本) の分野ごとの一覧

### Takeaway
回路図のレッスンは `LESSONS` に 94 本あり、すべてどれかのパスで使われている (パスに載っていないレッスンは 0 本)。手順 (Step) は 1 本あたり 2〜5 段で、状態を見て自動で印が付く。修理の 4 本は手順の代わりに仮想テスター。分野は DC 基礎・受動部品・半導体・オペアンプ・電源・マイコン周辺・USB・センサ・デジタル・信号/安定性・修理に広く分かれているが、マイコンのプログラミング (スケッチ) そのものを教えるレッスンは無い。

### Cited Findings
- `LESSONS` の並び (basics::DIVIDER から digital::RING_OSCILLATOR まで) — [lessons/mod.rs 75–163 行](E:/circuitCad/crates/app/src/schematic/lessons/mod.rs)
- レッスンの型: id・title・goal・summary・`limits`(「実物との違い」)・`real`(「実機で確かめよう」: 用意・つなぎ方・測り方・予想・安全)・`circuit`(埋め込みのサンプル)・`steps` — [lessons/mod.rs 36–50 行](E:/circuitCad/crates/app/src/schematic/lessons/mod.rs)
- 解く前に答えを予想させる問い (`Prediction`) があり、正解はシミュレーションの値に一番近い選択肢とする。問いがあるのは coin_cell・diode_curve・divider・emitter_follower・led・noise・phase_margin・rc_lowpass・rlc・zener の 10 レッスン (11 問程度) — [lessons/predict.rs](E:/circuitCad/crates/app/src/schematic/lessons/predict.rs)
- docs/features.md の記載は「レッスン 90 本以上・基板のレッスン 8 本: 手順つき、判定つき」— [docs/features.md 87 行](E:/circuitCad/docs/features.md)

以下、番号は画面の題名に付いている番号、[ファイル] は定義の置き場所、→ は使っているパス。

#### DC の基礎・回路図の読み方
- 52 `intro_ohm` はじめに 1: 電圧・電流・抵抗 (オームの法則) [parts] → 部品
- 53 `intro_read` はじめに 2: 回路図の読み方と値の書き方 (ラベル・GND・3.3k/100n/1Meg、M と m) [parts] → 部品
- 54 `resistor` 抵抗の素顔 (オームの法則・直列・並列・電力) [parts] → 部品
- 1 `divider` 電圧を分ける (分圧回路、許容差) [basics] → 電源・修理の前提
- 10 `coin_cell` 電池の内部抵抗と持ち時間 (CR2032) [devices] → 電源・電池
- 68 `battery_basics` 電池の素顔 (単3・18650・CR2032 の内部抵抗) [parts] → 部品

#### 受動部品・過渡現象
- 55 `capacitor` コンデンサの素顔 (I = C·dV/dt、τ = RC) [parts]
- 56 `inductor` コイルの素顔 (V = L·di/dt、τ = L/R) [parts]
- 6 `rlc` 行き過ぎとリンギング (RLC のステップ応答) [basics] → 信号
- 80 `electrolytic_life` 電解コンデンサの寿命 (リップル電流と温度) [parts] → 電源
- 26 `decoupling` 電源のバイパスコンデンサ (インピーダンスのグラフ) [micro] → Arduino

#### AC・フィルタ・周波数・安定性
- 3 `rc_lowpass` RC ローパスフィルタ (AC 解析) [basics] → 信号
- 48 `poles` 極と零 (RLC の共振を式で) [signals] → 信号
- 49 `phase_margin` 位相余裕 (容量負荷でオペアンプが不安定) [signals] → 信号
- 50 `noise` ノイズ解析 (トランジスタ増幅の雑音の内訳) [signals] → 信号
- 86 `hierarchy` 同じ回路を何回も使う (階層。CAD の使い方) [hierarchy] → 信号
- 24 `crystal` 水晶とクロック (細い共振の山) [micro] → Arduino / 77 `crystal_basics` 水晶の素顔 (Q) [parts] / 45 `crystal_load` 水晶の負荷容量 (22pF) [micro] → Mega
- 78 `speaker_basics` スピーカーとブザーの素顔 (インピーダンスは周波数で変わる) [parts]
- 70 `transformer_basics` トランスの素顔 (巻数比) [parts]

#### 半導体 (ダイオード・トランジスタ・MOSFET)
- 15 `diode_curve` ダイオードの特性 / 57 `diode_basics` ダイオードの素顔 (rd = 26mV/I) / 81 `diode_recovery` 逆回復
- 2 `led` LED と電流制限抵抗 [basics] / 58 `led_basics` LED の素顔 / 59 `segment_basics` 7 セグメント LED (共通抵抗の問題)
- 16 `zener` ツェナーで電圧を一定に / 60 `zener_basics` ツェナーの素顔
- 4 `bjt_switch` トランジスタのスイッチ (NOT) / 61 `transistor` トランジスタの素顔 (hFE) / 17 `emitter_follower` エミッタフォロワ / 13 `pnp_highside` PNP のハイサイドスイッチ
- 12 `mosfet` MOSFET のスイッチ (2N7000) / 62 `mosfet_basics` MOSFET の素顔 (Vth・Rds(on)) / 82 `switching_loss` スイッチング損失
- 38 `flyback` コイルの逆起電力 (リレー・モータのフライバックダイオード) [micro] → Arduino・スイッチ
- 76 `relay_basics` リレーの素顔 [parts]
- 66 `photocoupler_basics` フォトカプラの素顔 (CTR) [parts]
- 75 `analog_switch_basics` アナログスイッチの素顔 (Ron・サンプルホールド) [parts]
- 74 `switch_basics` スイッチの素顔 (接点抵抗) [parts]

#### オペアンプ
- 8 `opamp_noninv` 非反転増幅 [devices] / 14 `opamp_inv` 反転増幅 (仮想接地) [analog] / 63 `opamp_basics` オペアンプの素顔 (GBW・振れ幅) / 84 `slew_rate` スルーレート

#### 電源
- 9 `rectifier` 交流を直流にする (トランス・ブリッジ・リップル) [devices] / 71 `bridge_basics` ダイオードブリッジの素顔
- 18 `series_regulator` 直列レギュレータ (TL431・オペアンプ・パストランジスタ) [supply]
- 19 `regulator_78xx` 三端子レギュレータ (7805) / 69 `regulator_basics` 三端子レギュレータの素顔 (リップル除去比) / 36 `ldo` LDO (AMS1117 などとの比較) / 37 `tl431_ref` TL431 / 23 `variable_supply` LM317 の可変電源
- 20 `heat` 熱と定格 (放熱板の要否) [supply]
- 21 `protection` 電源を守る (逆接続・ショート・ヒューズ、P チャネル MOSFET) / 72 `fuse_basics` ヒューズの素顔 / 73 `varistor_basics` バリスタの素顔
- 22 `coil_layer_short` コイルのレアショート (故障が連鎖する) [supply]
- 11 `buck` 降圧コンバータ (デューティ・コイル電流・効率) [devices] / 83 `ldo_vs_buck` リニアとスイッチングの比較
- 7 `tp4056` リチウムイオン電池の充電 (TP4056、充電の 3 段階) [devices] → 電池
- 32 `usb_power` USB の電源 VBUS (ケーブルの電圧降下・突入電流) [micro]

#### マイコンの周辺回路・通信 (すべて回路の側。コードは書かない)
- 25 `reset` リセット回路 (DTR からの自動リセット) / 27 `pullup` プルアップ / 28 `pin_led` ピンで LED (ピンの電流の上限) / 29 `levels` 3.3V と 5V (分圧・BSS138) / 30 `serial` シリアル通信 (UART の波形、文字 'A') / 31 `datasheet` データシートの読み方 (絶対最大定格と 2 倍の余裕、1N4148) / 35 `pwm` PWM とフィルタ (analogWrite) / 39 `debounce` ボタンのチャタリング (RC) / 43 `adc_input` ADC の入力 (14pF のサンプルコンデンサ) / 44 `mega_power` 電源ピンが 5 組 / 85 `eeprom_basics` EEPROM (I²C) のオープンドレインとプルアップ
- USB: 33 `usb_speed` (D+ の 1.5kΩ) / 34 `usb_signal` (ESD 保護の容量) / 46 `ch340g` (差動受信)
- 47 `equiv_model` モデルのない IC を等価回路で再現する (ULN2803A) [models]

#### センサ
- 40 `temp_sensors` 温度センサ (LM35・TMP36・NTC) / 41 `light_sensors` 光センサ (LDR・フォトダイオード) / 42 `current_sensors` 電流センサ (シャント + INA180・ACS712) [micro]
- 64 `thermistor_basics` / 65 `phototransistor_basics` / 67 `hall_basics` (SS49E) [parts]

#### デジタル (ゲート・記憶・数える)
- 79 `logic_basics` ロジック IC の素顔 (しきい値・シュミットトリガ) [parts]
- 87 `half_adder` 半加算器 / 88 `sr_latch` SR ラッチ / 89 `counter_live` カウンタ (74HC161) / 90 `ring_oscillator` MOSFET でロジック (リング発振器・伝搬遅延) [digital]
- 5 `ne555` 555 で LED を点滅 [basics] → 信号

#### デバッグ・修理
- 51 `debug_circuit` 動かない回路を直す (ツェナー電源に故障 1 つ、始めるたびに変わる) [debug]
- 修理 1〜4: `repair_divider`「テスターの当て方」・`repair_zener`・`repair_led`・`repair_regulator` (手順の段は 0、仮想テスターで測って原因を当てる) [repair]

### Inferences
- 分野の厚み: 電源 (約 20 本) とマイコン周辺の回路 (約 15 本) と「部品の素顔」(32 本、部品 1 種類ずつ) が厚い。デジタルはゲート・ラッチ・カウンタの 5 本程度で、組み合わせ回路の設計 (デコーダ・マルチプレクサ・シフトレジスタ) のレッスンは無い (シフトレジスタ・デコーダはサンプルだけ。`docs/examples/shift_register*.schjson`・`counter_decoder.schjson`)。
- 「実機で確かめよう」の文の中にテスター (100 か所余り)・オシロ (33 か所)・ブレッドボード (38 か所) が出てくるので、実験の手順は各レッスンの付録として散らばって書かれている (grep の件数。重なりあり)。

### Gaps
- 各レッスンの手順 (Step) の中身と「実物との違い」の文は読み込んでいない (件数と goal の文だけ)。
- 画面の番号 (1〜90) と `LESSONS` の並び順は一致しない所がある (例: 51 debug は devices より後ろに並ぶ)。番号の付け方の意図は確かめていない。

## 3. チャレンジ (作ってみよう)・基板レッスン・実機の作業ガイド・修理の練習・デジタルを動かしながら見る・用語集・データシートの読み方

### Takeaway
チャレンジは 10 件 (8 件がレッスンの続き、2 件が最終課題)、基板レッスンは 8 本、実機の手作業のチェックリストは 4 つ (SMD はんだ付け・ブートローダ・100 ピンのはんだ付け・データシートの PDF)、組み立てて測るガイド 1 つ、最初の電源投入のガイド 1 つ、実機のタブの 5 段の手順。用語集は 197 語。

### Cited Findings
**チャレンジ (10 件)** — [challenge/mod.rs 108–119 行](E:/circuitCad/crates/app/src/schematic/challenge/mod.rs)、[basic.rs](E:/circuitCad/crates/app/src/schematic/challenge/basic.rs)、[power.rs](E:/circuitCad/crates/app/src/schematic/challenge/power.rs)、[arduino.rs](E:/circuitCad/crates/app/src/schematic/challenge/arduino.rs)
- `zener` ツェナーで 5V を作る / `divider` 分圧で 3.3V を作る / `led` LED を約 10mA で光らせる / `rc_lowpass` カットオフ 1kHz のローパス / `emitter_follower` 重い負荷でも保つ 4.2V / `opamp_inv` ゲイン −10 の反転増幅
- `series_regulator` TL431・オペアンプ・トランジスタで自作の 5V 電源 / `regulator_78xx` 7805 で 5V 電源 / `draw_psu` 安定化電源を自分で設計する (最終課題)
- `draw_arduino` Arduino 互換ボードの回路図を自分で描く (最終課題)
- 判定は `judge.rs` (回路をシミュレーションして条件を見る)。答えが通るテストあり (`challenge/tests.rs`)

**基板レッスン (8 本)** — [board_editor/lesson.rs 6–53 行](E:/circuitCad/crates/app/src/board_editor/lesson.rs)
1. 基板編「回路図から基板をつくる」(regulator_led.schjson) 2. 「配線の電圧降下」(thin_power_line.ccproj、配線の抵抗→太くする) 3. 「発注前チェックをすべて通す」(order_fix.ccproj) 4. 「置き方の勘所 (Arduino 互換)」(水晶は XTAL のそば・バイパスは電源ピンのそば・グリッド) 5. 「USB の配線 (差動ペア)」 6. 「100 ピンの基板を手で仕上げる (Mega)」(未配線 18 本から) 7. 「差動ペアどうしの長さを合わせる」 8. 「壊れた基板を直す (修理屋)」(attiny_blinker の基板に故障を仕込む)

**実機の作業ガイド (チェックリスト、`GUIDES = [SOLDER, BOOTLOAD, SOLDER_TQFP100, DATASHEET]`)** — [bench_guide.rs](E:/circuitCad/crates/app/src/bench_guide.rs)
- 「表面実装のはんだ付け (チップ部品と TQFP)」: 1. 準備 2. チップ部品 (0805) 3. TQFP-32 (0.8mm) 4. 電源を入れる前の点検
- 「ブートローダを書き込んで、LED を点滅させる」: 1. 用意とつなぎ方 2. ブートローダ 3. Blink をシリアルで書き込む 4. 確かめる
- 「100 ピン (0.5mm ピッチ) のはんだ付け (ATmega2560)」: 準備・小さい部品を先に・TQFP-100・ブリッジを直す・最初の起動
- 「データシートの PDF を読む」: ds_reading.rs の 8 段をチェックリストに。トラブル表 (見出しが無い・画像の PDF は OCR・min/typ/max の空欄の意味)
- 安全の注意・「動かないとき」の表つき。全部の印で済みになる (プログラムでは判定できない作業)

**データシートの読み方の案内 (8 段)** — [ds_reading.rs](E:/circuitCad/crates/app/src/ds_reading.rs)
- 1. 何の部品か (表紙と特長) 2. 絶対最大定格 3. 推奨動作条件 4. 電気的特性 (min・typ・max と条件) 5. 特性のグラフ 6. ピン配置 7. 応用回路と推奨の部品 8. 外形寸法とランドパターン。データシートの窓で開いた PDF の上で各段の見出しを検索する。同梱部品の PDF は配れないので、開いた PDF に沿う
- これとは別に、レッスン 31 `datasheet`「データシートの読み方 (絶対最大定格と 2 倍の余裕)」がある (シミュレーションで定格と比べる) — [lessons/micro.rs](E:/circuitCad/crates/app/src/schematic/lessons/micro.rs)

**組み立てて、性能を測る (`build_psu`)** — [measure_guide.rs 69–90 行](E:/circuitCad/crates/app/src/measure_guide.rs)
- 1. 部品表を作る 2. 組み立てる 3. 測る項目を表に入れる 4. 実機で測って書き込む 5. 差の理由を考える。実測は仕様の表に入り、シミュレーションと比べる。安全: 12V 以下の直流のみ、100V の配線はしない

**最初の電源投入のガイド** — [power_on_guide.rs](E:/circuitCad/crates/app/src/power_on_guide.rs)
- 開いた回路図から、電源ごとの正常な電流の範囲 (公差・データシートのばらつき・自己発熱)、電流制限の値、手順、読んだ電流の判定と疑う原因を出す (サンプル「最初の電源投入」あり)

**実機のタブ (B12)** — [bench_tab.rs 1–6 行](E:/circuitCad/crates/app/src/bench_tab.rs)
- 組み立て → 電源投入 → 測る → 合否 → 修理 の 5 段。配置図で付ける部品・測る点が光る。上の各ガイドと修理のレッスンをまとめて開く場所で、独自のレッスンは無い

**修理の練習** — [schematic/repair_practice.rs](E:/circuitCad/crates/app/src/schematic/repair_practice.rs)
- 回路図は正しい設計、「作った基板」に見えない故障が 1 つ。線やピンをクリックするとテスターで測った値 (±(0.3% + 2mV) のばらつき) が出る。部品と壊れ方を答え、故障診断 (`sim_core::troubleshoot`) が選ぶ測る順と自分の順を並べて見せる。修理のパス 4 本 + 基板の修理 1 本 + デバッグ 1 本

**デジタル回路を動かしながら見る (B8)** — [docs/features.md 89 行](E:/circuitCad/docs/features.md)、[samples.rs](E:/circuitCad/crates/app/src/samples.rs)
- スイッチをクリック・High/Low の色・真理値表・ロジックアナライザ・1 クロック送り・ゆっくり再生・グリッチや競合の印。教材は レッスン 87〜89 (半加算器・SR ラッチ・カウンタ) と、`live` 印のサンプル (7 セグメント・カウンタ・シフトレジスタなど。ブラウザ版にもある)

**用語集 (197 語)** — [glossary.rs](E:/circuitCad/crates/app/src/glossary.rs)
- 種類: 回路 158・基板 25・操作 14 (`t(...)` をスクリプトで数えた)。区分: 回路の基本 / 受動部品 / ダイオード / トランジスタと増幅 / 電源 / スイッチ・リレー・ロジック / 共振とセンサ / 古いレッスンの語 (部品・電源・マイコン・測定) / 基板 / このソフト
- 入っている語の例: オームの法則・実効値・時定数・インピーダンス・GBW・PWM・デューティ・プルアップ・オープンドレイン・真理値表・フリップフロップ・ボーレート・USB・ESD・熱抵抗・絶対最大定格・データシート・Arduino・ATmega328P・ブレッドボード・テスタ・オシロスコープ・プローブ・ガーバー・差動ペア・DRC
- 無い語の例 (一覧に見当たらない): I²C・SPI・はんだ (付け)・ストリップ基板・ユニバーサル基板・すずめっき線・スケッチ・ブロック・サーボ・キルヒホッフ・テブナン・デシベル以外の対数 (dB はある)

**豆知識 22 件** — [tips.rs](E:/circuitCad/crates/app/src/tips.rs): 操作のこつが中心 (ホバー・スイープ・ERC・基板の DRC・3D など)。試作のタブ・Arduino に触れたものは無い。

### Inferences
- 実機の作業ガイドは「表面実装の基板を発注して組む」前提に偏っていて、リード部品のはんだ付け (スルーホール)・ブレッドボードへの差し方・ストリップ基板の帯の切り方の手順書は無い。
- 修理の教材は 3 層 (回路図の仮想テスター → 基板の故障 → 実機のタブの修理の段) あり、他の初学者向け教材と比べて珍しく厚いと思われる (外部との比較は他の調査に任せる)。

### Gaps
- 用語集の各語の説明文と、レッスン文中の語に自動でリンクが付くかは確認していない (コメントに「一文字の別名はレッスンの中で探さない」とあるので、レッスン文の語を探す仕組みはあるらしい)。
- 操作ガイド help.rs のページ数・中身は数えていない。

## 4. よくある初学者の話題で、無い・薄いもの

### Takeaway
データシート・プルアップ・PWM・センサ・テスターの当て方は教材がある。一方で、キルヒホッフ/テブナン・抵抗のカラーコード・リード部品のはんだ付け・モーターの駆動 (H ブリッジ)・サーボ・ステッピング・I²C/SPI の通信の手順・割り込み・Arduino のプログラミングそのもの・ブレッドボード/ストリップ基板/ユニバーサル基板で組む手順は、レッスンが無いか、他のレッスンの付け足し程度。

### Cited Findings
- レッスンのファイルを語で数えた件数 (lessons/*.rs の grep): キルヒホッフ 0・テブナン 0・H ブリッジ 0・シフトレジスタ 0・ステッピング 0・サーボ 0・割り込み 0・スルーホール 0・デシベル 0、カラーコード 4・はんだ 4・SPI 4・I²C 7・コンパレータ 1、オシロ 33・発振 26・位相 16・実効値 10 — [crates/app/src/schematic/lessons/](E:/circuitCad/crates/app/src/schematic/lessons/)
- カラーコードを読むレッスンかチャレンジは候補 P2b に「残り」として載っている (まだ無い) — [docs/candidates.md 217 行](E:/circuitCad/docs/candidates.md)
- 測定器を自作して精度を上げる学習パス (L3、トレーサビリティ・不確かさ) は案のみで「まだ作っていない」 — [docs/learning-path.md 138–140 行](E:/circuitCad/docs/learning-path.md)、[docs/candidates.md 54 行](E:/circuitCad/docs/candidates.md)
- D6「トランジスタで組む論理回路の教材」はデジタルの残りの候補 — [docs/candidates.md 24 行](E:/circuitCad/docs/candidates.md)
- モーターはサンプル「モーターを回す (MOSFET + 負荷のトルク)」とレッスン 38 `flyback` だけ — [samples.rs](E:/circuitCad/crates/app/src/samples.rs)、[lessons/micro.rs](E:/circuitCad/crates/app/src/schematic/lessons/micro.rs)

話題ごとの今の状態:
| 話題 | 今あるもの | 判定 |
|---|---|---|
| オームの法則・直列並列 | intro_ohm・resistor・divider | ある |
| キルヒホッフの法則・テブナン | レッスン 0 件 (用語集にも無し) | 無い |
| 抵抗のカラーコード | 候補 P2b のみ | 無い |
| 回路図の読み方 | intro_read | ある |
| テスター (マルチメータ) の使い方 | repair_divider「テスターの当て方」・各レッスンの「実機で確かめよう」・修理の練習 | ある (電圧中心。電流・抵抗レンジの使い分けの専用の回は見当たらない) |
| オシロスコープの使い方 | 各レッスンの「実機で確かめよう」に 33 か所。専用のレッスンは無い | 薄い |
| はんだ付け | SMD (0805・TQFP-32)・TQFP-100 のチェックリスト | スルーホール・ストリップ基板・ユニバーサル基板のはんだ付けは無い |
| データシートを読む | レッスン 31・8 段の案内・チェックリスト | ある |
| プルアップ | pullup・eeprom_basics・levels | ある |
| PWM | pwm (フィルタで電圧)・mosfet で加減 | ある (サーボ・モーターの速度制御には触れない) |
| センサ | 温度・光・電流・ホール・サーミスタ・フォトトランジスタ | ある (I²C のデジタルセンサ SHT31 の回は無い) |
| モーター・リレー | flyback・relay_basics、サンプルのモーター | 薄い (H ブリッジ・ステッピング・サーボ無し) |
| 通信 (UART・I²C・SPI) | serial (UART の波形)・eeprom_basics (I²C の電気)・usb_* | 電気の側だけ。プロトコルの読み書きの回は無い |
| デジタル論理 | logic_basics・半加算器・SR ラッチ・カウンタ・リング発振器 | ある (組み合わせ回路の設計・ブール代数は薄い) |
| Arduino のプログラミング | レッスン無し (サンプル 7 件のみ) | 無い |
| ブレッドボード・ストリップ基板・ユニバーサル基板で組む | レッスン無し (自動の並べ方・組む順の窓・照合はある) | 無い |
| 安全 (感電・静電気) | 「感電」4・「静電気」4 件の言及、ガイドの安全欄 | 薄い (専用の回は無い) |
| 交流の基礎 (実効値・位相) | rectifier・transformer_basics・rc_lowpass に言及 | 薄い (正弦波・実効値の専用の回は無い) |

### Inferences
- 「部品から入る」話題はとても厚いが、「回路の法則 (KCL/KVL・テブナン)」「測定器の使い方」「組み立ての作業」の 3 つが、独立した回として抜けている。一般の入門書の最初の数章 (法則・テスター・ブレッドボード・はんだ付け) に当たる部分。
- マイコンについては「周辺回路の電気」は厚く、「プログラム」は 0。

### Gaps
- 語の件数は grep の数え (同じ行の重なり・コメントも数える) で、内容の深さは読んでいない。
- 「キルヒホッフ」などは別の言い方 (「電流の和」など) で説明されている可能性はある。確かめていない。

## 5. 新しい機能 (試作のタブ・Arduino・実機のタブ) で、教材の有り無し

### Takeaway
試作のタブ (ブレッドボード・ストリップ基板・ユニバーサル基板・Arduino のスケッチとブロック・ATtiny85・Mega・サーボ・SHT31・本物の命令) には、レッスン・チャレンジ・学習パスの項目が 1 つも無い。あるのはホバーの説明・サンプル回路・操作ガイド・docs/features.md の 1 行と、回路ごとに作られる「組む順と通電」の窓だけ。実機のタブは独自のレッスンは無いが、既存のガイド (電源投入・実測・チェックリスト・修理) をまとめて開く入口になっている。

### Cited Findings
- 学習パス・レッスン・チャレンジ・基板レッスン・実機のガイドのファイルに、「試作のタブ」「ストリップ」「ユニバーサル基板」の語は無い (`スケッチ` はレッスン 25 reset の goal「新しいスケッチを書き込めます」と bootload の説明のみ) — [lessons/micro.rs 50 行](E:/circuitCad/crates/app/src/schematic/lessons/micro.rs)、[paths.rs 216 行](E:/circuitCad/crates/app/src/learn/paths.rs)
- 試作のタブのフォルダ (`breadboard/`: blocks_ui・build_steps・canvas・code_edit・live・sketch_ui・strip_view など) に「レッスン」「学習パス」の語は無い。実機のタブ (`bench_tab.rs`) は学習パスの「組み立てて、性能を測る」を開くボタンを持つ — [crates/app/src/breadboard/](E:/circuitCad/crates/app/src/breadboard/)、[bench_tab.rs 418 行](E:/circuitCad/crates/app/src/bench_tab.rs)
- Arduino 系のサンプル (`SAMPLES`): 「Arduino で LED を点滅させる (スケッチ)」「Arduino をブロックで書く (押しボタンで LED)」「Arduino でボリュームを読んで LED の明るさを変える」「Arduino でサーボを回す (Servo ライブラリ)」「ATtiny85 で LED を点滅」「Arduino で温度と湿度を測る (SHT31、I²C)」「Arduino Mega で流れる光」 — [samples.rs](E:/circuitCad/crates/app/src/samples.rs)、ファイルは `docs/examples/arduino_blink・arduino_blocks・arduino_pot・arduino_servo・attiny85_blink・arduino_sht31・arduino_mega.schjson`
- 機能一覧での記載: Arduino のスケッチ (setup/loop・pinMode・digitalWrite/Read・analogRead/Write・delay・millis・Serial)、ブロックで書く (Scratch のように)、スケッチのエディタ、ATtiny85、サーボ、I²C のセンサー (SHT31)、Arduino Mega、本物のマイコンの命令 (arduino-cli でコンパイルし ATmega328P の命令を回路と一緒に) — [docs/features.md 109–116 行](E:/circuitCad/docs/features.md)
- ストリップ基板・ユニバーサル基板の機能 (自動の並べ方・区画の案・裏から見る・すずめっき線・電源を入れて動かす・テスターとオシロ・組む順と通電) — [docs/features.md 97–147 行](E:/circuitCad/docs/features.md)
- 「組む順と通電 (区画ごと)」の窓: 区画を 1 つずつ組んで確かめる順を出す (電源の区画から、区画の中は背の低い部品から)。回路ごとに作る手順で、教える文ではない — [docs/features.md 138 行](E:/circuitCad/docs/features.md)、[breadboard/build_steps.rs](E:/circuitCad/crates/app/src/breadboard/build_steps.rs)
- Arduino 互換パスのゴール文はいまも「マイコンの中身はシミュレーションできないので」と書いている — [paths.rs 156 行](E:/circuitCad/crates/app/src/learn/paths.rs)
- 用語集・豆知識にも、ストリップ基板・ユニバーサル基板・スケッチ・ブロック・サーボ・I²C の項は無い — [glossary.rs](E:/circuitCad/crates/app/src/glossary.rs)、[tips.rs](E:/circuitCad/crates/app/src/tips.rs)

機能ごとの教材:
| 機能 | レッスン/パス | チャレンジ | ガイド | サンプル | 用語集 |
|---|---|---|---|---|---|
| ブレッドボード | 無し (レッスンの「実機で確かめよう」で紙の上の説明として出るだけ) | 無し | 無し | 回路の多くが試作のタブで開ける | 「ブレッドボード」はある |
| ストリップ基板 | 無し | 無し | 組む順と通電の窓 (自動) | 専用のサンプルは見当たらない | 無し |
| ユニバーサル基板 | 無し | 無し | 組む順と通電の窓 (自動) | 専用のサンプルは見当たらない | 無し |
| Arduino のスケッチ (コード) | 無し | 無し | 無し | blink・pot・servo・sht31 | 「Arduino」のみ |
| Arduino のブロック | 無し | 無し | 無し | arduino_blocks | 無し |
| ATtiny85 | 無し (基板の修理のレッスンが attiny_blinker の基板を使うだけ) | 無し | 無し | attiny85_blink | 無し |
| Arduino Mega (試作) | Mega パスは基板を作る話で、試作のタブの Mega とは別 | 無し | 無し | arduino_mega | 無し |
| サーボ | 無し | 無し | 無し | arduino_servo | 無し |
| SHT31 (I²C) | 無し (I²C の電気は eeprom_basics) | 無し | 無し | arduino_sht31 | 無し |
| 本物の命令 (arduino-cli) | 無し | 無し | 無し | (どのスケッチでも) | 無し |
| 実機のタブ | 独自は無し (既存のガイド・修理のレッスンを開く) | — | 電源投入・測る・チェックリスト | 最初の電源投入 | — |

### Inferences
- 学習パスに「試作」の段 (ブレッドボードで組む → 電源を入れて動かす → テスターで測る) を足せば、既存のレッスン (divider・led・pullup・pwm・debounce・temp_sensors など) の「実機で確かめよう」と、試作のタブの電源・テスター・オシロがつながる。今は 2 つが別々。
- Arduino 互換パスのゴール文が古い (スケッチが回路と一緒に動く今の機能と食い違う)。プログラミングの入門 (Blink → digitalRead → analogRead → analogWrite → Servo → Wire) は、サンプルがすでにその順にそろっていて、レッスンにするための回路は手元にある。
- 「本物の命令」(ATmega328P を 1 クロックずつ) とブロックは、他の初学者向け教材と比べて特徴的な機能なのに、教材から一度も案内されていない。

### Gaps
- 試作のタブの画面の中のホバー説明や、サンプルの about の文がどこまで手順を説明しているかは、about の冒頭しか読んでいない。
- ブラウザ版 (crates/web) にレッスンがあるかは見ていない (features.md ではサンプルと DC の色・デジタルの動かす部分のみと読める)。
