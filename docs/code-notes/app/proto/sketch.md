# マイコンのスケッチ (B6 ⑤、2026-11-19、`app/src/breadboard/sketch_ui.rs`・`sim-core/src/sketch/`)

- 部品 `ARDUINO_NANO` (`part-lib/examples/`、DIP 30 ピン・列の間 15.24 mm、Nano のピン番号どおり)。モデルは D0〜D13・A0〜A7 のそれぞれに、中の電源 `VpD13` と抵抗 `RpD13` (最初は 0 V・1 GΩ = 入力)。5V のピンは USB の 5 V を 0.2 Ω で出し、本体が 250 Ω (約 20 mA)。3V3・AREF・VIN・RST はモデルに無い。
- スケッチは回路図のシートの設定 `sketches` (部品番号 → 文、`tolerance.rs`。ファイルの `"sketches"`)。書いていない板は点滅の見本 (`sketch_ui::BLINK`) を見せる (見ただけでは保存しない)。
- 左のパネル (`breadboard/sketch_ui.rs`): 板ごとに「スケッチ (U1)」の欄 (コードの欄、パネルの幅まで)、打つたびに読んで「✔ 読めました」か「n 行目: 理由」、「点滅の見本に戻す」、動いている間はシリアルモニタ (最後の 12 行) と、動いている途中の誤り (行と理由)。
- 動かす (`live.rs`): 電源を入れると `Live::add_mcu` で板ごとに `sim_core::sketch::Machine` を作り、ピンのネット (`sketch_ui::pin_nets`: 記号のピンの名前 D13 → 番号 → ブレッドボードのネット) を覚える。すぐ `setup` を走らせる。`advance` はスケッチのある間、次に起きる時刻 (`delay` の終わり、待たないスケッチは 1 ms 後) まで解いてはスケッチを進める、をくり返す (1 フレームの予算 40 ms は同じ)。ピンの出力は `RunIo::drive`: 出力 0 / 5 V と 25 Ω、INPUT_PULLUP 5 V と 35 kΩ、入力 1 GΩ を `set_source`・`set_resistance` で。読みは今のネットの電圧。スケッチを変えると (`sketches_key`) 始めから。
- 「部品と差す穴」の一覧は、表をやめて 1 部品 1 行の折り返し (30 ピンの板の 1 行がパネルを画面の大半まで広げた)。コードの欄も幅いっぱいを求めない。
- サンプル `arduino_blink` (D13 → 330 Ω → LED) と `arduino_pot` (A0 のボリュームで D9 の LED の明るさ、シリアルに読んだ値)。生成 `write_arduino_examples`。テスト `the_arduino_sketch_blinks_the_led`・`the_arduino_reads_the_knob_and_sets_the_led` (`live.rs`)。

## Arduino Mega (W5 ③、2026-11-26)

- 部品 `ARDUINO_MEGA` (`part-lib/examples/`、86 ピン。モデルは Nano と同じ形で D0〜D53・A0〜A15 の `VpD22`・`RpD22`…、5V のピン 3 本、GND のピン 5 本)。基板のフットプリントは、ヘッダの並び (上の 10・8・8 ピン、右の 2 列 36 ピン、下の電源 8・アナログ 8・8 ピン) を実物に近く並べただけの概算で、`land_unverified` (発注前チェックが警告する)。
- ボードは `sim_core::sketch::Board` (`Nano`・`Mega`): モデルの名前 (`from_model`)・arduino-cli の FQBN (`arduino:avr:mega`)・ピンの数と名前 (`pin_name`: D53、A8)・A0 の番号 (14・54)・`analogWrite` のパルスのピン (Mega は 2〜13・44〜46) と周波数 (タイマー 0 のピンは 980 Hz)。今の読み方は `compile(src, board)` で `A0`〜 の数をボードで決める。
- 試作のタブ: `sketch_ui::board_of` で板ごとのボード。`Live::add_mcu`・`add_real_mcu` はボードを持ち、`RunIo` のピンの名前もボードの。本物のチップは `avr_sim::Board::new(&MEGA, …)`。
- Mega はブレッドボードに差さらない: `breadboard::is_board_beside` (モデルが `BOARDS_BESIDE`) で、ボードの外に置き、ほかの部品とつながるピンだけジャンパー線 (86 本全部は引かない)。5V・3V3 のピンのネットはレールの電源になる。外の箱はピンの数だけ高く (`canvas.rs` の `off_box`・`off_pin`)、ピンは名前で出す (D22・A8。電源の + − は今までどおり)。
- サンプル `arduino_mega` (D22・D24・D26・D28 の LED と A8 のボリューム、流れる光)。試験 `the_mega_runs_a_light_along_its_leds` (`live.rs`: 外の箱のピンが 7 本、4 つの LED が順に点く)。

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
- シリアルモニタの下に「送る文字」の欄と「送る」(Enter でも)。本物のチップが動いているときだけ出す (今の読み方には `Serial.read` が無い)。打った文に改行を付けて `Board::send_text` に渡す。打ちかけの文は板ごとに `BreadboardView::serial_input`。
- 動かす: `run_mcus` で、`avr_sim::Board::advance(今 − real_from, ピンの電圧)` を呼ぶ。返ってきた `PinOut` を、今の読み方と同じ `RunIo::drive` (`Drive`) で回路に渡す (変わったピンだけ)。
- `next_wake` は、本物のチップがあると「今」を返す。なので 1 ms ごとに、回路を解く → チップを進める、をくり返す。
- 出力は、止まった High / Low か、パルス (`PinOut::Pwm`) のどちらか。
  - チップの側 (`Board::advance`) は、ピンの立ち上がりから次の立ち上がりまでで、周期と High の長さを測る。
  - 100 Hz (`PWM_MIN_HZ`) 以上で続いて切り替わっているピン (最後の変化が 1.5 周期より前でない) は、最後の 1 周期のデューティと周波数を返す。それより遅い変化は、1 ms ごとの終わりの値で追う。
- PWM のパルス (W5 ①、2026-11-26): 1 ms の刻みでは追えないので、回路にはパルスの波形そのものを渡す。
  - `RunIo::drive` の `Drive::Pwm { duty, hz }` → `pulses`: `Waveform::Pulse` (0 V → 5 V、今の時刻から、辺は周期の 1/1000)。
  - ソルバはパルスの角を刻みの区切りにするので、LED は High の間だけ光る (25 % で全開の 1/4 の電流)。RC は平均に均される。
  - 今の読み方の `analogWrite` も同じ: 0 と 255 は止まった Low と High、ほかは PWM のピンで `Drive::Pwm` (490 Hz、D5・D6 は 980 Hz、`pwm_hz`)。前は平均の電圧で、25 % (1.25 V) では LED が点かなかった。
- 表示は 1 フレームの平均 (`read_now` の `average`: 前のフレームから解いた点の台形の平均)。
  - LED の明るさ・テスター・定格の点検は、目やテスターが見るのと同じ平均。オシロは解いた点そのものなので、パルスが見える。
  - 速さ: ボリュームのサンプルで 1 秒分が、今の読み方の 490 Hz で 0.1 秒、本物のチップの 1 kHz で 0.4〜0.5 秒 (release)。
- 試験 (`live.rs`):
  - `the_real_chip_blinks_the_led`: 点滅のサンプルに `avr-sim/tests/firmware/check.elf` を入れ、D13 の 10 ms ごとの反転で LED が点いたり消えたりする。
  - `the_real_chips_pwm_dims_the_led`: ボリュームのサンプルで、D9 の 25 % の PWM で LED が全開の 1/4 の電流、ピンのネットは平均 1.25 V。
  - `analog_write_lights_an_led_for_its_share_of_the_time`: 今の読み方の `analogWrite(9, 64)` と `255` で、LED の電流の比が 1/4。
