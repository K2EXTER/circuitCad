# サンプル回路・同梱部品のメモ

`docs/examples/*.schjson` と `part-lib/examples` の同梱部品について。生成は `crates/app/src/tests/examples.rs` の `write_*` テスト(`cargo test -p app <名前> -- --ignored`)。

- トランス部品は `part-lib/examples/TRANS_*.part.json`(理想変圧器をBソースで作った`.subckt`: `Bs`=電圧源、`Bp`=`-n*I(Bs)`の電流源、巻線抵抗と励磁L)。
- 出力コンデンサの置き換え比較は `buck_cout_electrolytic.schjson` / `buck_cout_ceramic.schjson`(`buck_converter` のC1だけを電解 `CE_100u_16V` / セラミック `C_10u_0805` に替えたもの、生成 `write_buck_cout_examples`。ESRが効いてリップルは電解の方が大きい)。
- インピーダンス比較は `cap_impedance.schjson`(1Ωの直列抵抗ごとに電解/セラミック/並列をつなぎ、AC解析で V(ZE/ZC/ZP)≒|Z|。低域は電解、1MHz以上はセラミックが低い。生成 `write_cap_impedance_example`)。
- 電池は `part-lib/examples/{CR2032,AA_ALKALINE,LIION_18650}.part.json`(残量を蓄える巨大なC+残量で決まる電圧のBソース+内部抵抗。時間0だけ `Bpull` で残量を1に固定してDC解析でも満充電になる。サンプル `coin_cell_led.schjson`、レッスン10)。
- 充電できる電池は `LIION_18650_CHG`(`LIION_18650` と同じ式で、残量20%から始まり、満充電で電流を受けつけない。Biの電流は充電なら残量が増える向き)。
- サンプル `li_ion_cell_charge.schjson`(TP4056+この電池、PROG 1.2kで1A、生成 `write_cell_charge_example`。定電流→定電圧→STDBY点灯まで約2.7時間)。
- TP5100版は `tp5100_cell_charge.schjson`(`tp5100_charger` の電池をこの電池に替えたもの、Rs 0.1Ωで1A、生成 `write_tp5100_cell_charge_example`)。
- 降圧コンバータは `part-lib/examples/BUCK_DEMO.part.json`(150kHzのPWM: ランプ+積分型誤差アンプ+tanhのPWM比較器+スイッチ`S`。tanhを滑らかにしてデューティのジッタを抑え、時間0だけ誤差アンプを0に固定してDC解析を通す。サンプル `buck_converter.schjson`、レッスン11、`designcalc::buck_design`)。
- PNPハイサイドスイッチは `pnp_highside.schjson`(2N3906、生成 `write_pnp_example`、レッスン13、プルアップR2・負荷R3)、オペアンプ反転増幅は `opamp_inverting.schjson`(LM358、生成 `write_inverting_example`、レッスン14)。
- 電源の学習パス用に `diode_iv.schjson`(レッスン15)・`zener_regulator.schjson`(16)・`emitter_follower.schjson`(17、生成は `write_diode_example`/`write_zener_example`/`write_follower_example`)。
- `Lesson.real` は「実機で確かめよう」(用意するもの・配線・測り方・期待値・安全)。
- MOSFETスイッチは `docs/examples/mosfet_switch.schjson`(2N7000、生成は `write_mosfet_example`、レッスン12、負荷はR_2512で定格内)。
- 3相電源は `part-lib/examples/AC3P_10V_50Hz.part.json`(Y結線。ピン1=A 2=B 3=C 4=N、`.subckt` の中にSIN電圧源を3つ: 位相 0/-120/-240°、各相に直列抵抗 `Rser=0.2`(0Ωだと、空のコンデンサを充電する最初の1ステップで過渡解析の値が振動し、整流後の電圧が 44V まで跳ねて数十msかけて戻った。実物にも巻線抵抗があるので入れた)、相電圧 10Vrms=ピーク14.142V、50Hz。ACも同じ位相。記号は `glyph: "ac3p"`(`glyphs.rs`: 位相の違う正弦波の丸を3つ縦に並べ、左を中性点Nでまとめる。ピン名は A/B/C/N が必要)。フットプリントは4穴1列 5.08mm(`TerminalBlock_4P_5.08`、`flow.rs` の `three_phase_source_reaches_the_board` で取り込みを確認)。電圧・周波数は部品の中で固定で、変えるときは `SIN(...)` を書き換える)。サンプルは `three_phase_wye.schjson`(3相電源→100Ω×3のY負荷。A+B+C=0 と 120° のずれをテスト)と `three_phase_rectifier.schjson`(6ダイオードのブリッジ→470µF+100Ω。線間ピーク24.5V−ダイオード2個ぶん、リップルは300Hz=6×50Hz。Nは1MΩでGNDに落として基準にするだけ)。生成は `write_three_phase_examples`、テストは `example_sim_tests.rs` の `three_phase_*`。
- サンプルは `docs/examples/bridge_rectifier.schjson`(生成は `write_rectifier_example`)、レッスン9がこれを使う。
- 総合デモは `mixed_signal.schjson`(生成 `write_mixed_signal_example`、`tests/mixed.rs`。12V→逆接続ショットキー→LM7805→AMS1117、NE555(約680Hz)→74HC04で5Vに整形→74HC74×2の÷4カウンタ+パワーオンリセット+LED、NTC+LM393+2N3904の温度アラーム、LM358の単電源反転アンプ(×10)+RCローパス)。ベンチマーク兼READMEの画像用(`docs/images/mixed-signal-*.png`、`--tran 8m 10u --signals CLK,Q1,Q2,FILT` で撮る)。過渡解析(8ms・10µs刻み)は開発ビルドで約1.4秒。DC動作点はスイッチ幅の拡大(Widen)で求まり、-40〜100℃でも通る(テストで確認)。過渡解析はDCが求まらず「すべてゼロから」始める(555とカウンタは普通のこと)。構成を変えると 555+カウンタ+アラーム+アンプ をつなぐ組み合わせで DC が `Singular`/`NoConvergence` になることがあった(原因は「ニュートンの途中の反復で行列が特異になる」。`dc/newton.rs` の2回目以降の反復の特異は収束失敗として扱い、`solve_op` は他の戦略も試す)。
- レイアウト後シミュレーションの例は `thin_power_line.ccproj`(5 V・2.2 Ω の負荷を、60 mm 離れた細い配線 0.25 mm でつないだ基板つきプロジェクト。理想では負荷に 5.00 V、「シミュレーションに反映」で 4.56 V、「DC解析の電流を入れる」→「足りないネットをすべて推奨幅に」→反映し直すと 4.93 V。作り方は `ccad` で回路図・取り込み・配置、`.pcbjson` の R1 を +58 mm ずらして外形を広げ、`board route`・`board currents`、回路図と合わせて `{"version":1,"schematic","board"}`。テストは `tests/flow.rs` の `thin_power_line_project_shows_the_copper_drop_and_the_fix`)。
- 長い配線のリンギングの例は `ringing_trace.ccproj`(3.3 V・立ち上がり 0.5 ns のパルス → 22 Ω → 90 mm の細い配線(0.25 mm・下面にGNDベタ)→ 100 kΩ の負荷。基板は `ccad` で取り込み・配置し、`.pcbjson` の R2 を +90 mm ずらして外形とGNDベタ(下面)を足し、`board route`。配線はR 0.18 Ω・L 73 nH・C 5.1 pF)。回路図で「過渡解析」を終了時間 100n・刻み 0.1n にして実行すると負荷は 3.3 V できれいに立ち上がり、基板で「シミュレーションに反映」→ 実行し直し、波形の一覧の `V(R2.1.wire)` にチェックを入れると約 5.7 V まで跳ねて約 4 ns 周期(約 260 MHz = 1/(2π√(LC)))でリンギングする(画面確認済み。`--post-layout --tran 100n 0.1n --signals SIG,R2.1.wire` で `.ccproj` を開くと同じ画面になる。`.wire` ノードは波形の一覧に出す: `tran.rs` の `shown_signal`)(CLI なら `board postlayout on` のあと `tran 100n 0.05n probe=V(R2.1.wire)`)。直し方は直列抵抗を増やす(減衰)か、配線を短くする。テストは `tests/flow.rs` の `ringing_trace_project_overshoots_only_with_the_copper`
- 総合デモの基板は `mixed_signal.ccproj`(回路図と、手で配線した基板のプロジェクト。部品41個・配線249本・表面のGNDベタ。`--drc`/`--3d`/`--order` などの基板用オプションは `.ccproj` にも効く)。READMEの基板・3D・発注の画像はこれから撮る(`tools/gen_screenshots.ps1`)。開けること・DRC違反が無いことは `tests/mixed.rs` が確かめる。

### 総合デモの速さと発振周期(2026-10-19)
`tests/mixed.rs` の `mixed_signal_speed`(過渡 8ms・10µs 刻みの所要時間と点数)と `mixed_signal_oscillator`(555 の発振周期を自前・ngspice・理論 `0.693(R1+2R2)C` で比べる)。どちらも `--ignored`。
- 速さ: 解析の高速化(因数分解の使い回しなど)の前 2.75 秒 → 後 **1.6 秒**(2617 点、棄却 365 回、`cargo test -p app --release`)。同じ回路を ngspice(`reltol=1e-4`)で走らせた時間とほぼ同じ(1.65 秒)。
- 555 の周期: 自前は **1.47 ms**(立ち上がり6回のうち最後の周期。理論 1.455 ms と 1% 以内、最初の周期は起動のぶん 1.92 ms)。素の ngspice 書き出し(`tran 10u 8m`、`uic` つきも)では発振が始まらず `OSC` が 0V のまま(起動の違い)。アプリの検算(`--xcheck … --tran`)では 33 ノード中 9 ノードがズレ(エッジのずれが周期の 5.4%、TH は自前11・ngspice1 とエッジの数が違う)。どちらの周期が正しいかは決めていない。
