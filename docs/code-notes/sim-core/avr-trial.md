# simavr を MSVC で通す試し (W5 (b)、2026-11-26)

候補 W5 (b)「本物のファームを動かす」で、最初に決めることになっていた「simavr を使うか、avr8js を手本に Rust で書くか」のための半日の試し。**結果: simavr の中心 (`simavr/sim/` と ATmega328 のコア) は、MSVC の `cl` で 11 行を書き換え、小さなヘッダ 4 つを足せば通り、arduino-cli でビルドしたスケッチがそのまま動く。simavr を使う道で進めてよい。**

## やったこと

- simavr: `github.com/buserror/simavr` の master (13d4ed3、2026-11-19) を浅く clone。GPL-3.0。リポジトリには入れていない (スクラッチで試しただけ)。
- `cc` クレート (1.x) で、MSVC (`C:\BuildTools2022` の cl 14.44) を使って静的ライブラリにした。Build Tools に clang-cl は入っていない。
- 入れたファイル: `sim/` の `sim_*.c` と `avr_*.c`、`cores/sim_mega328.c` と `cores/sim_megax8.c`。
- 外したファイル: `sim_gdb.c` (ソケット)、`sim_elf.c`・`sim_dwarf.c` (libelf)、`run_avr.c` (コマンドの本体)、`avr_usb.c`。

## MSVC で通すために変えたもの

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

## 動かしたもの

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

## 速さと大きさ

| | debug | release |
|---|---|---|
| 1 の C (16 MHz の 1 s 分) | 0.19 s (84 MHz 相当) | 0.11 s (150 MHz 相当) |
| 2 のスケッチ (UART・タイマーの割り込み込み) | 0.36 s | 0.18 s |

実時間の 5〜9 倍の速さで回る。回路の側と刻みをそろえても、実時間には十分間に合う。

exe の大きさは、試しのバイナリ全体で 283 KB (release)。simavr の分はその一部。

## 残っている気になる所

- 1 の D13 の周期は 1.019 ms × 100 だった。ADC の変換に 13 × 128 クロック = 104 µs かかるはずなので、1.1 ms 前後を予想していた。simavr の ADC の変換時間の扱いを、使う前に確かめる。
- コアは ATmega328 だけ試した。Mega (2560)・ATtiny も同じ作りなので通るはず。
- ライブラリ (Servo・Wire など) は試していない。この PC の arduino-cli に入っていないため (入れるにはダウンロードが要る)。`tone` と `EEPROM` のようにコアの中身を使うものは動いた。ライブラリも、ビルドは arduino-cli の仕事なので、simavr の側では同じ。
- CI: Linux・macOS は gcc / clang なので、書き換えなしで通るはず。Windows の CI は MSVC で、上の書き換えが要る。

## 次にやること (W5 (b) の本体)

1. simavr の `sim/` を、リポジトリに入れる形を決める。サブモジュールか、決まった版を写して書き換えを当てるか。GPL-3.0 のファイルとして `THIRD_PARTY_LICENSES.txt` に足す。
2. 新しいクレート (仮に `avr-sim`) に `build.rs` と安全な Rust の包みを置く。ELF は Rust で読む。
3. `breadboard/live.rs` のスケッチの回し方 (次の時刻まで回路を解く → コアをその時刻まで進める、`RunIo::drive`) の後ろに、今の `sim_core::sketch::Machine` と並べて差し込む。arduino-cli が無いときは、今の自前の読み方のまま。
