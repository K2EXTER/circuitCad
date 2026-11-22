# ブラウザ版 `crates/web` (B9 の骨組み、2026-11-22)

インストール無しで学習の部分を試せるようにする版の骨組み。`app` (約 9.7 万行。ファイル・rfd・スレッド・基板・3D に頼る) を wasm に通すのではなく、別のクレートで `sch-core`・`sim-core`・`i18n` だけを使う egui の画面を作った。`app` には手を入れていない。

## 動かし方

```
rustup target add wasm32-unknown-unknown   # 一度だけ
cargo install --locked trunk               # 一度だけ (初回の trunk build で wasm-bindgen も入る)
cd crates/web; trunk serve                 # http://127.0.0.1:8080 。ソースを変えると作り直して読み込み直す
trunk build --release                      # crates/web/dist/ に配る形 (dist は git の対象外)
cargo run -p web                           # 同じ画面をウィンドウで (ブラウザ無しで試す)
```

`.claude/launch.json` の `web` (`trunk serve --config crates/web/Trunk.toml`) でプレビューの枠に出せる。デバッグ版の wasm は 22 MB (日本語のフォント 4.7 MB を含む)。

## ファイル

- `main.rs`: 入口。ネイティブは `eframe::run_native`、wasm は `index.html` の `#ccad_canvas` に `WebRunner::start`。起動できたら「読み込んでいます」の文を消し、失敗なら理由をそこに書く。
- `app.rs` `WebApp`: 日本語のフォント (app の `assets/fonts/BIZUDGothic-Regular.ttf` を `include_bytes!`。ブラウザにはシステムのフォントが無い)、サンプルの一覧、下の帯 (サンプルの説明・DC の結果・選んだ部品とピンの電圧)。幅が `NARROW` (700 pt) 未満ならサンプルの一覧を上の帯のプルダウンに畳み、広ければ左のパネル。
- `samples.rs` `SAMPLES`: `docs/examples` の `.schjson` を `include_str!` で埋め込む (ブラウザでは開くファイルが無い)。試験 `every_sample_opens_and_solves` (全部開けて DC が解ける)。足すときはここに 1 行。
- `view.rs` `Sheet`: 読み込んだら `from_schematic` → `dc_op` で解き、電線ごとに `net_at` のネットの電圧で色 (青 = 低い → 緑 → 赤 = 高い。範囲は電線の電圧だけ)。ネットごとにいちばん長い電線のそばに電圧を 1 つ、縦の電線なら左に、横なら下に書く (ラベルと部品の名前は右と上に出るので重ならない)。部品は app と同じ記号 (`sch_core::symbol_shapes::component_shapes` の線と円を `rot_f` で回して描く。ピンの名前と番号はまだ書かない)。部品の名前は `sheet_box` で、立っていれば右、寝ていれば上に。ドラッグで動かす、ピンチ・Ctrl + ホイール・ホイールで指の位置を中心に拡大。タップでいちばん近い部品を選ぶ。キャンバスの大きさが変わったら (読み込み直後・スマホを回したとき) 全体を収め直す。

## 記号の形を sch-core に移した (2026-11-22)

記号の線と円 (`Sh`、`symbol`、`source_symbol`、`extents`・`sym_extents`・`comp_extents`、`sheet_box`、`boxes_overlap`、`rot_f`) を `app/src/schematic/symbols.rs` から `sch-core/src/symbol_shapes.rs` へ、ライブラリ部品の標準の記号 (`glyphs.rs` の全部) を `sch-core/src/glyphs.rs` へ移した。app はそれを `use` し直すだけで、描き方 (`draw_component` などの文字・抵抗の色の帯) は app に残る。部品の絵を 1 つの線と円のリストにする `component_shapes` は新しく足した (app の `draw_custom` の分岐と同じ: 標準の記号 → チップ部品は基本の記号と足 → 箱とピンと中の印)。試験 `every_basic_part_is_drawn_up_to_its_pins` (基本の部品は、どのピンにも絵の線の端か円が届く)。同梱の部品が記号に合うかの試験は part-lib が要るので app の `glyphs.rs` に残した。

## wasm のために core を直した所

- `sim-core/src/dc/newton.rs` の `newton_long`: 4 秒の時間切れ (`Instant::now`) を wasm では付けない。`wasm32-unknown-unknown` の `Instant::now` は panic するため。ブラウザでは反復の上限だけで止まる。
- `sim-core/src/par.rs` はそのままで良い: wasm では `available_parallelism` が Err なので 1 本で順に回す。
- 残る `Instant`: `sch-core/src/perfboard.rs` (ユニバーサル基板の並べ方の時間の予算)。ブラウザ版が使うようになったら同じように直す。

## 次の段 (候補 B9 の残り)

- 記号の中の文字 (箱の部品のピンの名前と番号、ヘッダのピンの名前)。
- 学習モード (B6・B8 の動かしながら見るモード、レッスン・チャレンジ) を載せる。どれも今は `SchEditor` の中にある。
- スマホでの日本語の入力、長押しを右クリックの代わりに、ホバーの説明の代わり ((?) を押して出す)、古いスマホでの重さを確かめる。
- 公開の場所 (GitHub Pages など) と、リリースで `trunk build --release` するか。
