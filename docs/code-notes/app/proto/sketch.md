# マイコンのスケッチ (B6 ⑤、2026-11-19、`app/src/breadboard/sketch_ui.rs`・`sim-core/src/sketch/`)

- 部品 `ARDUINO_NANO` (`part-lib/examples/`、DIP 30 ピン・列の間 15.24 mm、Nano のピン番号どおり)。モデルは D0〜D13・A0〜A7 のそれぞれに、中の電源 `VpD13` と抵抗 `RpD13` (最初は 0 V・1 GΩ = 入力)。5V のピンは USB の 5 V を 0.2 Ω で出し、本体が 250 Ω (約 20 mA)。3V3・AREF・VIN・RST はモデルに無い。
- スケッチは回路図のシートの設定 `sketches` (部品番号 → 文、`tolerance.rs`。ファイルの `"sketches"`)。書いていない板は点滅の見本 (`sketch_ui::BLINK`) を見せる (見ただけでは保存しない)。
- 左のパネル (`breadboard/sketch_ui.rs`): 板ごとに「スケッチ (U1)」の欄 (コードの欄、パネルの幅まで)、打つたびに読んで「✔ 読めました」か「n 行目: 理由」、「点滅の見本に戻す」、動いている間はシリアルモニタ (最後の 12 行) と、動いている途中の誤り (行と理由)。
- 動かす (`live.rs`): 電源を入れると `Live::add_mcu` で板ごとに `sim_core::sketch::Machine` を作り、ピンのネット (`sketch_ui::pin_nets`: 記号のピンの名前 D13 → 番号 → ブレッドボードのネット) を覚える。すぐ `setup` を走らせる。`advance` はスケッチのある間、次に起きる時刻 (`delay` の終わり、待たないスケッチは 1 ms 後) まで解いてはスケッチを進める、をくり返す (1 フレームの予算 40 ms は同じ)。ピンの出力は `RunIo::drive`: 出力 0 / 5 V と 25 Ω、INPUT_PULLUP 5 V と 35 kΩ、入力 1 GΩ を `set_source`・`set_resistance` で。読みは今のネットの電圧。スケッチを変えると (`sketches_key`) 始めから。
- 「部品と差す穴」の一覧は、表をやめて 1 部品 1 行の折り返し (30 ピンの板の 1 行がパネルを画面の大半まで広げた)。コードの欄も幅いっぱいを求めない。
- サンプル `arduino_blink` (D13 → 330 Ω → LED) と `arduino_pot` (A0 のボリュームで D9 の LED の明るさ、シリアルに読んだ値)。生成 `write_arduino_examples`。テスト `the_arduino_sketch_blinks_the_led`・`the_arduino_reads_the_knob_and_sets_the_led` (`live.rs`)。

## ATtiny85 (W5 ③、2026-11-26)

- 部品 `ATTINY85-20PU` (`part-lib/examples/ATTINY85_DIP-8.part.json`、DIP-8)。モデルは Nano と同じくピンごとの電源と抵抗だが、電源は VCC に比例する (`BpPBn = V(cn) × V(vcc)/5`): 電源の無いチップは何も出さない。既存の `ATtiny85` (SOIC-8) はモデル無しのまま。
- ボードは `sim_core::sketch::Board::Tiny85`: ピン 0〜5 (`pin_name` は PB0〜PB5、部品のピン「PB5/RESET」は `/` の前の名前で引く)、A0〜A3 はピン 5・2・4・3 の別名 (`analog_pin`。`analogRead(0)` を A0 と読む決まりは使わない)、`LED_BUILTIN` は 1 (`led`)、`analogWrite` のパルスは 0・1・4。arduino-cli の FQBN は ATTinyCore の `ATTinyCore:avr:attinyx5:chip=85,clock=8internal` (ATTinyCore はボードマネージャに追加の URL で入れる。無いときの説明は `firmware::explain`)。
- USB が無いので、`Board::supply_pins` (VCC・GND) のネットを `Live::set_supply` で渡す (ボードを足す前に。`setup` も電源を待つ)。電圧が 1.8 V より低い間は止まり、ピンを放す (入力に)。今の読み方は `delay` の待ちを止まっていた時間だけ後ろへ (`Machine::delay_by`) ずらし、電源が戻るとピンの出力を戻す。本物のチップは時間が止まる (リセットからやり直さない。部品の説明に書いた)。
- シリアルモニタ: 本物のチップはソフトウェアのシリアルを avr-sim が読み戻す (`serial_baud` でスケッチの `Serial.begin` の速さ)。送る欄は UART のあるチップだけに出す。
- 見本のスケッチ (点滅) のコメントの「(D13)」は、ボードの LED のピン (PB1) に。
- サンプル `attiny85_blink` (5 V の電源・PB1 の LED・A1 のボリューム)。試験 `the_attiny85_blinks_on_its_own_supply_only` (今の読み方。電源のつまみを 0 V にすると消え、戻すとまた点滅) と `the_real_attiny85_sends_its_serial_in_software` (avr-sim の `tiny85.elf`、ボリュームの電圧がシリアルに)。画面で、今の読み方の点滅と、本物の命令 (アプリの中で ATTinyCore でコンパイル、1492 バイト) でシリアルモニタに行が出るのを確かめた。
- `--shot-frames N`: `--screenshot` の前に待つフレームの数 (シリアルモニタがたまるのを撮るとき)。

## I²C のセンサー (SHT31、W5 ⑤、2026-11-26)

- 部品 `SHT31_MODULE` (`part-lib/examples/`、1 列 5 ピン VIN・GND・SCL・SDA・ADR。回路のモデルはモジュールの上の 10 kΩ の引き上げ・ADR の引き下げ・待機の電流だけ)。測る値と I²C の応答は avr-sim の側 (`c/i2c_parts.c`、`docs/code-notes/avr-sim.md`)。
- 試作のタブ: `power::add_sensors` が回路図の SHT31 を見つけ、ピンのネットを `Live::add_sht31` に。`Live::give_program` で、本物のチップの SDA・SCL (`sim_core::sketch::Board::i2c_pins`: Nano は A4・A5、Mega は D20・D21) のネットがセンサーの SDA・SCL と同じなら `avr.add_sht3x(アドレス)` (ADR が VIN の半分より高ければ 0x45)。`I2cSensor::on` にどのボードのどのアドレスか。
- 動いている間、1 ms ごとに `set_sht3x(アドレス, 答えるか, 周りの温度, 湿度)`。答えるのは、VIN が 2.15〜6 V で、SDA・SCL が VIN の 70 % より高い (引き上げられている) とき。温度は「周りの温度」(`temp_c`。変えると計算をやり直す)、湿度は新しいつまみ「周りの湿度」(`BreadboardView.humidity`、50 %RH から。変えてもやり直さない)。
- 読まれないセンサーには理由を出す: スケッチが今の読み方 (Wire が無い) なら「本物のマイコンの命令で動かす」を、本物でつながっていなければ「SDA・SCL がつながっていません」。
- サンプル `arduino_sht31` (28 ℃ 以上で D13 の LED)。試験 `the_real_chip_reads_the_sht31_on_its_bus` (avr-sim の `sht3x.elf` で読む・湿度を変える・SDA を外すと応答なし)。サンプルのスケッチそのものは arduino-cli が要るので試験にせず、手で 27 ℃ で消え 30 ℃ で点く (8.6 mA) のを確かめた。
- arduino-cli には Arduino IDE 2 の設定ファイル (`~/.arduinoIDE/arduino-cli.yaml`) を `--config-file` で渡す (`firmware::ide_config`)。渡さないと、arduino-cli は OneDrive が動かした「ドキュメント」の下を探し、IDE で入れたライブラリが見つからなかった。

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
  - 100 Hz (`PWM_MIN_HZ`) 以上 (20 Hz 以上でも短いほうの幅が 5 ms より短ければ: サーボのパルス) で続いて切り替わっているピン (最後の変化が 1.5 周期より前でない) は、最後の 1 周期のデューティと周波数を返す。それより遅い変化は、1 ms ごとの終わりの値で追う。
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
