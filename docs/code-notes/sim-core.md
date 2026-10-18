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
- 未対応のまま: `.func`、`.include`/`.lib`、`J K T` 素子、XSPICE の `A` 素子(LTspice の SCHMITT など)、暗号化モデル(LTspice の Encrypted File、PSpice の $ENCRYPTED_LIB。`part_form/trial.rs` が検出して案内する)、BJT の基板容量(CJS・CCS)・バイアスで変わる TF(ITF/VTF/XTF)・BE降伏(BVBE)・雑音(KF/AF)、MOSFET の LEVEL 2 以上の式(体効果・移動度低下など)、JFET のゲート接合(IS)。
- テスト: `tests_flat.rs`。

## BJT の早期効果と直列抵抗 (VAF/VAR/RC/RE)
`dc/devices.rs bjt_eval`: 輸送電流に `k = 1 − vbc/VAF − vbe/VAR`(0.05 で下限)を掛ける(ベース電流は変えない)。`RB`/`RC`/`RE` はそれぞれ内部ノード(`Ce::Q` の `bi`/`ci`/`ei`)を作り、接合は内部ノードで評価する。`.model` の `VAF`(別名 `VA`)・`VAR`(別名 `VB`)・`RC`・`RE` を `spice_in` が読む。外付け抵抗と同じ結果になることを `tests_bjt.rs` で確認(`RC`/`RE`)。メーカーの 7805 モデルで、出力が 4.76V → 4.78V と少し近づいた(残りは寄生容量・その他の項)。

## BJT の電荷(接合容量 CJE/CJC・遷移時間 TF/TR)
`dc/devices.rs`: `depletion`(空乏容量。`FC·VJ` までは `CJ0/(1−v/VJ)^M`、その先は接線で延長、電荷は積分形)、`bjt_qbe`/`bjt_qbc`(空乏電荷+拡散電荷 `TF·If` / `TR·Ir` とその容量 dQ/dV)。電荷を持つ素子は `Ce::Q.cap`(積分器の枠を2つ=BE,BC)。過渡は容量と同じ台形/後退オイラーを、電荷 Q(v) を反復点で線形化して使う(`dc/mna.rs`、状態の更新は `tran.rs`)。ACとノイズは動作点の `C=dQ/dV` を BE・BC の間に入れる。`.model` の `CJE/VJE(PE)/MJE(ME)/CJC/VJC(PC)/MJC(MC)/FC/TF/TR`。検証: `tests_bjt.rs`(AC の折れ点が 1/√2、過渡の時定数、`|h21| = βf/√(1+(ωβfTF)²)`)。

## ダイオードの電荷(CJO/VJ/M/FC/TT)
BJT と同じ作り(`diode_q` = 空乏電荷 + 拡散電荷 `TT·Id`)。`Ce::D` に `dm`(モデルの複製)と `cap`(積分器の枠1つ)を持ち、接合は `ai`–`k` の間(RS があれば内部ノード側)。過渡・AC・ノイズは BJT と同様。`AREA` は CJO も倍にする。検証: `tests_diode_cap.rs`(逆バイアスのAC折れ点が 1/√2、過渡の時定数、順方向で `rd·C = TT`)。フォーム側は `part-lib/diode.rs` の `cj0_from`(Cj と測定した逆電圧から CJO)・`tt_from_trr`(trr と IF/IR から TT)。

## 実在モデルでの追加確認(2回目、2026-10-15)
KiCad-Spice-Library の11個(TI の ua741・tl082、2n3904、OPA197/OPA211、MCP6001、irfz44n、IR2110、lm337、LM35、NE555)を `SPICE_MODEL_DIR` の試し読み(`vendor_models_*` テスト)で確かめて足したもの: 入れ子の `.subckt` 定義(スタックで読む。定義は全体で共有)、`TABLE {式} ((x,y)(x,y))`(`=` なし・二重括弧)と `table(x0 y0 x1 y1 …)`(G/E で制御ノード2つ)、`VALUE { 式 }`(`=` なし)、`R1 a b MODEL 値`(RES モデル形式。値はモデル名の次の語)、パラメータに旗(`noiseless`)があっても読む、JFET(`NJF`/`PJF`、`J` 素子。MOSFET と同じ平方則で `kp=2·BETA`、ピンチオフは n チャネル向きで常に負、`MosModel.jfet`)、`uramp()`、LTspice の `A` 素子のうち OTA(電流=G·(第2ノード−第1ノード−ref) を ±Iout で制限して出力へ、Cout があれば出力と GND の間の容量。SCHMITT・SRFLOP・DFLOP・AND・OR などは記憶や遅れを持つので未対応と案内)、値が `{…temp…}` の `V`/`I` 電源は式の B 素子にして温度スイープに追従(LM35 が 0.01V/℃ で動く)。
結果: 読めて動くのを確認したもの = ua741(バッファ 2.0000V)、TL082(JFET入力、1.9999V)、lm337(−1.2497V の基準)、irfz44n(Rds(on) 約17mΩ)、LM35(27℃で 0.2700V)、OPA197、2N3904。読めるが挙動が合わなかった2つは原因が分かって直った(下の「OPA211 と MCP6001」)。読めないもの = NE555・IR2110(SCHMITT・SRFLOP・DFLOP などの `A` 素子)。

## OPA211 と MCP6001 の原因(ngspice との節点ごとの比較で特定)
- **OPA211(出力が ±0.7mV に張り付いた):** `M` 素子の `W=`/`L=`/`M=` を無視していた。レベル1の相互コンダクタンスは `KP·W/L`(並列数 M 倍、W と L の既定は 100µm)なので、`mos_geometry` で掛けるようにした。OPA211 のスイッチ用 MOSFET(`KP=200u`、W/L が 100 倍違う)が効かず、出力段が働いていなかった。直して、バッファが 2.0000V になった。
- **MCP6001(DC で Singular):** LTspice の B 素子のオプション `Rpar=…` `Cpar=…`(電源の並列抵抗・並列容量)を捨てていた。電流源 `I=` だけがつながるノードは抵抗が無くて電圧が決まらない。`Rpar`/`Cpar` を抵抗・容量として足すようにして解けた(バッファ 1.9994V)。uc3845a の「電源なしの動作点が求まらない」も同じ原因で消えた。
- 調べ方: ngspice で同じ回路の動作点を出し、全節点を比べて、差の大きい所(OPA211 は 54 節点が 50mV 以上ずれ、出力段のスイッチ回路が最大)から原因の素子を探す。ngspice が読めないモデル(MCP6001)は、素子を1つずつ外して解けるようになる素子を探す(`bisect_singular` テスト)。

## 残りのモデルパラメータ(2026-10-15)
- **BJT**(`dc/devices.rs bjt_eval`): 漏れ電流 `ISE/NE`(BE)・`ISC/NC`(BC) をベース電流へ足す(コレクタ電流は BC 漏れだけ引く)。高注入 `IKF`/`IKR`: 輸送電流を `2/(1+√(1+4·q2))`(`q2=If/IKF+Ir/IKR`)で割る。早期効果 `k` との積で微分を取る。`EG`・`XTI`・`XTB`(温度 `temperature.rs`。`XTB` の既定は従来どおり 1.5)。`.model` の別名 `C2`(=ISE)・`C4`(=ISC)・`IK`(=IKF)。
- **ダイオード**: 再結合電流 `ISR/NR`(`diode_recomb`。DC/過渡/ノイズ/素子の電流表示で共通)、温度 `EG`・`XTI`(ショットキーは `EG≈0.69`・`XTI≈2` で、温度での飽和電流の増え方がシリコンと大きく違う)。
- **MOSFET**: ゲート重なり容量 `CGSO/CGDO`(F/m)に、素子の `W` と並列数 `M` を掛けて固定の CGS/CGD にする(`spice_in.rs` の `M` 行)。
- 温度では、`ISE`・`ISC`・`ISR` も各自の `N` で同じ式にかけ、各 `N` を温度比で伸ばす(解く側の `VT` が 27℃ 固定のため)。
- 検証: `tests_params.rs`(全効果を入れた BJT の微分が差分と一致、IKF の式、漏れ電流、ISR、EG/XTI/XTB の温度式、CGSO、`.model` の読み込み)。実在モデルの警告は、2N3904 が `XTI EG NE ISE IKF XTB NC ISC` → `ITF VTF XTF` だけに、LM7805 が `EG CCS` → `CCS` だけになった。
- **過渡解析の刻み制御(2026-10):** 誤差で棄却された刻みのあと、`h_ctrl` を古い大きな値のまま残していたため、毎ステップ「大きい刻みを試す→棄却→縮める」を繰り返していた(総合デモで採用 2464 点に対し棄却 5018 回)。棄却したステップのあとは採用した刻みから出発し、伸びは 1.05 倍までにした(棄却 約360回、時間は半分以下)。1.2 倍以上だと `cmos_inverter` の窓1の最小が ngspice より下がり(3.10V対3.30V、`xcheck_tran` のテストが落ちる)、1.0 倍だとマルチバイブレータの反転で収束しなくなる。
- 収束の耐久テストは `app/src/tests/converge.rs`(サンプル全部を -40/125℃ と抵抗のランダム倍率で DC。`ne555_blinker` と `mixed_signal` は発振回路なので対象外)。
- `diagnose` は、DCが収束しない回路に 555(内部ノード `Uxx.rcmp` と `Uxx.scmp` が両方ある部品)が入っていれば、「555 の点滅回路はDCで1点に定まらない。過渡解析で見る」という案内を先頭に出す(`converge.rs` の `a_555_blinker_without_an_operating_point...`)。

## 公開モデル集での互換性の一括確認(2026-10-18)
`tests_vendor_bulk.rs`(`SPICE_MODEL_DIR=<モデルのフォルダ> [SPICE_BULK_OUT=<csv>] [SPICE_BULK_TRAN=1] cargo test -p sim-core vendor_bulk -- --ignored --nocapture`)。フォルダの全ファイルの全 `.subckt`(1ファイル6個まで)を、ピン名から電源(5V)・GND・入力(1kΩ経由2V)・その他(10kΩでGND)に振り分けた簡易回路につなぎ、読めるか・DC動作点が求まるか(`SPICE_BULK_TRAN` で 2µs の過渡も)を数える。電源のピン名が分からない(数字だけのピン)ものは DC の成否を判定しない。失敗の理由は文言ごとに集計して出る。`bisect_read`(`SPICE_DUMP_FILE`/`SPICE_DUMP_TOP`)は、読めない `.subckt` の原因の行を1行ずつ外して探す。
- 使った集め物: KiCad-Spice-Library(GitHub、約4800個の `.subckt`。リポジトリには入れない)。
- 結果(読めて解けた割合): 最初 52%(2508/4812) → 73%(3507/4812)。DC が収束しなかったのは、電源が判別できるものでは 8 個だけ。過渡も 2 個増えただけ(ソルバーは、読めたものには粘り強い)。
- 効いた直し: ①`.model` が1枚読めないだけで、同じファイルの全モデルが読めなかった(1ファイルに数十枚あるライブラリで最大。使う素子の分だけ読めれば足りるように)。②パラメータの `=` の前後の空白(`KP= {X}`)、`.params`、`PARAMS: A=1, B=2,` のカンマ、`POLY (2)`/`POLY 3 …`、`TABLE` の点が括弧なし、`DC 0 PULSE(..)`、値のない電源(`Vsense a b`、電流の測り役)。③`K`(結合インダクタ、`Element::Mutual`、DC/過渡/AC/ノイズ)。④`POLY` の積の項(SPICE の項の並び順)。⑤`log10`、`==`/`!=`。⑥VDMOS モデルと3端子の `M`。⑦`.subckt` の見出しの既定値が本体の `.param` を指す書き方(`vcc1={vcc}`)。⑧B 素子の式の中のパラメータ名の置き換え。⑨UTF-8 でないファイル(`µ` が1バイト)の読み込み(`decode_spice_text`、部品作成フォームの「SPICEファイルを読む」も)。
- まだ読めない主なもの(約27%): PSpice/XSPICE のデジタル部品(`A` 素子の `[in] out model` 形・`U` 素子・SCHMITT/SRFLOP/DFLOP、計 約300)、伝送線路 `T`(33)、電荷型コンデンサ `C … Q=式`(21)、拡散・ラプラス・`?:` を使う式、`I(D名)`(ダイオードの電流を式で読む)、呼び出し側が与えるはずのパラメータ(`RON`・`Cnom`・`DCR` など。モデルの不具合ではない)。

## デジタル部品(段階1: 組合せ回路、2026-10-18)
アナログ解析器の中に、デジタル用の別エンジン(イベント駆動)を足す前の第1段階。ゲートを**1つの B 電圧源**として表す(`spice_flat.rs` の `gate_device` / `u_device`)。入力は `0.5+0.5*tanh((V(x)-vt)/w)`(なめらかな0〜1、`w` は振幅の5%)にして、AND は積、OR は `1-Π(1-x)`、XOR は `a+b-2ab` の畳み込み、反転は `1-x`。出力は `vlow + (vhigh-vlow)*式`。滑らかにしてあるのは、段差だと傾きがゼロでニュートン法が進めないから。
- 読めるもの: LTspice の8ピンの `A`(`AND/NAND/OR/NOR/XOR/XNOR/BUF/INV`、入力は1〜4ピン・出力は7ピン、`vlow`/`vhigh`/`vt` を読む。既定は LTspice と同じ 0 V と 1 V)、ngspice の XSPICE 形(`aX [a b] y model` と `aX in out model`、モデルは `d_and d_nand d_or d_nor d_xor d_xnor d_buffer d_inverter`)、オープンコレクタ `d_open_c`(入力が偽のとき100Ωで低レベルへ引く)、`d_tristate`(許可されたとき100Ω越しに入力のレベルを出す)、PSpice の `U nand(2) DPWR DGND in.. out ...`(電源ピンは無視して 0 V と 5 V)と、見出しの `optional: DPWR=$G_DPWR`(`$` で始まる値は大域ノードで、パラメータではない)。
- 読めないもの(段階2以降): SCHMITT・SRFLOP・DFLOP・カウンタなど記憶のある部品と、遅延。遅延は無視する(`td`、`rise_delay` など)。接地につないだ入力は「使っていないピン」として無視する(LTspice と同じ)。
- 効果(公開モデル集での一括確認): 読めて解けた割合 73% → 75.5%(3507 → 3632 / 4812)。

## デジタル部品(段階2: 記憶のある部品、2026-10-18)
フリップフロップ・ラッチ・シュミットトリガは `Element::Seq`(`SeqKind`: Dff/Tff/Jkff/Srff/Dlatch/Srlatch/Schmitt、`SeqLevels`: vlow/vhigh/vt/vh と set/reset の極性)。**状態(0か1)は回路方程式の未知数にしない**: 出力 q・qb は「状態のレベルを出す電圧源」(接地に対して、`Ce::Seq`、`dc/mna.rs`)で、状態は解と解の間に `dc/seq.rs` の `seq_update` が更新する。だからニュートン法はラッチの帰還ループを解かずに済み、DC の収束と起動時の中間電圧での居座りが起きない。
- 更新の規則(`seq_update`): セット/リセットは即時(同時のときは今の状態を保つ)。クロックの立ち上がり(前の採用点では閾値より下、今は上)で、データは**エッジの前の採用点の値**を取り込む。ラッチ(`Dlatch`/`Srlatch`)は許可が真の間ずっと追従。シュミットは `vt±vh` を超えたときだけ反転。DC(`solve_op`)は、状態を更新してはもう一度解く、を最大16回(状態が電圧と合うまで)。過渡は、採用した刻みのあとに更新し、状態が変わったら電源の角と同じ扱い(次は後退オイラー、履歴を捨てる)。
- 精度の限界: 状態の変化は次の刻みから効く(遅れは最大で1刻み。エッジの時刻を刻みの途中に合わせる処理はまだ)。遅延(`td`、`clk_delay` など)は無視。
- 読める書き方: LTspice の `A` 素子 `DFLOP`/`SRFLOP`/`JKFLOP`/`SCHMITT`(ピンは `in1 in2 clk set reset qbar q`、set/reset は正論理、`Vt`/`Vh`/`vlow`/`vhigh`。使いかたからの推定で、LTspice の資料では確認していない)と、ngspice の `d_dff d_tff d_jkff d_srff d_dlatch d_srlatch`(使わない端子は `NULL`)。フラット化後は内部の `Z` 行(`Z名 種類 q qb vlow vhigh vt vh set_low reset_low 入力…`)になる。
- 読めないまま: PSpice の `U dff(1)` などの記憶つき部品、`ONESHOT`/`PHASEDET`/`MODULATOR` などの `A` 素子。
- 効果(一括確認): 読めて解けた割合 75.5% → 76.1%(3632 → 3660 / 4812)。DC が収束しなかったのは 11 個(いずれも電源のピン名が曖昧か、電圧源だけのサブサーキットなど。`OPA170`・`OPA1662` は DC は通るが過渡で `Singular`: 調査中)。
- **OPA170・OPA1662 の過渡解析が `Singular` だった原因(2026-10-18):** 一括確認の簡易回路では電源ピン `V+` に 5V をつなぐが、モデルの中に `C15 V+ V- 10P IC=0` がある。過渡解析の開始点は、コンデンサの初期電圧 `IC=` を電圧源で固定して求める(`tran.rs start_point`)ので、`V+`〜`V-` を 0V に固定する源と 5V の電源が並列になり、行列が構造的に特異になっていた(行列の構造的ランクを調べて、`V+` の列に行が2つしか無いこと=電圧源2本が同じ点を固定、と特定)。DC は IC を使わないので解けていた。SPICE は UIC なしでは IC を無視する。直し: IC を固定した解が `Singular` のときは、IC なしの普通の動作点から始める。調べ方は `tests_vendor_bulk.rs` の `tran_debug`(`SPICE_UNKNOWNS=番号,...` で未知数の名前も出る)。

## 残りの DC 失敗9個の内訳(2026-10-18、公開モデル集の一括確認)
行列の構造的ランク(各行と各列を組にできるか)を調べると、モデルの異常ではなく、簡易回路のつなぎ方による見かけの失敗だった。
- **電圧源どうしの衝突(5個): `ixdd`(VCC 端子が2本)、`ina250`・`ina301a2_1` の `VOFFS`、`TSV912` の `VOS_SRC_0`、`LM5145_TRANS`。** サブサーキット自身が端子の電圧を決めている(電圧オフセット源のサブサーキット、`VCC` が内蔵 LDO の出力、2本の `VCC` 端子を同じ網につなぐ、など)のに、簡易回路が電源や GND をつなぐので、同じ点を2本の電圧源が固定して特異になる。直す対象ではない(`Hall` の条件で見つかる: 行が2つで列が1つだけ)。
- **発振回路(2個): `UCC28C45`(PWM コントローラ)。** 555 と同じで、安定な動作点が無い。過渡解析は通る。
- **`4316RMS_THAT`(RMS-DC 変換): ** DC は収束しない。過渡は通る(入力 0V の簡易回路では RMS の式が 0 割りに近い)。
- この調べで分かった LTspice の `A` 素子のピン: **入力は 1〜5 番、真の出力は 7 番、反転出力は 6 番**(`A a b 0 0 0 y 0 0 AND` は NAND、`A a 0 0 0 0 y 0 0 BUF` は反転)。スイッチングレベルは `Ref=`(ゲート)・`Vt=`(シュミット)。`gate_device` と `seq_device` はこの規約で読む(以前は出力を 7 番だけとしていたので、6 番の出力を持つゲートが読めなかった)。出力抵抗 `Rout=`・容量 `Cout=`・遅延 `Td=` はまだ無視。


## PSpice の記憶つき U 素子(2026-10-18)
`spice_flat.rs u_seq_device`: `U dff(n)`(`PREBAR CLRBAR CLK D Q QBAR`)・`jkff(n)`(`… CLK J K Q QBAR`)・`dltch(n)`(`… GATE D Q QBAR`)・`srff(n)`(`… GATE S R Q QBAR`、ゲートつき SR ラッチとして `Srlatch` に)を、既存の `Z` 行(`Element::Seq`)に変換する。プリセット/クリアは負論理(`set_low`/`reset_low`=1)、レベルは 0〜5V。`$D_HI` は 5V の電圧源(`__hi…`)、`$D_LO` は GND。複数ビット(`dff(2)`)は1ビットずつ `Z` 行にする。遅延は無視、カウンタなどは未対応のまま。テストは `tests_flat.rs` の `pspice_*`。

## 伝送線路 T(2026-10-18)
`spice_flat.rs transmission_line`: 無損失線路 `T名前 a1 b1 a2 b2 Z0= TD=`(または `F=`・`NL=`、既定 0.25 波長、`TD=NL/F`)を、40 区間の L-C はしご(L=Z0·TD/N、C=TD/(Z0·N)、両端は C/2)に置き換える。素子はそのまま使えるので DC(つながった線)・AC・過渡のどれでも動く。近似なので、使える帯域は約 N/(π·TD) まで(それより速い立ち上がりはなまる/揺れる)。戻り側 b1 と b2 が別の網のときは 1µΩ でつなぐ。損失・`IC=`・`LEN=` は未対応(無視)。テストは `tests_flat.rs` の `a_matched_transmission_line_delays_a_step_by_td` ほか。

## ダイオードの電流 I(D名)(2026-10-18)
`spice_in.rs add_flat_subckt`: 式(B 素子など)が `I(D1)` を読むダイオードだけ、アノード側に 0V の電圧源(名前 `D1`)を直列に入れ、ダイオード本体は `D1.d` という名前にする(`current_refs` が本文中の `I(...)` を先に集める)。電流の向きはアノード→カソードが正。読まないダイオードは今まで通り。テストは `tests_bsource.rs the_current_of_a_diode_can_be_read`。

## 電荷型コンデンサ C … Q=式(2026-10-18)
`C1 a b Q=式`(式は `v(a,b)` などの電圧の関数。電荷 Q(v))を `Element::ChargeCap` → `Ce::Cq` にする(`spice_in.rs` の `'C'` の枝、展開は `spice_flat.rs` が式のノード名を置き換える)。電流は dQ/dt で、DC では開放。状態は `n_caps` を2つ使う(`idx`=両端の電圧=刻みの誤差見積り用、`idx+1`=電荷=状態)。過渡(`dc/mna.rs`)は `Ce::D` の接合容量と同じ電荷の差分(後退オイラー/台形)で、Q(x) を式の微分(`CExpr::eval` の勾配)で今の反復の点に線形化する。AC(`ac.rs`)は動作点での dQ/dx を `jω` 倍して行(a, b)に置く。ノイズなし。`IC=`(初期電圧)は未対応。テストは `tests_flat.rs` の `a_charge_type_capacitor…` `a_nonlinear_charge_capacitor…`。
