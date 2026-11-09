# sim-core: ファイルごとの一覧(CLAUDE.md の地図の元の文)

(`docs/code-notes/sim-core.md` の目次から分けた。内容は元のまま)

## ファイル一覧(sim-core)

(CLAUDE.md の地図から移した、ファイルごとの詳しい一覧。CLAUDE.md には1行ずつの要約だけ残してある)
- `dc/` 動作点 (`build.rs` 回路→コンパイル済み形 / `devices.rs` 素子の式 / `mna.rs` 行列 / `newton.rs` ニュートン反復 / `linalg.rs` 疎LU / `digital.rs` デジタルセル(ゲート・FF を方程式の外で評価。部品の `Y` 行。設計 `docs/digital-engine-design.md`))
- `tran.rs` 過渡(刻みは自動調整) / `ac.rs` / `noise.rs` / `dcsweep.rs` / `spectrum.rs` FFT・統計 / `paramsweep.rs` パラメータスイープ / `montecarlo.rs` ばらつき解析 / `temperature.rs` 温度
- `from_schematic.rs` 部品→素子の変換 (新しい部品種別はここ) / `waveform.rs` 電源波形 / `value.rs` SPICE数値 / `spice_in.rs` ライブラリ部品の `.model`/`.subckt` 読込
- `options.rs` 計算精度 `SimOptions` / `par.rs` 独立な計算を全コアで並列(`par_map`。ばらつき解析・パラメータスイープ・故障・CLI の `sens`) / `diagnose.rs` 失敗時の原因と直し方
- `measure.rs` 波形の自動測定・安定余裕 / `stepmetrics.rs` ステップ応答の指標・整定時間(`settling_times`。CLI の `tran settle=`) / `power.rs` 電力と効率 / `spec.rs` スペック(合否の表)
- `sensitivity.rs` 感度(部品を1つずつ ±1% 動かして効く順に。画面は「温度とばらつき」の「感度を計算」)
- `worstcase.rs` 最悪ケース解析(全部品を許容差の端に置いた角を実際に解く。CLI `worst`、画面はばらつき解析の窓の「最悪ケース」。スペックの合否には `SpecKind::*Worst`)
- `loopgain.rs` ループゲイン(ループを1か所切って電圧・電流を1回ずつ注入する Middlebrook 法。位相余裕。CLI `loopgain`、画面は解析タブの「ループゲイン」)
- `polezero.rs` 極と零(AC応答に分数の式を当てはめて根を求める。CLI `pz`、画面は解析タブの「極と零」)
- `impedance.rs` ネットから見たインピーダンス(電源をすべて切り 1 A の試験電流を入れる。山・谷の検出。CLI `zout`、画面は解析タブの「インピーダンス」)
- `ratings.rs` 定格チェック / `ripple.rs` 電解コンデンサのリップルと寿命
- `fault.rs` 故障シミュレーション / `chain.rs` 連鎖故障
- `selfheat.rs` 抵抗の自己発熱(電力×熱抵抗で温まり、温度係数で値が変わるのを落ち着くまで計算。CLI `heat`/`thermal`、画面は解析タブの「自己発熱」)
- `heat_tran.rs` 温度の時間変化(過渡解析の電力 → 部品ごとの熱容量・熱抵抗 → 温度。部品どうしの熱の結合。半導体・抵抗は温度で値・モデルが戻る。CLI `heattran`、画面は解析タブの「自己発熱」の下)
- `coverage.rs` シミュレーションが知らないこと(モデルがない部品・汎用の標準値の部品・モデルの注意書き・試験用。CLI `coverage`、画面は解析タブの「モデルの前提」)
- `designcalc.rs` 初学者向けの値の計算 (足すときは `schematic/wizard.rs` の `CALCS` と `cli/src/design.rs` の `CALCS` にも)
- `spectrum.rs` の `thd` ひずみ率(整数周期に切ってフーリエ和。CLI `thd`、画面は過渡解析の波形の「ひずみ率 (THD)」)
- `tests_*.rs` は機能別 (555, logic, ac, noise, thermal ...)
