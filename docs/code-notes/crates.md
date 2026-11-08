# クレートの役割(詳しい版)

CLAUDE.md の表の元の文。各クレートの機能の幅が分かる。


| クレート | 役割 |
|---|---|
| `sch-core` | 回路図モデル (`Schematic`/`Component`/`Wire`/`Label`/`Kind`)、ネット抽出 `netlist.rs` (`extract`, `net_at`, `erc` — 指摘は `Problem{message,at,hint,severity}`、hintに原因と直し方を書く)、SPICE出力 `spice.rs`、ngspice方言 `ngspice.rs`、基板で入れ替えたピンを回路図へ戻す `backannotate.rs`、バス(`D[0..7]` のラベルと太い線。ネットの名前にはならず、点検だけ)`bus.rs`、書き込み用ピンがヘッダに出ているかの点検 `programming.rs`、階層(シートの記号と子シート。`flat()` の中で展開する。設計 `docs/hierarchy-design.md`)`hierarchy.rs` |
| `sim-core` | GUI非依存のシミュレータ。`from_schematic.rs` で回路図→`Circuit` |
| `design-spec` | 設計の要件の表の型(`DesignSpec`: ネットの役割・近くに置く組・離す組・バイパスの対応・道筋の測定 `Measure`・承知した例外 `Waiver`)。回路図 `Schematic.requirements` と基板 `Board.place_groups`/`keep_apart` が同じ型を使う |
| `pcb-core` | 基板モデル `model.rs`、DRC、ラッツネスト、自動配線、Gerber/Excellon/BOM出力 |
| `part-lib` | 部品JSON (`*.part.json`: 記号+パッケージ+SPICEモデル)。`examples/` が同梱部品。`kicad.rs` はKiCadの `.kicad_sym`/`.kicad_mod` を `PartDef` に変換、`kicad_sch.rs` はKiCadの回路図 `.kicad_sch` から部品とネット(どのピンがつながるか)を取り出し、`kicad_import.rs` がそれを回路図に組み立てる(画面「KiCad の回路図を開く」・CLI `import_kicad`。詳細 `docs/code-notes/part-lib.md`)、`passive.rs` はチップ部品のテンプレート、`vendorzip.rs` はメーカーのSPICEモデルzip(村田など。自分でダウンロードしたもの)の索引、`diode.rs` はダイオードの IS/N/RS をグラフの点から決める計算、`thru_hole.rs` はリード付き部品の穴・パッド・ピッチの目安、`passive.rs` にはコイル・抵抗のインピーダンス当てはめと温度係数の当てはめも |
| `datasheet` | データシート(PDF)を開いてページを絵にし、語の位置と検索・見出し(日英)・型番と分類の候補を出す(純Rustの hayro。GUI非依存。`text.rs`=語・検索 / `headings.rs` / `suggest.rs` / `value.rs`=数字と単位の換算 / `pin_table.rs`=囲んだ表からピンと突き合わせ / `dimensions.rs`=図面ページの数字)。部品作成フォームの「データシート」窓が使う。設計と段階は `docs/datasheet-viewer-design.md` |
| `app` | egui のGUI。回路図エディタ・基板エディタ・波形表示 |
| `cli` | `ccad`: 画面なしの操作窓口。1行1コマンドのスクリプトで、名前ベースの回路作成・ERC・解析・基板・製造データを実行し、答えを1行1個のJSONで返す (AIエージェント・自動化向け。`ccad mcp` で MCP サーバーにもなる。使い方 `docs/cli.md`、詳細 `docs/code-notes/cli.md`) |
