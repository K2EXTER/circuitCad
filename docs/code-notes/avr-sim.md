# avr-sim (本物の AVR の命令、W5 (b)、2026-11-26)

`crates/avr-sim`: simavr (GPL-3.0-or-later) を入れて、arduino-cli・avr-gcc がビルドしたプログラムを ATmega328P の本物の命令で動かす。試作のタブでの使い方は `docs/code-notes/app/proto/sketch.md` の「本物のマイコンの命令で動かす」。

## クレートの中身

- **`simavr/`**: simavr の `sim/` (gdb・ELF・DWARF・コマンドの本体・USB を除く) と、ATmega328 のコア (`cores/sim_mega328.c`・`sim_megax8.*`・`sim_core_declare.h`・`cores/avr/iom328p.h`) と ATmega2560 のコア (`sim_mega2560.*`・`cores/avr/iom2560.h`・`iomxx0_1.h`、W5 ③)。
  - `COPYING` と、写した版の `VERSION` も入れてある。
  - 写すのは `python tools/vendor_simavr.py <simavr を clone したフォルダ>`。MSVC 向けの書き換え (下の表) もこれが当てる。手で直さない。
  - simavr の不具合の直しも、このスクリプトの `FIXES` が当てる (上流で直ったら消す。合わなくなったら止まる)。
    - ADC の変換時間 (2026-11-26): `current_prescale` が `uint8_t` だった。Arduino の分周 128 × 2 = 256 が 0 にあふれ、変換の終わりが標本と同時 (16 µs) になっていた。しかも終わりは標本の遅れの 11 倍あと、という数え方だった。
    - 直しでは、`current_prescale` に ADC の 1 クロック (CPU のクロック数) を持たせる。標本は 2 クロック (1 回目は 14) あと、終わりは 11 クロックあと。合わせて 13 クロック・1 回目 25 クロックで、データシートどおり (104 µs・200 µs)。上流に報告した: https://github.com/buserror/simavr/issues/609 (2026-11-26)。
- **`shim/`**: MSVC のときだけ使う代わりのヘッダ。
  - `msvc_first.h`: `/FI` で全部の前に読ませる。`__attribute__`・`__sync_synchronize`・`strsep` の宣言。
  - `gcc_first.h`: GCC・Clang (Linux・macOS) で `-include` で全部の前に読ませる。写しの可変長の配列は MSVC の `_alloca` に書き換えてあるので、`alloca` に読み替えるだけ (2026-11-26 に CI の Linux・macOS がこれで落ちていた)。
  - `unistd.h`・`sys/time.h`・`strings.h`。
- **`c/`**:
  - Makefile が作るはずの `sim_core_config.h`・`sim_core_decl.h` (コアは mega328 と mega2560)。
  - gdb の代わり `gdb_stub.c`。
  - MSVC だけで使う `posix_shim.c` (`strsep`・`clock_gettime`)。
  - Rust への窓口 `glue.c` (`ccavr_*`)。
- **`glue.c` の中身**:
  - 作るとき (`ccavr_new`):
    - ポート A〜L (I を除く 11 個) の 88 本のピンの変化を、記録に入れる通知を付ける (`on_pin`。チップに無いポートは IRQ が無いので飛ばす)。`ccavr_ports` も 11 個のポートの PORT・DDR・PIN を返す。
    - UART0 の出力の通知を付ける (`on_byte`)。
    - UART の「コンソールに出す」と「待つ間 usleep」を切る。
    - `avr->sleep` を何もしない関数にする (SLEEP 命令で実時間を待たない)。
  - ピンの変化の記録: simavr はタイマーの比較出力を、値に `AVR_IOPORT_OUTPUT` (0x100) を足して知らせる。これを `timer` の印にして、値は下の 8 ビットで読む。
  - 回路からの入力: `ccavr_set_pin` で入れる。自分で上げた入力は、記録に入れない (`driving`)。
- **`src/lib.rs`**:
  - `read_elf`: ELF32 の読み込み用のセグメントを、置く番地へ。0x810000 からは EEPROM、0x800000 からの RAM は捨てる。
  - `Avr`: `run_to`・`ports`・`set_pin`・`set_adc`・`send`・`take_events`・`take_serial`・`stopped`。
  - `Board::send_text`: シリアルモニタの文を待ち行列に入れ、1 ms ごとに 8 バイト (`BYTES_PER_STEP`) ずつチップに渡す (simavr の UART は 64 バイトしか持たない)。
  - `BoardDef` (ボード: チップの名前・クロック・電源と、Arduino のピン番号 → ポートとビットの表・A0 の番号): `NANO` (D0〜D13・A0〜A7。A6・A7 は ADC だけ) と `MEGA` (D0〜D53・A0〜A15。表は Arduino の pins_arduino.h の mega のとおり)。
  - `Board::advance(t, volts)` (`Board::new(&NANO か &MEGA, image)`):
    1. 入力のピンに、回路の電圧 (電源の半分より上で High) を入れる。A0〜A7 は ADC にも入れる。
    2. `t` まで回す。
    3. 出力のピンは、終わりの値 `PinOut::Out` か、100 Hz 以上で続いて切り替わっていれば最後の 1 周期の `PinOut::Pwm { duty, hz }` を返す (`Edges`: ピンごとの最後の立ち上がり・立ち下がり)。
    4. タイマーが握るピンは、タイマーの変化だけで数える。simavr はポートへの書き込みも、そのピンに知らせてくるため。
    5. 入力は `Input`、ポートのビットが 1 の入力は `Pullup`。
- **試験** (`tests/nano.rs`): `tests/firmware/check.c` を avr-gcc でビルドした `check.elf` を入れてある。試験に avr-gcc は要らない。作り直すのは `build.sh`。
  - ELF とその EEPROM の読み。
  - D13 の反転、D2 → D11 の追従、A0 → D12、D9 のタイマーの PWM (25 %)、D4 のプルアップ。
  - シリアルの `ee=42` と `a0=409`、受けた文字を送り返す。60 文字の行 (`Board::send_text`) もそのまま返る。
  - `adc_time.elf` (`adc_time.c`): ADC の変換時間が 200 µs (1 回目)・104 µs (2 回目から)。
  - `mega.elf` (`mega.c`、ATmega2560): Mega のピン番号で、D13 (PB7) の反転・D2 (PE4) → D22 (PA0)・A8 (ADC8、MUX5) → D53 (PB0)・D11 (OC1A) の PWM・シリアル。

## はじめの試し (simavr を MSVC で通せるか)

候補 W5 (b)「本物のファームを動かす」で、最初に決めることになっていた「simavr を使うか、avr8js を手本に Rust で書くか」のための半日の試し。**結果: simavr の中心 (`simavr/sim/` と ATmega328 のコア) は、MSVC の `cl` で 11 行を書き換え、小さなヘッダ 4 つを足せば通り、arduino-cli でビルドしたスケッチがそのまま動く。simavr を使う道で進めてよい。**

### やったこと

- simavr: `github.com/buserror/simavr` の master (13d4ed3、2026-11-19) を浅く clone。GPL-3.0。リポジトリには入れていない (スクラッチで試しただけ)。
- `cc` クレート (1.x) で、MSVC (`C:\BuildTools2022` の cl 14.44) を使って静的ライブラリにした。Build Tools に clang-cl は入っていない。
- 入れたファイル: `sim/` の `sim_*.c` と `avr_*.c`、`cores/sim_mega328.c` と `cores/sim_megax8.c`。
- 外したファイル: `sim_gdb.c` (ソケット)、`sim_elf.c`・`sim_dwarf.c` (libelf)、`run_avr.c` (コマンドの本体)、`avr_usb.c`。

### MSVC で通すために変えたもの

| 何 | どう |
|---|---|
| `case A ... B:` (GCC の範囲) 8 か所 (`sim_hex.c`・`avr_acomp.c`・`avr_adc.c`・`avr_flash.c`) | `case` を並べる形に書き換え (スクリプトで機械的に) |
| `0b11` (2 進の数) 1 か所 (`avr_spi.c`) | 10 進に |
| `#define STATE(_f, args...)` (名前付きの可変引数) 1 か所 (`sim_core.c`、トレースを切ったときの空の定義) | `#define STATE(...)` |
| 可変長の配列 `char n[l + 10]` 2 か所 (`sim_io.c`・`sim_vcd_file.c`) | `char * n = _alloca(l + 10)` |
| `__attribute__((…))` 4 か所 | 全ファイルの前に読ませるヘッダ (`/FI`) で空に |
| `__sync_synchronize()` | 同じヘッダで `_ReadWriteBarrier()` に |
| `<unistd.h>` (`usleep`)・`<sys/time.h>` (`gettimeofday`)・`<strings.h>` (`ffs`) | 代わりのヘッダを置く (Windows の `Sleep`・`GetSystemTimeAsFileTime`・`_BitScanForward`) |
| `clock_gettime`・`strsep` | 小さな C ファイルで用意 (`QueryPerformanceCounter`) |
| `sim_core_config.h`・`sim_core_decl.h` (Makefile が作るもの) | 手で書く (`CONFIG_MEGA328 1`、`avr_kind[] = { &mega328, NULL }`) |
| gdb の関数 (`avr_gdb_init`・`avr_gdb_processor`・`avr_gdb_handle_watchpoints` …) | 何もしない関数を置く |

書き換えは合わせて 8 ファイル・11 行 (`git diff --ignore-cr-at-eol --stat`)。どれも動きを変えない書き換え。上流に MSVC 対応として出せる大きさ。

### 動かしたもの

C の小さな窓口 (`trial_new`・`trial_load`・`trial_run_until`・`trial_watch`・`trial_drive`・`trial_adc_mv`・`trial_uart`) を作り、Rust から呼んだ。内側では `avr_make_mcu_by_name`・`avr_loadcode`・`avr_run`・`avr_io_getirq` + `avr_irq_register_notify`・`avr_raise_irq` を使っている。

コードは ELF を `avr-objcopy -O binary -j .text -j .data` した .bin を `avr_loadcode` で読ませた。本番では ELF を Rust の側で読めば、libelf は要らない。

1. **avr-gcc の C** (`util/delay.h`、ポートを直接):
   - D13 が 0.1019 s ごとに反転した (10 回 / 1 s)。
   - D2 を 0.25 s で High にすると、D11 が 0.2505 s で追従した。
   - A0 を 0.6 s に 3 V にすると、D12 が 0.6007 s で High になった。
2. **arduino-cli でビルドしたスケッチ** (`arduino:avr:nano`。`tone(8, 1000)`・`EEPROM.read/write`・`Serial.print`・`analogRead(A0)`。どれも今の自前の読み方 `sim-core/sketch/` では動かない、または近似しかしないもの):
   - Serial の文が UART0 の出力から 1 文字ずつ取れた: `boot 255`、`A0=337` (1.65 V)、`A0=818` (4.0 V)。どちらも 5 V 基準の 1023 分の値どおり。
   - D8 は 1000.0 Hz。
   - 新しい EEPROM は 255 から始まる。

### 速さと大きさ

| | debug | release |
|---|---|---|
| 1 の C (16 MHz の 1 s 分) | 0.19 s (84 MHz 相当) | 0.11 s (150 MHz 相当) |
| 2 のスケッチ (UART・タイマーの割り込み込み) | 0.36 s | 0.18 s |

実時間の 5〜9 倍の速さで回る。回路の側と刻みをそろえても、実時間には十分間に合う。

exe の大きさは、試しのバイナリ全体で 283 KB (release)。simavr の分はその一部。

### 残っている気になる所

- 1 の D13 の周期は 1.019 ms × 100 だった。予想は 1.1 ms 前後 (ADC の 104 µs 込み)。原因は simavr の ADC の不具合で、上の `FIXES` で直した。
- コアは ATmega328 だけ試した。Mega (2560)・ATtiny も同じ作りなので通るはず。
- ライブラリ (Servo・Wire など) は試していない。この PC の arduino-cli に入っていないため (入れるにはダウンロードが要る)。`tone` と `EEPROM` のようにコアの中身を使うものは動いた。ライブラリも、ビルドは arduino-cli の仕事なので、simavr の側では同じ。
- CI: Linux・macOS は gcc / clang なので、書き換えなしで通るはず。Windows の CI は MSVC で、上の書き換えが要る。

## 残り (候補表 W5)

- ほかのチップ: ATtiny など。コアのファイルを `vendor_simavr.py` の `CORES` に足し、`sim_core_config.h`・`sim_core_decl.h` に並べ、`BoardDef` を書く (Mega はこの形で 2026-11-26 に足した)。
- ライブラリ (Servo・Wire) のスケッチ: arduino-cli でビルドできれば同じに動くはず。試していない。
