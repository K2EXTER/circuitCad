# マイコンのスケッチ (B6 ⑤、2026-11-19、`app/src/breadboard/sketch_ui.rs`・`sim-core/src/sketch/`)

- 部品 `ARDUINO_NANO` (`part-lib/examples/`、DIP 30 ピン・列の間 15.24 mm、Nano のピン番号どおり)。モデルは D0〜D13・A0〜A7 のそれぞれに、中の電源 `VpD13` と抵抗 `RpD13` (最初は 0 V・1 GΩ = 入力)。5V のピンは USB の 5 V を 0.2 Ω で出し、本体が 250 Ω (約 20 mA)。3V3・AREF・VIN・RST はモデルに無い。
- スケッチは回路図のシートの設定 `sketches` (部品番号 → 文、`tolerance.rs`。ファイルの `"sketches"`)。書いていない板は点滅の見本 (`sketch_ui::BLINK`) を見せる (見ただけでは保存しない)。
- 左のパネル (`breadboard/sketch_ui.rs`): 板ごとに「スケッチ (U1)」の欄 (コードの欄、パネルの幅まで)、打つたびに読んで「✔ 読めました」か「n 行目: 理由」、「点滅の見本に戻す」、動いている間はシリアルモニタ (最後の 12 行) と、動いている途中の誤り (行と理由)。
- 動かす (`live.rs`): 電源を入れると `Live::add_mcu` で板ごとに `sim_core::sketch::Machine` を作り、ピンのネット (`sketch_ui::pin_nets`: 記号のピンの名前 D13 → 番号 → ブレッドボードのネット) を覚える。すぐ `setup` を走らせる。`advance` はスケッチのある間、次に起きる時刻 (`delay` の終わり、待たないスケッチは 1 ms 後) まで解いてはスケッチを進める、をくり返す (1 フレームの予算 40 ms は同じ)。ピンの出力は `RunIo::drive`: 出力 0 / 5 V と 25 Ω、INPUT_PULLUP 5 V と 35 kΩ、入力 1 GΩ を `set_source`・`set_resistance` で。読みは今のネットの電圧。スケッチを変えると (`sketches_key`) 始めから。
- 「部品と差す穴」の一覧は、表をやめて 1 部品 1 行の折り返し (30 ピンの板の 1 行がパネルを画面の大半まで広げた)。コードの欄も幅いっぱいを求めない。
- サンプル `arduino_blink` (D13 → 330 Ω → LED) と `arduino_pot` (A0 のボリュームで D9 の LED の明るさ、シリアルに読んだ値)。生成 `write_arduino_examples`。テスト `the_arduino_sketch_blinks_the_led`・`the_arduino_reads_the_knob_and_sets_the_led` (`live.rs`)。

## 本物のマイコンの命令で動かす (W5 (b)、2026-11-26、`crates/avr-sim`・`sch-core/firmware.rs` の `compile_sketch`)

- 板ごとの選択「本物のマイコンの命令で動かす (arduino-cli)」。回路図のシートの設定 `sketch_real` (部品番号の集まり、`tolerance.rs`。ファイルの `"sketch_real"`。CLI の `EXTRA_KEYS` にも) に入れる。
- コンパイル: `firmware::compile_sketch(text, "arduino:avr:nano")`。スケッチを一時フォルダの `circuitCad-sketch/<文の hash>/sketch/sketch.ino` に書き、`arduino-cli compile --output-dir` の `sketch.ino.elf` を読む。同じ文は 2 度目からその ELF を使う。誤りは `error` を含む行 (一時フォルダのパスは「スケッチ」に置き換え) と `explain` の一言。
- arduino-cli は PATH に無くても、Arduino IDE 2 に同梱のもの (`Arduino IDE/resources/app/lib/backend/resources/arduino-cli.exe`。Program Files・LOCALAPPDATA・macOS の /Applications) を `find_tool` が見つける。W4 のビルドと書き込みも同じ。
- 別のスレッドで動かす (`BreadboardView::real_build`、文の hash ごとに `RealBuild` = 終わったら `Result<avr_sim::Image, String>`)。
  - 電源を入れたとき、選んだ板は `Live::add_real_mcu` で「待ち」(ピンは入力のまま)。
  - 毎フレーム `give_real_programs` が、終わったビルドを `Live::give_program` で渡す。
  - チップはその時刻から動き出す (`real_from`。チップの時刻は 0 から)。
  - 左のパネルは、コンパイル中 (くるくる)・✔ とプログラムの大きさ・誤り、の 3 つを出す。「コンパイル」のボタンで、電源の前にも確かめられる。
  - 画面の撮影は、ビルドが終わるまで待つ (`real_busy`)。
- 動かす: `run_mcus` で、`avr_sim::Nano::advance(今 − real_from, ピンの電圧)` を呼ぶ。返ってきた `PinOut` を、今の読み方と同じ `RunIo::drive` (`Drive`) で回路に渡す (変わったピンだけ)。
- `next_wake` は、本物のチップがあると「今」を返す。なので 1 ms ごとに、回路を解く → チップを進める、をくり返す。
- 出力は、その 1 ms の間に High だった割合 (平均の電圧)。PWM と `tone` も平均になる (今の読み方の `analogWrite` と同じ扱い)。
  - そのため 25 % の PWM は 1.25 V になり、LED は点かない (本物は 1/4 の明るさで光る)。
  - 直すには、PWM のピンを「全開の電流 × 割合」として扱うか、刻みを細かくする。候補表の W5 の残り。
- 試験 (`live.rs`):
  - `the_real_chip_blinks_the_led`: 点滅のサンプルに `avr-sim/tests/firmware/check.elf` を入れ、D13 の 10 ms ごとの反転で LED が点いたり消えたりする。
  - `the_real_chips_pwm_reaches_the_circuit_as_its_average`: ボリュームのサンプルで、D9 の 25 % の PWM がピンのネットに 1.25 V で出る。
