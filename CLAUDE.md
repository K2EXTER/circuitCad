# circuitCad — コードマップ

目的: 回路図 → SPICEシミュレーション → 基板 → ガーバー出力を1本のソフトで。初学者向け(日本語UI、ホバー説明、ヒント、サンプル)。
機能を足すときは「GUI表示 + サンプル + 説明文」までセットで作る。UIの文言は日本語、コード・コメントは英語。

作業前に全ファイルを読まず、下の表で目的のファイルを特定してから読むこと。

```
cargo test --workspace        # 全テスト (CIと同じ)
cargo clippy --workspace --all-targets   # CIはエラーを禁止 (警告は可)
cargo run -p app              # 起動 (引数で .schjson / .pcbjson を開ける: main.rs apply_args)
```

## クレート (依存: app → pcb-core, sch-core, sim-core, part-lib)

| クレート | 役割 |
|---|---|
| `sch-core` | 回路図モデル (`Schematic`/`Component`/`Wire`/`Label`/`Kind`)、ネット抽出 `netlist.rs` (`extract`, `net_at`, `erc` — 指摘は `Problem{message,at,hint,severity}`、hintに原因と直し方を書く)、SPICE出力 `spice.rs` |
| `sim-core` | GUI非依存のシミュレータ。`from_schematic.rs` で回路図→`Circuit` |
| `pcb-core` | 基板モデル `model.rs`、DRC、ラッツネスト、自動配線、Gerber/Excellon/BOM出力 |
| `part-lib` | 部品JSON (`*.part.json`: 記号+パッケージ+SPICEモデル)。`examples/` が同梱部品。`kicad.rs` はKiCadの `.kicad_sym`(ピン)と `.kicad_mod`(パッド・穴・長穴)を `PartDef` に変換(`parse_symbols`/`parse_footprint`/`to_part`。フットプリント無しは仮の1列パッド)。GUIは `schematic/kicad_import.rs`(ライブラリの「KiCadから取り込み…」) |
| `app` | egui のGUI。回路図エディタ・基板エディタ・波形表示 |

## sim-core (`crates/sim-core/src`)
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
- `spec.rs` スペック(合否の表): `Spec{kind,target,at,lo,hi}` を `evaluate`(`SpecInputs{dc,tran,ac}` の結果から値を読み、範囲と比べて `SpecEval{value,pass,note}`)。種類 `SpecKind`(DC電圧/電流、過渡の平均・リップル(後半)・最大・最小、AC の-3dB点・ある周波数のゲイン・位相余裕)。GUIは `schematic/spec_table.rs`(解析タブの「スペック」、`--spec <種類番号> <対象> <下限|-> <上限|-> [周波数]`)。いまは保存されない(セッション内)
- `power.rs` 平均電力 `mean_power`(表示範囲での v*i の時間平均)と効率 `efficiency`。GUIは `schematic/power_ui.rs`(過渡の波形パネルの「電力と効率」、`--tran … --power <入力V> <入力I> <出力V> <出力I>`)
- `tests_*.rs` は機能別 (555, logic, ac, noise, thermal ...)

## pcb-core (`crates/pcb-core/src`)
- `model.rs` `Board`/`Track`/`Pad`/`Via`/`Zone`/`Rules` ほか全データ型。銅箔は `Side` (Top/Bottom/`Inner(n)`)で表し、`Board.inner_layers`(内層の数、0=2層)と `copper_layers()` が層の一覧(上から下)。トラックとゾーンだけが内層に置け、部品・パッド・シルクは `Side::outer()` で表裏のみ。スルーホールとビアは全層につながる(ブラインド/ベリードビアは未対応)。DRC・ラッツネスト・自動配線(`autoroute.rs` は N 層格子)・IR ドロップ・ガーバー(`Copper,L2,Inr`、`export.rs` が内層を `.G1`… で出す)・発注前チェックが対応。GUIは `board_editor/layers.rs`(層数 2/4/6/8、減らすと内層の銅があれば断る。`layer_overlay` は画面右上の折りたたみ式の層パネル=作業層の選択と層ごとの表示/非表示、Lキーで順送り)。`Hole`=めっきなしの穴(長穴 `slot`/`slot_v`、部品付きは `refdes`。部品の穴は `parts.rs` の `set_part` が出し入れして部品に追従)、`Pad.slot`=めっき長穴。`footprints.rs` の `Footprint.holes`(`FpHole`)と `package.rs` の `Explicit{pads,holes}`(`PadSpec.slot`/`HoleSpec`)が元。長穴は `excellon.rs` が `G85` で出し、`gerber_view.rs` の `parse_excellon_slots` が読み戻す。部品作成フォームの「追加のパッドと穴」(`part_form.rs` の `parse_extras`、脚/穴/パッド)が入力口、見本は `USB_DEMO_TH`
- `drc.rs` `check`(全体)・`clearance`・`shorts`・`connectivity`(銅の接続グループ、ネット判定) — 自動配線もこれを使う
- `current.rs` 配線の太さと電流(IPC-2221: `width_for_current`/`current_for_width`、`Board.net_currents` との照合 `net_widths`、`widen_net`) / `drc.rs` の `silk_checks` はシルク注意、`decoupling_checks` はICの電源ピンのバイパスコンデンサ(無い/8mm超で警告、`MAX_DECOUPLING_DISTANCE`)、`acute_angle_checks` は90°未満の鋭角(酸のたまり。パッドやビアの上にある角は、銅で埋まるので除く)、`thermal_via_checks` は放熱パッド(`heat_pads`: 表面実装で 4mm² 以上、他のパッドの 2 倍超)の中のサーマルビアの数(2.5mm² に 1 個、最低 2 個)。同じネットのベタ(実際の銅の形で100mm²以上=`POUR_SPREAD_AREA`)が接していれば半分(`thermal_via_need`)、小さい/切れ切れなら半分にしない。`add_thermal_vias` が足りない分を格子状に置く(DRCが悪化するなら置かない、GUIは基板エディタの「サーマルビアを置く」)、`zone_checks` はベタの細いかけら(スリバー)と、どこにもつながらない銅の島(`zonefill` の実際の形で調べる)、`mask_dam_checks` は異なるネットのパッドの間のソルダーマスクの帯(0.1mm 未満で警告、`MIN_MASK_DAM`)、`missing_ground_fill` はGNDベタが無いときのヒント。どちらも発注前チェックで警告 / DRCの指摘は日本語で原因と直し方を書く
- `irdrop.rs` 配線の電圧降下(トラック・ビア・パッドを抵抗の網にして、入口のパッド(コネクタ優先)から各パッドまでの抵抗を解く。`net_drop`/`net_drops`、`Board.net_currents` の電流で mV を出す。ベタは「ハブ」に抵抗でつなぐ近似、各パッドが電流の全部を引く最悪見積り。節点が 900 を超えると計算しない)。GUIは `board_editor/width.rs` の「配線の電圧降下」(`--ir <net> <A>`)
- `body.rs` 部品の本体 `BodySpec`(四角/円+向きの印 点/棒/＋、部品JSONの `body`)→ シルク線 `body_lines`(パッドに重なる所は切る)。`PartDef::footprint` が `Footprint.silk` にして、`parts.rs` の `set_part` がシルクとして置く(`footprints.rs` の `outline` は内蔵フットプリントの本体)。部品作成フォームは `part_form.rs` の「本体の外形」「放熱パッド」(放熱パッドは中心の追加パッドになる)と、形のプレビュー `footprint_preview`(パッド・穴・シルク)
- `outline.rs` 曲線の外形(`round_corners`・`rounded_rect`・`ellipse`。外形は多角形のまま、曲線は細かい線分にする) / `model3d.rs` 3Dプレビュー用の部品の立体(`part_boxes`が部品ごとに複数の箱/円柱を返す。チップ+電極、ICの足、ヘッダのピン、円柱。種類は `part_kind`、高さは `part_style` でフットプリントから推測)。向きの印: ICの1番ピン側の白い点(`pin1_dot`、ピン名が「1」の部品)、ダイオード/LEDのカソード(ピン名 `K`)、電解コンデンサの−側の縞。高さは `Footprint.height`(部品JSONの `height`、`PartDef::footprint` が載せる)があればそれを使い(`part_height_on`)、無ければ名前から推測
- `zonefill.rs` ベタの銅を図形として求める(`zone_fill`: ゾーン ∩ 外形 − 他ネットの配線・パッド・ビア・取付穴・基板端のクリアランス。`i_overlay` で計算、曲線は誤差 `CURVE_TOLERANCE` 以下の短い線分)。エディタの描画(`board_editor/draw.rs`、`geometry_key` でキャッシュ)とガーバー(`gerber.rs` の `zones`)が同じ結果を使う / `outline.rs` の曲線は `ARC_TOLERANCE` で半径に応じて細かくする / `gerber_view.rs` の `Preview` は 2×2 で描いて平均する(縁がなめらか)
- `diffpair.rs` 差動ペア(`DiffPair{p,n,gap,max_skew}` は `Board.diff_pairs` に保存)。`pair_report`(長さ・スキュー・並走の長さ・間隔)、`check_pairs`(片方だけ配線/長さの差/間隔のずれ/並走が少ない。DRC実行と発注前チェックに出る)、`tune_length`(短いほうに蛇行を足す。DRCが悪化するなら何もしない)、`suggest_pairs`(`USB_DP/USB_DM` `D+/D-` などネット名から探す)。ペア配線は `autoroute::route_pair`(＋側を引いてから、－側は `RouteOptions.follow` で＋側の隣のセルが安くなる)。GUIは `board_editor/pairs.rs`(配線パネルの「差動ペア」)、「全部」自動配線もペアを先に引く。
- `shove.rs` 押しのけ配線(`add_tracks`=新しい配線を足して邪魔な別ネットの配線を脇へ寄せる、`move_track_shoving`=配線を1本動かす、`rip_up_net`=ネットの配線を全部消す。パッド/ビアに付いた端は動かさず、両端が固定の配線は動かせない、最大 `MAX_SHOVE`、DRC違反が減らず接続が切れるなら採用せず普通に足す)。GUIは `board_editor/shoving.rs`(経路プレビュー `route_preview`=橙の線で押しのけられる配線の今の位置(細い線)と動く先(太い線)をカーソルに合わせて出す/赤=違反。基板は変えない。1回の計算が60ms(`PREVIEW_BUDGET`)を超える基板では、結果を250ms保って間引く=約450本の配線(release)から。小さい基板は毎回計算、選択ツールで配線をドラッグ、選択パネルの「このネットの配線を全部消す」。ツール設定の「押しのけ配線」で切替)
- `ratsnest.rs` 未配線(`Airwire`) / `autoroute.rs` 格子A*の自動配線 (`autoroute`, `route_airwire`、DRCで検証して採用)
- `netlist.rs` 回路図→基板の取込 / `parts.rs` `place_part` / `footprints.rs`・`package.rs` フットプリント
- `fab.rs` 発注先プリセット(`PRESETS`)と発注前チェック(`preflight`) / `gerber_view.rs` 書き出したガーバー・ドリルを読み戻して絵にする(`Preview`)
- `export.rs` 製造一式 (zip) / `gerber.rs` / `excellon.rs` / `assembly.rs` BOM・部品配置CSV / `strokefont.rs` シルク文字

## app (`crates/app/src`)
開発用の検算 `xcheck.rs` / `xcheck_tran.rs`: `circuitCad --xcheck <file|dir>` が自前のDC動作点(電圧と電流)を、`--xcheck <file|dir> --tran` が過渡波形を(電源が時間変化しない回路は `xcheck_tran.rs` の `own_dynamics` の表の長さで。DC動作点が無くゼロ状態から始まった回路は ngspice にも `uic` を付ける。時間で重みを付けた規格化RMS誤差1%未満か、エッジ主体の波形はエッジ時刻のずれが周期の2%以内なら一致)ngspice と比べて `xcheck_report.txt` / `xcheck_tran_report.txt` に書く(ウィンドウは開かない。ngspice の場所は `--ngspice` か環境変数 `NGSPICE`、既定 `F:\Tools
gspice\Spice64in
gspice_con.exe`)。ngspice は reltol=1e-6 で解かせる(既定の1e-3だと ngspice 自身の電流が食い違う)。差が出たら gmin=1e-15 でも解き直し、それで一致すれば「gmin依存」。スイッチ(`S` と `.model SW`)は ngspice に `vw` が無いので、方言変換が自前と同じ滑らかな導通の B 電流源 `I=V(a,b)*exp(…)` に置き換える(これで降圧コンバータ・NE555・論理ICが ngspice と合う)。既知の差: 電源投入直後のフリップフロップなど双安定の回路は、自前が釣り合い点にとどまり ngspice が片側に倒れるので合わない(`logic_blinker`、`variable_supply`の浮いた2次側)。ngspice が止まる回路もある(トランス、`led_blinker`)。方言変換は `sch-core/src/ngspice.rs`(`to_spice_for(.., Dialect::Ngspice)`: if/step/limit/log/temp)。`--ourstep` `--ngstep` `--lte` で片方の刻みや許容だけ変えて、どちらのエンジンが粗い刻みに弱いかを見られる。

電源記号は値で形が変わる(`symbols.rs` の `source_symbol`: 直流=±の円、SIN=正弦波、PULSE=方形波、PWL/NOISE=折れ線)。`plot.rs` の `scale_bar` は横軸・縦軸(グループごと)の範囲入力・全体・自動(1行、過渡・ACの波形パネルの上に出る。拡大縮小は軸の数字の上のホイール)。軸の数字の上でホイール=その軸だけ拡大縮小(`draw_group`)。`PlotState.cursors` が測定カーソルA/B(クリックで置く、`cursor_report` が差・周期を出す)。`schematic/challenge.rs` は「作ってみよう」(目標だけ示して自分で部品を置き・配線し、「判定する」でシミュレーションして判定する。1つの模範解答とは比べない)。チャレンジは `CHALLENGES` のデータ(開始シート `parts0/parts1/grounds*/start_values`、答え=サンプルの `drop/values/labels/rename`、判定 `checks`: `Erc`/`Named`(出力ネット `VOUT`)/`Voltage`(負荷・入力を足して測る、値の範囲ごとの助言 `explain`)/`Stay`(入力を2点に振る)/`Current`/`Gain`(AC)/`Ratings`)。足すときは `CHALLENGES` に1件とテスト(答えが通る)。いまは zener・divider・led・rc_lowpass・emitter_follower・opamp_inv・series_regulator・regulator_78xx と、最終課題 draw_psu(lesson は protection、学習パスの `Kind::Build` の項目から id で開く。部品の個数を調べる `Check::Has` あり、答えは `power_supply.schjson`)。`--challenge <id> [0|1] [--judge]`、通過は進み具合に `<id>+build`、レッスン窓の「チャレンジを始める」と学習パスの「作ってみよう」から開く。`learn.rs` は学習パス(`PATHS` に段階と項目を書く。レッスンは `id` で指定、準備中は `planned`)と完了の記録(`%APPDATA%\circuitCad\progress.json`)と「学習パス」窓(`--learn`)。設計は `docs/learning-path.md`。`help.rs` は「操作ガイド」窓(ヘルプメニュー・F1・各パネルの「操作」ボタン、`--guide`。操作の説明は小さい文字で各所に書かず、ここの `SCHEMATIC`/`WAVEFORM`/`BOARD`/`VIEW3D` に足す)。`project.rs` プロジェクト(`.ccproj`=回路図+基板を1つのJSONに。`version` つき。ファイル→プロジェクトを開く/保存、起動引数に `.ccproj`。`SchEditor::adopt`/`BoardEditor::adopt` で両方を置き換える。個別の `.schjson`/`.pcbjson` の保存も残してある)。未保存の変更は `SchEditor::dirty`/`BoardEditor::dirty`(`rev` と `saved_rev`: 回路図は `commit_history`・`reset_history`、基板は `checkpoint`・`adopt` で進む。保存で `mark_saved`)で調べ、`main.rs` の `confirm_unsaved`(プロジェクトを開く・新規・開く・ウィンドウを閉じる。`--screenshot` の自動終了では聞かない)が rfd のはい/いいえ/キャンセルで確認する。基板が回路図とずれているかは `BoardEditor::schematic_diff`(取込と同じ `netlist_of` で、追加/削除/値・形/つなぎを数える。回路図と基板の `rev` でキャッシュ)で、基板タブの ● と基板パネルの橙の文に出る。`main.rs` ウィンドウ・メニュー・`TIPS`(豆知識)・引数処理・統合テスト。「ファイル → 画面をPNGで保存」は eframe の Screenshot(`finish_screenshot`)で、起動引数 `--screenshot file.png` は数フレーム後に撮って終了(画面の確認・文書づくり用)。`plot.rs` 波形描画、`part_form.rs` 部品作成フォーム(上に部品の種類 `PartType`=IC/トランジスタ/ダイオード/チップ/コネクタを選ぶバー `kind_bar`→`apply_type` が例を入れる、タブ `FormTab`=基本/ピン/基板の形/シミュレーション/定格、右に記号 `symbol_preview`(`schematic/glyphs.rs` の `symbol_lines`)と基板の形 `footprint_preview` と指摘と保存を常に出す `preview_pane`。起動引数 `--new-part <ic|transistor|diode|chip|connector> --form-tab <basic|pins|package|sim|ratings>`。本体の高さと外形は「基板の形」タブ。SPICEの`.model`/`.subckt`を貼る/ファイルから読むと `guess_spice`→`autofill_from_spice` が種類・名前・素子記号・ピン表・ノード順を自動入力)、`part-lib/src/passive.rs`(チップ部品のテンプレート `PassiveTemplate`=値+ESR/ESL/DCR/並列容量/DCバイアス低下→`.subckt`、`fit_cap`=インピーダンス表から ESR・ESL を最小二乗で求める、`esl_from_srf`/`cp_from_srf`。部品JSONの `glyph` で記号をコンデンサ/抵抗/コイルの形に描く=`symbols.rs` の `draw_custom`。フォームは `passive_ui`、標準フットプリント `fp_preset` も選べる。見本 `C_100n_0603` `C_10u_0805` `L_10u_0805` `R_10k_0603` は `write_chip_examples` で生成)、`schematic/library.rs` の `LibPrefs`(お気に入り★と最近使った部品。ライブラリのフォルダの `favorites.json`。一覧は `panel.rs` でカテゴリごとに折りたたみ、検索中は開く)、`source_form.rs` 電源値フォーム。

エディタは1つの型を複数ファイルの `impl` に分割している (子モジュールから親の非公開フィールドが見える)。型・フィールドは各 `mod.rs`。

### `schematic/` (`SchEditor`)
| ファイル | 内容 |
|---|---|
| `mod.rs` | 型定義(`Tool`,`Sel`,`DcView`,`TranView`...)、`SchEditor`のフィールド、`volt_color`,`eng`,`kind_tip` |
| `canvas.rs` | 描画エリアの流れ(`canvas`: パン・ズーム、キー、描画→操作の順) / `draw.rs` シートの描画(配線・部品・電圧/電流オーバーレイ・ERCの丸) / `interact.rs` マウス操作(範囲選択・ドラッグ・ツールのクリック) / `symbols.rs` 記号の図形・座標変換・重なり判定 |
| `panel.rs` | 左パネル。「作図」タブ(ツール・部品一覧・選択部品のプロパティと値スライダー・ERC・部品作成フォーム)と「解析」タブの切替(`PanelTab`、レッスンの手順に合わせて自動で切り替わる)。解析タブは `ANALYSIS_SECTIONS` の折りたたみ(基本3つだけ最初から開く、レッスン中は全部開く、「すべて開く/基本だけ」ボタン)。解析パネルを足すときはここに1行足す / `analysis_panel.rs` 「解析」タブの中身(DC・過渡・AC・スイープ・ノイズ・温度とばらつき・結果一覧) / `hint.rs` 画面下のヒント文 |
| `fault.rs` 「故障シミュレーション」パネル(部品全体かピン単位で壊す/全部品と、IC・トランジスタの全ピン断線を順に壊して影響順に並べる。`--fault` で起動引数からも。「比べる」でDC/過渡/ACを切替(`FaultDomain`)、波形系は `WaveMode::Fault` の波形パネルに重ね描き) / `wizard.rs` 「学ぶ → 値の決め方」窓(`CALCS`、選択中の部品へ値を反映) / `param.rs` パラメータスイープのパネルと重ね描き(`--param` で起動引数からも実行可) / `settings.rs` 「設定 → シミュレーション設定」ウィンドウ / `panel.rs` の `part_popup_window` は部品の右クリックで出る設定小窓(選択部品プロパティと同じ中身) |
| 波形の「前」との比較は `Ghost`(`mod.rs`)。過渡・ACの「いまの波形を残す」で薄く残し、再計算(自動も)しても保つ。描画は `plot::ghost_color` |
| `noise.rs` の `probe_node` は「回路図で選ぶ」(ピン/配線=電圧、部品の本体かShift+ピン=電流)。`wave.rs` の `live_update` は収束しない値では直前の良い結果を残す。値スライダーの幅は `slider_span`(細かい/ふつう/広い) |
| `quick.rs` クイック追加(Aキー・「クイック追加」ボタン・`--quick <文字>`。`quick_matches` が基本部品+ライブラリを名前/別名/カテゴリ/パッケージ/ピン/説明で検索、全単語一致、お気に入り→最近使った順に優先。↑↓Enter Esc) / `edit.rs` 選択・当たり判定・削除・配線追加 / `history.rs` Undo・Redo / `file.rs` 保存・読込 / `library.rs` ライブラリ |
| `lessons.rs` 「学ぶ」メニューのレッスン(サンプル回路埋め込み+手順、各手順は状態で自動チェック。足すときは `LESSONS` に追加) / `variation.rs` 温度スイープ・ばらつき解析(モンテカルロ)の実行と表示 / `dc.rs` `run_dc`・`sim_circuit`(温度適用済みの回路) / `tran.rs` / `ac.rs` / `sweep.rs` / `noise.rs` 各解析の実行と波形パネル / `wave.rs` 表示中の解析切替・自動再計算 `live_update` |

### `board_editor/` (`BoardEditor`)
| ファイル | 内容 |
|---|---|
| `mod.rs` | 型定義(`Tool`,`Item`,`RouteMode`)、`BoardEditor`のフィールド、`snap`等の補助 |
| `canvas.rs` 描画エリアの流れ・キー操作 / `interact.rs` 範囲選択とポインタ操作 / `draw.rs` 描画(銅、ゾーン、ハイライト) / `panel.rs` 左パネル・選択パネル・ヒント |
| `select.rs` 当たり判定・範囲選択・移動・削除 / `route.rs` 配線の折れ方 / `tools.rs` クリック時のツール動作 |
| `view3d.rs` 「3Dで見る」窓(製造ファイルから作った絵を外形に貼り、部品の立体を描く小さなソフトウェアレンダラ。`Cam` が射影、`raster3d.rs` が深度バッファ付きの三角形描画で、製造ファイルの絵はその絵の枠(`Preview.frame`)で貼る。部品の選択は、ピクセルごとの部品番号の表で行う。`net_glow` は選んだネットの銅を三角形にして板の少し上に描く(基板エディタで配線/パッド/ビアを選ぶとそのネットが光る)、`focus_on` は選択部品への寄り、`snapshot` は輪郭なしの絵=PNG保存。`Quality::Draft` は回す・動かす・拡大の最中に半分の大きさで描く粗い絵、止めると `Fine`。`set_limits` はケース高さ(表/裏の上限mm)を超える部品を赤くして上限の枠を描く) / `width.rs` 「配線の太さ(電流から)」(ネットごとの電流→推奨幅→太くする) / `order.rs` 「発注の準備」ウィンドウ(発注先・チェック・プレビュー・書き出し・注文手順) / `file.rs` 保存・読込・製造出力・回路図取込 / `actions.rs` 自動配線・DRC実行 / `history.rs` / `view.rs` |

## 「〜を変えたい」早見表
- 新しい解析を足す → `sim-core/<名前>.rs` + `schematic/<名前>.rs` (実行+パネル) + `schematic/panel.rs` にボタン + `mod.rs` の `WaveMode` と `wave.rs` + `docs/examples/` にサンプル
- 新しい部品種別(基本素子) → `sch-core/lib.rs` の `Kind`、`symbols.rs`、`sim-core/from_schematic.rs`
- ライブラリ部品を足す → `part-lib/examples/*.part.json` (形式は `docs/part-format.md`)
- DRCルール → `pcb-core/drc.rs` / 設計ルール既定値 → `model.rs` の `Rules`
- ヒント文 → `schematic/hint.rs`, `board_editor/panel.rs` の `hint` / 豆知識 → `main.rs` の `TIPS`

トランス部品は `part-lib/examples/TRANS_*.part.json`(理想変圧器をBソースで作った`.subckt`: `Bs`=電圧源、`Bp`=`-n*I(Bs)`の電流源、巻線抵抗と励磁L)。出力コンデンサの置き換え比較は `buck_cout_electrolytic.schjson` / `buck_cout_ceramic.schjson`(`buck_converter` のC1だけを電解 `CE_100u_16V` / セラミック `C_10u_0805` に替えたもの、生成 `write_buck_cout_examples`。ESRが効いてリップルは電解の方が大きい)。インピーダンス比較は `cap_impedance.schjson`(1Ωの直列抵抗ごとに電解/セラミック/並列をつなぎ、AC解析で V(ZE/ZC/ZP)≒|Z|。低域は電解、1MHz以上はセラミックが低い。生成 `write_cap_impedance_example`)。電池は `part-lib/examples/{CR2032,AA_ALKALINE,LIION_18650}.part.json`(残量を蓄える巨大なC+残量で決まる電圧のBソース+内部抵抗。時間0だけ `Bpull` で残量を1に固定してDC解析でも満充電になる。サンプル `coin_cell_led.schjson`、レッスン10)。充電できる電池は `LIION_18650_CHG`(`LIION_18650` と同じ式で、残量20%から始まり、満充電で電流を受けつけない。Biの電流は充電なら残量が増える向き)。サンプル `li_ion_cell_charge.schjson`(TP4056+この電池、PROG 1.2kで1A、生成 `write_cell_charge_example`。定電流→定電圧→STDBY点灯まで約2.7時間)。TP5100版は `tp5100_cell_charge.schjson`(`tp5100_charger` の電池をこの電池に替えたもの、Rs 0.1Ωで1A、生成 `write_tp5100_cell_charge_example`)。降圧コンバータは `part-lib/examples/BUCK_DEMO.part.json`(150kHzのPWM: ランプ+積分型誤差アンプ+tanhのPWM比較器+スイッチ`S`。tanhを滑らかにしてデューティのジッタを抑え、時間0だけ誤差アンプを0に固定してDC解析を通す。サンプル `buck_converter.schjson`、レッスン11、`designcalc::buck_design`)。PNPハイサイドスイッチは `pnp_highside.schjson`(2N3906、生成 `write_pnp_example`、レッスン13、プルアップR2・負荷R3)、オペアンプ反転増幅は `opamp_inverting.schjson`(LM358、生成 `write_inverting_example`、レッスン14)。電源の学習パス用に `diode_iv.schjson`(レッスン15)・`zener_regulator.schjson`(16)・`emitter_follower.schjson`(17、生成は `write_diode_example`/`write_zener_example`/`write_follower_example`)。`Lesson.real` は「実機で確かめよう」(用意するもの・配線・測り方・期待値・安全)。MOSFETスイッチは `docs/examples/mosfet_switch.schjson`(2N7000、生成は `write_mosfet_example`、レッスン12、負荷はR_2512で定格内)。サンプルは `docs/examples/bridge_rectifier.schjson`(生成は `write_rectifier_example`)、レッスン9がこれを使う。

配布: 依存クレートを変えたら `python tools/gen_third_party_licenses.py` で `THIRD_PARTY_LICENSES.txt` を作り直す(CIが `--check` で古さを検査、リリースのzipに同梱)。`.github/workflows/release.yml` がタグ `v*` でWindows用のzipを作り、プレリリースとして公開する(macOS/Linuxは動作確認ができるまでmatrixでコメントアウト中)。日本語フォントは `main.rs` の `install_japanese_font` がOSごとの候補パスから読み、無ければ `assets/fonts/BIZUDGothic-Regular.ttf`(OFL、`include_bytes!`)を使う。

## ドキュメント・サンプル
`docs/learning-path.md` 学習パス(安定化電源)の設計 / `docs/board-editor.md` 基板の使い方 / `docs/library.md` 部品ライブラリ / `docs/part-format.md` 部品JSON形式 / `docs/examples/*.schjson` サンプル回路。

## 規約
- ボタン・入力欄には `on_hover_text` で説明を付ける。エラーメッセージは原因と対処が分かる日本語で。
- 回路図の変更はフレームごとの比較(`schematic/history.rs`)でUndoに入る。`self.sch` を直接変えれば自動で履歴になる。基板は変更前に `checkpoint()` を呼ぶ。
- 自動配線・自動再計算のように「結果を採用する前に検証する」処理は、失敗時に元の状態を変えないこと。
