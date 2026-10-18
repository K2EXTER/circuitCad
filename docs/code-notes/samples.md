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
- サンプルは `docs/examples/bridge_rectifier.schjson`(生成は `write_rectifier_example`)、レッスン9がこれを使う。
- 総合デモは `mixed_signal.schjson`(生成 `write_mixed_signal_example`、`tests/mixed.rs`。12V→逆接続ショットキー→LM7805→AMS1117、NE555(約680Hz)→74HC04で5Vに整形→74HC74×2の÷4カウンタ+パワーオンリセット+LED、NTC+LM393+2N3904の温度アラーム、LM358の単電源反転アンプ(×10)+RCローパス)。ベンチマーク兼READMEの画像用(`docs/images/mixed-signal-*.png`、`--tran 8m 10u --signals CLK,Q1,Q2,FILT` で撮る)。過渡解析(8ms・10µs刻み)は開発ビルドで約1.4秒。DC動作点はスイッチ幅の拡大(Widen)で求まり、-40〜100℃でも通る(テストで確認)。過渡解析はDCが求まらず「すべてゼロから」始める(555とカウンタは普通のこと)。構成を変えると 555+カウンタ+アラーム+アンプ をつなぐ組み合わせで DC が `Singular`/`NoConvergence` になることがあった(原因は「ニュートンの途中の反復で行列が特異になる」。`dc/newton.rs` の2回目以降の反復の特異は収束失敗として扱い、`solve_op` は他の戦略も試す)。
- 総合デモの基板は `mixed_signal.ccproj`(回路図と、手で配線した基板のプロジェクト。部品41個・配線249本・表面のGNDベタ。`--drc`/`--3d`/`--order` などの基板用オプションは `.ccproj` にも効く)。READMEの基板・3D・発注の画像はこれから撮る(`tools/gen_screenshots.ps1`)。開けること・DRC違反が無いことは `tests/mixed.rs` が確かめる。
