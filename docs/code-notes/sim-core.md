# sim-core 詳細メモ (`crates/sim-core/src`)

CLAUDE.md の地図から参照される、各ファイルの詳しい説明。

- `dc/` 動作点 (`dc_op`、`DcResult`/`ElementResult`、収束戦略 `Strategy` は `mod.rs`)。`build.rs` 回路→コンパイル済み形(`Ce`,`Compiled`) / `devices.rs` トランジスタ等の式 / `mna.rs` 行列の組み立てと各素子のスタンプ(`Ctx`,`Dyn`) / `newton.rs` ニュートン反復 / `linalg.rs` 疎LU分解
- `tran.rs` 過渡(`tstep` は最大の刻み。コンデンサ電圧とコイル電流の局所打ち切り誤差を見て刻みを自動で縮め、電源の折れ点では履歴を捨てて小さい刻みから再開する。許容は `SimOptions.lte_reltol`(既定1e-4、設定窓の「過渡解析の刻みの誤差」)。出力の時間軸は等間隔ではない。瞬時に切り替わる回路でNewtonが4回失敗したら最大刻みを1度試す) / `ac.rs` AC / `noise.rs` ノイズ / `dcsweep.rs` DCスイープ / `spectrum.rs` FFT・統計
- `waveform.rs` 電源波形 (SIN/PULSE/PWL) / `value.rs` `parse_value`・`format_value`(SPICE数値) / `spice_in.rs` ライブラリ部品の `.model`/`.subckt` 読込
- `from_schematic.rs` 部品→素子の変換 (素子名 = 部品番号 refdes)。新しい部品種別はここ
- `temperature.rs` 温度 (`Circuit::at_temperature` がモデルを書き換える、`temp_sweep`。B素子の式の `temp`(℃)も置き換えるので、温度センサ LM35/TMP36 と NTC/PT100 が温度設定に連動する。部品は `part-lib/examples`、テスト `tests_parts_power.rs`。光・磁界などは `PinDef.sim_only`=シミュレーション専用ピン(パッド無し、基板取込 `board_editor/file.rs` で除外)に電圧で与える: LDR_GL5528/BPW34/SS49E。電流センサ ACS712-05B/INA180A1〜A3)。サンプル `temperature_sensors.schjson`(LM35/TMP36/NTCを温度スイープで比べる)・`light_sensors.schjson`(LDRとフォトダイオード。光量源 V2/V3 を変える)・`current_sensors.schjson`(10mΩシャント+INA180A2とACS712で同じ電流を読む)。生成とテストは `schematic/sensor_example_tests.rs`(`write_sensor_examples`) / `montecarlo.rs` 許容差ばらつき解析 (`monte_carlo`、感度つき。測る量は `McMeasure`: DC電圧 / ACの-3dBカットオフ / 過渡のステップ応答指標)
- `stepmetrics.rs` ステップ応答の指標 `step_metrics` (立ち上がり・行き過ぎ・整定、波形ビューアと `McMeasure::Tran` が使う)
- `options.rs` 計算精度の設定 `SimOptions`(reltol/vntol/abstol/gmin/反復上限、スレッドごと。GUIの「設定」メニューが毎フレーム `set_options`) / `diagnose.rs` 解析失敗時の原因と直し方(日本語ヒント)
- `measure.rs` 波形の自動測定 `wave_measure`(最小/最大/p-p/周波数/デューティ)。`stability_margins`(位相余裕・ゲイン余裕。反転なら180°ずらす)。ACの-3dB点は `montecarlo.rs` の `minus_3db`
- `designcalc.rs` 初学者向けの値の計算(LED抵抗・分圧・RCフィルタ・78xx・トランジスタスイッチ。結果+手順+注意点+故障パターンの `CalcOut`、E24丸め)。足すときは `schematic/wizard.rs` の `CALCS` にも追加(平滑コンデンサ `smoothing_cap`、降圧 `buck_design`、昇圧 `boost_design`、MOSFET `mosfet_switch`、オペアンプ `opamp_gain`、熱と放熱板 `heat_design`、ヒューズ `fuse_design` あり)
- `fault.rs` 故障シミュレーション(部品を断線/ショート/逆向き/値10倍・1/10/DC電源の低下・上昇にしてDCを解き、元と比較。ピン単位の故障(断線/GNDショート/別ネットとブリッジ)は `break_pin` が `from_schematic_with_pin_fault` で回路図側のピン接続を差し替える(ICの内部モデルもそのまま使える)。`simulate_fault`、`FaultReport::summary` が日本語の説明。`simulate_fault_tran`/`simulate_fault_ac` は過渡波形・ゲインの正常/故障比較。`assess_safety` は故障後のDCに `check_ratings` をかけ、正常時から新しく出た/悪化した定格超えだけを `Danger`(Safe/Caution/Danger)にする。計算不能は Danger。GUIは単発の結果の下と「全部品を順に壊す」の並び順・印(🔥定格超え/⚠余裕なし))。`Fault::TurnShort(x)` はコイルの層間ショート(巻数 x が短絡: L×(1-x)²、`<名前>.DCR`×(1-x)。`TURN_SHORTS`=10/20/50%)。`chain.rs` の `chain_failure` は故障後に過渡解析を回し、ピーク電流・逆電圧・平均電力(コイルは巻線抵抗の発熱)が定格を超えた部品を、部品ごとの壊れ方(抵抗/LED/コイル=断線、半導体/コンデンサ=ショート)で順に壊して `ChainReport`(段・終わり方・電源のピーク電流・`LoopHeat`=ショートした巻線の循環電流と発熱)にする。電気的な過負荷が先、ショートした巻線の焼損は最後。GUIは故障パネルの「連鎖故障を追う」(`--fault <部品> <種類 7/8/9=レアショート10/20/50%> chain`)。サンプル `coil_layer_short.schjson`(リレーコイル+2N7000、生成 `write_coil_example`)、レッスン22 `coil_layer_short`(10%→コイルだけ焼ける、50%→Q1が先に壊れる、ヒューズの選び方。手順は `fault_chain` の中身で自動判定)
- `ripple.rs` 電解コンデンサのリップル電流と寿命(部品JSONの `ratings.ripple`(定格リップル電流+周波数補正係数)と `ratings.life`(耐久時間+温度)。`ripple_reports` が過渡解析の後半の `I(<部品>.C1)` の交流分の実効値と周波数(上向きのゼロ交差)を測り、`RippleRating::allowed_at` の許容値と比べ、内部温度=周囲+10℃×(電流/定格)²から `LifeRating::at`(10℃で2倍)で寿命を出す。GUIは過渡の波形パネルの「部品の定格」、`TranView.parts`。古い回路は `with_library_ratings` がライブラリから補う)
- `ratings.rs` 定格チェック `check_ratings`(部品のJSONの `ratings`(最大電圧/電流/電力+データシートの行。`sch_core::PartRatings`、`CustomSymbol.ratings` に載って回路図と一緒に保存される)があればそれと比べ、無ければパッケージの目安。`stress`/`requirement`/`meets` は「この回路で必要な定格(余裕2倍)」と同種の候補部品の判定で、`schematic/panel.rs` の `advice_ui` が使う。古い回路は `with_library_ratings` でライブラリから補う。ライブラリの既存インストールには `part-lib` の `ratings_key` の移行が一度だけ届ける)。以下は元の説明:(DC結果から、抵抗・IC等の消費電力をフットプリントごとの目安(`package_power_limit`)と比較、LEDの電流・逆電圧、小形コンデンサの電圧。`RatingIssue{level,message,hint}`、DC解析パネルに⚠(赤=定格超え、橙=余裕が少ない)で表示。サンプルは定格超えにしない回帰テストあり)
- `paramsweep.rs` パラメータスイープ(部品1つの値を振って過渡/ACを繰り返す、`param_sweep`) 
- `spec.rs` スペック(合否の表): `Spec{kind,target,at,lo,hi}` を `evaluate`(`SpecInputs{dc,tran,ac}` の結果から値を読み、範囲と比べて `SpecEval{value,pass,note}`)。種類 `SpecKind`(DC電圧/電流、過渡の平均・リップル(後半)・最大・最小、AC の-3dB点・ある周波数のゲイン・位相余裕)。GUIは `schematic/spec_table.rs`(解析タブの「スペック」、`--spec <種類番号> <対象> <下限|-> <上限|-> [周波数]`)。表は `.schjson` と `.ccproj` の回路図部分の `"specs"` に保存される(`SpecItem`=入力した文字のまま、種類は `SpecKind::key()` の安定した名前。`spec_table.rs` の `with_specs`/`take_specs`、古いファイルや未知の種類は空/読み飛ばし)。表を変えると未保存の変更になる(`saved_specs` と比べる)。回路図本体(`Schematic`)には入れない=Undoの履歴に混ざらない
- `power.rs` 平均電力 `mean_power`(表示範囲での v*i の時間平均)と効率 `efficiency`。GUIは `schematic/power_ui.rs`(過渡の波形パネルの「電力と効率」、`--tran … --power <入力V> <入力I> <出力V> <出力I>`)
- `tests_*.rs` は機能別 (555, logic, ac, noise, thermal ...)

## 温度係数 (TC1/TC2)
`.subckt` の R/C/L 行の `TC=a,b` / `TC1=a` / `TC2=b`(1/℃、1/℃²。27℃基準)を `spice_in` が読み、`Circuit::tempcos`(素子名→係数)に入れる。`Circuit::at_temperature` が値を `1+tc1·ΔT+tc2·ΔT²` 倍する(下限0.01倍)ので、温度設定・温度スイープのすべての解析に効く。係数の無い素子と基本部品は従来どおり温度で変わらない。生成側は `part-lib/passive.rs`(`fit_tempco`、コイルのDCRには銅の係数 `COPPER_TC` を自動で付ける)。テスト: `tests_tempco.rs`。

## コイルの飽和 (ISAT / LDROP)
`.subckt` の L 行の `ISAT=<A> LDROP=<0..1>`(LDROP 省略は 0.3)を `spice_in` が読み、`Circuit::saturations`(素子名→`i0`)に入れる。`i0 = Isat·√((1−LDROP)/LDROP)`、増分インダクタンス `L/(1+(i/i0)²)`、磁束 `L·i0·atan(i/i0)`。過渡は磁束の台形/後退オイラーを反復点まわりに線形化(`dc/mna.rs` の `Ce::L`、`i0>0` の素子があると非線形扱い)。ACは直流動作点の電流での増分インダクタンスを使う。DCは従来どおり短絡。ngspiceには無い独自の書き方。テスト: `tests_saturation.rs`(`i = i0·tan(V·t/(L·i0))` の解析解と比べる)。

## spice_flat.rs (ベンダー形式のSPICE文の展開)
`add_subckt_instance` は、`.param` / `{式}` / `'式'` / `.subckt ... params:` の既定値 / 入れ子の `X` 呼び出しを含む文を、まず `spice_flat::flatten` で1つの平らな `.subckt` に直してから `spice_in` で読む(どれも使わない文は素通し)。
- パラメータは数値の式として評価(`expr.rs` の `Expr` を流用。使える関数は `abs min max limit sqrt exp ln pow tanh if step`、`pi` `temp`)。呼び出し側の `k=値` が `.subckt` の既定値と内部の `.param` に勝つ。
- 入れ子の中の素子名は `R1.X1`(頭文字は保つ)、内部ノードは `X1.n`、端子は呼び出し側のノードに置き換える。B素子の式の `V(...)` `I(...)` も同じ規則で書き換える。深さ12まで(自分自身を呼ぶとエラー)。
- 実在のメーカーモデル(KiCad-Spice-Library の TI・IR・コミュニティ版)で試して足したもの(2026-10-15): `.param` の順序に依存しない評価、`.model` カードの `{param}`(インスタンスごとに展開)、`{式}` が `V()`/`I()` を含むときは式のまま(パラメータ名だけ置換)、`E`/`G` の `VALUE={式}`・`TABLE {式} = (x,y)…`(折れ線を `if` の入れ子に)・`POLY(n)`(1制御は多項式、複数制御は1次項のみ)、`F`/`H`(電流制御源を `I(Vsrc)` を使う B素子に)、4ノードの `Q`(基板ノードは捨てる)、`AREA`(IS倍・RB/RS割り)、`LPNP`→PNP、`VSWITCH`(Von/Voff か Vt/Vh)、`.end` で終わる `.subckt`、B素子の末尾の `Rpar=…` 等の捨てる、式に `& | ! buf inv u pwr tbl()`。未対応の種類の `.model`(JFET等)は、使う素子が出たときだけエラー。使われないモデルパラメータは `ignored_model_params` が返す(VAF・CJE・RC など。試し読みが警告する)。
- 未対応のまま: `.func`、`.include`/`.lib`、`J K T` 素子、XSPICE の `A` 素子(LTspice の SCHMITT など)、暗号化モデル(LTspice の Encrypted File、PSpice の $ENCRYPTED_LIB。`part_form/trial.rs` が検出して案内する)、BJT の高注入(IKF)・漏れ電流(ISE/ISC)・基板容量(CJS)・CCS、MOSFET の LEVEL 3 など。
- テスト: `tests_flat.rs`。

## BJT の早期効果と直列抵抗 (VAF/VAR/RC/RE)
`dc/devices.rs bjt_eval`: 輸送電流に `k = 1 − vbc/VAF − vbe/VAR`(0.05 で下限)を掛ける(ベース電流は変えない)。`RB`/`RC`/`RE` はそれぞれ内部ノード(`Ce::Q` の `bi`/`ci`/`ei`)を作り、接合は内部ノードで評価する。`.model` の `VAF`(別名 `VA`)・`VAR`(別名 `VB`)・`RC`・`RE` を `spice_in` が読む。外付け抵抗と同じ結果になることを `tests_bjt.rs` で確認(`RC`/`RE`)。メーカーの 7805 モデルで、出力が 4.76V → 4.78V と少し近づいた(残りは寄生容量・その他の項)。

## BJT の電荷(接合容量 CJE/CJC・遷移時間 TF/TR)
`dc/devices.rs`: `depletion`(空乏容量。`FC·VJ` までは `CJ0/(1−v/VJ)^M`、その先は接線で延長、電荷は積分形)、`bjt_qbe`/`bjt_qbc`(空乏電荷+拡散電荷 `TF·If` / `TR·Ir` とその容量 dQ/dV)。電荷を持つ素子は `Ce::Q.cap`(積分器の枠を2つ=BE,BC)。過渡は容量と同じ台形/後退オイラーを、電荷 Q(v) を反復点で線形化して使う(`dc/mna.rs`、状態の更新は `tran.rs`)。ACとノイズは動作点の `C=dQ/dV` を BE・BC の間に入れる。`.model` の `CJE/VJE(PE)/MJE(ME)/CJC/VJC(PC)/MJC(MC)/FC/TF/TR`。検証: `tests_bjt.rs`(AC の折れ点が 1/√2、過渡の時定数、`|h21| = βf/√(1+(ωβfTF)²)`)。

