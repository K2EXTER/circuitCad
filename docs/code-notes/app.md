# app 詳細メモ (`crates/app/src`)

CLAUDE.md の地図から参照される、各機能の詳しい説明。

## 開発用の検算 (xcheck)
- 開発用の検算 `xcheck.rs` / `xcheck_tran.rs`: `circuitCad --xcheck <file|dir>` が自前のDC動作点(電圧と電流)を、`--xcheck <file|dir> --tran` が過渡波形を(電源が時間変化しない回路は `xcheck_tran.rs` の `own_dynamics` の表の長さで。DC動作点が無くゼロ状態から始まった回路は ngspice にも `uic` を付ける。時間で重みを付けた規格化RMS誤差1%未満か、エッジ主体の波形はエッジ時刻のずれが周期の2%以内なら一致)ngspice と比べて `xcheck_report.txt` / `xcheck_tran_report.txt` に書く(ウィンドウは開かない。ngspice の場所は `--ngspice` か環境変数 `NGSPICE`、既定 `F:\Tools\ngspice\Spice64\bin\ngspice_con.exe`)。ngspice は reltol=1e-6 で解かせる(既定の1e-3だと ngspice 自身の電流が食い違う)。差が出たら gmin=1e-15 でも解き直し、それで一致すれば「gmin依存」。スイッチ(`S` と `.model SW`)は ngspice に `vw` が無いので、方言変換が自前と同じ滑らかな導通の B 電流源 `I=V(a,b)*exp(…)` に置き換える(これで降圧コンバータ・NE555・論理ICが ngspice と合う)。既知の差: 電源投入直後のフリップフロップなど双安定の回路は、自前が釣り合い点にとどまり ngspice が片側に倒れるので合わない(`logic_blinker`、`variable_supply`の浮いた2次側)。ngspice が止まる回路もある(トランス、`led_blinker`)。方言変換は `sch-core/src/ngspice.rs`(`to_spice_for(.., Dialect::Ngspice)`: if/step/limit/log/temp)。`--ourstep` `--ngstep` `--lte` で片方の刻みや許容だけ変えて、どちらのエンジンが粗い刻みに弱いかを見られる。

- CIでの検算: `docs/xcheck/ngspice_dc.json` に、同梱サンプルの ngspice のDC動作点(生の出力)が保存してあり、テスト `dc_points_still_match_the_stored_ngspice_answers` が自前の結果と比べる(ngspice は不要)。保存するのは既定設定で一致した回路だけ(`ip2332_charger` は gmin依存、`variable_supply` はズレなので外れている)。モデルを意図して変えたら `circuitCad --xcheck docs/examples --golden docs/xcheck/ngspice_dc.json` で作り直し、差分を読む。

## 描画・波形・プロット
- 電源記号は値で形が変わる(`symbols.rs` の `source_symbol`: 直流=±の円、SIN=正弦波、PULSE=方形波、PWL/NOISE=折れ線)。
- `plot.rs` の `scale_bar` は横軸・縦軸(グループごと)の範囲入力・全体・自動(1行、過渡・ACの波形パネルの上に出る。拡大縮小は軸の数字の上のホイール)。
- `plot.rs` 波形描画 (`scale_bar`・`draw_group`・`PlotState`)。
- 軸の数字の上でホイール=その軸だけ拡大縮小(`draw_group`)。
- `PlotState.cursors` が測定カーソルA/B(クリックで置く、`cursor_report` が差・周期を出す)。

## 作ってみよう・学習パス・操作ガイド
- `schematic/challenge/` は「作ってみよう」(`mod.rs`=型・`CHALLENGES` の並び・開始/判定の入口、`basic.rs`/`power.rs`=チャレンジ1件ずつの `const`、`judge.rs`=模範解答・開始シート・判定、`window.rs`=窓)(目標だけ示して自分で部品を置き・配線し、「判定する」でシミュレーションして判定する。1つの模範解答とは比べない)。
- チャレンジは `CHALLENGES` のデータ(開始シート `parts0/parts1/grounds*/start_values`、答え=サンプルの `drop/values/labels/rename`、判定 `checks`: `Erc`/`Named`(出力ネット `VOUT`)/`Voltage`(負荷・入力を足して測る、値の範囲ごとの助言 `explain`)/`Stay`(入力を2点に振る)/`Current`/`Gain`(AC)/`Ratings`)。
- 足すときは `CHALLENGES` に1件とテスト(答えが通る)。
- いまは zener・divider・led・rc_lowpass・emitter_follower・opamp_inv・series_regulator・regulator_78xx と、最終課題 draw_psu(lesson は protection、学習パスの `Kind::Build` の項目から id で開く。部品の個数を調べる `Check::Has` あり、答えは `power_supply.schjson`)。
- `--challenge <id> [0|1] [--judge]`、通過は進み具合に `<id>+build`、レッスン窓の「チャレンジを始める」と学習パスの「作ってみよう」から開く。
- `learn.rs` は学習パス(`PATHS` に段階と項目を書く。レッスンは `id` で指定、準備中は `planned`)と完了の記録(`%APPDATA%\circuitCad\progress.json`)と「学習パス」窓(`--learn`)。初回起動だけ `welcome_card`(引数なし・進捗なし・未回答のとき、`progress.json` の `"welcomed"` で一度きり)。学習パス窓の説明文は `spaced`(行の高さ)と `NOTE`(灰色)で読みやすくしてある。回路図のライブラリ部品の欄は初期状態で閉じている。
- 設計は `docs/learning-path.md`。
- `help.rs` は「操作ガイド」窓(ヘルプメニュー・F1・各パネルの「操作」ボタン、`--guide`。操作の説明は小さい文字で各所に書かず、ここの `SCHEMATIC`/`WAVEFORM`/`BOARD`/`VIEW3D` に足す)。

## プロジェクト・未保存の変更
- `project.rs` プロジェクト(`.ccproj`=回路図+基板を1つのJSONに。`version` つき。ファイル→プロジェクトを開く/保存、起動引数に `.ccproj`。`SchEditor::adopt`/`BoardEditor::adopt` で両方を置き換える。個別の `.schjson`/`.pcbjson` の保存も残してある)。
- 未保存の変更は `SchEditor::dirty`/`BoardEditor::dirty`(`rev` と `saved_rev`: 回路図は `commit_history`・`reset_history`、基板は `checkpoint`・`adopt` で進む。保存で `mark_saved`)で調べ、`files.rs` の `confirm_unsaved`(プロジェクトを開く・新規・開く・ウィンドウを閉じる。`--screenshot` の自動終了では聞かない)が rfd のはい/いいえ/キャンセルで確認する。
- 基板が回路図とずれているかは `BoardEditor::schematic_diff`(取込と同じ `netlist_of` で、追加/削除/値・形/つなぎを数える。回路図と基板の `rev` でキャッシュ)で、基板タブの ● と基板パネルの橙の文に出る。

## ファイル構成の補足 (main.rs まわり)
- `main.rs` ウィンドウ・メニュー(`App`)、`tips.rs` `TIPS`(豆知識)、`args.rs` 引数処理、`files.rs` 保存・読込・未保存の確認、`screenshot.rs` 画面保存、`startup.rs` クラッシュログ・フォント、`tests/` 統合テスト(`flow.rs`)とサンプル生成(`examples.rs` の `write_*`、`--ignored` で実行)。
- 「ファイル → 画面をPNGで保存」は eframe の Screenshot(`finish_screenshot`)で、起動引数 `--screenshot file.png` は数フレーム後に撮って終了(画面の確認・文書づくり用)。

## 部品作成フォーム・ライブラリ・電源値フォーム
- `part_form/` 部品作成フォーム(`mod.rs`=型とフォームの流れ `ui`、`parse.rs`=入力→`PartDef`(`to_def`)・SPICE貼り付けの解析、`kind.rs`=部品の種類バー、`tabs.rs`=各タブ、`chip.rs`=チップ部品、`template.rs`=充電ICテンプレート、`preview.rs`=右のプレビュー、`checklist.rs`=データシートのチェックリスト、`tests.rs`。以下は中身の説明: 上に部品の種類 `PartType`=IC/トランジスタ/ダイオード/チップ/コネクタを選ぶバー `kind_bar`→`apply_type` が例を入れる、タブ `FormTab`=基本/ピン/基板の形/シミュレーション/定格、右に記号 `symbol_preview`(`schematic/glyphs.rs` の `symbol_lines`)と基板の形 `footprint_preview` と指摘と保存を常に出す `preview_pane`。起動引数 `--new-part <ic|transistor|diode|chip|connector> --form-tab <basic|pins|package|sim|ratings>`。本体の高さと外形は「基板の形」タブ。SPICEの`.model`/`.subckt`を貼る/ファイルから読むと `guess_spice`→`autofill_from_spice` が種類・名前・素子記号・ピン表・ノード順を自動入力)、
- `part-lib/src/passive.rs`(チップ部品のテンプレート `PassiveTemplate`=値+ESR/ESL/DCR/並列容量/DCバイアス低下→`.subckt`、`fit_cap`=インピーダンス表から ESR・ESL を最小二乗で求める、`esl_from_srf`/`cp_from_srf`。部品JSONの `glyph` で記号をコンデンサ/抵抗/コイルの形に描く=`symbols.rs` の `draw_custom`。フォームは `passive_ui`、標準フットプリント `fp_preset` も選べる。見本 `C_100n_0603` `C_10u_0805` `L_10u_0805` `R_10k_0603` は `write_chip_examples` で生成)、
- `schematic/library.rs` の `LibPrefs`(お気に入り★と最近使った部品。ライブラリのフォルダの `favorites.json`。一覧は `panel.rs` でカテゴリごとに折りたたみ、検索中は開く)、
- `source_form.rs` 電源値フォーム。

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
| `lessons/` (`mod.rs`=型・`LESSONS` の並び・窓、`conditions.rs`=手順の自動チェック関数、`basics.rs`/`devices.rs`/`analog.rs`/`supply.rs`=レッスン1件ずつの `const`) 「学ぶ」メニューのレッスン(サンプル回路埋め込み+手順、各手順は状態で自動チェック。足すときは `LESSONS` に追加) / `variation.rs` 温度スイープ・ばらつき解析(モンテカルロ)の実行と表示 / `dc.rs` `run_dc`・`sim_circuit`(温度適用済みの回路) / `tran.rs` / `ac.rs` / `sweep.rs` / `noise.rs` 各解析の実行と波形パネル / `wave.rs` 表示中の解析切替・自動再計算 `live_update` |

### `board_editor/` (`BoardEditor`)
| ファイル | 内容 |
|---|---|
| `mod.rs` | 型定義(`Tool`,`Item`,`RouteMode`)、`BoardEditor`のフィールド、`snap`等の補助 |
| `canvas.rs` 描画エリアの流れ・キー操作 / `interact.rs` 範囲選択とポインタ操作 / `draw.rs` 描画(銅、ゾーン、ハイライト) / `panel.rs` 左パネル・選択パネル・ヒント |
| `select.rs` 当たり判定・範囲選択・移動・削除 / `route.rs` 配線の折れ方 / `tools.rs` クリック時のツール動作 |
| `view3d/` (`mod.rs`=状態 `View3d`、`window.rs`=窓の描画と操作、`scene.rs`=`Scene` の構築と描画、`camera.rs`=`Cam`・`camera`・`focus_on`、`picture.rs`=`Quality`・`snapshot`、`glow.rs`=`net_glow`、`tests.rs`) 「3Dで見る」窓(製造ファイルから作った絵を外形に貼り、部品の立体を描く小さなソフトウェアレンダラ。`Cam` が射影、`raster3d.rs` が深度バッファ付きの三角形描画で、製造ファイルの絵はその絵の枠(`Preview.frame`)で貼る。部品の選択は、ピクセルごとの部品番号の表で行う。`net_glow` は選んだネットの銅を三角形にして板の少し上に描く(基板エディタで配線/パッド/ビアを選ぶとそのネットが光る)、`focus_on` は選択部品への寄り、`snapshot` は輪郭なしの絵=PNG保存。`Quality::Draft` は回す・動かす・拡大の最中に半分の大きさで描く粗い絵、止めると `Fine`。`set_limits` はケース高さ(表/裏の上限mm)を超える部品を赤くして上限の枠を描く) / `width.rs` 「配線の太さ(電流から)」(ネットごとの電流→推奨幅→太くする) / `order.rs` 「発注の準備」ウィンドウ(発注先・チェック・プレビュー・書き出し・注文手順) / `file.rs` 保存・読込・製造出力・回路図取込 / `actions.rs` 自動配線・DRC実行 / `history.rs` / `view.rs` |
