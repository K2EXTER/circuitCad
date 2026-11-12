# app: 回路図エディタ・各解析の画面・階層とシート・レッスン・ERC まわり

(`docs/code-notes/app.md` の目次から分けた。内容は元のまま)

## 部品作成フォーム・ライブラリ・電源値フォーム
- 回路図の文字の重なり(`schematic/draw.rs`): ① 片側に3本以上ピンがある部品(IC)は、名前と値を本体の左中央ではなく、上と下に置く(`side_pinned`。左右の短い線とネットラベルでふさがるため。基本部品とピンが少ない部品は今まで通り) ② DC解析のピン電圧は、ネットラベルがそのピンの上にあるか、ピンから水平の短い線の先にあるとき、ラベルの外側に出す(`label_anchor`、`VoltageSpot`: 本体の右側のピン=名前の後ろ、左側のピン=ラベルの点の手前、上下の縁のピン=名前の上に積む(隣のピンが近いため)。電源シンボル(`Kind::Power`)の上のピンも、その値の文字の上に積む) ③ 電流の数字は、既定の位置がネットラベルに重なるとき、線の左→右の順にずらす(`first_clear`、`label_rects`)。テストは `draw.rs` の `tests`。
- 故障のスキャンの一覧の1行(`ScanRow`)は、いちばん動いたノードの「前 → 後」(`worst`、「VIN が 12V → 0V」)と、定格に近づく部品の名前(`at_risk`、「🔥定格超え (Q1・R3)」)を持つ。見出しのホバーに読み方の凡例
- 故障のスキャン(ボタンは「よくある故障を順に試す」)は、`sim_core::likelihood` が Rare の故障を省く(「まれな故障も含める」= `scan_rare` で入れる。省いた件数は状態の行に出す)。テスト `the_fault_scan_leaves_out_the_rare_faults_unless_asked`
- 故障のスキャン(`schematic/fault.rs` の `scan_plan`)は、「逆向きにつなぐ」(`Fault::Reverse`)を含めない。向き間違いは故障ではなく組み立てのミスなので、部品を1つずつ選ぶ方(`ALL_FAULTS` の並びと `--fault` の種類の番号は変えていない)だけで試せる。
- レッスンの「予測してみよう」(`lessons/predict.rs`、`lessons/mod.rs` の窓): 手順の途中で、やる前に結果を選んで予測する選択式の問い(いまは10問: 分圧・LED・ツェナー・エミッタフォロワ・電池の内部抵抗・ダイオード・位相余裕(AC 解析)・ノイズ(入力換算の雑音)・RC のカットオフ(AC 解析。点の間を対数で補間 `cutoff_hz`)・RLC の行き過ぎ(過渡解析 `overshoot_pct`)。`measure` は DC 以外の解析の結果も読める。テストの `run_for_prediction` に解析を足す。`PREDICTIONS` に1件足す)。手順のデータ(`Step`)は変えず、`(レッスンの id, 手順の番号)` で引く。選択肢は「ラベルと、それが意味する数」の組で、**正解は手書きしない**: 手順が済んだとき(`update_lesson`)のシミュレーションの数(`measure`)に**いちばん近い数の選択肢**が正解(`nearest`)。当たり外れと実際の数と理由(`explain`)は、手順が済んだあとに出す(`ActiveLesson.verdicts`)。手順の説明(答えを書いていることが多い)は、選ぶか「予測せずに進める」(`NO_GUESS`)まで隠す。予測しなくても手順は進む(判定は出ない)。テスト `every_question_has_an_option_that_is_close_to_what_the_simulation_gives`(教材の数が変わって選択肢の数と食い違うと失敗する。ほかの選択肢が近すぎて「どちらも正解」にならないことも確かめる)・`a_guess_is_judged_by_the_number_when_the_step_is_done`。開発用の引数 `--lesson <id> --dc`(最初の解析を済ませた状態)
- レッスンの「ヒント」(`lessons/mod.rs` の `stuck_hint`・`conditions.rs` の `Unmet`): 手順が進まないとき、何が足りないかを窓の黄色の「ヒント」に出す。手順のデータには何も足さない。手順の条件が、新しい結果を要る判定(`dc_fresh`・`ac_fresh`・`tran_fresh`)で結果が「ない」か「古い(回路を変えたあと)」と見つけたとき、`SchEditor.lesson_unmet`(`Cell`)にボタンの名前と古いかどうかを書く(`update_lesson` が判定の前に空にする)。`changed(..) && dc_fresh(..)` のように手前で止まる条件は何も書かない(まだ値を変えていないうちは、解析の話をしない)。窓は、手順か回路(`signature_cached`)が最後に変わってから `HINT_AFTER`(25秒)たったときだけ出す。変えるたびに数え直す。文には「ボタン名」を「」で入れるので、`lit` でそのボタンも光る。値が条件に届かない理由も書く(`Unmet::Value{refdes, want, have}`: 「R2 を 30Ω 以下にします (いまは 47Ω)」。条件は `value_at_most`・`value_at_least`・`value_between` で書く。単位は部品名の頭文字 R/C/L/V で付ける。**値の条件は、新しい結果の条件より先に書く**(`value_at_most(..) && dc_fresh(e)`): 最初に足りないのは値の変更で、そのあとが解析のやり直しだから)。`closure` で複数の値を比べる条件(`let v = |r| ...`)はまだ理由を書かない。`lesson_unmet` は `RefCell`。テスト `a_step_that_wants_a_value_says_which_value_and_what_it_is_now`・`value_hints_use_the_unit_of_the_part_and_say_when_a_range_is_wanted`・ `a_condition_says_when_the_result_it_needs_is_missing_or_old`・`the_hint_comes_after_a_quiet_time_and_starts_over_with_every_change`
- 修理のレッスン 51 `debug_circuit`(`lessons/debug.rs`): `zener_regulator.schjson` に故障を1つ仕込んで始める(`FAULTS` の5種: R1 が15k・R2 が5Ω・V1 が3V・配線2か所の切れ。`setup` が時計の値で選ぶ。`SchEditor::debug_inject(seed)` は回路を読み直して仕込み、履歴と `lesson_base` を作り直す。`debug_fault` に番号)。手順2「直った」は `dc_fresh` かつ VOUT が 4.8〜5.5V。直っていないとき、`Unmet::Note` で VOUT の値と探す場所の種類(値か配線か。場所そのものは言わない)を25秒後のヒントに出す。直したあと、窓に「仕込まれていた故障」を出す。故障を足すときは `FAULTS` に1件と、テストの直し方(`the_repair_lesson_starts_broken_and_each_fault_can_be_mended`)にも。基板側の修理は `pcb_core::breakage`
- レッスンで今押すボタンを光らせる(`lessons/mod.rs` の `lit`): いまの手順の文の中で「」に入っている名前と同じラベルのボタンに、オレンジの枠を脈打たせる(解析パネルとパネルの `ui.button("…")` は `self.lit(…)` で包む)。手順のデータには何も足さない。ボタンの名前を変えたら、レッスンの文の「」も直す(光らなくなる)。テスト `lit_tests`
- 学習パスの地図(`learn.rs` の `tree_map`): 箱は 196×60(タイトルの折り返しが不自然にならない幅)。「次のおすすめ」(`next_item`)の箱だけ黄色の太い枠と、上端に「★ 次はこれ」の札を付ける。ほかの「いま学べる」は橙の細い枠。地図の上に色の凡例(★・橙・緑・灰)
- 過渡解析の波形(`schematic/tran.rs`): 「ゼロの状態から始めました(発振回路では普通のことです)」は警告(橙)ではなく、ⓘ の青い注記にする。後半(終了時間の半分から)にどの信号も、画面の最大電圧の2%より動かないときは、「終了時間が短すぎるかも」の案内と「終了時間を10倍にして計算し直す」ボタンを出す(555を20msで見ると平らな線になる、への対策)。左の統計の表は、信号の数に合わせて幅を広げる(100+80×列、250〜520px。以前は250px固定で右の列が切れた)
- ネットの窓はタブ(`BoardEditor.net_tab`: 0 線幅 / 1 クラス / 2 整え方)。クラスの選択だけは全タブの上に出す。「線幅」タブに、電流と推奨・線幅スライダー・インピーダンス・線幅の例外を集めた(線幅の入力欄はスライダー1つ。クラスの `track_width` もこれが変える。「クラス」タブには線幅の欄を置かない)。起動引数 `--net-window ネット名 [タブ番号]` で開いたまま画面を撮れる
- ベタで切れ切れになったパッドの未配線の線(`view.rs` の `cached_airwires` が `drc::pour_split_airwires` を足す)。テスト `the_editor_shows_an_airwire_for_a_pad_cut_off_by_its_pour`
- ネットの窓の電流の自動取得: 電流が決まっていないネットの窓を開くと(1回だけ。`net_win_asked`)、`net_current_request` に入れ、`main.rs` が `dc_net_currents()` で答える(`fill_net_current`)。そのネットだけに電流を入れる(ほかのネットの値は置き換えない。ほとんど流れないときや、取れないときは、理由を窓に出す `net_win_note`)。過渡解析の結果があれば、窓に実効・ピークと「実効値を入れる」。テスト `a_net_window_fetches_the_dc_current_of_its_net_and_the_list_window_draws`
- 鎖の折れ目(はっきりした線の縁のとき、幅 `ROUND_ENDS_FROM` 以上): 折れ目の外側のくさびを、円弧の扇(数枚の三角形)で埋めて、外側の角を丸くする(`add_strip`)。ミター継ぎだと外側が尖り、ビアの下の隅が四角く出た(2026-11)。以前は線ごとの丸い端で丸く、ガーバー(丸い絞り)とも合う。テスト `the_outer_corner_of_a_turn_is_round_not_a_mitre`
- 描画の軽さ(`draw.rs`): ベタの三角形は `zone_cache`(`PourCache`)に1回だけ作って毎フレーム使い回す。形が変わったあとのベタと島は `refresh_zone_caches` が別スレッドで作り直し(`zone_job`。数十 ms かかるため)、できるまでは前のものを描く(最初の1回と、部品をドラッグ中は今までどおり)。三角形と穴の輪郭は画面に入るものだけ描き、3 px 未満の穴の輪郭は描かない。実測は `cargo test --release -p app zone_frame_cost -- --ignored --nocapture`(`CCAD_BOARD` に基板)。画面の外にある配線・涙型・シルクの線と文字・ベタのかたまりは描かない(`near_screen`、線の太さの分は余白に含める)。テスト `near_screen_leaves_out_only_what_is_far_away`
### `schematic/` (`SchEditor`)
| ファイル | 内容 |
|---|---|
| `mod.rs` | 型定義(`Tool`,`Sel`,`DcView`,`TranView`...)、`SchEditor`のフィールド、`volt_color`,`eng`,`kind_tip` |
| `canvas.rs` | 描画エリアの流れ(`canvas`: パン・ズーム、キー、描画→操作の順) / `draw.rs` シートの描画(配線・部品・電圧/電流オーバーレイ・ERCの丸) / `interact.rs` マウス操作(範囲選択・ドラッグ・ツールのクリック) / `symbols.rs` 記号の図形・座標変換・重なり判定 |
| `panel/` | 左パネル(`mod.rs` タブ・ツール・ERC・フォーム / `parts.rs` 部品一覧。メーカーのシリーズ (`part_lib::series`) は 1 行にまとめ (シリーズ名・大きさ・いまの値)、選んだ部品の欄の「抵抗値」で型番を切り替える (`series_value_ui`、作られていない値は近い値のボタン。品質の印は行と欄に出す。シリーズの部品は削除のボタンを出さない)。起動の引数 `--lib-part 名前 [値]` と `--lib-search 語` で、その状態を画面保存できる / `properties.rs` 選択部品の設定)。「作図」タブ(ツール・部品一覧・選択部品のプロパティと値スライダー・ERC・部品作成フォーム)と「解析」タブの切替(`PanelTab`、レッスンの手順に合わせて自動で切り替わる)。解析タブは `ANALYSIS_SECTIONS` の折りたたみ(基本3つだけ最初から開く、レッスン中は全部開く、「すべて開く/基本だけ」ボタン)。解析パネルを足すときはここに1行足す / `analysis_panel.rs` 「解析」タブの中身(DC・過渡・AC・スイープ・ノイズ・温度とばらつき・結果一覧) / `hint.rs` 画面下のヒント文 |
| `fault.rs` 「故障シミュレーション」パネル(部品全体かピン単位で壊す/全部品と、IC・トランジスタの全ピン断線を順に壊して影響順に並べる。`--fault` で起動引数からも。「比べる」でDC/過渡/ACを切替(`FaultDomain`)、波形系は `WaveMode::Fault` の波形パネルに重ね描き) / `wizard.rs` 「学ぶ → 値の決め方」窓(`CALCS`、選択中の部品へ値を反映) / `param.rs` パラメータスイープのパネルと重ね描き(`--param` で起動引数からも実行可) / `settings.rs` 「設定 → シミュレーション設定」ウィンドウ / `panel.rs` の `part_popup_window` は部品の右クリックで出る設定小窓(選択部品プロパティと同じ中身) |
| 波形の「前」との比較は `Ghost`(`mod.rs`)。過渡・ACの「いまの波形を残す」で薄く残し、再計算(自動も)しても保つ。描画は `plot::ghost_color` |
| `noise.rs` の `probe_node` は「回路図で選ぶ」(ピン/配線=電圧、部品の本体かShift+ピン=電流)。`wave.rs` の `live_update` は収束しない値では直前の良い結果を残す。値スライダーの幅は `slider_span`(細かい/ふつう/広い) |
| `clipboard.rs` コピー・切り取り・貼り付け・複製(Ctrl+C/X/V/D と左パネルのボタン)。コピーは `SchEditor.clipboard`(`sch_core::Fragment`)に持ち、別の回路を開いても貼り付けられる(システムのクリップボードは使わない)。**egui は Ctrl+C/X/V を `Event::Copy`/`Cut`/`Paste` で渡す**(キー押下にならない)ので `canvas.rs handle_keys` はイベントを見る(Ctrl+D だけキー。修飾キーは `Event::ModifiersChanged` で渡す=テストの `send_with`)。部品をコピーすると、そのピンにあって配線の端でないネットラベル(`attached_labels`)も一緒に入る。貼り付け(`sch.paste_fragment`)は部品番号を新しく振り(`next_refdes`。電源記号は `#PWRnn`)、`paste_offset` が「ほかの部品に重ならず、ピン・配線の端・ラベルがシートの既存のものと同じ点に乗らない(うっかり結線しない)」いちばん近い移動量を探す(右下を先に、半径80まで)。貼ったものは選択状態になり(1つなら単独選択、複数なら `multi`)そのままドラッグで動かせる。Undoは回路図の差分履歴で自動 |
| `quick.rs` クイック追加(Aキー・「クイック追加」ボタン・`--quick <文字>`。シリーズは 1 つだけ出す (打った値をそのシリーズが作っていればその値、なければ既定の値。`erj 4.7k`・`抵抗 4k7`)。`quick_matches` が基本部品+ライブラリを名前/別名/カテゴリ/パッケージ/ピン/説明で検索、全単語一致、お気に入り→最近使った順に優先。↑↓Enter Esc) / `edit.rs` 選択・当たり判定・削除・配線追加 / `history.rs` Undo・Redo / `file.rs` 保存・読込 / `library.rs` ライブラリ |
| `lessons/` (`mod.rs`=型・`LESSONS` の並び・窓、`conditions.rs`=手順の自動チェック関数、`basics.rs`/`devices.rs`/`analog.rs`/`supply.rs`/`models.rs`(モデルのないICの等価回路)=レッスン1件ずつの `const`) 「学ぶ」メニューのレッスン(サンプル回路埋め込み+手順、各手順は状態で自動チェック。足すときは `LESSONS` に追加) / `variation.rs` 温度スイープ・ばらつき解析(モンテカルロ)の実行と表示 / `dc.rs` `run_dc`・`sim_circuit`(温度適用済みの回路) / `tran.rs` / `ac.rs` / `sweep.rs` / `noise.rs` 各解析の実行と波形パネル / `wave.rs` 表示中の解析切替・自動再計算 `live_update` |

- モデルの出どころ(`panel/mod.rs` の `origin_ui` `model_origin`): 部品一覧・ホバー・配置した部品の「基板とモデル」に、モデルの種類(等価 / メーカー / 一般 / 記録なし)・出典と、同梱の表 `part_lib::model_checks` があれば「データシートとの比べ」を出す。左のパネルの幅 (400 px) に収まるよう 3 列 (項目・比べた相手の typ か max・モデル)。テストで確かめている行はモデルの値が緑。min / typ / max と条件、差はホバー。表は横スクロールの枠に入れて、パネルを押し広げない。`origin` の無い写しは、表の指紋が合えば等価モデルとみなす。テスト `a_model_shows_where_it_comes_from_and_its_data_sheet_table`。
- `bench_guide.rs` は「実機の作業」(`learn::Kind::Hands`)のチェックリスト窓。アプリが判定できない作業(はんだ付け `solder`、ブートローダ `bootload`)を、安全の注意・道具と手順・自分で付けるチェック・「うまくいかないとき」の表で見せる。チェックを全部付けると、その項目の id が進み具合に入る(チェックの状態は、アプリを閉じると消える)。項目を足すときは `GUIDES` に `Guide` を足し、学習パスの `Item` を `kind: Kind::Hands`・同じ id にする(`every_lesson_of_a_path_exists_and_ids_are_unique` が確かめる)。内容は一般的な手順なので、`limits` に「最新の資料で確かめる」を必ず書く。
### schematic/stretch.rs — 配線が部品に付いてくる
部品をドラッグ(1個・複数選択)または回転すると、ピンの上に端がある配線は、その端がピンに付いてくる。斜めになる場合は1箇所だけ折れ曲がる(折れた先の線は配線の末尾に足す)。ドラッグ中は開始時の配線から毎フレーム計算し直す(`begin_stretch`/`apply_stretch`)ので、元の位置に戻せば配線も元に戻る。一緒に選んだ配線は平行移動するので対象外。ピンの途中に重なるだけの配線(T字)は付いてこない。

### 使わないピン (NC) — `Schematic.no_connects`
意図して未接続のピンを (部品番号, ピン番号) の組で持つ(`sch-core/noconnect.rs` の `is_no_connect`・`set_no_connect`。全シートで 1 つの一覧。空なら書かない)。`sch_core::erc` は、印のあるピンが一人ぼっちなら何も言わず、つながっている(配線または名前つきのネット)と「「使わない (NC)」の印がありますが、つながっています」の警告を出す。ERC の「未接続」の指摘(`Problem.pin` に部品とピン番号)は、開くと「使わないピン (NC) にする」のボタンが出る。部品の設定(左パネルと右クリックの小窓)の「使わないピン (NC)」は、ピンごとのチェック(3 ピン以上の部品とカスタム記号)。回路図には、印のあるピンに青い × を描く(`draw.rs`)。CLI は `nc REF.PIN … [remove=1]`。記号側のピン名が `NC` のものは、これまでどおり黙る。同梱サンプルは `logic_blinker.schjson`(74HC14 と 74HC74 の使わない出力 7 本。ERC の警告 0)。取り消しは回路図の履歴(Undo)に入る。

### sch-core ERC: 電源ピンの「電源につながっていません」
IC の電源ピン (VCC/VDD/V+/VIN) のネットは、電圧源・ヘッダ、ほかの部品の出力ピン (OUT/VOUT/VO)、または **直列の MOSFET (SPICE 要素が `M`、ピン名 D/S) のもう一方の端が電圧源・ヘッダのネット** のどれかにつながっていれば OK (`netlist.rs` の `fed_through_switch`。電池の逆接続の保護・ロードスイッチ用。1 段だけ)。

### schematic/coverage.rs — モデルの前提
解析タブの折りたたみ「モデルの前提 (何がわからないか)」(`panel_coverage`)。`sim_core::model_coverage` の結果を、計算に入っていない部品(橙)・汎用の標準値で計算・モデルの注意書き・シミュレーション専用の4つの一覧で出す。モデルがない部品があるときは、解析タブの上に ⚠ の1行を出す(その部品は回路にないものとして計算されるため)。開発用の起動引数 `--analyze` で解析タブを開いて撮れる。

### schematic/selfheat.rs — 自己発熱
解析タブの折りたたみ「自己発熱 (部品の温度上昇)」(`panel_selfheat`)。周囲の温度(`heat_ambient`)を決めて「自己発熱を計算」で `run_self_heat`(`sim_core::self_heat`)。結果 `heat_view` の表は、電力・上昇・値の変化・熱抵抗・係数。熱抵抗と係数の欄はそのまま `Component.theta`/`tc_ppm` に書く(回路図の履歴に入る。計算はボタンで再実行)。温まって動いたネットも上位8個。DC が収束しない回路は計算できない(エラーを赤で表示)。

### schematic/selfheat_tran.rs — 温度の時間変化
「自己発熱」の下のブロック(`panel_heat_tran`)。部品(`htran_parts`、コンマ区切り)・時間(`htran_stop`)・結合(`htran_links`、`A:B:K/W`)を入れて「温度の変化を計算」で `run_heat_tran`(`sim_core::heat_tran`)。結果 `htran` の表は、電力の最大・上昇の最大・熱抵抗・熱容量・定常なら。熱抵抗と熱容量の欄は `Component.theta`/`cth` に書く。各部品の上昇の曲線(`draw_rise_chart`)と、温まって動いた電圧(`HeatTran::moved`)も出す。結合は回路図に保存されない(入力欄だけ)。過渡解析を回すので、大きい回路では時間がかかる(画面は固まる)。

### schematic/erc_skip.rs — 描きかけの範囲
`Schematic.erc_skip`(最小角・最大角の組)の中にある ERC の指摘は `sch_core::erc` が捨てる(画面・CLI・チャレンジ判定とも共通。場所のない指摘=GNDなしなどは残る)。ERC ボタンの下の「選択を描きかけにする」で、選択物を囲む範囲(2マス余白)を足す。灰色の枠で表示(`draw_problems`)、×で外す。ファイルに保存され、Undo の対象。

### schematic/snippet.rs — スニペット
選択を `.ccsnip`(クリップボードの `sch_core::Fragment` をそのまま JSON にしたもの)に保存し、別の回路で「スニペットを置く…」で貼り付ける。貼り付けは `paste_clipboard` と同じ(空き地を探す・部品番号を振り直す・ピン上のラベルも一緒)。自作部品は記号をファイルの中に持つので、ライブラリが無い回路でも置ける。基板の配置・配線のスニペットは未対応。

### 複数シート (`sch-core/src/sheets.rs`、`app/schematic/sheets_ui.rs`)
1つの `Schematic` に複数のシートを持てる。**編集中のシートは `Schematic` の `components/wires/labels/erc_skip` そのもの**、ほかのシートは `sheets`(`Sheet`)に預けてある(編集中のシートの欄は空)。`sheets` が空なら今までどおり1枚。切り替えは `switch_sheet`(`park`/`unpark`で中身を入れ替える)、追加 `add_sheet`、削除 `remove_sheet`(1枚に戻ると `sheets` は空に戻る)。履歴・保存・`.ccproj` は `Schematic` 全体を扱うので、そのまま複数シートに対応する(シート切り替えも1回のUndoになる)。
- ERC の指摘 `Problem.sheet` と `at` は、そのシートの座標に戻してある(`sheet_of`)。別のシートの指摘をクリックするとそのシートに切り替わる。

### 階層(シートの記号と子シート) (`sch-core/src/hierarchy.rs`。設計 `docs/hierarchy-design.md`。段階1=モデルと展開、CLI。段階2=画面)
- **展開は `flat()` の中。** 部品の `sub: Some(SubRef{sheet})` が「シートの記号」(`Kind::Custom` + `custom` のピンが子シートのポート。`sim_only`)。`flat()` は記号を外し、子シートの中身を記号1つにつき1回コピーして入れる。どの記号からも指されないシートは、今までどおり1枚の独立したシート。下流(ネットリスト・ERC・シミュレーション・基板)は変えていない。
- **名前の規則**: コピーの部品番号は `R1_CH1`(元の番号 + `_` + 記号の番号。入れ子は `R1_M_L`。先頭の文字を変えない=SPICE が素子の種類を先頭の文字で決めるため)。コピーのネットラベルは `CH1.NAME`(コピーごとに別のネット)。電源記号(VCC・GND)とバスのラベルはそのまま。
- **ポート**は `Schematic.ports` / `Sheet.ports`(`SheetPort{pos,name,dir}`)。展開で、親の記号のピンと子のポートの位置に、同じ作り名のラベルを置いてつなぐ。作り名は先頭に `GENERATED`(`\u{1}`)を付け、`extract` の名前決めで、人が書いた名前より後にして、最後に外す(親の線の名前 `SIG` が、`CH1.IN` に負けない)。
- **置き場所(slot)**: 独立したシートは今までどおり `k * SHEET_STRIDE`、コピーは `sheet_count()` 番目から1つずつ。`instances()` が一覧、`slot_sheet` / `first_slot_of` / `sheet_name(slot)`(「子 (CH1)」)で引く。ERC の指摘はコピーの中でも子シートの座標で返す。`net_at` は子シートの最初のコピーで探す。
- 記号のピンは、ポートを置いた順(`sheet_symbol`)。ポートを足す・消すと、ピンは箱の中央にそろえ直されて動くので、`refresh_sheet_symbols`(`follow_pins`)が、動いたピンにつながっていた線(元の端に短い線を足す)とラベル(ピンへ動かす)を追いかける。
- 輪(自分自身を含む参照)は展開しない(`collect_copies` の経路の検査、深さ `MAX_DEPTH`=8)。シートを消すと、そのシートの記号も消え、後ろの番号が詰まる(`forget_sheet_refs`)。ポートを足したら `refresh_sheet_symbols` で記号のピンを直す。
- CLI: `sheet port NAME REF.PIN [dir=in|out|io]`、`sheet place N|NAME`(記号を置く。`sheet use` は編集中のシートの切り替えで、前から別の意味)。
- 画面(段階2、`app/schematic/hierarchy_ui.rs`): シートのタブを右クリック →「いまのシートに、記号として置く」(`place_sheet_symbol`)、記号をダブルクリックで中を開く(`enter_sheet`。`sheet_trail` に戻り先を積み、タブの横の「親へ戻る」で `leave_sheet`)、ラベルツールの「ポートにする」(`label_port`。`place_port`)と向き、ポートは旗の形で描く(`draw_ports`)。ポートは `Sel::Port` で選べて(名前と向きは `port_editor_ui`)、消すと記号のピンも消える。ポートは動かせない・コピーされない(消して置き直す)。
- 階層の点検(段階3、`hierarchy_problems`。`erc` が呼ぶ): つながっていない入力/入出力ポート(記号のピン側と子シートの中側)・ポート名の重複・ポートのないシートの記号・輪(自分自身を含む記号)。点の上に何かあるかは `netlist::lonely_points`(同じ場所のピン・ラベルも数える)。サンプルは `hierarchy_rc_stages`。用語集の「階層設計」「ポート」は、どれかのレッスンに出る語だけを置く決まり(`every_term_is_used_by_a_lesson`)なので、レッスンを作るときに足す。
- レッスン86「同じ回路を何回も使う (階層)」(`lessons/hierarchy.rs`。サンプル `hierarchy_rc_stages`。4手順: 中を開く・AC解析・C1 を直して全部のコピーが動くのを見る・記号を増やす。学習パスは「信号を扱おう」の1段目、`rc_lowpass` のあと)と、用語集の「階層設計」「ポート」。テスト `the_hierarchy_lesson_ticks_its_steps_and_one_c1_moves_all_three_stages`。
- コピーごとの値(`SubRef.values`: 子シートの部品番号 → このコピーでの値。`expand` が `Instance.values` を当てる。`value_rows` が表の行。記号を選ぶと左パネルの「このコピーだけの値」、CLI は `sheet value CH2 R1 4.7k`)。部品の追加・つなぎ替えなど形の違いは、コピーごとには変えられない。入れ子の孫の部品の値は、外側の記号からは変えられない(内側の記号の値で変える)。
- 部品番号は `next_refdes` が全シートを見て振る(シートをまたいでもダブらない)。ダブりは ERC が見つける。
- 故障シミュレーション(部品の一覧・ピン・連鎖故障・安全の判定)とパラメータスイープは、全シートの部品を見る(入口で `flat()` にする)。限界: 回路図の上に重ねる電圧・電流の表示、コピー・スニペット・整列、「選択中の部品」で選ぶ操作は、開いているシートの中だけ。CLI は `sheet` 命令でシートを作れる(`ccad` の `sheet add`・`sheet move`。`net` で作った、配線なしでラベルだけの回路を、機能ごとに分けられる。複数シートのファイルを開いて解析・基板取り込みもできる)。ブロックを置いて同じ回路を繰り返し使う階層(子シート)は未対応。

### schematic/loopgain.rs — ループゲインの実行と注記
`run_loop` が `sim_core::loop_gain` を呼び、結果を AC の表示 (`AcView.loop_run`) に出す。結果の上の警告の欄に、切った部品と向きに加えて、回路に同梱のオペアンプモデル (部品の中の最初の増幅段 `U1.Bgm`、または前のモデルの `U1.Eop` がある) があるときは `opamp_model_note` の注意を出す (モデルは極が1つだけで、位相余裕が実物より大きく出る。検証は `sim-core/src/tests_loopgain_parts.rs`)。レッスン 49 の「扱わないこと」にも同じ趣旨を書いてある。

- 部品の表示(`Visible.parts`、2026-10-26): 物の表示とロックの「部品」の目。隠すと、部品のパッド・シルクの外形・部品番号が描かれず、選べない(部品に属さないパッド・シルク・配線・ビアは、そのまま)。

- ゲートの入れ替え: 左パネルの「ゲートを入れ替え」(`actions.rs` の `swap_gates`。`Board.swap_specs` があるときだけ出る)。詳しくは `pcb-core.md` の `swap.rs`
