# cli (`crates/cli`, 実行ファイル `ccad`)

画面なしの操作窓口。AIエージェントや自動化が、座標を書かずに「部品を置く → 名前でつなぐ → 検査 → 解析 → 基板 → 製造データ」を実行できる。使い方は `docs/cli.md`。

## ファイル
- `main.rs` 引数(`--lib DIR`、`--keep-going`、`-c`、`-`、ファイル)・1行ずつの実行・1コマンド=1行のJSON出力。最初の失敗で止まる(終了コード1)。
- `session.rs` `Session`(回路図・部品ライブラリ・基板・部品を置く位置のカーソル)、コマンド表 `run`、`HELP`、語の分割(`"` で囲める)、`key=value` の分離。
- `design.rs` 設計の手助け: `calc`(`sim-core/designcalc.rs` の14種。アプリの `wizard.rs` の `CALCS` とは別の表なので、計算を足したらどちらにも。基板の配線の太さはアプリだけ)/`ac`(`ac_analysis`。1デケードごとの値と -3dB・位相余裕)/`spec`(`sim-core/spec.rs` の `evaluate_spec`。対象の名前は大文字小文字を問わず結果の綴りに直す)。`dc`/`tran`/`ac` の結果は `Session.last_*` に残り、`new/open/add/value/net/remove` で消える(古い答えを新しい回路の合否にしないため)。
- `models.rs` `models`(別名 `murata`。`part_lib::vendorzip` の検索。zipは `zip=` か、アプリが `settings.json` に保存した `model_zips`(旧 `murata_zip` も読む)。`model=型番` で `.subckt` と注意を返す。`models use 型番` は `VendorIndex::part_def` の部品を `Session.lib` に入れる(同名は置き換え。保存はしない)。テスト `tests/script.rs a_model_from_a_maker_zip_can_be_found_used_added_and_simulated`)。
- `build.rs` 回路図側: `parts`/`part`/`add`/`value`/`net`/`remove`/`list`/`erc`/`dc`/`tran`。
- `board.rs` 基板側(`pour [net=名]` はアプリの「GND ベタ」=`groundpour::ground_pour`。`import`→`pour`→`autoplace`→`route` の順で、ベタのネットを外して詰めた配置になり、`autoplace` が `repoured_vias` を返す。`route` はアプリと同じ流れ: `pad_lead`(`lead=mm`、既定0.5、0で切る)で引き、新しい配線に `padentry::align_entries` と `merge::merge_collinear`。答えは `aligned_pads`・`joined_tracks`): `board new|import|autoplace|route|drc|status|save|open|fab|postlayout|currents`。`currents [tran] [from=F] [widen]` は `sim_core::net_currents`(`tran` なら直前の `tran` の答え `last_tran` から `net_currents_tran`、答えの `stats` に rms/burst_rms_10ms/peak/spiky)(DC動作点→ネット電流)を `pcb_core::current::set_net_currents` で基板に入れて `net_widths` を返す(`widen` は `widen_net`)。`postlayout on` は `Session.post_layout` を立て、`circuit()` が `pcb_core::irdrop::wiring_resistances` を `sim_core::from_schematic_with_wiring` に渡す(切り替えで前回の `dc`/`tran`/`ac` の答えは消す)。回路図→基板の取り込み(`netlist_of`)はアプリの `board_editor/file.rs` と同じ規則(シミュレーション専用ピンは除く)。
- `runner.rs` 1行ずつの実行(`run_script`: 失敗で止まる/`keep_going`)。コマンドラインと MCP が共有する。
- `mcp.rs` `ccad mcp`: MCP サーバー(JSON-RPC 2.0、1メッセージ1行の標準入出力。`initialize`・`ping`・`tools/list`・`tools/call`。通知には返事をしない)。ツールは `ccad_run`(スクリプトを実行、失敗で `isError`)と `ccad_help`。`Session` はサーバーの間ずっと生きるので、回路図と基板が呼び出しをまたぐ。依存は増やしていない(`serde_json` のみ)。
- `tests/mcp.rs` 実際の `ccad mcp` と JSON-RPC で話すテスト(握手・ツール一覧・未知のメソッドの -32601、回路が呼び出しをまたぐこと、壊れた入力や未知のツールでも落ちないこと)。
- `tests/script.rs` 実際の `ccad` を走らせるテスト(分圧の理論値、名前の衝突で止まること、誤りの文言、555の周期が理論値の5%以内 + 配線完了 + DRC 0 + ガーバー出力)。

## 設計判断
- **配線は座標ではなくネット名のラベルで作る。** `net NAME REF.PIN ...` は各ピンの位置に同じ名前のラベルを置く(`sch-core` は同名のラベルを同じネットにする)ので、LLMがピンの座標を間違えてつながらない事故が起きない。GNDだけは、最初のピンにGNDの記号も1つ置く。
- **ピンは番号でも名前でも書ける。** 名前が複数ピンで共通(GNDピンが2つなど)なら全部につなぐ。見つからないときは、そのパーツの全ピンを答えに含める。
- **別のネット名がついたピンに別の名前を付けるのは拒否する**(黙ってショートさせない)。直すには `remove` で部品ごと消して置き直す。
- **部品は自動で重ならない位置に並べる**(左から右へ、右端で折り返す。ピンとボディの外形 + 余白10格子)。ピンの点が他の部品と重なると意図しない接続になるため。`at=X,Y` で上書きできる。
- **値は R/C/L だけ数値かを検査する**(`sim_core::parse_value`。SPICE式の単位つき表記 `10kk` なども通る)。
- **`tran` は要約を返す**(最小・最大・平均・最終値・等間隔の点 `samples`、繰り返す波形なら `period`/`frequency`/`duty` = 中央レベルの立ち上がりの交差が3回以上のとき)。全点は返さない。`probe=` で信号を選ぶ。
- 部品ライブラリは、アプリと同じく `%APPDATA%\circuitCad\library` を読み、無ければ同梱部品をそこへ複製する(`--lib DIR` で別の場所)。

- **`ok` は「コマンドが動いた」だけ。** 回路の良否は `erc` の `errors`/`circuit_ok`。`--strict`(`Session.strict`)なら `erc` が失敗し、`dc`/`tran`/`board import` は ERC エラーのとき拒む(`erc_gate`)。`--strict` なしでは、これらの答えの `warnings` に「ERC reports N error(s)」が入る。初見のAIの試験で、`erc` が `ok:true` のまま、エラーつきの回路で `dc` が数値を返し、見落とす危険があったため。
- **`dc` は名前に `.` を含む素子(ライブラリ部品のマクロモデルの内部。`U1.Ex` など)を隠す**(`dc full` で出す)。内部の電圧 2000V・電力 -4kW が故障に見えるため。
- `dc`・`tran`・`board import` の答えに `circuit_ok`(ERCにエラーがなければ true)。`dc` はノードも `.` を含む名前(内部)を隠す。ミッシング GND のシミュレータのエラー文には ccad 向けの直し方を足す(`cli_sim_error`)。
- **`dc` の答えに `ratings` と、オペアンプの飽和の警告を載せる**(`build.rs` の `dc_notes`)。「`ok` = 動いた」を「使える」と取り違えないため(2026-10-21、AIに作らせたレギュレータで、出力の頭打ちと定格オーバーを `warnings` が何も言わなかった)。`ratings` は `sim_core::check_ratings`(アプリの定格チェック)をそのまま。オペアンプの判定は、記号のピン名が `IN+`/`IN-`/`OUT` の部品で、`|V(IN+) − V(IN-)| > 10 mV`(負帰還がかかっていればほぼ0)。コンパレータなら誤警報になるので、文に「その場合は無視」と書いてある。`tran` は `tran_notes`(`build.rs`)。後半の半分を時間で重みづけして見る: 抵抗の実効電力、LED のピーク電流、`sim_core::ripple_reports`(電解コンデンサ、周囲温度は27℃固定)、オペアンプの入力差が 10 mV を超える時間の割合が 20% より大きいとき。発振器やコンパレータでは誤警報になるので文に「無視してよい」と書く。トランジスタ・ダイオードの電力は `tran` では見ない(`dc` のみ)。GND ネットには `V(GND)` の信号が無いので 0 V として扱う。
- `erc` の答えの `tips` に、CLI向けの直し方(GNDは「GND」というネット名、など)を入れる。ERCの `hint` はGUI向けの文言のため。

## 初見のAIによる試験(2026-10-19)
`docs/cli.md` だけを渡した別のエージェントが、LED 10mA、RC低域通過(160µs で 0.63212V)、LM7805 + 基板 + ガーバー(未配線0、DRC 0)を、1〜2回で成功させた。指摘を直した(上の3項目、`part LED` が基本部品のピンを出す、約束事の節を `cli.md` に)。直していないもの: ネットの切り離し・改名(`unnet`)、`tran` の時刻指定、`parts` の英語検索。

## 今はできないこと(足すなら)
ノイズ・スイープ・故障の解析、波形の全点の出力、ベタ・多層・差動ペアなど基板の細かい設定、回路図の `.ccproj`、配線の座標指定、MCPサーバー(`ccad` を包めば作れる)。
- `board check [fab=ID]`(`board.rs`): アプリの発注前チェック `pcb_core::fab::preflight` をそのまま返す(`ready`・各レベルの数・ok 以外の `list`)。回路図との食い違いは `pcb_core::netlist::diff`(アプリの `SchDiff` と共通。`NetlistDiff::blocking()` で Error/Warn を分ける)を `schematic_sync_check` にして先頭に入れる。取り込み前(基板に部品が無い)や回路図が空のときは比べない。テスト `board_check_says_ready_after_routing_and_notices_a_schematic_change`
- `noise`(`design.rs`): `sim_core::noise_analysis` を呼ぶだけ。出力は大文字小文字を区別せずネット名に直す(無いときは使えるネット名を返す)。`input=` は V/I 素子の名前。答え: `out_rms_v`、`in_rms`(input= のとき)、`per_decade`(密度)、`top_contributors`(名前・rms・割合)。テスト `noise_of_an_rc_filter_is_the_kt_over_c_value_and_names_the_resistor`(kT/C = 0.203µV、12.9nV/√Hz、寄与のほぼ100%が R1)。
- `sheetcmd.rs`: `sheet`(一覧・add・use・name・move)。`Schematic` のシートの仕組み(`sch-core/sheets.rs`)をそのまま使う。`move` は、動かす部品のピンの位置にあるラベルを一緒に動かす(配線が残っているシートは断る: 配線はシートをまたげない)。`net` は操作対象のシートの部品のピンだけを見るので、シートをまたぐネットは、各シートで同じ名前の `net` を書く。テスト `sheets_split_a_label_circuit_without_changing_its_answer`(分けても DC の答えが同じ、ERC エラー 0)。
- `sheet tidy [all]`(`sheetcmd.rs` の `tidy_active`): 部品の箱(ピンと、ライブラリ部品なら本体の枠。`part_bounds`)の面積の合計から段の幅(面積 × 1.7 の平方根)を決めて、作った順に段へ詰める。ラベルは、動かす前に「そのラベルの位置にピンがある最初の部品」に決めてから、その部品の動きと一緒に動かす(二度動かさない)。配線のあるシートは断る。実例: 3 シート版の 49 部品のシートが 横 96 × 縦 320 → 90 × 76。テスト `tidy_packs_a_sprawling_sheet_into_a_compact_one_and_keeps_the_circuit`。
- `libcmd.rs`: `lib [reload]`(`Session.lib_dir` から読み直す)と `part import`。`.subcircuit` の本体は、コメント行を省いて `sim.text` に入れる。取り込む前に `add_subckt_instance` で読めるか試す(読めない素子は、ここで理由つきで断る)。ピンの辺は `side_for`(名前から)、フットプリントは `package=` の部品から借りる。ADA4522(ADI)の `.cir` で `pins=IN+,IN-,V+,V-,OUT map=3,2,8,4,1` を使って確かめた。**取り込んだ部品はあなたのライブラリの中の1ファイルで、メーカーのモデルの再配布になることがある**ので、リポジトリには入れない。
- `dc probe=` と `sweep`(`build.rs`): `probe=` は括弧の外のカンマで区切る(`V(a,b)` があるため)。`sweep` は部品の値を書き換えて `dc_op` を呼ぶだけ(`sim_core::param_sweep` は使わない: あれは解析の種類ごとの曲線用)。終わったら値を戻す。
- `value` は残りの語をスペースでつなぐ(`split_words` が引用符を扱うので、どちらの書き方でも同じ値になる)。
- `tests/script.rs` の `run` は、呼び出しごとに別の部品フォルダを使う(並列のテストが同じフォルダの同梱部品のコピーや `part import` を半端に読むことがあった)。
- `board thermal`(`board.rs`): `pcb_core::drc::add_thermal_vias` を呼ぶだけ(アプリのボタンと同じ)。テスト `board_thermal_puts_vias_in_a_heat_pad_once`(IP2332 の裏のパッドに置いて、2回目は 0 個)。
- `sheet use`・`sheet add`・`sheet tidy` のあとの `add` は、操作対象のシートの既存の部品の下から並べる(`cursor_below`)。以前は (8, 8) から始めて、既存の部品の上に重ね、ピンの位置がぶつかって `net` が「すでに別のネットにある」と断った。
