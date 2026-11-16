# クレートの役割(詳しい版)

CLAUDE.md の表の元の文。各クレートの機能の幅が分かる。


| クレート | 役割 |
|---|---|
| `sch-core` | 回路図モデル (`Schematic`/`Component`/`Wire`/`Label`/`Kind`)、ネット抽出 `netlist.rs` (`extract`, `net_at`, `erc` — 指摘は `Problem{message,at,hint,severity}`、hintに原因と直し方を書く)、SPICE出力 `spice.rs`、ngspice方言 `ngspice.rs`、基板で入れ替えたピンを回路図へ戻す `backannotate.rs`、バス(`D[0..7]` のラベルと太い線。ネットの名前にはならず、点検だけ)`bus.rs`、書き込み用ピンがヘッダに出ているかの点検 `programming.rs`、階層(シートの記号と子シート。`flat()` の中で展開する。設計 `docs/hierarchy-design.md`)`hierarchy.rs`、複数の基板(E5)のつなぎの照合と全体の部品表 `system.rs`(下の「複数の基板」) |
| `sim-core` | GUI非依存のシミュレータ。`from_schematic.rs` で回路図→`Circuit` |
| `design-spec` | 設計の要件の表の型(`DesignSpec`: ネットの役割・近くに置く組・離す組・バイパスの対応・道筋の測定 `Measure`・承知した例外 `Waiver`)。回路図 `Schematic.requirements` と基板 `Board.place_groups`/`keep_apart` が同じ型を使う |
| `pcb-core` | 基板モデル `model.rs`、DRC、ラッツネスト、自動配線、Gerber/Excellon/BOM出力 |
| `part-lib` | 部品JSON (`*.part.json`: 記号+パッケージ+SPICEモデル)。`examples/` が同梱部品。`kicad.rs` はKiCadの `.kicad_sym`/`.kicad_mod` を `PartDef` に変換、`kicad_sch.rs` はKiCadの回路図 `.kicad_sch` から部品とネット(どのピンがつながるか)を取り出し、`kicad_import.rs` がそれを回路図に組み立てる(画面「KiCad の回路図を開く」・CLI `import_kicad`。詳細 `docs/code-notes/part-lib.md`)、`passive.rs` はチップ部品のテンプレート、`vendorzip.rs` はメーカーのSPICEモデルzip(村田など。自分でダウンロードしたもの)の索引、`diode.rs` はダイオードの IS/N/RS をグラフの点から決める計算、`thru_hole.rs` はリード付き部品の穴・パッド・ピッチの目安、`passive.rs` にはコイル・抵抗のインピーダンス当てはめと温度係数の当てはめも |
| `datasheet` | データシート(PDF)を開いてページを絵にし、語の位置と検索・見出し(日英)・型番と分類の候補を出す(純Rustの hayro。GUI非依存。`text.rs`=語・検索 / `headings.rs` / `suggest.rs` / `value.rs`=数字と単位の換算 / `pin_table.rs`=囲んだ表からピンと突き合わせ / `dimensions.rs`=図面ページの数字)。部品作成フォームの「データシート」窓が使う。設計と段階は `docs/datasheet-viewer-design.md` |
| `app` | egui のGUI。回路図エディタ・基板エディタ・波形表示 |
| `cli` | `ccad`: 画面なしの操作窓口。1行1コマンドのスクリプトで、名前ベースの回路作成・ERC・解析・基板・製造データを実行し、答えを1行1個のJSONで返す (AIエージェント・自動化向け。`ccad mcp` で MCP サーバーにもなる。使い方 `docs/cli.md`、詳細 `docs/code-notes/cli.md`) |

## 複数の基板 (E5、2026-11-16)
- `sch-core/src/system.rs`: `BoardLink{a, b: LinkEnd{design, refdes}, pairs}`(ピンの組。空なら同じ番号どうし、短いほうのコネクタまで)。`check(&[(名前, &Schematic)], &links) -> Vec<Finding{severity, link, pins, message, hint}>`: 基板・部品が無い(誤り)、ピンの数が違う(注意)、ピンごとに両側のネットの名前(`pin_nets`: `extract` のネット。名前の無い 1 ピンだけのネットは「つながっていない」)を比べる: 片側だけつながる(注意)、名前の無いネット `N$` は照合できないとまとめて 1 件(注意)、名前が同じなら良し、電源と GND(`role`: GND・0・VSS・AGND… と VCC・VDD・VIN・VBAT・+5V・3V3… の名前で見分ける)なら誤り、相手の別のピンにこちらの名前があれば「入れ替わっていませんか」(誤り)、ほかは名前が違う(注意)。`system_bom(&designs, &copies) -> Vec<BomRow>`: 電源の記号とシミュレーション専用を除いた部品を、部品名(ライブラリ部品の名前、基本部品は `Kind::label`)・値・フットプリントで数え、基板ごとの部品番号と、作る枚数を掛けた合計。`bom_csv`: UTF-8 の BOM つき、基板ごとの列。試験は 4 本(名前の見分け、ピンごとの照合の各場合、無い部品と大きさの違い、部品表)。
- アプリ `project.rs`: 形式 2(`version: 2`、`designs: [{name, copies, schematic, board}]`、`active`、`links`)。基板 1 枚でつなぎが無ければ形式 1 のまま書く(`save_system`。サンプルや古い版と同じ)。`Project` の `schematic`〜`settings` は開いている基板、ほかは `System{names, copies, active, parked: Vec<Option<Design>>, links}`(開いている基板の `parked` は `None`)。`parse` は形式 1 を基板 1 枚の `System` にする。
- アプリ `multiboard.rs`: `App.system`・`system_dirty`(しまった基板に未保存の変更)・`boards`(窓)。`switch_board` はいまの基板を `current_design` でしまい、次を `adopt` で開く(元に戻す履歴はそこから)。`add_board`(空の回路図と 50 × 40 mm の基板)、`remove_board`(その基板のつなぎも消す)、`rename_board`(つなぎの名前も直す)。窓「基板どうしのつなぎ」(「ファイル → 基板どうしのつなぎ… (複数の基板)」): 基板の一覧(開く・作る枚数・名前・外す)、基板を足す、つなぎの一覧と消す、つなぎを足す(両端の基板とコネクタのコンボ: J・P・CN・X・K と数字の部品番号とピンヘッダ。ピンの組「1-1 2-2 3-4」)、照合の結果(毎フレーム。誤りは赤、注意は橙、ヒントはホバー)、「全体の部品表 (CSV) を保存…」。メニューバーに基板が 2 枚以上なら「基板: 名前」のコンボ。`files.rs` の保存は `save_system`、読み込みで `System` を受け取る、未保存の判定に `system_dirty`。サンプル `docs/examples/multiboard_i2c.ccproj`(メニュー「サンプル回路を開く」の最後、撮影用 `--multiboard-sample`)。
- CLI `project check FILE` / `project bom FILE OUT`(`cli/src/projectcmd.rs`。セッションの回路図・基板は変えない)。
- 残り(段 ②③): 3D でコネクタを合わせて基板を置き、干渉と全体の STEP。回路図をつないで全体のシミュレーション。ネットの名前の対応表(SDA と I2C_SDA を同じとみなす)。コネクタの「相手の向き」(ヘッダとソケットで番号が鏡になる)の既定。基板ごとの元に戻す履歴を切り替えで残す。

