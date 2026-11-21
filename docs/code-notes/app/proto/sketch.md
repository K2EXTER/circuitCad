# マイコンのスケッチ (B6 ⑤、2026-11-19、`app/src/breadboard/sketch_ui.rs`・`sim-core/src/sketch/`)

- 部品 `ARDUINO_NANO` (`part-lib/examples/`、DIP 30 ピン・列の間 15.24 mm、Nano のピン番号どおり)。モデルは D0〜D13・A0〜A7 のそれぞれに、中の電源 `VpD13` と抵抗 `RpD13` (最初は 0 V・1 GΩ = 入力)。5V のピンは USB の 5 V を 0.2 Ω で出し、本体が 250 Ω (約 20 mA)。3V3・AREF・VIN・RST はモデルに無い。
- スケッチは回路図のシートの設定 `sketches` (部品番号 → 文、`tolerance.rs`。ファイルの `"sketches"`)。書いていない板は点滅の見本 (`sketch_ui::BLINK`) を見せる (見ただけでは保存しない)。
- 左のパネル (`breadboard/sketch_ui.rs`): 板ごとに「スケッチ (U1)」の欄 (コードの欄、パネルの幅まで)、打つたびに読んで「✔ 読めました」か「n 行目: 理由」、「点滅の見本に戻す」、動いている間はシリアルモニタ (最後の 12 行) と、動いている途中の誤り (行と理由)。
- 動かす (`live.rs`): 電源を入れると `Live::add_mcu` で板ごとに `sim_core::sketch::Machine` を作り、ピンのネット (`sketch_ui::pin_nets`: 記号のピンの名前 D13 → 番号 → ブレッドボードのネット) を覚える。すぐ `setup` を走らせる。`advance` はスケッチのある間、次に起きる時刻 (`delay` の終わり、待たないスケッチは 1 ms 後) まで解いてはスケッチを進める、をくり返す (1 フレームの予算 40 ms は同じ)。ピンの出力は `RunIo::drive`: 出力 0 / 5 V と 25 Ω、INPUT_PULLUP 5 V と 35 kΩ、入力 1 GΩ を `set_source`・`set_resistance` で。読みは今のネットの電圧。スケッチを変えると (`sketches_key`) 始めから。
- 「部品と差す穴」の一覧は、表をやめて 1 部品 1 行の折り返し (30 ピンの板の 1 行がパネルを画面の大半まで広げた)。コードの欄も幅いっぱいを求めない。
- サンプル `arduino_blink` (D13 → 330 Ω → LED) と `arduino_pot` (A0 のボリュームで D9 の LED の明るさ、シリアルに読んだ値)。生成 `write_arduino_examples`。テスト `the_arduino_sketch_blinks_the_led`・`the_arduino_reads_the_knob_and_sets_the_led` (`live.rs`)。
