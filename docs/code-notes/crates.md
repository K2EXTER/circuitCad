# クレートの役割(詳しい版)

CLAUDE.md の表の元の文。各クレートの機能の幅が分かる。


| クレート | 役割 |
|---|---|
| `sch-core` | 回路図モデル (`Schematic`/`Component`/`Wire`/`Label`/`Kind`)、推奨の周辺部品を置く `companions.rs` (`Schematic::place_companions`: 部品の `CustomSymbol::companions` をピンの外側に縦に置き、ピンからの線と GND の記号か相手のピンへの線。同じピンの 2 つ目からは 3 升ずつ外へ)、ピンの電気的な種類 `pinkind.rs` (`PinKind`: 入力・出力・入出力・オープンドレイン・電源入力・電源出力・受動・NC、`guess` はデータシートの I/O の欄 → 名前。ERC は種類のあるピンで 出力どうし・入力だけのネット・つないだ NC を指摘し、電源ピンの点検にも使う)、ネット抽出 `netlist.rs` (`extract`, `net_at`, `erc` — 指摘は `Problem{message,at,hint,severity}`、hintに原因と直し方を書く)、SPICE出力 `spice.rs`、ngspice方言 `ngspice.rs`、基板で入れ替えたピンを回路図へ戻す `backannotate.rs`、バス(`D[0..7]` のラベルと太い線。ネットの名前にはならず、点検だけ)`bus.rs`、書き込み用ピンがヘッダに出ているかの点検 `programming.rs`、設計レビューの点検 `review.rs`(下の「設計レビューの点検」)、階層(シートの記号と子シート。`flat()` の中で展開する。設計 `docs/hierarchy-design.md`)`hierarchy.rs`、複数の基板(E5)のつなぎの照合と全体の部品表 `system.rs`(下の「複数の基板」) |
| `sim-core` | GUI非依存のシミュレータ。`from_schematic.rs` で回路図→`Circuit` |
| `design-spec` | 設計の要件の表の型(`DesignSpec`: ネットの役割・近くに置く組・離す組・バイパスの対応・道筋の測定 `Measure`・承知した例外 `Waiver`)。回路図 `Schematic.requirements` と基板 `Board.place_groups`/`keep_apart` が同じ型を使う |
| `pcb-core` | 基板モデル `model.rs`、DRC、ラッツネスト、自動配線、Gerber/Excellon/BOM出力 |
| `part-lib` | 部品JSON (`*.part.json`: 記号+パッケージ+SPICEモデル)。`examples/` が同梱部品。`kicad.rs` はKiCadの `.kicad_sym`/`.kicad_mod` を `PartDef` に変換、`kicad_sch.rs` はKiCadの回路図 `.kicad_sch` から部品とネット(どのピンがつながるか)を取り出し、`kicad_import.rs` がそれを回路図に組み立てる(画面「KiCad の回路図を開く」・CLI `import_kicad`。詳細 `docs/code-notes/part-lib.md`)、`passive.rs` はチップ部品のテンプレート、`vendorzip.rs` はメーカーのSPICEモデルzip(村田など。自分でダウンロードしたもの)の索引、`diode.rs` はダイオードの IS/N/RS をグラフの点から決める計算、`thru_hole.rs` はリード付き部品の穴・パッド・ピッチの目安、`passive.rs` にはコイル・抵抗のインピーダンス当てはめと温度係数の当てはめも |
| `datasheet` | データシート(PDF)を開いてページを絵にし、語の位置と検索・見出し(日英)・型番と分類の候補を出す(純Rustの hayro。GUI非依存。`text.rs`=語・検索 / `headings.rs` / `suggest.rs` / `value.rs`=数字と単位の換算 / `pin_table.rs`=囲んだ表からピンと突き合わせ / `dimensions.rs`=図面ページの数字)。部品作成フォームの「データシート」窓が使う。設計と段階は `docs/datasheet-viewer-design.md` |
| `app` | egui のGUI。回路図エディタ・基板エディタ・波形表示 |
| `web` | ブラウザ版 (B9 の骨組み)。sch-core・sim-core・i18n だけを使う別の egui の画面。詳細 `docs/code-notes/web.md` |
| `cli` | `ccad`: 画面なしの操作窓口。1行1コマンドのスクリプトで、名前ベースの回路作成・ERC・解析・基板・製造データを実行し、答えを1行1個のJSONで返す (AIエージェント・自動化向け。`ccad mcp` で MCP サーバーにもなる。使い方 `docs/cli.md`、詳細 `docs/code-notes/cli.md`) |

## 複数の基板 (E5、2026-11-16)
- `sch-core/src/system.rs`: `BoardLink{a, b: LinkEnd{design, refdes}, pairs}`(ピンの組。空なら同じ番号どうし、短いほうのコネクタまで)。`check(&[(名前, &Schematic)], &links) -> Vec<Finding{severity, link, pins, message, hint}>`: 基板・部品が無い(誤り)、ピンの数が違う(注意)、ピンごとに両側のネットの名前(`pin_nets`: `extract` のネット。名前の無い 1 ピンだけのネットは「つながっていない」)を比べる: 片側だけつながる(注意)、名前の無いネット `N$` は照合できないとまとめて 1 件(注意)、名前が同じなら良し、電源と GND(`role`: GND・0・VSS・AGND… と VCC・VDD・VIN・VBAT・+5V・3V3… の名前で見分ける)なら誤り、相手の別のピンにこちらの名前があれば「入れ替わっていませんか」(誤り)、ほかは名前が違う(注意)。`system_bom(&designs, &copies) -> Vec<BomRow>`: 電源の記号とシミュレーション専用を除いた部品を、部品名(ライブラリ部品の名前、基本部品は `Kind::label`)・値・フットプリントで数え、基板ごとの部品番号と、作る枚数を掛けた合計。`bom_csv`: UTF-8 の BOM つき、基板ごとの列。試験は 4 本(名前の見分け、ピンごとの照合の各場合、無い部品と大きさの違い、部品表)。
- アプリ `project.rs`: 形式 2(`version: 2`、`designs: [{name, copies, schematic, board}]`、`active`、`links`)。基板 1 枚でつなぎが無ければ形式 1 のまま書く(`save_system`。サンプルや古い版と同じ)。`Project` の `schematic`〜`settings` は開いている基板、ほかは `System{names, copies, active, parked: Vec<Option<Design>>, links}`(開いている基板の `parked` は `None`)。`parse` は形式 1 を基板 1 枚の `System` にする。
- アプリ `multiboard.rs`: `App.system`・`system_dirty`(しまった基板に未保存の変更)・`boards`(窓)。`switch_board` はいまの基板を `current_design` でしまい、次を `adopt` で開く(元に戻す履歴はそこから)。`add_board`(空の回路図と 50 × 40 mm の基板)、`remove_board`(その基板のつなぎも消す)、`rename_board`(つなぎの名前も直す)。窓「基板どうしのつなぎ」(「ファイル → 基板どうしのつなぎ… (複数の基板)」): 基板の一覧(開く・作る枚数・名前・外す)、基板を足す、つなぎの一覧と消す、つなぎを足す(両端の基板とコネクタのコンボ: J・P・CN・X・K と数字の部品番号とピンヘッダ。ピンの組「1-1 2-2 3-4」)、照合の結果(毎フレーム。誤りは赤、注意は橙、ヒントはホバー)、「全体の部品表 (CSV) を保存…」。メニューバーに基板が 2 枚以上なら「基板: 名前」のコンボ。`files.rs` の保存は `save_system`、読み込みで `System` を受け取る、未保存の判定に `system_dirty`。サンプル `docs/examples/multiboard_i2c.ccproj`(メニュー「サンプル回路を開く」の最後、撮影用 `--multiboard-sample`)。
- CLI `project check FILE` / `project bom FILE OUT`(`cli/src/projectcmd.rs`。セッションの回路図・基板は変えない)。
- **段 ②(3D で重ねる、2026-11-16)**: `pcb-core/src/stack.rs`。`Place{m: 符号つきの置換行列 [[i8;3];3](行列式 +1: z のまわりの 90° と、y のまわりに裏返す), offset}`(`new`・`apply`・`apply_box`・`then`・`inverse`・`is_over`)。軸がそろったままなので、箱は箱のまま、STEP の置き方はふつうの軸の置き方になる。`mate(A, A のコネクタ, B, B のコネクタ, ピンの組, すき間, 向かい合わせ)`: 両方のコネクタのピン(同じ名前のパッドの中心)を組にし、4 つの回転(向かい合わせなら裏返してから)それぞれで、組のピンの重心が合うように動かし、いちばんずれの小さい回転を採る(`Mated{place, worst, pins}`)。B の裏の面を A の表 + すき間に(向かい合わせなら B の表の面を)。`connector_height`(すき間の既定: A のコネクタの 3D の高さ)。`clashes(&[(名前, &Board, Place)])`: 基板ごとの 3D の箱(部品の箱・メッシュの外形・基板そのもの)を置いて、別の基板どうしの重なり(0.05 mm より浅い接触は数えない)。`board_mesh`: 基板を `step_out::write` で STEP にして `step::read_step` で読み戻した 1 つのメッシュ(穴・丸い部品・色つき)、`placed_mesh`。`step_out::write_system(名前, &[(名前, &Board, Place)])`: 基板ごとに 1 つの製品(その基板のすべての立体)を、`Place` の置き方で(`write` と共通の `write_assembly`、子ごとに原点・z 軸・x 軸)。試験: 置き方の合成と逆、縦と横のヘッダのピンが重なる回転、番号が逆のヘッダは半回転で合う、向かい合わせのすき間、近すぎるとぶつかり離せばぶつからない、メッシュにして置いても体積が同じ、システムの STEP を読み戻すと 2 枚分の大きさと体積。
- アプリ: `System.places: Vec<Option<Place>>`(形式 2 の基板ごとの `place`。`None` は 3D にまだ置いていない)。窓のつなぎごとに「3D で重ねる」(`stack_link`: 1 枚目がまだ置かれていなければ原点に、2 枚目を 1 枚目の置き方 ∘ `mate` に。ずれが 0.2 mm を超えれば誤りとして知らせる。すき間 0 は 1 枚目のコネクタの高さ、「向かい合わせ」)、「3D で全体を見る」、「全体の STEP を保存…」、重ねた基板どうしのぶつかりの一行。`refresh_stack`: 開いている基板の置き方の逆 ∘ ほかの基板の置き方で、ほかの基板のメッシュを 3D の窓へ(`BoardEditor::set_view3d_others` → `Scene.others`、CPU と GPU の両方で、部品番号なし = 選べない)とぶつかりを計算し直す。切り替え・読み込み・サンプルを開くときにも。サンプル `multiboard_i2c.ccproj` は、部品を取り込んで手で置き(main 40 × 30、sensor 20 × 15)、センサー基板をメイン基板のヘッダの上 8.5 mm(ヘッダの高さ)に重ねてある。撮影用 `--multiboard-sample --3d`。CLI `project step FILE OUT`(置いた基板の STEP とぶつかり)。SolidCraft で開いて、エラー 0、基板 2 つの部品の組に分かれ、重なった位置に出た。
- **段 ③(全体のシミュレーション、2026-11-16)**: `sch-core/system.rs` の `merge(&designs, &links) -> (Schematic, Vec<LinkProbe>)`: 基板ごとの `flat()` を横に並べ(前の基板の右へ 60 単位)、部品番号の後ろに基板の名前(`R1_main`。名前の英数字だけ、無ければ `B2`)、ネットの名前の前に基板の名前(`main.SDA`。ラベルと電源記号の値。GND と 0 はすべての基板で 1 つ)。つなぎのピンの組ごとに、シミュレーション専用の 0 V の電源 `VLINK<つなぎ>_<ピン>` を、すべての基板の右の列に置き、+ を A 側のネット、− を B 側のネットの名前に(名前の無いネットは、コネクタのピンに `main.J1-3` のラベルを付けてその名前で)。両側が GND のピンは電源を置かない(同じ 1 つのネット: 置くと解けない)。両側が同じ 2 つのネットのピン(+5V が 2 本など)は 1 つの電源を共有する(並べた 0 V の電源は解けない。`shared` に本数、電流はその合計)。次の電源はピンの幅 + 6 だけ下に(ピンどうしが重なると別のつなぎがつながる)。`sim-core/system.rs` の `system_dc` が `from_schematic` と `dc_op` で解き、ピンごとに両側の電圧と、電源の電流(+ に入る向き = A から B へ)を返す(`PinReading`)。アプリの窓に「全体をシミュレーション (DC)」(ピン・両側のネット・電圧・電流の表、共有なら「N ピンの合計」)と「全体の回路図を保存…」(つないだ 1 枚の `.schjson`。開けば過渡・AC も全体で)。撮影用 `--multiboard-sample --system-dc`。CLI `project dc` / `project merge`。サンプルはメイン基板に 5 V の電源 V1(シミュレーション専用)、センサー基板に 1 kΩ の負荷 R1 を足し、J1 の 1 番を 5 mA が流れる。試験: 5 V と 5 mA、つながなければ電流なし、名前の無いネットのピン、同じ電源の 2 本のピンは合計、部品番号の付け方、サンプルの 5 mA。
- **作っていて分かったこと**: 段 ① のサンプルは、ヘッダの 3 番のラベルの線が 4 番のピンを通っていて、両方の基板で SDA と SCL がつながっていた(試験用のラベルの付け方の、ピンの向きの決め方のせい)。照合は両側の名前を比べるので、同じ間違いが両方の基板にあると通ってしまう。全体のシミュレーションで、2 つの 0 V の電源が並んで解けなくなって見つかった。サンプルのヘッダのラベルを右に出すようにし、テストはヘッダの 4 本のピンが別々のネットにあることも見る。
- 残り: ネットの名前の対応表。ヘッダとソケットで番号が鏡になる場合の既定。基板ごとの元に戻す履歴を切り替えで残す。ケーブルでつなぐ基板の置き方(今は重ねるだけ)。ネットの名前の対応表(SDA と I2C_SDA を同じとみなす)。コネクタの「相手の向き」(ヘッダとソケットで番号が鏡になる)の既定。基板ごとの元に戻す履歴を切り替えで残す。

## 設計レビューの点検 (N1、2026-11-19)

`sch-core/src/review.rs` の `review_problems` を `erc_all` が呼ぶ。つながりは正しいが「付けるべき部品が無い」失敗を、今の ERC と同じ `Problem` で出す。部品が何かは部品のデータ(`companions`・glyph・`ratings`・ピンの種類)で決め、名前で推し量らない。

- **推奨の周辺部品** (`CustomSymbol::companions`): 同じ (ピン, つなぎ先, 種類) の行をまとめ、ピンのネットとつなぎ先 (GND か部品のピンのネット) の間に、その種類 (R・C・L。基本素子か、ライブラリ部品の glyph) の 2 端子の部品があるかを見る。無ければ注意「U1 の推奨のコンデンサがありません: CIN (330n、ピン1 (IN) と GND の間)」。コンデンサは合計が推奨の合計の 0.5 倍 (`TOO_SMALL`) 未満で「小さすぎます」。ピンがつなぎ先に直接つながっている (リセットを VCC に直結) なら何も言わない。ピンが未接続なら言わない (未接続の指摘がある)。リセットのプルアップは `R` の companion (`RRST 10k RESET → VCC のピン`) で表す。
- **水晶の負荷容量**: glyph `crystal` で `ratings.load_pf` のある部品。両端 (GND でないピン 2 本) のどちらかに IC (glyph の無いライブラリ部品) がつながるときだけ見る (水晶だけの AC 解析の練習は除く)。両端から GND へのコンデンサ C1・C2 で CL = C1·C2/(C1+C2) + 浮遊容量 5 pF (`STRAY_PF`)、水晶の CL から 25 % (`LOAD_TOLERANCE`) 外れたら注意。目安の値は E12 に丸めた 2·(CL − 5 pF)(CL 18 pF → 27 pF)。片側だけ・両方無しも注意。
- **IC の出力に直接の LED**: LED の片端が `PinKind::Output`/`Bidir` のピンのネット、もう片端が GND か電源 (電源記号・電源入力のピン・電圧源・レギュレータの出力)。電源から直接の LED は前からの ERC (`netlist.rs`) が言う。
- **電源の入口の保護** (`Severity::Info`、新設): コネクタ (`Header` か、J・P・CN の部品番号のライブラリ部品) のピンが電源のネット (電源記号の名前か、IC の電源入力がつながる) にあり、ほかに電圧源・レギュレータの出力が無く、ネットにダイオード (glyph diode・zener・schottky・tvs、または SPICE の D) も MOSFET も無いとき、コネクタごとに 1 回。情報は一覧の最後 (`erc` で並べ替え)、画面は青の ℹ。
- 同梱部品のデータ (2026-11-19 に足した): レギュレータ AMS1117 (10µ / 22µ タンタル)・78xx (0.33µ / 0.1µ)・L78L05 (同)・LM317 (0.1µ / 1µ)・MCP1700 (1µ / 1µ)、マイコン ATmega328P・ATmega2560・ATtiny85 のリセットのプルアップ 10k と VCC のバイパス 100n、水晶 XTAL_12MHz・16MHz の CL 18 pF。部品作成のフォームの定格の欄に「負荷容量 CL [pF] (水晶)」(`rt_load`)。
- サンプルは、ライブラリ部品を回路図に埋め込んだ時の形で持つので、上のデータを足す前に作ったサンプルの部品には companions が無い (点検も出ない)。`regulator_heatsink.schjson` は作り直した。
- テスト `review::tests` (レギュレータ・リセット・水晶・LED と入口)。

## 選んだ部品の役割 (B11、2026-11-19)

`sch-core/src/roles.rs` の `roles_of(sch, refdes, volts)` が、選んだ部品がまわりと組んでいる役割 (`Role`: 名前・一緒に組む部品・計算した値・理由の文) を返す。部品の種類は基本素子の `Kind` か、ライブラリ部品の glyph (resistor・capacitor・electrolytic・led・opamp) で見分ける (名前では推し量らない)。電圧は `volts` (画面は DC の結果が今の回路のものならそれ、無ければ `fixed_volts`: GND・− が GND の直流の電圧源の +・5V / 3V3 / +12V のような名前の電源記号)。

- **LED の電流制限** (抵抗・LED のどちらを選んでも): 抵抗と LED が 2 つだけでつながる点があるとき。電流 = (V(抵抗の外の端) − VF 2 V − V(LED の外の端)) ÷ R。
- **分圧**: 選んだ抵抗の片端が固定の電圧 (電源か GND)、もう片端 X が固定でなく、X から別の固定の電圧へ抵抗がちょうど 1 本。X の電圧。X にほかに何がつながっていても (負荷) よい。
- **RC ローパスフィルタ** (抵抗・コンデンサのどちらでも): 抵抗の片端からコンデンサが GND か電源へ。fc = 1 ÷ (2πRC)。IC の電源ピンのある点のコンデンサ (パスコン) は除く。
- **プルアップ / プルダウン**: 電源か GND から、入力 (ピンの種類 Input・Bidir・OpenDrain、RESET、スイッチの部品) のある点へ、ほかに抵抗の無い 1 本。押したときの電流。
- **バイパスコンデンサ** (1.1 µF 以下で IC の電源ピンのある電源の線) / **電源の平滑** (それより大きい)、**結合コンデンサ** (どちらの端も電源・GND でない。後ろの抵抗との fc)。
- **オペアンプ** (glyph opamp、ピンの名前が …IN-・…IN+・…OUT の組ごと): OUT と IN- が同じ点ならボルテージフォロワ、OUT → IN- の抵抗 Rf と IN- からのもう 1 本 Rg: Rg の先が GND (か、+ 入力が GND でない電源) なら非反転 1 + Rf/Rg、そうでなければ反転 −Rf/Rg。抵抗を選ぶと「帰還の抵抗」とその利得。
- 電源の線の見分けで、レギュレータの OUT は電源だが、オペアンプの OUT は信号 (`is_regulator`: IN のピンもある部品)。最初は OUT という名前だけで見て、オペアンプの帰還の抵抗を「分圧」と言っていた。
- 画面は `app/src/schematic/roles_ui.rs`: 部品を 1 つだけ選んだとき、その右 (場所が無ければ左) に吹き出し。役割ごとに名前 (青)・値・一緒の部品・理由、最後に値の出どころ (DC の結果か電源の値)。「×」でその部品の分を閉じる (ほかを選べばまた出る)。計算は回路図の変更・部品・DC の有無が同じ間は使い回す (`RolesCache`)。動かしながら見るモードと複数選択では出さない。
- テスト `roles::tests` (分圧 2.50 V・RC 1.59 kHz・LED 9.09 mA・非反転 11 倍・プルアップ 500 µA)。
