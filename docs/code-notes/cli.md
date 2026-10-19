# cli (`crates/cli`, 実行ファイル `ccad`)

画面なしの操作窓口。AIエージェントや自動化が、座標を書かずに「部品を置く → 名前でつなぐ → 検査 → 解析 → 基板 → 製造データ」を実行できる。使い方は `docs/cli.md`。

## ファイル
- `main.rs` 引数(`--lib DIR`、`--keep-going`、`-c`、`-`、ファイル)・1行ずつの実行・1コマンド=1行のJSON出力。最初の失敗で止まる(終了コード1)。
- `session.rs` `Session`(回路図・部品ライブラリ・基板・部品を置く位置のカーソル)、コマンド表 `run`、`HELP`、語の分割(`"` で囲める)、`key=value` の分離。
- `design.rs` 設計の手助け: `calc`(`sim-core/designcalc.rs` の14種。アプリの `wizard.rs` の `CALCS` とは別の表なので、計算を足したらどちらにも。基板の配線の太さはアプリだけ)/`ac`(`ac_analysis`。1デケードごとの値と -3dB・位相余裕)/`spec`(`sim-core/spec.rs` の `evaluate_spec`。対象の名前は大文字小文字を問わず結果の綴りに直す)。`dc`/`tran`/`ac` の結果は `Session.last_*` に残り、`new/open/add/value/net/remove` で消える(古い答えを新しい回路の合否にしないため)。
- `build.rs` 回路図側: `parts`/`part`/`add`/`value`/`net`/`remove`/`list`/`erc`/`dc`/`tran`。
- `board.rs` 基板側: `board new|import|autoplace|route|drc|status|save|open|fab`。回路図→基板の取り込み(`netlist_of`)はアプリの `board_editor/file.rs` と同じ規則(シミュレーション専用ピンは除く)。
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
- `erc` の答えの `tips` に、CLI向けの直し方(GNDは「GND」というネット名、など)を入れる。ERCの `hint` はGUI向けの文言のため。

## 初見のAIによる試験(2026-10-19)
`docs/cli.md` だけを渡した別のエージェントが、LED 10mA、RC低域通過(160µs で 0.63212V)、LM7805 + 基板 + ガーバー(未配線0、DRC 0)を、1〜2回で成功させた。指摘を直した(上の3項目、`part LED` が基本部品のピンを出す、約束事の節を `cli.md` に)。直していないもの: ネットの切り離し・改名(`unnet`)、`tran` の時刻指定、`parts` の英語検索。

## 今はできないこと(足すなら)
ノイズ・スイープ・故障の解析、波形の全点の出力、ベタ・多層・差動ペアなど基板の細かい設定、回路図の `.ccproj`、配線の座標指定、MCPサーバー(`ccad` を包めば作れる)。
