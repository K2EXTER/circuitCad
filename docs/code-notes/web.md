# ブラウザ版 `crates/web` (B9 の骨組み、2026-11-22)

インストール無しで学習の部分を試せるようにする版の骨組み。`app` (約 9.7 万行。ファイル・rfd・スレッド・基板・3D に頼る) を wasm に通すのではなく、別のクレートで `sch-core`・`sim-core`・`i18n` だけを使う egui の画面を作った。`app` には手を入れていない。

## 動かし方

```
rustup target add wasm32-unknown-unknown   # 一度だけ
cargo install --locked trunk               # 一度だけ (初回の trunk build で wasm-bindgen も入る)
cd crates/web; trunk serve                 # http://127.0.0.1:8080 。ソースを変えると作り直して読み込み直す
trunk build --release --cargo-profile web-release --dist dist-release   # 配る形 (dist・dist-release は git の対象外)
cargo run -p web                           # 同じ画面をウィンドウで (ブラウザ無しで試す)
```

`.claude/launch.json` の `web` (`trunk serve --config crates/web/Trunk.toml`) でプレビューの枠に出せる。`web-release` は `dist-release` をそのまま出す (python の http.server、ポート 8081)。

## 配る形の大きさ (2026-11-22 に測った)

| 作り方 | wasm | gzip |
|---|---|---|
| デバッグ版 (`trunk serve`) | 22 MB | - |
| `--release` (opt-level 3)、wgpu、フォント全部 | 12.8 MB | 5.3 MB |
| `web-release` (opt-level "s"・LTO・codegen-units 1)、wgpu、フォントを絞る | 8.1 MB | 3.8 MB |
| 同じ、描画を glow (WebGL) に | **6.2 MB** | **3.0 MB** |

- `web-release` はワークスペースの `Cargo.toml` の profile (release を元に)。ほかのクレートは使わない。
- 描画: wasm のときだけ eframe を `default-features = false, features = ["glow", "default_fonts"]` にした (`crates/web/Cargo.toml` の target ごとの依存)。ネイティブ (`cargo run -p web`) と app は wgpu のまま。eframe は glow と wgpu の両方の feature があると既定の描き方が glow になるので、同じ target で両方を有効にしないこと。
- wasm-opt (binaryen) は trunk が初回に取ってくる (`data-wasm-opt="z"`)。
- 残りの大きい所はコード (sim-core・egui)。さらに減らすなら、使わない解析を feature で外す。

## ファイル

- `main.rs`: 入口。ネイティブは `eframe::run_native`、wasm は `index.html` の `#ccad_canvas` に `WebRunner::start`。起動できたら「読み込んでいます」の文を消し、失敗なら理由をそこに書く。
- `app.rs` `WebApp`: 日本語のフォント (`crates/web/assets/BIZUDGothic-web.ttf` を `include_bytes!`。ブラウザにはシステムのフォントが無い。下の「フォントを使う字に絞る」)、サンプルの一覧、下の帯 (サンプルの説明・DC の結果・選んだ部品とピンの電圧)。幅が `NARROW` (700 pt) 以上ならサンプルの一覧は左のパネル。狭ければ「(今のサンプル) ▼」のボタンで上の帯の下に開く (指で押しやすい大きさ。選ぶと閉じる)。デジタルのサンプルは一覧の後ろに「動かして見る (デジタル)」の見出しで。
- `samples.rs` `SAMPLES`: `docs/examples` の `.schjson` を `include_str!` で埋め込む (ブラウザでは開くファイルが無い)。試験 `every_sample_opens_and_solves` (全部開けて DC が解ける)。足すときはここに 1 行。
- `live_ui.rs`: 動かしている回路の操作 (▶/⏸・1 クロック・1 イベント・クロックの速さ・ゆっくり・時刻、狭い画面では折り返す)、グリッチと競合の行、真理値表 (狭い画面では高さ 3 割までで巻き取る)。app の帯 (`schematic/live_logic.rs` の `live_logic_ui`) と同じ文。ロジックアナライザはまだ無い (`app/logic_plot.rs` にある)。
- `view.rs` `Sheet`: 読み込んだら `from_schematic` → `dc_op` で解き、電線ごとに `net_at` のネットの電圧で色 (青 = 低い → 緑 → 赤 = 高い。範囲は電線の電圧だけ)。ネットごとにいちばん長い電線のそばに電圧を 1 つ、縦の電線なら左に、横なら下に書く (ラベルと部品の名前は右と上に出るので重ならない)。部品は app と同じ記号 (`sch_core::symbol_shapes::component_shapes` の線と円を `rot_f` で回して描く)。箱の部品 (標準の記号もチップ部品の形も無いもの) は、ピンの名前を箱の中に、番号を足の横に、app と同じ置き方で (1 升 7 px 以上のときだけ)。デジタルのサンプル (`Sample::live`) は `sim_core::live_logic::LiveLogic` で動かす: 線の色は High 赤・Low 青・X 黄 (app の暗いキャンバスの色)、光った LED・7 セグメント・グリッチの線・フリップフロップの記憶・スイッチのレバー・ロジック部品のピンの 0 / 1 を app の `draw_live_logic` と同じに描く。スイッチを押すとその場で切り替わり (押しボタンは指を離すまで)、ほかの所のタップは部品を選ぶ。時刻は egui の時計の差で `tick` し、動いている間は描き直しを頼む。部品の名前は `sheet_box` で、立っていれば右、寝ていれば上に。ドラッグで動かす、ピンチ・Ctrl + ホイール・ホイールで指の位置を中心に拡大。タップでいちばん近い部品を選ぶ。画面全体の大きさが変わったら (読み込み直後・スマホを回したとき) 全体を収め直す。キャンバスだけの大きさでは収め直さない (上下の帯が伸びるたびに拡大が戻ってしまった: 2026-11-22 に実機で、スイッチを押すと拡大がリセットされた)。

## フォントを使う字に絞る (2026-11-22)

app の BIZ UD ゴシック (4.67 MB) を、ブラウザ版が出す字だけに絞って `crates/web/assets/BIZUDGothic-web.ttf` (1.38 MB、3907 字) にした。作るのは `python tools/web_font_subset.py` (fonttools)。入れる字: ASCII と Latin-1、かな、全角、矢印・図形、よく使う記号とギリシャ文字、JIS 第 1 水準の漢字、web・sch-core・sim-core・i18n のソース (`.rs`・`.tsv`) に出てくる字、ブラウザ版のサンプルの `.schjson` に出てくる字。ヒントは捨てる。OFL 1.1 は予約されたフォント名が無ければ改変版を同じ許諾で配れるので、`index.html` が `OFL.txt` をページの横に写す (trunk の `copy-file`)。試験 `the_cut_down_font_has_every_character_shown` (web のソースとサンプルの字が全部フォントにある)。落ちたらスクリプトを回し直してコミットする。

## 記号の形を sch-core に移した (2026-11-22)

記号の線と円 (`Sh`、`symbol`、`source_symbol`、`extents`・`sym_extents`・`comp_extents`、`sheet_box`、`boxes_overlap`、`rot_f`) を `app/src/schematic/symbols.rs` から `sch-core/src/symbol_shapes.rs` へ、ライブラリ部品の標準の記号 (`glyphs.rs` の全部) を `sch-core/src/glyphs.rs` へ移した。app はそれを `use` し直すだけで、描き方 (`draw_component` などの文字・抵抗の色の帯) は app に残る。部品の絵を 1 つの線と円のリストにする `component_shapes` は新しく足した (app の `draw_custom` の分岐と同じ: 標準の記号 → チップ部品は基本の記号と足 → 箱とピンと中の印)。試験 `every_basic_part_is_drawn_up_to_its_pins` (基本の部品は、どのピンにも絵の線の端か円が届く)。同梱の部品が記号に合うかの試験は part-lib が要るので app の `glyphs.rs` に残した。

## wasm のために core を直した所

- `sim-core/src/dc/newton.rs` の `newton_long`: 4 秒の時間切れ (`Instant::now`) を wasm では付けない。`wasm32-unknown-unknown` の `Instant::now` は panic するため。ブラウザでは反復の上限だけで止まる。
- `sim-core/src/par.rs` はそのままで良い: wasm では `available_parallelism` が Err なので 1 本で順に回す。
- 残る `Instant`: `sch-core/src/perfboard.rs` (ユニバーサル基板の並べ方の時間の予算)。ブラウザ版が使うようになったら同じように直す。

## 試しに載せる (自分だけが見られるページ)

`trunk build --release --cargo-profile web-release --public-url ./ --dist ../../target/web-artifact` で相対パスのビルドを作り、外側のタグを外したページ (`target/web-artifact/circuitcad-web.html`。trunk が出力のフォルダを空にするので、ビルドのあとに書き直す。js と wasm の名前はビルドごとに変わる) と js・wasm・`OFL.txt` を Artifact として載せる。非公開で、claude.ai にログインしたスマホから開ける。wasm の実行も通った。

## 次の段 (候補 B9 の残り)

- ヘッダのピンの名前 (`Component::pin_names`)。
- ロジックアナライザ (`app/logic_plot.rs` を共有するか、簡単な帯を描く)。
- レッスン・チャレンジを載せる (今は `SchEditor` の中にある)。B6 のブレッドボードも。
- スマホでの日本語の入力、長押しを右クリックの代わりに、ホバーの説明の代わり ((?) を押して出す)、古いスマホでの重さを確かめる。**実機の指の操作**: 2026-11-22 に claude.ai の非公開の Artifact に載せ (下の「試しに載せる」)、スマホの実機で、サンプルの切り替え・スイッチのタップ・ドラッグ・ピンチが動いた。プレビューでスマホの大きさ (指の端末のまね) にしたときにクリックが効かなかったのは、まねのせいだった (原因は追っていない)。iPhone の Safari はまだ。
- 公開の場所 (GitHub Pages など) と、リリースで `trunk build --release` するか。
