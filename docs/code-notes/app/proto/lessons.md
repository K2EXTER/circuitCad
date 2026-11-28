# 試作のレッスン (2026-11-28)

調査 `reports/電気回路の学習カリキュラム調査.md` の A3・A4 (試作の道・テスターとオシロ)。回路図のレッスン (`schematic/lessons/`) は `SchEditor` だけを見て手順を判定するので、試作のタブの操作 (電源・テスター・オシロ・押しボタン・スケッチ) は判定できない。そこで、試作のタブの状態で手順を判定する、別の小さなレッスンの形を足した。

## 形
- `app/src/proto_lessons.rs`: `ProtoLesson { id, title, goal, about, safety, sample, steps }` と `ProtoStep { title, detail, done: fn(&LessonState, &mut Memo) -> bool }`。`LESSONS` に 7 本。`impl App` に `start_proto_lesson` (サンプルを `open_sample` で開き、試作のタブ・ブレッドボード・電源は切・道具は「選択」) と `proto_lesson_frame` (毎フレーム、試作のタブにいるときだけ判定して窓を描く。全部できたら `Progress::mark`)。
- 判定するもの: `breadboard/lesson_state.rs` の `LessonState` (電源・道具・テスターのレンジ・テスターとオシロが最後に見せた値 `Seen`・回路図どおりか・部品の電流・スイッチ・つまみ・最初のマイコンのシリアルとスケッチ・ブロックか)。`Seen` はテスターとオシロのパネル (`instruments.rs`) が描くときに書く。だから値はパネルを描いた次のフレームから見える。
- **手順は条件が `STEADY_FRAMES` (6) フレーム続いて済む**。押しボタンを放した直後や、スケッチを変えた直後の 1〜2 フレームは、回路が追いつく前の状態で、「押していないのに LED が点いている」のような手順を間違って済ませていた。
- 時間をまたぐもの (点いた・消えたの両方を見た、つまみの両端、ボタンを放した後の LED) は `Memo` に毎フレーム書く (`note`)。トグルは「放した次のフレームの LED」で見る。
- 学習パスの項目の種類 `learn::Kind::Proto(id)` と `Action::Proto(id)`。パス「試作の道」(`PROTO_PATH`、`PATHS` の 2 本目)。テスト `every_lesson_of_a_path_exists_and_ids_are_unique_in_a_path` が、`Kind::Proto` の id のレッスンがあることを確かめる。
- 起動の引数 `--lesson proto_button --proto --proto-power` (撮影用)。

## レッスン
| id | サンプル | 手順 |
|---|---|---|
| `proto_breadboard` | `proto_basics` (V1 5V・R1 1k・R2 1k の分圧と R3 330Ω + LED。生成 `write_foundation_examples`) | 電源を入れて LED → LED を 1 列ずらす (照合が崩れる) → 戻す |
| `proto_meter` | `proto_basics` | 電圧のレンジ → MID と GND で 2.5V → わざと電流のレンジで電源の + と GND (ヒューズ) → R3 を 1 列ずらして LED の道を切り、R3 の下の足と LED の上の足に当てる (直列で数 mA) → 電源を切って抵抗のレンジで R1 (1kΩ。並ぶ道は LED が逆向きに通さないので R1 だけが読める。回路につないだまま測ると並んだ部品も測ることがある、を説明する) |
| `proto_blink` | `arduino_blink` | 点滅を見る (点いた・消えたの両方) → delay(500) を 2 か所とも delay(100) に |
| `proto_scope` | `arduino_blink` | 電源とオシロ → R1 の D13 側の足で方形波 (振れ幅 3V 超) → LED の GND 側で平らな線。電源の + の列は、Nano が USB で動くこのサンプルではどこにもつながっていないので使わない |
| `proto_button` | `arduino_blocks` | 押している間 LED → 2 回押して「presses: 2」→ ブロックの HIGH と LOW を入れ替えて、押していない間に点く (見本のままでは放すと消えるので、動きで判定できる) |
| `proto_pot` | `arduino_pot` | シリアルに数が 2 行以上 → つまみを両端 (0.1 未満と 0.9 超) まで |
| `proto_code` | `arduino_blocks` | 「コード」に切り替える → 押すたびに LED が入れ替わる (トグル) を書く。研究 (Weintrop & Wilensky 2019) が「ブロックの伸びは文の言語へ自動では移らない」とするので、最後にコードだけで書く段を置いた |

## 試験
`breadboard/tests.rs` の `proto_app` (アプリを丸ごと画面なしで動かす) で、7 本とも実際の試作のタブで手順を最後まで進める: `every_prototype_lesson_opens_on_the_breadboard_and_runs`・`the_breadboard_lesson_ticks_as_the_led_is_moved_and_put_back`・`the_meter_lesson_reads_volts_blows_the_fuse_reads_amps_in_series_and_ohms`・`the_blink_and_scope_lessons_see_the_led_blink_and_its_square_wave`・`the_button_lesson_counts_presses_and_wants_the_blocks_turned_round`・`the_pot_lesson_reads_the_serial_and_the_knob_turned_end_to_end`・`the_code_lesson_wants_code_and_a_led_that_toggles` (見本のスケッチのままでは済まないことも)。

## 足すとき
`LESSONS` に 1 件 (`id` は変えない)、パスに `proto("id", …)` の項目、`breadboard/tests.rs` に手順を進める試験。手順の条件に要る状態が `LessonState` に無ければ、`lesson_state.rs` に足す。
