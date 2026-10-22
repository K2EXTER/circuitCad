# sim-core 詳細メモ (`crates/sim-core/src`)

CLAUDE.md の地図から参照される、各ファイルの詳しい説明。

- `dc/` 動作点 (`dc_op`、`DcResult`/`ElementResult`、収束戦略 `Strategy` は `mod.rs`)。`build.rs` 回路→コンパイル済み形(`Ce`,`Compiled`) / `devices.rs` トランジスタ等の式 / `mna.rs` 行列の組み立てと各素子のスタンプ(`Ctx`,`Dyn`) / `newton.rs` ニュートン反復 / `linalg.rs` 疎LU分解
- `tran.rs` 過渡(`tstep` は最大の刻み。画面の「刻み」欄・CLIの `tran` で省くと `auto_tstep` = 終了時間/1000(`SchEditor::tran_times`)。コンデンサ電圧とコイル電流の局所打ち切り誤差を見て刻みを自動で縮め、電源の折れ点では履歴を捨てて小さい刻みから再開する。許容は `SimOptions.lte_reltol`(既定1e-4、設定窓の「過渡解析の刻みの誤差」)。出力の時間軸は等間隔ではない。瞬時に切り替わる回路でNewtonが4回失敗したら最大刻みを1度試す) / `ac.rs` AC / `noise.rs` ノイズ / `dcsweep.rs` DCスイープ / `spectrum.rs` FFT・統計
- `waveform.rs` 電源波形 (SIN/PULSE/PWL) / `value.rs` `parse_value`・`format_value`(SPICE数値) / `spice_in.rs` ライブラリ部品の `.model`/`.subckt` 読込
- `netcurrent.rs` DC動作点からネットごとの電流(ノードに付く素子端子の電流の絶対値の和の半分=キルヒホッフ。ライブラリ部品のモデルの素子(`U1.Rin` のような名前)は、**部品ごとに符号つきで足してから**絶対値を取る(`group_of`)。部品の中で電流が出て戻る輪(マクロモデルの理想電源など)は、ネットを通る電流ではない。以前は素子ごとの絶対値を足していて、7805 のサンプルで GND が 5.1 A(実際は 0.05 A)と出ていた。テスト `a_models_inner_loop_is_not_current_through_the_net`)。基板の配線の太さの入力(`pcb-core::current::set_net_currents`)。`net_currents_tran` は過渡解析用: ノードごとの電流の時系列から、後半(`from_frac`、既定0.5=起動を除く)の時間重み(台形)の実効値・`HEAT_WINDOW`(10 ms)窓の実効値の最大・ピークを返す(`NetStat`)。太さに使うのは実効値で、ピークは参考。ピーク>実効の10倍は `spiky`(起動サージ・スパイク)。ダイオードの逆回復や理想電源へのコンデンサ突入のような一瞬の電流が太さを不当に太くするのを避ける。BJT のベース電流は過渡の信号に無いので、エミッタ=コレクタ電流とする近似
- `from_schematic.rs` 部品→素子の変換 (素子名 = 部品番号 refdes)。新しい部品種別はここ。`from_schematic_with_wiring(s, &[WiringR])` はレイアウト後シミュレーション: 指定ピンを専用ノード `REF.PIN.wire` に移し、元のネットとの間に基板の銅の抵抗 `REF.PIN.RW` とインダクタンス `REF.PIN.LW`(`WiringR.henry`、DCでは短絡)を直列に入れる。`from_schematic_with_layout(s, wiring, &[WiringK])` は `WiringK{a,b,k}` で2つの `LW` を `Element::Mutual` で結合する(片方が無ければ無視)(`.` を含む名前なので dc では内部扱い)。`WiringR.farad` > 0 なら `REF.PIN.wire` からグランドへ容量 `REF.PIN.CW` を入れる(`LW` と合わせて長い配線のリンギングになる。DCには効かない)。`farad` と `henry` が両方あるピンには、`LW` と並列に減衰用の抵抗 `REF.PIN.RD` (= `WIRING_Q_MAX`(8) × √(L/C)) も入れる(銅の損失は mΩ だけだと Q が数千になり、数 GHz のリンギングを刻みが追って過渡解析が数百倍遅くなったため。DCは変わらない。テスト `wiring_resonance_far_above_the_signal_does_not_make_the_transient_crawl`)
- `temperature.rs` 温度 (`Circuit::at_temperature` がモデルを書き換える、`temp_sweep`。B素子の式の `temp`(℃)も置き換えるので、温度センサ LM35/TMP36 と NTC/PT100 が温度設定に連動する。部品は `part-lib/examples`、テスト `tests_parts_power.rs`。光・磁界などは `PinDef.sim_only`=シミュレーション専用ピン(パッド無し、基板取込 `board_editor/file.rs` で除外)に電圧で与える: LDR_GL5528/BPW34/SS49E。電流センサ ACS712-05B/INA180A1〜A3)。サンプル `temperature_sensors.schjson`(LM35/TMP36/NTCを温度スイープで比べる)・`light_sensors.schjson`(LDRとフォトダイオード。光量源 V2/V3 を変える)・`current_sensors.schjson`(10mΩシャント+INA180A2とACS712で同じ電流を読む)。生成とテストは `schematic/sensor_example_tests.rs`(`write_sensor_examples`) / `montecarlo.rs` 許容差ばらつき解析 (`monte_carlo`、感度つき。測る量は `McMeasure`: DC電圧 / ACの-3dBカットオフ / 過渡のステップ応答指標)
- `stepmetrics.rs` ステップ応答の指標 `step_metrics` (立ち上がり・行き過ぎ・整定、波形ビューアと `McMeasure::Tran` が使う)
- `options.rs` 計算精度の設定 `SimOptions`(reltol/vntol/abstol/gmin/反復上限、スレッドごと。GUIの「設定」メニューが毎フレーム `set_options`) / `diagnose.rs` 解析失敗時の原因と直し方(日本語ヒント)
- `measure.rs` 波形の自動測定 `wave_measure`(最小/最大/p-p/周波数/デューティ)。`stability_margins`(位相余裕・ゲイン余裕。反転なら180°ずらす)。ACの-3dB点は `montecarlo.rs` の `minus_3db`
- `designcalc.rs` 初学者向けの値の計算(LED抵抗・分圧・RCフィルタ・78xx・トランジスタスイッチ。結果+手順+注意点+故障パターンの `CalcOut`、E24丸め)。足すときは `schematic/wizard.rs` の `CALCS` にも追加(平滑コンデンサ `smoothing_cap`、降圧 `buck_design`、昇圧 `boost_design`、MOSFET `mosfet_switch`、オペアンプ `opamp_gain`、熱と放熱板 `heat_design`、ヒューズ `fuse_design` あり)
- `fault.rs` の `likelihood`(`Likelihood` = Common/Occasional/Rare): 全部品のスキャンがどの故障を試すかを決める目安。半導体の断線・ショートとコンデンサのショートがよくある、抵抗の断線・コンデンサの断線と容量抜け・コイルの断線とレアショート・電源の低下と上昇がときどき、それ以外(抵抗のショートや値の10倍・1/10、コイルのショート、コンデンサの値10倍、逆向き)がまれ。故障率の数字ではなく、一般的な傾向の目安(部品・ストレス・温度で変わる。実際の故障率はメーカーのデータ)。ICとトランジスタのピンの断線(はんだ不良)は常に試す
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
- 未対応のまま: `.include`/`.lib`、XSPICE の `A` 素子(LTspice の SCHMITT など)、暗号化モデル(LTspice の Encrypted File、PSpice の $ENCRYPTED_LIB。`part_form/trial.rs` が検出して案内する)、BJT の基板容量(CJS・CCS)・バイアスで変わる TF(ITF/VTF/XTF)・BE降伏(BVBE)・雑音(KF/AF)、MOSFET の LEVEL 2 以上の式(体効果・移動度低下など)、JFET のゲート接合(IS)。
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
- 読めないまま: PSpice の `U dff(1)` などの記憶つき部品、`ONESHOT`/`PHASEDET` などの `A` 素子(`MODULATOR` は読める。下記)。
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

## LTspice の MODULATOR(2026-10-18)
`A fm am - - - - out common MODULATOR mark= space=`(`spice_flat.rs modulator`)。出力は振幅 `V(am)`(ピンが接地なら 1V)の正弦波で、周波数は `space + (mark-space)*V(fm)`(Web 上の LTspice の説明と、公開モデル `ISO16750-2.lib`・`LM2678xx.lib` の使い方の両方に合う。ピン順の公式資料は確認できていない)。位相 = 周波数の積分を、1F のコンデンサ(`IC=0`)の電圧で持つ(電流源 `I=周波数`)ので、周波数が途中で変わっても位相は飛ばない。DC では位相が定まらないので過渡解析だけ意味がある。刻みは正弦波に合わせて縮まない(刻みは周期よりずっと小さく)。式に `sin`/`cos` を足した(`expr.rs`)。テストは `tests_flat.rs an_ltspice_modulator…`。

## 一括確認の回路の改良とパラメータ式の波かっこ(2026-10-18)
- `tests_vendor_bulk.rs build_bench`: 呼び出し側が与えるはずのパラメータ(`vcc1={vcc}` の `vcc` など)が無くて読めないとき、`guess_param`(電源 5、抵抗 1k、容量・時間 1n、他 1)で `.param` を足して読み直す(最大12回)。読めて解けた割合 77.9% → 79.6%。
- 実際の不具合も見つかった: パラメータの式の中の `({vcc1}-0.5)*{speed1}` のような波かっこ(括弧と同じ意味でまとめるだけ)を読めなかった。`spice_flat.rs` の `eval_expr`/`substitute_params` が `{` `}` を `(` `)` として扱う。79.6% → 82.6%(3973/4812)。
- 新しく読めて DC が収束しないもの: `TC4421`/`TC4422`(MOSFET ドライバ、NoConvergence)・`UCC3803`(発振回路)・`4316RMS_THAT`。

## TC4421/TC4422 の DC が収束しなかった原因と直し(2026-10-18)
MOSFET ドライバ `TC4421-JT`/`TC4422-JT`(Microchip)は、`tanh` で飽和する G 電源(電流制限つきの出力段と、それで組んだラッチ)でできていて、DC が収束しなかった。調べ方: `tests_vendor_bulk.rs` の `strategy_debug`(戦略ごとの成否、`SPICE_ITERS` で反復上限、`SPICE_VIN` で入力電圧)と `dc_bisect`(1行ずつ外して収束するか)。
- 原因: ニュートン法が発散したのではなく、**収束が遅い**だけだった。反復上限を 150 → 2000 にすると gmin 追跡が通る。gmin を 1e-3 → 1e-4 に下げる1段(1桁)で 388 回、TC4422 は 1500 回以上かかっていた。飽和した `tanh` は傾きがほぼ 0 で、ニュートン法の1歩が巨大(1e8 V)になって行き来し、少しずつしか進まない。
- 直し(`dc/mod.rs` の `Strategy::Gmin`): ①1桁の段が通らなければ半分の刻みに分けて進む(最小 1/8 桁)。②それでも通らない小さな段は、反復上限を 40 倍(`newton_long`)にして粘る。長く粘るのは**最大4秒**まで(`dc/newton.rs` の `DEADLINE`。本当に解のない回路で待たせないため)。
- 効果: 公開モデル集で DC 失敗 13 → 10(残りは電圧源どうしの衝突と発振回路)。一括確認の所要時間は 39 秒 → 70 秒(発振回路が各 4 秒ほど粘って失敗する分)。

## 古い PSpice の書き方(2026-10-18、公開モデル集の読めない理由の上位)
- **`.subckt` の見出しと `X` 呼び出しのパラメータが1つの波かっこの組** (`.subckt CAP 1 2 {TTest=27 Cnom=.1U}`、`X1 1 2 SUB {T=1+TOL/100 C=10n}`): `spice_flat.rs unbrace` が `name=value` の語に開く。
- **`{LEN=???}`**: `???` は「呼び出し側が必ず与える(既定値なし)」の印。`assignments` が既定値から外し、与えられないまま使うと「パラメータ「LEN」の値が分かりません」になる(一括確認の回路は `guess_param` で埋める)。
- 効果: 読めて解けた割合 82.6% → 84.2%(3976 → 4052 / 4812)。
- まだ読めない上位: `LOGICEXP` の `~`(PSpice のデジタル式、12+10)、周波数依存の抵抗 `R = {式} … Freq`(10)、損失のある伝送線路 `O`/`LTRA`(9)、`A` 素子の別の形(CM2.LIB・HARUFMOS.LIB など)、`PWL` の時間が増加しない(LM2678)、`TABLE` の点が1つ。

## 読めない理由の上位(続き、2026-10-18)
- **TABLE**: 点の区切りがタブでもよい(空白と `,` だけで切っていた)。`TABLE` を単語として探す(素子名 `E_VakPeakTable` やノード名 `VAKTABLE` の中の `table` を拾っていた: `find_word`)。`G … TABLE=(x0,y0,x1,y1)` の書き方も読む。
- **PWL**: 同じ時刻の2点(`pwl(0 0 0 1 …)`、縦の段差)を許し、後の点を 1ps ずらす(戻る向きの時刻は今まで通り誤り)。
- **電源の `Rser=`/`Rpar=`/`Cpar=`**(LTspice の書き方): V 電源の直列抵抗、V/I 電源に並列の抵抗・容量(`spice_in.rs` の `'V' | 'I'` の枝)。
- 効果: 4052 → 4070(84.6%)。LM2678/LM2596 の次の壁は `P(V(Freq))`(PSpice の周期関数)。他に `LOGICEXP` の `~`、`R = {式} … Freq`、損失線路 `O`/`LTRA`、`R1(I(V1))`(`R1` という名の関数)が上位に残る。

## `.func`・VALUE の中の波かっこ・A 形式のスイッチ(2026-10-18)
- **`.func 名前(引数) {本体}`** (`spice_flat.rs expand_funcs`): 他の行の `名前(式, ...)` を、引数を括弧つきで入れた本体の文字列に置き換える(本体が別の `.func` を呼んでもよい。6回まで繰り返す)。引数なし(`VP()`)も可。本体が `table(...)` などのものは、置き換えたあと各素子の読み込みで扱う。
- `VALUE = {({SENS} * V(10,6) + …) …}` のように `VALUE` の式の中に `{名前}` があるとき、パラメータを置き換える(`controlled` の VALUE の枝が `substitute_params` を通していなかった)。
- `A a b cp cn モデル` でモデルが `VSWITCH`(ngspice 風に A の名前で書かれたスイッチ)は `S` スイッチとして読む。
- 一括確認の回路: 式の中の「未定義の名前」(`「sens」は使えません。電圧は…`)も、足りないパラメータとして `guess_param` で補う。
- 効果: 4070 → 4127(85.8%)。上位に残る: `LOGICEXP` の `~`、`R = {式} … Freq`、損失線路 `O`/`LTRA`、`idtmod`(積分して折り返す関数。LM2678 の鋸歯状波)、`d_osc`、`s_xfer`。

## idt/idtmod・三項演算子・逆三角関数ほか(2026-10-18)
- **`idt(x, ic)` / `idtmod(x, ic, m [, offset])`** (`spice_flat.rs extract_integrals`): B 素子の式の中の時間積分。1F のコンデンサ(`IC=ic`)に電流源 `x` で充電し、呼び出しをコンデンサの電圧に置き換える(`idtmod` は `[offset, offset+m)` に折り返す: `v - m*floor((v-offset)/m)`)。DC で浮かないよう 1GΩ の漏れ抵抗つき。DC の値は意味がなく、過渡解析が `ic` から始まる。`MODULATOR` と同じ「積分 = コンデンサ」の方法。
- 式に `floor` `tan` `asin/arcsin` `acos/arccos` `atan/arctan` `sinh` `cosh` `asinh` と、三項演算子 `a ? b : c`(最も優先順位が低い)、`~`(`!` と同じ論理否定)を足した(`expr.rs`)。
- `.model` の続きの行(`+ IS = 9.79E-015`)の `=` の前後の空白を詰める(つないだあとで詰めていなかった)。`.model` の `mfg=` `type=` `source=` などの文字の値は読み飛ばす。結合係数 `K` は負の値も可(巻き方向が逆になるだけ。0 と絶対値 1 超が誤り)。
- 効果: 85.8% → 89.1%(4127 → 4286 / 4812)。上位に残る: 周波数依存の抵抗 `R = {式} … Freq`、損失線路 `O`/`LTRA`、`d_osc`・`s_xfer` などの XSPICE、`TABLE` の変則形、`T` の遅れ時間が式。

## EXP 電源・pwrs/sgn・pulldn・複数ビットの dff(2026-10-18)
- **`EXP(v1 v2 td1 tau1 td2 tau2)`** 電源 (`waveform.rs Waveform::Exp`): `td1` から `v2` に向かって `tau1` で上がり、`td2` から `v1` に向かって `tau2` で戻る。省略時は td1=0、tau=1µ、td2=td1+1µ。LTspice の7つ目の数は読み飛ばす。角(`td1` `td2`)に刻みを合わせる。
- 式に `sgn`/`sign` と `pwrs(x, y)`(= sgn(x)|x|^y)を足した。
- **PSpice の `U pullup(n)` / `pulldn(n)`**: ピンごとに 1kΩ で 5V / 0V へ。
- **複数ビットの `dff(n)` `jkff(n)` `dltch(n)` `srff(n)`**: ピンの並びは「PREBAR CLRBAR CLK(GATE)、全ビットのデータ(D…、または J… K…)、全ビットの Q、全ビットの QBAR」(ビットごとの繰り返しではない。最初の実装は誤っていて、`dff(1)` だけ偶然合っていた)。
- 効果: 89.1% → 90.1%(4286 → 4335 / 4812)。

## 損失のある線路・式の抵抗・E 素子の電流(2026-10-18)
- **損失のある線路** `O名前 a1 b1 a2 b2 モデル` (`.model … LTRA(R= L= C= G= LEN=)`)と、PSpice の `T名前 a1 b1 a2 b2 モデル` (`.model … TRN(len= r= l= c=)`): `spice_flat.rs lossy_line`。無損失の `T` と同じ 40 区間のはしご(`ladder`)に、区間ごとの直列抵抗 R(全長 R·LEN)と並列コンダクタンス G を足す。値は単位長さあたりで、全長 LEN を掛けて分ける。モデルの値の `{式}` は評価する。
- **式で書いた抵抗** `R1 a b R = {式}`: B 電流源 `I = V(a,b) / (式)` にする。式の中の `Freq` は 0(AC 解析の外ではそう)。
- **`I(E名)`**: フラット化が `VALUE=` の E/G 素子を `B_名` という B 電圧源にするので、`I(名)` が見つからなかった。`expr.rs canon_branch` が名前の `B_` を外して比べる(`dc/build.rs` と `spice_in.rs check_current_refs`)。
- 効果: 90.1% → 91.4%(4335 → 4396 / 4812)。残る上位: `A` 素子の別の形(CM2.LIB の `d_osc`・`s_xfer` など)、`TABLE` の変則形(Triac)、XICOR の `A` カウンタ、`.lib` で外のファイルを読むもの。

## ddt・PWL の相対時刻・XSPICE の PWL ブロックほか(2026-10-18)
- **`ddt(x)`** (`extract_integrals`): 電荷が `x` のコンデンサ(`C … Q=x`、電荷型)と、そのノードを 0V に固定する 0V の電圧源 `V_…` を足し、呼び出しを `(-I(V_…))` に置き換える(コンデンサの電流 = dQ/dt を電圧源の電流で読む)。DC では 0。AC は `jω·dx`。`sdt(x)` は `idt(x)` と同じ。`expl` は `exp`。
- **PWL の時刻の `+`**: `pwl(0 0 1m 0 +1u 5)` の `+1u` は、前の時刻より後にならないときだけ前の時刻からの増分(LTspice)。後になる普通の正の数(`pwl(0 0 +1m 5)`)はそのまま。I 電源の最後の `load` は読み飛ばす。
- **`A in out PWL2 xy_array=[x0 y0 x1 y1 …]`**: XSPICE の折れ線ブロック。`V(out) = f(V(in))` の B 電圧源。
- **TABLE のかっこが不揃い**(`(0 0 (1 10)`): 点が2つ未満なら、数を順に2つずつ組にして読み直す。
- 効果: 91.4% → 91.9%(4396 → 4425 / 4812)。

## B 素子の後ろのオプション・古い多項式の G/E(2026-10-18)
- **オプション**(`Rpar=1 Cpar=5n tripdv=50m tripdt=5n NoJacob`、`ymax=1e300`): B 素子と、`VALUE` を持つ E/G の式の後ろに付く語は、式から切り離してそのまま渡す(`spice_flat.rs is_b_option`)。以前は式といっしょに `.param` の置き換えを受け、同名のパラメータ(`ymax` など)があると `(1e300)=1e300` のようになって式が壊れた。`NoJacob` は読み飛ばす(`spice_in.rs`)。
- **係数**: `POLY(2) V1 V2 (0,7E-5,7E-5)` のように、係数がかっこの中でカンマ区切りでもよい。`G n+ n- cp cn p0, p1, p2, …` のように `POLY` の語がなくても係数が複数あれば、制御電圧の多項式 `p0 + p1 x + p2 x² + …`(SPICE2 の書き方)。
- 効果: 91.9% → 92.8%(4425 → 4464 / 4812)。

## K の3個以上・PULSE の回数・制御ノードの書き方ほか(2026-10-19)
- **`K1 L1 L2 L3 k`**: 並べた誘導子のどの2つも k で結合(`spice_in.rs`。展開側は誘導子の名前すべてに経路をつける)。
- **`PULSE(… 周期 回数)`** の8つ目の数(LTspice): 回数ぶんのパルスの角を並べた折れ線 `Pwl` に書き出す(5000回まで)。
- **制御ノードが1語**: E/G/S で `(cp,cn)` や `cp,cn` と書かれたものを2つに分ける(`spice_flat.rs expand`)。
- **`TABLE = {式} (…)`**(式の前の余分な `=`)と、`cond ? x`(`: else` が無い。0 とする)を読む。
- 効果: 92.8% → 93.1%(4464 → 4479 / 4812)。新しく読めた `LM2675-ADJ_TRANS`(スイッチングレギュレータ)は DC が収束しない(発振回路なので過渡で見る)。

## TBL 電流源と s_xfer(2026-10-19)
- **`I1 p n TBL(v0 i0 v1 i1 …)`**: 電源の両端の電圧に対する電流の折れ線(電源電流の特性。`LM2902_AB.sub` の `icc VCC VSS TBL(0 0 2 180u 100V 185u)`)。B 電流源 `I = pwl(V(p,n))`。
- **XSPICE `s_xfer`** (`A in out MODEL`、`num_coeff=[…] den_coeff=[…] gain in_offset denorm_freq`、係数は最高次から): `spice_flat.rs s_xfer`。積分器の鎖(可制御正準形): 状態 X0..X(n-1) は 1F のコンデンサ(`IC=0`)で、電流源 `X(k)' = X(k+1)` で充電する。X(n) は den(s)·X = u から求め、出力は X(k) の重みつき和なので伝達関数は厳密。1GΩ の漏れ抵抗で DC も定まる(純積分器 `1/s` だけは DC に解がない)。`s → s/denorm_freq` の置き換えつき。
- 効果: 93.1% → 93.3%(4479 → 4490 / 4812)。`SPICE_BULK_TRAN=1` で調べると `4316RMS_THAT` が 20 秒を超える(DC は通るが過渡が遅い。未調査)。

## 電流制御スイッチ W(2026-10-19)
`W1 n+ n- Vctl MODEL` + `.model MODEL CSW(Ron= Roff= It= Ih=)` (`spice_flat.rs`): B 電流源 `I = V(n+,n-)·g`、`g` は `1/Roff` から `1/Ron` へ、`I(Vctl)` が `It` を超えるところで `tanh` でなめらかに変わる(幅は `Ih` と `It` の2% の大きいほう。ヒステリシスは持たない)。幅が広すぎると開いたときに漏れる(幅 `It/5` では `g` の 5e-5 が残り、テストが落ちた)。
- **4316RMS_THAT の過渡が遅かった原因(解決)**: DC が「収束」した点の中身が、節点電圧 −4.8e11 V のような暴走だった。相対許容(`reltol`)だと、1e11 V の点は前の反復との差が相対的に小さくて「収束」と判定される。電流源が容量にしかつながらない節点(1fF に 7.2µA)と BJT の指数関数が組んで暴走し、その暴走した点から過渡を始めると、容量の電圧が 5 fs で 1e9 V 動き、刻みが最小(`tstep` の 1e-4)に張りつく(`tstop` に関係なく約 19.5 万点)。直し: `dc/mod.rs` の gmin 追跡で、`newton_long`(40 倍の反復)が返した解に、絶対値が 1e6 V を超える節点があれば失敗として扱う。DC は正直に `NoConvergence` になり、過渡は走らない。

## 公開モデル集での ngspice との検算(2026-10-19)
`tests_vendor_bulk.rs vendor_xcheck`(`SPICE_MODEL_DIR=<dir> [NGSPICE=<exe>] [SPICE_XCHECK_N=件数] [SPICE_XCHECK_OUT=<csv>] [SPICE_XCHECK_ONLY=<サブサーキット名>] [NGBEHAVIOR=ltpsa] cargo test -p sim-core vendor_xcheck --release -- --ignored --nocapture`)。一括確認と同じ簡易回路(`build_bench_text`: 補った `.param` ごとの本文を返す)を、自前と ngspice(`.spiceinit` で `set ngbehavior=ltpsa` = LTspice/PSpice 互換)で解き、端子の電圧を比べる(許容: 絶対 1mV + 相対 1%)。自前で解けたもののうち、電源と GND のピンがはっきりしているものを、名前のハッシュ順に取り出す。`SPICE_XCHECK_ONLY` を指定すると、1つのサブサーキットの全ノードを差の大きい順に出す。
- 結果(150個、最初): 一致 47、ズレ 30、ngspice が読めない 73(互換モードでも読めない書き方。`ltpsa` なしだと `if` 関数などで 31/40 が読めなかった)。
- **見つかった不具合**: `limit(x, lo, hi)` の上限と下限が逆順に書かれたとき(`limit(v*gm, +IL*e, -IL*e)`)、LTspice は3つの中央値なのに、自前は「下限、上限」の順を仮定して誤った。`expr.rs` で中央値にした(一致 42 → 47。LM4562 の出力が -420V になっていたのも同じ原因)。
- **残るズレの原因(調べた範囲)**: ①モデルの式の差 — CMOS ゲートの MOSFET が LEVEL 3(`74HCU04`: 出力 4.67V 対 4.32V。MOSFET の LEVEL 2 以上は未対応)、PSpice のダイオードの `Vfwd=`/`Ron=`/`Roff=`(`TC4421` の OUT)。②簡易回路の問題 — 差動入力に同じ電圧をつなぐオペアンプ(出力が入力のわずかな差で決まる)、比較器の同値判定(`IR21xx` のゲートドライバ。`C … IC=-5V` と `IF(V(1)>V(2),…)` のにらみ合い)。これらは互いの解がどちらも正しい可能性がある。

### ngspice との検算の続き(2026-10-19)
検算の回路を直したら、一致が増えた(150個中、最初 42 → 62、ズレ 35 → 12、ngspice が読めない 74)。直したのは回路と、自前の不具合2つ。
- 回路: ①非反転入力らしい端子(`IN+`・`INP`・`IP` など)は 2.1V、ほかの入力は 2V(同じ電圧だと、オペアンプの出力が入力のごくわずかな差で決まって比べられない)。②ngspice へ渡す節点名で `+` と `-` がどちらも `_` になり、`IN+` と `IN-` が同じ節点になっていた(`p`/`m` にした)。
- **不具合: VSWITCH の `VON < VOFF`**(`spice_in.rs` と `circuit.rs`): 制御電圧が低いと閉じる向きの逆スイッチを、普通の向きとして読んでいた。`SwitchModel.vw` を負にして「逆向き」を表す(`eval_widened`)。`IR21xx` などのゲートドライバの出力が合うようになった。
- **不具合: パラメータつき `.model` の名前の衝突**(`spice_flat.rs expand`): 1つのサブサーキットを値を変えて2回呼ぶ(`VSW_BASIC_GEN` を RON=2 と RON=0.857)と、中の `.model _VSWITCH …{RON}…` が同じ名前で2枚できて、片方の値が使われていた。2つ目以降の呼び出しでは、パラメータつきの `.model` の名前に経路をつけて区別する(1つ目は元の名前のまま: 内側のサブサーキットがその外側のモデルを使うことがあるため)。`LM5112` が合った。
- 残るズレ12: MOSFET の LEVEL 3(`74HCU04`)、PSpice のダイオード(`TC4421`)、にらみ合いの比較器(`UCC28C42`、`LM5122`)など。
- **不具合: 節点名の大文字小文字**(`circuit.rs Circuit::node`): SPICE の節点名は大文字小文字を区別しない(`VCC` と `vcc` は同じ)のに、自前は別々の節点にしていた(`TSZ181` で `VAL_VDEP_SINK` と `val_vdep_sink` が別の節点になり、片方が宙に浮いて 632V、もう片方が 0V)。名前の表を小文字の鍵にした(表示は最初の綴り)。`DcResult::voltage` も大文字小文字を無視して探す。一致 62 → 64(150個中)、ズレ 10。

### 過渡の検算と LTspice の理想ダイオード(2026-10-19)
- `tests_vendor_bulk.rs vendor_xcheck_tran`(環境変数は `vendor_xcheck` と同じ。`SPICE_XCHECK_ONLY` で1つ指定すると波形を14点ずつ並べる): 最初の入力ピンに 0→レベルのパルス(1µs 遅れ、幅 6µs、周期 14µs)を入れて 20µs の過渡を ngspice と比べる。ngspice は `wrdata` で書き出す。ピンごとに、差の最大が「波形の振れ幅の 3%(最小 50mV)+20mV」以内なら一致。エッジが数十 ns ずれるのは別の答えではないので、±60ns の範囲でいちばん近い点と比べる。結果(98個): 一致 33、ズレ 14、ngspice が読めない 51(60個では一致 26/ズレ 8)。
- **LTspice の区分線形ダイオード** `D(Ron= Roff= Vfwd= [epsilon= Vrev= Rrev= revepsilon=])` (`DiodeModel.ideal`、`spice_in.rs` の `'D'` の枝): `Vfwd` か `Ron` があるカードは、指数関数の接合ではなく、B 電流源 `I = V/Roff + soft(V−Vfwd)/Ron − soft(−V−Vrev)/Rrev`(`soft(x) = (x+√(x²+ε²))/2`、ε は最小 1mV)にする。これまで `Vfwd=25 Ron=0.65` の TVS ダイオードも 0.7V で導通する普通のダイオードとして読んでいた。公開モデル集に 169 枚ある。ピークの定格(`Vpk` など)は見ない。結果: 検算の一致 64 → 65(`TC4421`)。
- 過渡の検算の見直し: 入力のエッジ(立ち上がり 1.0〜1.25µs、立ち下がり 7.02〜7.27µs、次の立ち上がり 15.0〜15.25µs)の直後に出るスパイクは、エンジンがたまたま取った刻みで高さが変わる(`Lm386` の `g1` で 1.485V 対 1.609V。波形のほかの部分は 1mV 以内で一致)ので比べない。そうすると 98 個中、一致 45、ズレ 2(`74HCU04`: MOSFET の LEVEL 3、`INA250A1`: DC も合わない暴走状態)、ngspice が読めない 51。比べられたものの 96% が一致。

## 解析の速度(2026-10-19)
`vendor_xcheck_tran` が自前と ngspice の所要時間を出す(`TIME` 行、最後に `TIMES` の中央値・最大・合計)。`tran_profile`(`SPICE_MODEL_DIR` `SPICE_DUMP_FILE` `SPICE_DUMP_TOP` で1つ指定)は、1つのモデルの過渡の点数と、行列の因数分解を使い回せた回数を出す。
- **測った結果(公開モデルの過渡 20µs、47個)**: 中央値は自前 0.065 秒、ngspice 0.061 秒(プロセスの起動を含む)でほぼ同じ。合計は自前 39 秒、ngspice 5 秒。遅いのは大きなマクロモデル(行列が 130〜300)と、刻みが小さくなってしまう少数のモデル。
- **時間の内訳**(`OPA197`、行列 132): ニュートン法の1反復あたり約 200µs のうち、行列の組み立て 約 30%、**因数分解 約 65%**、解く部分 2%。因数分解は毎回「ピボットの選び直し+疎な消去+行の作り直し」を最初からやっていた(係数は 867 個、因数分解後 2935 個と、うめ込みは小さい。時間は構造の探索と配列の確保に使われていた)。
- **直し1: 因数分解の使い回し**(`dc/linalg.rs` の `Symbolic`・`refactor`、`dc/newton.rs` の `solve_reusing`): 前の因数分解のピボットの順と、うめ込みの模様(`Symbolic::plan` が模様だけから作る。値がたまたま 0 でも模様には入れる)を覚え、同じ模様の行列は、探索なしで数値だけ計算し直す。使い回した答えは残差(`residual_ok`、相対 1e-8)で確かめ、悪ければ最初から因数分解してやり直す。**行列が 60 より小さいときは使わない**(小さな回路のマルチバイブレータの反転は行列がほぼ特異で、そこでのニュートン法の動きがピボットの選び方に左右され、使い回すと 2 つのテストが収束しなくなった)。
- **直し2: 過渡の通常のニュートン法の反復上限を 40 に**(`newton_with` の `plain`): 収束しない刻みの試行が全反復の 75% を占めていた(`OPA197`: 21566 反復のうち、解けた回の合計は約 5200)。線探索つきの試行は 150 のまま(上限を全部の試行に 40 としたら、マルチバイブレータの反転で収束しなくなった)。
- **効果**(過渡 20µs): `OPA197` 4.4 → 1.6 秒、`LM5106_TRANS` 10.2 → 5.8 秒、`TSV912` 1.8 → 1.35 秒。検算の一致数は変わらない(DC 65、過渡 45)。
- 残る遅さ: `74HCU04`(MOSFET の LEVEL 3、31 万点)は刻みが最小に張りつく。`LM5106_TRANS` は刻みの数が 1.3 万点で、因数分解の次は行列の組み立てと刻みの棄却(1810 回)が重い。

### 刻みの決め方を調べた(2026-10-19)
`tran_profile` が、刻みを決めたのは何か(`STEPS by limit`: 1 折れ点・2 棄却のあと・3 後退オイラーの再試行・4 最大刻みのまま・0 誤差の見積り)と、誤差の見積りで最悪だった容量・コイル(`ERROR CONTROL worst element`)と、刻みの分布を出す(`tran.rs` の `STATS`/`WORST` はテスト用)。
- **`LM5106_TRANS`(刻み 1.3 万点)**: ほぼすべての刻みが誤差の見積りで決まり、最悪は `LT_EN_*` — 自前の伝送線路(`T_EN`、Z0=1k、TD=4ns)のはしごのコイルだった。損失のないはしごは、エッジで高い周波数のモードが励起されると**いつまでも減衰せずに鳴り続け**、刻みが 0.1ns に張りつく。直し: はしごの**各コイルに 20·Z0 の抵抗を並列**に入れる(`ladder`)。DC と信号の帯域には影響せず(コイルは DC で短絡、信号の帯域では 20·Z0 に流れる電流は無視できる)、最高のモードだけが数 ns で減衰する。刻み 12928 → 5620 点、5.8 → 2.6 秒。
- **`OPA197`・`TSV912`**: 最悪はモデル内の容量(`C12` など)で、本当に速く動く節点。刻みは正当。
- **`74HCU04`(31 万点)**: 最悪は `L2.Xu04`(6.87nH の接地のボンディングワイヤ)。1.5pF との共振(1.6GHz、周期 0.64ns)が減衰せず、誤差の許容 1e-4 だと 1 周期 26 点で追う。ngspice の許容(reltol 1e-3 × trtol 7 ≒ 7e-3)は自前の 70 倍ゆるい。
- **許容 `lte_reltol` を 1e-3 にすると**: 点数は 25〜35% 減るが(`LM5106` 5620 → 3972、`OPA197` 1561 → 1045)、アプリのテスト(`buck_sample_regulates…` と保存した ngspice の統計との比較 `transient_runs_still_match_the_stored_ngspice_statistics`)が落ち、検算の一致も 45 → 44 になる。1e-4 のままにする。許容を緩めて速くするのではなく、速さと正確さの釣り合いは今の値でとれている。

## 過渡の素朴なニュートン反復の打ち切り(40回)と、プラットフォームによる収束の差
- `dc/newton.rs` の素朴な反復(`plain`)は、過渡の刻みでは `PLAIN_CAP`(40)回で打ち切る(刻みを小さくして再試行したほうが速いため)。ただし打ち切るのは、**大きい系(未知数 `REFACTOR_MIN_SIZE`=60 以上)の、再試行の最初の `CAPPED_TRIES`(6)回だけ**(`Dyn.capped`)。小さい系は、これまで通り上限(150)まで粘る。
- 経緯: d9a931a で、全部の系・全部の再試行に 40 回の打ち切りを入れたところ、Linux と macOS の CI で `logic_blinker`(未知数 40、双安定のロジック。Windows の過渡では最大 39 回で、40 回に1回足りない所まで来ていた)が `NoConvergence` で落ちた(Windows は通る)。最初は「最初の6回だけ打ち切る」に直したが、それでも Linux・macOS は落ちた(打ち切りの対象が小さい系にも及んでいたため)。ピボットの再利用(`solve_reusing`)は 60 未満の系では動かないので無関係。
- 手元(Windows)の調べ: 刻みの更新(`powf`)に相対誤差 -1e-15 を足すと同じ失敗が再現する(Linux の `libm` との最後の桁の違いの代わり)。exp/ln/pow/tanh の最後の桁を入力ごとにばらつかせる実験(40通りの種)では、この回路は落ちなかった。Linux だけの失敗を Windows で完全には再現できていないので、小さい系を打ち切りの対象から外して、d9a931a より前の挙動(Linux で通っていた)に戻した。
- マルチバイブレータの弱点(直した): `tests_blinker` の無安定マルチバイブレータ(特に `symmetric_circuit_and_zero_state_start_also_run` の `uic=true`)は、数値の揺らぎで `NoConvergence` になることがあった。原因は、フリップの瞬間に、コンデンサ電圧を固定した代数方程式が古い状態の近くに解を持たない(折り返し点)こと。`tran.rs` の対策は「4回失敗したら最大刻み(`tstep`)を1回だけ試す」で、その1回で着地できないと、元の刻みと最大刻みの間の大きさは誰も試さず、刻みを 1e-14 まで縮めても収束しなかった。直し: 最大刻みから 1/16 ずつ、失敗中の刻みの 64 倍まで段階的に試してから元の刻みに戻る(`h_first`)。試行の上限は 12 → 16 回。揺らぎを入れた実験(エミュレートした `libm` の誤差を入力ごとにばらつかせた40通りの種のうち 1〜16、ステップ幅の更新の相対誤差 ±2e-16〜3e-15)で、直す前は失敗していた種・揺らぎ(3, 7, 9、-3e-16、-1e-15)を含め、`tests_blinker` の5件と sim-core の全テストが通った。

## 性能の測定(並列化の前に)
- `tests_bench.rs`(`cargo test -p sim-core bench_ -- --ignored --nocapture --test-threads=1`)が、独立な計算を繰り返す解析の時間を測る。製品のコードに並列処理は無い(8論理コアのPCで測定)。
- 測定(最適化ビルド): AC 3200点 0.03 秒、ノイズ 600点 0.001 秒、モンテカルロ 1000回(DC)0.46 秒・300回(カットオフ)0.016 秒、温度スイープ 660点 0.03 秒、小さな回路(5〜7部品)の故障のスキャン 0.003〜0.025 秒 — どれも速く、並列化の効果は小さい。過渡は LED の点滅(マルチバイブレータ 5 秒)5.5 秒、555 の点滅 0.5 秒、降圧コンバータ 1.1 秒。
- 遅かったのは、総合デモ(41部品)の故障のスキャン: 118件で **361 秒**(1件 1.5〜3.8 秒)。故障のたびに、正常な回路の動作点(この回路は 1.5 秒かかる)を解き直していたため。`simulate_broken_with(c, b, &baseline)` で正常側を1回にして **175 秒**(約2倍、報告は `simulate_broken` と同じ=テスト `a_scan_with_the_healthy_circuit_solved_once_gives_the_same_reports`)。アプリのスキャン(`schematic/fault.rs`)は使っている。
- 残りの 1.5 秒/件は、壊れた回路自体の動作点。試したが採用しなかった案: 正常な解のノード電圧を初期値にしたニュートン反復(ウォームスタート)。ミリ秒で解ける故障が約6割あったが、(1) 正常な解は「スイッチの幅を広げた」戦略で見つけた点で、1回の反復では着地できず(同じ回路を自分の解から解いても失敗)、広い順に数段の梯子を使う必要があった、(2) 従来は「解なし」と報告していた故障(`V1` を 0.5 倍)で解を返すなど、結果の意味が変わる、の2点で、判断が要るため保留。並列化(故障ごとに独立)は、これとは別に効く。
- 並列化: `par_map`(`par.rs`、`std::thread::scope`。結果は入力の順で、スレッドごとに `SimOptions` を引き継ぐ。コアが1つか項目が1つなら呼び出したスレッドで実行)を、アプリの「全部の部品を壊す」スキャン(`schematic/fault.rs` の `run_fault_scan`)に使った。壊れた回路は互いに独立なので、全コアで解く(部品の故障も、ICとトランジスタのピン断線も)。総合デモ(41部品)のスキャン全体が、アプリ上で約 53 秒(起動と画面保存を含む、8コア)。以前は部品の故障だけで 361 秒。`par_map_progress` は終わった数(`done`)を数え、`cancel` が立つと新しい項目を始めない(始めたものは終わるまで続く。始めなかった項目は `None`)。アプリのスキャンは別スレッドで走り(`start_fault_scan`)、解析タブの「全部品を順に壊す」に進捗バー(`done / total`、最初の1.5秒ほどは正常な回路を計算中の表示)と「中止」ボタンが出る。`poll_fault_scan` が毎フレーム結果を受け取り、実行中は 100 ms ごとに再描画する。コマンドラインとテストは、呼び出したスレッドで最後まで走る `run_fault_scan`(`--fault scan`)。撮影用に `--fault scan-bg`。
