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
| `part-lib` | 部品JSON (`*.part.json`: 記号+パッケージ+SPICEモデル)。`examples/` が同梱部品 |
| `app` | egui のGUI。回路図エディタ・基板エディタ・波形表示 |

## sim-core (`crates/sim-core/src`)
- `dc/` 動作点 (`dc_op`、`DcResult`/`ElementResult`、収束戦略 `Strategy` は `mod.rs`)。`build.rs` 回路→コンパイル済み形(`Ce`,`Compiled`) / `devices.rs` トランジスタ等の式 / `mna.rs` 行列の組み立てと各素子のスタンプ(`Ctx`,`Dyn`) / `newton.rs` ニュートン反復 / `linalg.rs` 疎LU分解
- `tran.rs` 過渡 / `ac.rs` AC / `noise.rs` ノイズ / `dcsweep.rs` DCスイープ / `spectrum.rs` FFT・統計
- `waveform.rs` 電源波形 (SIN/PULSE/PWL) / `value.rs` `parse_value`・`format_value`(SPICE数値) / `spice_in.rs` ライブラリ部品の `.model`/`.subckt` 読込
- `from_schematic.rs` 部品→素子の変換 (素子名 = 部品番号 refdes)。新しい部品種別はここ
- `temperature.rs` 温度 (`Circuit::at_temperature` がモデルを書き換える、`temp_sweep`) / `montecarlo.rs` 許容差ばらつき解析 (`monte_carlo`、感度つき。測る量は `McMeasure`: DC電圧 / ACの-3dBカットオフ / 過渡のステップ応答指標)
- `stepmetrics.rs` ステップ応答の指標 `step_metrics` (立ち上がり・行き過ぎ・整定、波形ビューアと `McMeasure::Tran` が使う)
- `options.rs` 計算精度の設定 `SimOptions`(reltol/vntol/abstol/gmin/反復上限、スレッドごと。GUIの「設定」メニューが毎フレーム `set_options`) / `diagnose.rs` 解析失敗時の原因と直し方(日本語ヒント)
- `measure.rs` 波形の自動測定 `wave_measure`(最小/最大/p-p/周波数/デューティ)。`stability_margins`(位相余裕・ゲイン余裕。反転なら180°ずらす)。ACの-3dB点は `montecarlo.rs` の `minus_3db`
- `designcalc.rs` 初学者向けの値の計算(LED抵抗・分圧・RCフィルタ・78xx・トランジスタスイッチ。結果+手順+注意点+故障パターンの `CalcOut`、E24丸め)。足すときは `schematic/wizard.rs` の `CALCS` にも追加(平滑コンデンサ `smoothing_cap`、降圧 `buck_design`、昇圧 `boost_design`、MOSFET `mosfet_switch`、オペアンプ `opamp_gain` あり)
- `fault.rs` 故障シミュレーション(部品を断線/ショート/逆向き/値10倍・1/10/DC電源の低下・上昇にしてDCを解き、元と比較。ピン単位の故障(断線/GNDショート/別ネットとブリッジ)は `break_pin` が `from_schematic_with_pin_fault` で回路図側のピン接続を差し替える(ICの内部モデルもそのまま使える)。`simulate_fault`、`FaultReport::summary` が日本語の説明。`simulate_fault_tran`/`simulate_fault_ac` は過渡波形・ゲインの正常/故障比較)
- `ratings.rs` 定格チェック `check_ratings`(DC結果から、抵抗・IC等の消費電力をフットプリントごとの目安(`package_power_limit`)と比較、LEDの電流・逆電圧、小形コンデンサの電圧。`RatingIssue{level,message,hint}`、DC解析パネルに🔥/⚠で表示。サンプルは定格超えにしない回帰テストあり)
- `paramsweep.rs` パラメータスイープ(部品1つの値を振って過渡/ACを繰り返す、`param_sweep`) 
- `tests_*.rs` は機能別 (555, logic, ac, noise, thermal ...)

## pcb-core (`crates/pcb-core/src`)
- `model.rs` `Board`/`Track`/`Pad`/`Via`/`Zone`/`Rules` ほか全データ型
- `drc.rs` `check`(全体)・`clearance`・`shorts`・`connectivity`(銅の接続グループ、ネット判定) — 自動配線もこれを使う
- `current.rs` 配線の太さと電流(IPC-2221: `width_for_current`/`current_for_width`、`Board.net_currents` との照合 `net_widths`、`widen_net`) / `drc.rs` の `silk_checks` はシルク注意、`decoupling_checks` はICの電源ピンのバイパスコンデンサ(無い/8mm超で警告、`MAX_DECOUPLING_DISTANCE`)、`acute_angle_checks` は90°未満の鋭角(酸のたまり)、`missing_ground_fill` はGNDベタが無いときのヒント。どちらも発注前チェックで警告 / DRCの指摘は日本語で原因と直し方を書く
- `outline.rs` 曲線の外形(`round_corners`・`rounded_rect`・`ellipse`。外形は多角形のまま、曲線は細かい線分にする) / `model3d.rs` 3Dプレビュー用の部品の立体(`part_boxes`が部品ごとに複数の箱/円柱を返す。チップ+電極、ICの足、ヘッダのピン、円柱。種類は `part_kind`、高さは `part_style` でフットプリントから推測)
- `ratsnest.rs` 未配線(`Airwire`) / `autoroute.rs` 格子A*の自動配線 (`autoroute`, `route_airwire`、DRCで検証して採用)
- `netlist.rs` 回路図→基板の取込 / `parts.rs` `place_part` / `footprints.rs`・`package.rs` フットプリント
- `fab.rs` 発注先プリセット(`PRESETS`)と発注前チェック(`preflight`) / `gerber_view.rs` 書き出したガーバー・ドリルを読み戻して絵にする(`Preview`)
- `export.rs` 製造一式 (zip) / `gerber.rs` / `excellon.rs` / `assembly.rs` BOM・部品配置CSV / `strokefont.rs` シルク文字

## app (`crates/app/src`)
電源記号は値で形が変わる(`symbols.rs` の `source_symbol`: 直流=±の円、SIN=正弦波、PULSE=方形波、PWL/NOISE=折れ線)。`plot.rs` の `scale_bar` は横軸・縦軸(グループごと)の範囲入力・全体・自動(1行、過渡・ACの波形パネルの上に出る。拡大縮小は軸の数字の上のホイール)。軸の数字の上でホイール=その軸だけ拡大縮小(`draw_group`)。`PlotState.cursors` が測定カーソルA/B(クリックで置く、`cursor_report` が差・周期を出す)。`help.rs` は「操作ガイド」窓(ヘルプメニュー・F1・各パネルの「操作」ボタン、`--guide`。操作の説明は小さい文字で各所に書かず、ここの `SCHEMATIC`/`WAVEFORM`/`BOARD`/`VIEW3D` に足す)。`main.rs` ウィンドウ・メニュー・`TIPS`(豆知識)・引数処理・統合テスト。「ファイル → 画面をPNGで保存」は eframe の Screenshot(`finish_screenshot`)で、起動引数 `--screenshot file.png` は数フレーム後に撮って終了(画面の確認・文書づくり用)。`plot.rs` 波形描画、`part_form.rs` 部品作成フォーム、`source_form.rs` 電源値フォーム。

エディタは1つの型を複数ファイルの `impl` に分割している (子モジュールから親の非公開フィールドが見える)。型・フィールドは各 `mod.rs`。

### `schematic/` (`SchEditor`)
| ファイル | 内容 |
|---|---|
| `mod.rs` | 型定義(`Tool`,`Sel`,`DcView`,`TranView`...)、`SchEditor`のフィールド、`volt_color`,`eng`,`kind_tip` |
| `canvas.rs` | 描画エリアの流れ(`canvas`: パン・ズーム、キー、描画→操作の順) / `draw.rs` シートの描画(配線・部品・電圧/電流オーバーレイ・ERCの丸) / `interact.rs` マウス操作(範囲選択・ドラッグ・ツールのクリック) / `symbols.rs` 記号の図形・座標変換・重なり判定 |
| `panel.rs` | 左パネル。「作図」タブ(ツール・部品一覧・選択部品のプロパティと値スライダー・ERC・部品作成フォーム)と「解析」タブの切替(`PanelTab`、レッスンの手順に合わせて自動で切り替わる)。解析タブは `ANALYSIS_SECTIONS` の折りたたみ(基本3つだけ最初から開く、レッスン中は全部開く、「すべて開く/基本だけ」ボタン)。解析パネルを足すときはここに1行足す / `analysis_panel.rs` 「解析」タブの中身(DC・過渡・AC・スイープ・ノイズ・温度とばらつき・結果一覧) / `hint.rs` 画面下のヒント文 |
| `fault.rs` 「故障シミュレーション」パネル(部品全体かピン単位で壊す/全部品と、IC・トランジスタの全ピン断線を順に壊して影響順に並べる。`--fault` で起動引数からも。「比べる」でDC/過渡/ACを切替(`FaultDomain`)、波形系は `WaveMode::Fault` の波形パネルに重ね描き) / `wizard.rs` 「学ぶ → 値の決め方」窓(`CALCS`、選択中の部品へ値を反映) / `param.rs` パラメータスイープのパネルと重ね描き(`--param` で起動引数からも実行可) / `settings.rs` 「設定 → シミュレーション設定」ウィンドウ / `panel.rs` の `part_popup_window` は部品の右クリックで出る設定小窓(選択部品プロパティと同じ中身) |
| `noise.rs` の `probe_node` は「回路図で選ぶ」(ピン/配線=電圧、部品の本体かShift+ピン=電流)。`wave.rs` の `live_update` は収束しない値では直前の良い結果を残す。値スライダーの幅は `slider_span`(細かい/ふつう/広い) |
| `edit.rs` 選択・当たり判定・削除・配線追加 / `history.rs` Undo・Redo / `file.rs` 保存・読込 / `library.rs` ライブラリ |
| `lessons.rs` 「学ぶ」メニューのレッスン(サンプル回路埋め込み+手順、各手順は状態で自動チェック。足すときは `LESSONS` に追加) / `variation.rs` 温度スイープ・ばらつき解析(モンテカルロ)の実行と表示 / `dc.rs` `run_dc`・`sim_circuit`(温度適用済みの回路) / `tran.rs` / `ac.rs` / `sweep.rs` / `noise.rs` 各解析の実行と波形パネル / `wave.rs` 表示中の解析切替・自動再計算 `live_update` |

### `board_editor/` (`BoardEditor`)
| ファイル | 内容 |
|---|---|
| `mod.rs` | 型定義(`Tool`,`Item`,`RouteMode`)、`BoardEditor`のフィールド、`snap`等の補助 |
| `canvas.rs` 描画エリアの流れ・キー操作 / `interact.rs` 範囲選択とポインタ操作 / `draw.rs` 描画(銅、ゾーン、ハイライト) / `panel.rs` 左パネル・選択パネル・ヒント |
| `select.rs` 当たり判定・範囲選択・移動・削除 / `route.rs` 配線の折れ方 / `tools.rs` クリック時のツール動作 |
| `view3d.rs` 「3Dで見る」窓(製造ファイルから作った絵を外形に貼り、部品の立体を描く小さなソフトウェアレンダラ。`Cam` が射影、部品にホバー/クリックで選択) / `width.rs` 「配線の太さ(電流から)」(ネットごとの電流→推奨幅→太くする) / `order.rs` 「発注の準備」ウィンドウ(発注先・チェック・プレビュー・書き出し・注文手順) / `file.rs` 保存・読込・製造出力・回路図取込 / `actions.rs` 自動配線・DRC実行 / `history.rs` / `view.rs` |

## 「〜を変えたい」早見表
- 新しい解析を足す → `sim-core/<名前>.rs` + `schematic/<名前>.rs` (実行+パネル) + `schematic/panel.rs` にボタン + `mod.rs` の `WaveMode` と `wave.rs` + `docs/examples/` にサンプル
- 新しい部品種別(基本素子) → `sch-core/lib.rs` の `Kind`、`symbols.rs`、`sim-core/from_schematic.rs`
- ライブラリ部品を足す → `part-lib/examples/*.part.json` (形式は `docs/part-format.md`)
- DRCルール → `pcb-core/drc.rs` / 設計ルール既定値 → `model.rs` の `Rules`
- ヒント文 → `schematic/hint.rs`, `board_editor/panel.rs` の `hint` / 豆知識 → `main.rs` の `TIPS`

トランス部品は `part-lib/examples/TRANS_*.part.json`(理想変圧器をBソースで作った`.subckt`: `Bs`=電圧源、`Bp`=`-n*I(Bs)`の電流源、巻線抵抗と励磁L)。電池は `part-lib/examples/{CR2032,AA_ALKALINE,LIION_18650}.part.json`(残量を蓄える巨大なC+残量で決まる電圧のBソース+内部抵抗。時間0だけ `Bpull` で残量を1に固定してDC解析でも満充電になる。サンプル `coin_cell_led.schjson`、レッスン10)。降圧コンバータは `part-lib/examples/BUCK_DEMO.part.json`(150kHzのPWM: ランプ+積分型誤差アンプ+tanhのPWM比較器+スイッチ`S`。tanhを滑らかにしてデューティのジッタを抑え、時間0だけ誤差アンプを0に固定してDC解析を通す。サンプル `buck_converter.schjson`、レッスン11、`designcalc::buck_design`)。PNPハイサイドスイッチは `pnp_highside.schjson`(2N3906、生成 `write_pnp_example`、レッスン13、プルアップR2・負荷R3)、オペアンプ反転増幅は `opamp_inverting.schjson`(LM358、生成 `write_inverting_example`、レッスン14)。MOSFETスイッチは `docs/examples/mosfet_switch.schjson`(2N7000、生成は `write_mosfet_example`、レッスン12、負荷はR_2512で定格内)。サンプルは `docs/examples/bridge_rectifier.schjson`(生成は `write_rectifier_example`)、レッスン9がこれを使う。

配布: 依存クレートを変えたら `python tools/gen_third_party_licenses.py` で `THIRD_PARTY_LICENSES.txt` を作り直す(CIが `--check` で古さを検査、リリースのzipに同梱)。`.github/workflows/release.yml` がタグ `v*` でWindows/macOS/Linuxのzipを作る。日本語フォントは `main.rs` の `install_japanese_font` がOSごとの候補パスから読み、無ければ `assets/fonts/BIZUDGothic-Regular.ttf`(OFL、`include_bytes!`)を使う。

## ドキュメント・サンプル
`docs/board-editor.md` 基板の使い方 / `docs/library.md` 部品ライブラリ / `docs/part-format.md` 部品JSON形式 / `docs/examples/*.schjson` サンプル回路。

## 規約
- ボタン・入力欄には `on_hover_text` で説明を付ける。エラーメッセージは原因と対処が分かる日本語で。
- 回路図の変更はフレームごとの比較(`schematic/history.rs`)でUndoに入る。`self.sch` を直接変えれば自動で履歴になる。基板は変更前に `checkpoint()` を呼ぶ。
- 自動配線・自動再計算のように「結果を採用する前に検証する」処理は、失敗時に元の状態を変えないこと。
