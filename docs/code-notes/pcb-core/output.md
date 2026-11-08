# pcb-core: 回路図の取込・フットプリント・発注前チェック・製造データ・STEP・DXF・SVG・購入リスト、ファイル一覧

(`docs/code-notes/pcb-core.md` の目次から分けた。内容は元のまま)

- `netlist.rs` 回路図→基板の取込 / `parts.rs` `place_part` / `footprints.rs`・`package.rs` フットプリント
- `fab.rs` 発注先プリセット(`PRESETS`)と発注前チェック(`preflight`)。`preflight` には料金の枠(`FabPreset.cheap_size`、JLCPCB・PCBWay は 100×100 mm。超えると Info、ブロックしない)と配線の電圧降下(`net_currents` を入れたネットで最悪のパッドが `MAX_DROP_V`=0.1 V 超なら Warn)も入る。回路図と基板の食い違いは `schematic_sync_check`(アプリが `SchDiff` から作って先頭に入れる。ネットの違い・部品の追加/削除は Error、値・形だけなら Warn) / `gerber_view.rs` 書き出したガーバー・ドリルを読み戻して絵にする(`Preview`)
- 部品のシルクの極性の印(`parts.rs` の `set_part`): 外形を持たないフットプリント(ライブラリ部品のダイオードなど、パッドだけのもの)は、パッドを囲む四角の枠になる。そのうち**パッドが `K` と `A` の 2 つだけで、2 つのパッドの間に 2 mm 以上のすき間がある**ものには、カソード側(`K` のパッドの内側 0.5 mm)に、枠を横切る太さ 0.3 mm の線(帯)を足す(DO-35 など。0805 のような小さい部品には足さない)。内蔵の `D_DO-35_7.62` と `LED_5mm` は、自前の外形と帯を持つので触らない。
- `footprints.rs` の `R_0805_Reflow` / `C_0805_Reflow`: YAGEO の RC 抵抗の推奨ランド(リフロー。全長 3.0・すき間 1.2・パッドの長さ 0.9・幅 1.2 mm。出典 YAGEO「Chip Resistor Surface Mount」Fig.4・Table 1)。ふつうの `R_0805`(すき間 0.8・パッド 1.025×1.4 mm)は一般的な CAD の値で手はんだしやすいが、YAGEO の推奨とは違う。Samsung の CL21 には推奨ランドの図が無いので、コンデンサも同じ数値。フットプリントの選択(回路図の部品の「フットプリント」、CLI の `footprint REF NAME`)で切り替える。
- 部品を作り直す生成テスト(`part-lib/src/tests.rs` の `write_chip_examples`・`write_kicad_mega_parts`)は、書き出す前に既存ファイルの `shops` を `existing_shops` で読んで引き継ぐ(店の型番は手で確かめたものなので消さない)。注意: 生成コードは部品ファイルより新しくなっていることがある(例: コイルの DCR の温度係数、説明文)ので、再生成すると `shops` 以外の差も出る。差を確かめてから採用する。
- `shops.rs` 購入リスト(値・フットプリント・店の型番ごとに行をまとめ、店ごとに型番のページか検索ページへのリンクを付ける。店のサイトは読まない)。型番は部品JSONの `shops` → `CustomSymbol.shops` → `Board.part_shops`(`netlist::import_shops` が取り込みのときに写す)。**抵抗・コンデンサのような基本部品には部品JSONが無いので、`Component.shops`(部品ごとの型番。回路図の「買える店の型番」欄と CLI の `shop REF digikey=…`)に持たせる。`Component::shop_numbers` が、部品JSONの型番に部品ごとの型番を重ねて(空欄はその店の型番を消す)取り込みへ渡す。基本部品のままなので、最悪ケース解析・ばらつき解析・自己発熱はそのまま使える(ライブラリ部品の抵抗・コンデンサは、最悪ケース解析で許容差のある部品として数えられない。2026-11-07 に確かめた)。**BOM の `LCSC Part #` 列もここから。店のURLの形は `SHOPS` に1か所
- `export.rs` 製造一式 (zip) / `gerber.rs` / `excellon.rs` / `assembly.rs` BOM・部品配置CSV / `strokefont.rs` シルク文字

### step.rs・mesh.rs — メーカーの STEP を3Dの形にする
`read_step(&[u8]) -> Mesh3d`(ミリメートル・部品の座標、`mesh.rs` の `Mesh3d` = 頂点・三角形・色の表)。文字コードは問わない(バイト列で読む)。読むもの: `MANIFOLD_SOLID_BREP` / `BREP_WITH_VOIDS` / `SHELL_BASED_SURFACE_MODEL` の面(`ADVANCED_FACE`)、面は平面・円柱・円錐・球・トーラス(ほかは「外形の点を通る平らな面」)、辺は直線・円・楕円・B-スプライン、長さの単位(`unit_mm`。インチ変換つき単位を先に見る)、色(`STYLED_ITEM` から `COLOUR_RGB` をたどる)。読まないもの: 組み立て(`NEXT_ASSEMBLY_USAGE_OCCURRENCE`・`MAPPED_ITEM`)による部品の移動、曲面の内側の点(丸い面は外形の点だけで切るので、ドームは少数の平らな面になる)。
- 三角形分割は `i_triangle`。**曲面の面は「長方形に点がたくさん」なので、`preserve_input_collinear` を立てて点を落とさないこと**(落とすと円柱の側面が2枚の三角形になる)。面の向きは、面の外向き(平面は軸、円柱・円錐は軸からの向き、球は中心から)に合わせて三角形の回り順を直す。テストは体積(発散定理)で向きを確かめる。
- 曲線のあらさは `Step::tol`(0.03 mm)。円は弧の高さが tol 以内になる分割数、B-スプラインは33点にしてから Douglas-Peucker。実物(アルミ電解コンデンサ、SolidWorks 出力、1.6MB)で約3400三角形・35ms。
- **組み立ての配置(2026-11-02)**: 足のように「1個の形を定義して、位置を変えて何度も置く」STEP は、`Step::placements` が配置の鎖(`Chain`=部品の座標→組み立ての座標の枠の組)を作り、置く場所の数だけ立体を読む。読むのは `MAPPED_ITEM`(`REPRESENTATION_MAP` の原点→対象の枠)と、2つの表現の間の `REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION`(単独でも複合でも。どちらが部品かは `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION` の組み立て使用で決め、無ければ rep1 が部品)の `ITEM_DEFINED_TRANSFORMATION`。配置がなければ従来どおり(座標のまま)。以前は未対応で、SOIC の足が全部中心の1か所に重なった。
- 三角形が `MAX_TRIANGLES`(30万)を超えると、部品でなく組み立て全体として断る。
- `model3d.rs` の `part_meshes(board)` が、`Board.models3d`(フットプリント名 → `Mesh3d`)を、部品の位置・回転・裏面(左右反転して、基板の下にぶら下げる)に合わせて並べる。メッシュを持つ部品は `part_boxes` から外れる(推測の箱を描かない)。
- 実ファイルで試す: `CCAD_STEP=<ファイル> cargo test -p pcb-core read_the_file_in_ccad_step -- --ignored --nocapture`。

- **ランドパターンが概算の部品**: 部品JSONの `land_unverified`(`PartDef`)→ 取り込みのときに `Board.land_unverified`(フットプリント名→部品名。アプリは `board_editor/file.rs`、CLI は `board.rs` の `import`)→ `fab.rs preflight` が「ランドパターンが概算の部品があります: U7 (ADS124S08)」を警告にする(テスト `tests_order.rs`)。同梱の `ADS124S08`・`ADG2712` と、`ccad part import` で作る部品(パッケージを借りる)が立てる。
- `picture.rs`(2026-10-26): `render(board, &PictureOpts)` が基板の絵(`Picture{w,h,rgba,legend}`)を返す。`gerber_view` の `Prim`・`Frame`・`rasterize` を使って、設計(ガーバーではない)から、層ごとに被覆を求めて色を重ねる(2×2 の超サンプリング)。順: 基板 → 他の層(うす) → ベタ → 配線・パッド・ビア → 穴 → ネットの色 → シルク(`strokefont`) → 格子と数字 → 未配線と DRC の番号。CLI は `board picture`(PNG は `image` クレート)。

- `board picture marks=1 finish=1` の DRC の印は、整えた形の検査(`check_final`、アプリと同じ)。`finish` なしは元の配線の検査。
- `requirements.rs` 設計の要件の表を基板に取り込み(`import_requirements`)と点検(`check`)。表は `design-spec` クレート(`DesignSpec`: `NetRule`・`PlaceGroup`・`KeepApart`。回路図 `Schematic.requirements` と基板が同じ型)。取り込み: ネットの行ごとに、その名前のネットクラスを作る/更新する(基板にあるネットだけ。ネットは他のクラスから移る。線幅・間隔が 0 の行は基板の値のまま。役割は `Role`)、電流を `net_currents` に、組と距離を `Board.place_groups`・`keep_apart` に(基板のものは置き換える)。`NetHints.requirements` から `import_hints` が呼ぶ。点検: 組はいちばん離れた2部品のパッドの重心どうし、離す組はいちばん近い2パッドの中心どうし(`a`/`b` は部品番号が先、なければネット名。末尾 `*` は前方一致)。上限・下限が 0 の行は調べない。基板にない名前は `missing`。自動配置は `Tuning.groups`(既定オン)で、組の部品(コピーや IC のブロックに入っていないもの)を1つの `Array`(圧縮した並び)にして動かす。ミリオームメーターの要件(入口・ブリッジ・電流経路の3組)で、入口の組の広がりが 72 → 40 mm 以内になり、通らない接続は 14 → 6 本、配線は +8%・ビア +20%。離す距離はまだ配置に効かない(降圧と ADC が約 10 mm で、要件の 20 mm を守れない)。部品間の線幅: `NetRule.links`(`LinkWidth{a,b,width}`)を取り込みで `Board.link_widths`(`model::LinkWidth{net,a,b,width}`。基板に両方の部品のパッドがあるものだけ。毎回置き換え)にし、`autoroute.rs` の `autoroute_mode` が、通常の最短の木より先に、その2部品のパッドのいちばん近い組(`link_airwire`。すでにつながっていれば何もしない)を、その幅で引く。ネットの残りは、あとから(木の辺で)つながる。その幅で引けなければ(小さい IC のパッドに太い線を付ける、など)ネット自身の幅で引き直し、それでも引けなければ通らない接続として残る。ミリオームメーターの BATP で試すと、J4 のパッドには 0.8 mm が付き、U7(0.3 mm 幅のパッド)への 0.8 mm は引けず、BATP の通らない 2 本は前と同じだった。 離す距離は自動配置のコストにも入る(`Tuning.apart`、既定オン。`autoplace.rs` の `Apart`・`apart_cost`): 2つの辺の部品(部品番号か、そのネットに足のある部品。`requirements::apart_on_board`)のマス(コピー・IC のブロック・組は1つのマス)のすき間が下限に足りない分(mm)に `APART_WEIGHT`(300)をかけて、ネットの長さに足す。焼きなましの順番の評価、`attach_layout` の置き場所の選び(足りないときは盤全体を 1 mm 格子で探す)、仕上げの `repair_apart`(足りない部品を空きへ1つずつ動かす)で使う。重みは 10・30 では効かなかった(大きな塊の引っぱりに負ける)。ミリオームメーターで、降圧(BKSW 側)と ADC の側 20 mm の違反が 5.8 mm → なし。未配線 6 → 4、配線 +3.2%、ビア +3.4%、組「電流経路」(Q1-RS1 は塊になっていない)は 11.7 → 22.0 mm に悪化。マスはパッドより大きいので、実際の距離は下限より少し余る。 組の上限も同じ形でコストに入る(`Apart.near`: 組の部品のマスの中心どうしが `max_dist` を超えた分(mm)に `APART_WEIGHT` をかける。塊が1つのマスになった組は対象外)。塊に入れられなかった部品(コピーか IC のブロックにまとまっている部品)は `PlaceReport.group_notes` で知らせる(CLI の `board autoplace` の答え `group_notes`、基板のパネルの「配置: …」)。ミリオームメーターで、組「電流経路」(Q1-RS1。RS1 はコピーの中にいて塊にできない)は 22.0 → 11.8 mm(上限 10 mm。マスの中心とパッドの重心は少し違う)、未配線 4 → 5、配線 +0.5%、ビア 301 → 306。発注前チェック(`fab::preflight`)にも、要件を満たしていない所を警告(場所つき)、満たしていれば OK で出す。`marks` の `GroupMark.at` は組のパッドの重心。 バイパスの対応(`design_spec::Bypass`: IC と、そのコンデンサの一覧。`Board.bypass`): 電源のネットが共通だと、どのコンデンサがどの IC のためかは回路図から分からない(ラベルでつなぐと置いた場所も当てにならない)ので、表に書く。自動配置は、表にあるコンデンサを、その IC のブロックに入れ(`ic_blocks`)、引き寄せも(`bypass_nets`・`attach_bypass_caps`)その IC のピンだけにする(`bypass_ic`)。書いていないコンデンサは、これまでどおり「両側のレールを持つ IC → 持っているコンデンサの少ない IC」。点検は、コンデンサの電源パッドと、その IC の同じネットのいちばん近いパッドの距離が `MAX_DECOUPLING_DISTANCE`(8 mm)を超えると指摘(`FindingKind::Bypass`)、`marks().bypass` はその線(基板の絵に細い線と「C5→U3 3.2 mm」。緑・赤)。他のソフトの様子(調べた範囲): Quilter は回路図の結線→電圧名のピン→(ラベルは低い優先)で自動に決め、人が上書きできる。Allegro X AI は部品の対応表(CSV)。KiCad は自動の対応づけなし。
  - 測定 `tap`(`measure_one`): 実装用パッドから行き先への銅の道筋(`CopperGraph::path`)のうち、パッドの矩形の中にある最後の点がつなぎ目。値は、そこから「もう片方のパッドに向いた辺」までの深さ(0=内側の縁、パッドの全長=外側の縁)。描く線はつなぎ目から内側の縁まで。パッドは回転後の幅・高さの矩形とみなす(斜めのパッドは測れない)。
  - 測定 `gap` の `except`(`Measure.except`、`drc::net_gap_except`): 間隔から除く部品の名前のリスト。`pads` と書くと全部のパッドを除いて、配線とビアの間隔だけを見る。IC の電源ピンと入力ピンは近くて当然なので、「信号線を電源線から 5 mm 離す」のような配線の規則はこれで書く(ADA4522 のデータシートの推奨)。
  - 測定 `area`(`drc/area.rs` の `pad_copper_area`): 指定のパッド(同じ部品・ピンのパッドは全部。露出パッドとピンを合わせる)につながった銅の面積(mm²)。同じ接続の組(`Connectivity`)のパッド・ビア・配線と、そのネットのベタのうち、組のパッド・ビア・配線の端を中に含む塊(`zone_fill`)を、層ごとに 1 つの形に合わせて(`zonefill::copper_area`。重なりは 1 回)面積を出し、層の分を足す(貫通のパッドとビアは銅のある全部の層に入る)。`Measure.reach` が 0 より大きいと、1つ目のパッドの中心からその半径の円の中だけ。NDT3055L のドレインの銅(1 in² = 645 mm² で 42 K/W)のような、放熱の銅の面積の下限を書く。ネット全体の銅なので、そのパッドから遠くへ伸びる配線(ブリッジへの太い線など)も入る(放熱に効く銅は、パッドの近くの分だけ)。
  - 道筋のビアの数(`path.rs` の `CopperGraph::path`): ビアは層ごとにノードを持ち、隣の層どうしを辺でつなぐので、内層のある基板の貫通ビアは辺が何本も続く。続く辺は1個と数える(以前は辺ごとに数え、4層で貫通ビア1個が3個になっていた。2026-11)。テスト `a_via_through_inner_layers_is_one_via`
  - 測定 `width`(`drc/path.rs` の `narrowest_on_way`): 2 つのパッドの間の道筋(`CopperGraph::path`)の点を 2 つずつたどり、その区間を載せている配線の太さの一番細いもの(場所は区間の真ん中)。ネットクラスの太さは設定で、これは実際の配線が途中で細くなっていないかを見る(QS・RT1 の電流の線など)。
  - 測定 `parallel`(`drc/path.rs` の `parallel_run`): 1つ目のネットの配線を 0.2 mm ごとに見て、2つ目のネットの銅(`except` のパッドを除く)が、同じ層で縁から `reach`(既定 1 mm)以内にある長さ。ガードの被覆と同じ刻み方。距離の最小値(`gap`)では分からない、雑音の結合の長さを見る(降圧のスイッチングノードと検出線など)。
  - 測定 `resistance`(`path.rs` の `way_resistance`。トラックは長さ×幅、層をまたぐビアは `irdrop::via_resistance`)・`under`(`under_cover`。隣の層は積層の上下1つ)・`stub`(`path.rs` の `stubs`。トラックを他の配線の端で切り(端が他の配線の銅の中にあれば、二つの半幅の内側で、つながっているとみなして中心線に寄せる)、パッド・ビア・ベタに触れていない端(トラックの丸い端が半幅だけ届けば触れている)から、つながる所か3本の分かれ目まで戻る。両端が自由な配線は数えない)・`balance`(ビア数の差 + `way_bends` の差)・`drop`(`way_resistance` × 電流。電流は `Measure.reach`、0 ならネットの電流)・`return`(`return_path`。信号の道筋の隣の層のグラウンドの銅を 0.3 mm の格子にして8方向の最短路。隣の層は1つの銅として扱う。信号の道筋が両端の間の直線より短いときは、直線との比。見つからないと 1000 %)・`overlap`(`layer_overlap`。層の組ごとに `zonefill::overlap_shapes`)、`thermal_vias`(`drc/area.rs` の `pad_via_count`): パッド(同じ部品・ピンの全部)の外接の四角の中にあるビアの数。
  - 測定 `loop`・`match`(`measure_one`): パッド 4 個 = 2 本の道筋(出発,行き先,出発,行き先)。`loop` は 2 本の長さの合計(降圧の入力ループ: IC の電源ピン→コンデンサのプラス、コンデンサのマイナス→IC のグラウンドピン)、`match` は長さの差(対になる経路)。ビアの数やはんだ接合の数の差は見ない(必要なら `vias` の行を足す)。
  - 測定 `ring`(`drc/path.rs` の `ring_cover`): パッドの中心から 72 方向に、パッドの縁から `reach`(既定 1 mm)の範囲を 0.1 mm ごとにたどり、ガードのネットの銅(貫通のパッドは両面)に当たる向きの割合(%)。パッド 1 個とガードのネットを `pads` に書く。

- 取り込みで基板に届かなかった行(`RequirementsImport.unmatched` = ネットのパッドが基板にない役割の行 / `dropped_layers` = 基板にない層)は `warnings()` で文にして、取り込み後の注意(`import_notes`)と CLI `board import` の `warnings` に出す(`netlist::import_hints_report`)。表を直したあとは、`BoardEditor::sync_requirements`(`board_editor/file.rs`)が、表を直したとき・回路図を開いたときに立つ旗(`SchEditor.req_dirty`)を見て、回路図の表と最後に取り込んだ表(`req_synced`)を比べ、違えば `import_requirements` だけを(Undo つきで)やり直す。開いたばかりで最後が分からない基板は、基板が持つ組・距離・バイパス・例外・測定と比べる。

## dxf.rs — ケース設計用の DXF(2026-11-08)
- `dxf::mechanical(&Board) -> String`: ASCII の DXF R12(AC1009。LINE・CIRCLE・ARC・TEXT だけ)。単位は mm。基板の y も DXF の y も上向きなので、そのまま書く(上から見た形になる)。
- 層: `BOARD_OUTLINE`(外形の線)、`HOLES`(取り付け穴。長穴は2本の線+2つの円弧)、`PAD_DRILLS`(パッドのドリル)、`PARTS`(部品のパッド全体を囲む箱。部品の本体の大きさではない)、`PART_NAMES`(部品番号)。
- 入口: 画面の「ファイル → 基板の外形・穴・部品の位置をDXFで保存…」(`board_editor/file.rs` の `export_dxf`)、CLI の `board dxf FILE`。
- 確認: 単体テスト(線・円・円弧の数、y の反転)だけ。ケースの CAD で開いての確認は、まだしていない。

## svg.rs — 基板の1つの面を SVG に(2026-11-08)
- `svg::board_svg(&Board, &SvgOpts{side, silk}) -> Result<String,String>`。銅・シルク・穴は、発注用の Gerber・ドリルの文字(`gerber::copper`・`silk`、`excellon`)を `gerber_view::parse_gerber` で読み戻して描く。だから、ベタ・仕上げ・ベタの抜きも含めた「できあがりの姿」になる(画面の描画とは別の経路)。
- Gerber の「消す」(clear)は、そこまでの絵を `<mask>` で包んで抜く(消したあとに描いたものは抜かれない)。
- 基板の y は上向き、SVG は下向きなので、全体を `scale(1,-1)` の組で包む。裏面は `scale(-1,-1)` で、下から見た向き(左右反転)。大きさは mm(`width`/`height` が mm)。
- 色は表面=金、裏面=青、内層=紫、基板=緑、シルク=白。入口は画面の「ファイル → 基板の表面/裏面をSVGで保存…」(`board_editor/file.rs` の `export_svg`)と CLI の `board svg FILE [side=] [silk=0]`。
## dxf_in.rs — ケースの図面から外形と穴を取る(2026-11-08)
- `dxf_in::read(text, layer) -> Result<DxfShape{outline, holes, notes}>`、`apply(&mut Board, &shape, to_origin)`。ASCII の DXF の LINE・ARC・CIRCLE・LWPOLYLINE・POLYLINE(VERTEX のふくらみ `42` も)だけを読む。それ以外(TEXT・SPLINE・INSERT など)は読まずに、種類と数を案内に出す。
- 端が 0.01 mm 以内で合う線・円弧・ポリラインをつないで輪にする(向きが逆でもよい)。閉じた輪のうち面積がいちばん大きいものが外形。ほかの輪(切り欠き・長穴)は案内に数だけ出す。外形の中にある円が穴(外にある円は穴にしない)。閉じた輪がなければ、層の名前を挙げて断る。
- 入口: 画面「ファイル → ケースの図面 (DXF) から外形と穴を取り込む…」(`board_editor/file.rs` の `import_dxf`。元に戻せる)、CLI `board dxf_import FILE [layer=] [origin=0]`。
- 確認: 自分で書いた DXF の読み戻し(外形の面積・穴の位置)、線3本+円弧1つ(順不同)、インチと LWPOLYLINE、ふくらみ(半円)、開いた形と DXF でないファイルの断り。実際のケースの CAD が書いた DXF では、まだ確かめていない(バイナリ DXF・SPLINE・楕円は未対応)。

## case.rs — 箱のケースに入るか(E2。2026-11-08)
- `case::check(&Board, &CaseSpec{inner_w, inner_d, top, bottom, clearance}) -> CaseReport`。底に入るか(縁の隙間 `clearance` を両側に見込む。90° 回せば入るときは `turned`)、部品の高さ(`model3d::part_height_on`。表の部品は `top`、裏の部品は `bottom` の高さと比べる。0 は調べない)。基板の大きさは外形と部品を囲む箱。
- 入口: 3D 表示の「ケース高さ」を入れたときの「内寸 幅・奥行」(`view3d/window.rs`。青い枠が内側。入るかの一行を出す。枠は `scene.rs` の `set_case`)、CLI の `board case`。
- 未対応: ねじ柱・コネクタの穴あけ・ケースの STEP との干渉(E4)。基板の固定穴とケースの位置合わせもしていない。

## model3d.rs の背の高い部品(E1 の第1段階。2026-11-08)
- ケースの高さの確認(`case.rs`・3D の「ケース高さ」)は `part_height_on` を使う。以前は TO-220 が 4.5 mm、端子台・トランス・電池ホルダ・ヒューズが 2〜3 mm 扱いで、実物より低く出て、ケースに入らない部品が見逃されていた。
- `Kind::To220`(フットプリント名に `to-220`): 脚の並びにそって 10 × 4.5 mm、高さ 9.3 mm の樹脂の本体 + その上に 6.5 mm 出る金属の板(タブ)+ 脚。合計 15.8 mm。タブがどちら側を向くかは分からないので、決め打ち(基板の y が小さい側 / x が小さい側)。
- `TALL_FOOTPRINTS`(フットプリント名の一部 → 本体の高さ・色)に、端子台 10・トランス(EI30)25・電池ホルダ(単3)16・(CR2032)5・ヒューズホルダ(5×20)9 mm。**一般的な形の目安で、メーカーの図面ではない**。フットプリントの `height` があればそちらが優先。
- `Kind::To92`(`to-92`): 脚の並びにそって 4.8 × 3.8 mm の本体(パッドの大きさではなく、パッケージの大きさ)+ 細い脚。以前はパッドを囲んだ薄い板だった。表面実装の水晶 `xtal_3225` は 0.9 mm。
- 確認: 単体テスト(TO-220 の合計の高さ・部品の箱の数・ケースの確認が拾うこと)と、`mixed_signal` の 3D で背の高い部品が出ること。形の細かい見た目(タブの向き・USB やジャックなどのコネクタ)は未対応。

## ファイル一覧(pcb-core)

(CLAUDE.md の地図から移した、ファイルごとの詳しい一覧。CLAUDE.md には1行ずつの要約だけ残してある)
- `model.rs` 全データ型 (層は `Side`、内層は `Board.inner_layers`) / `footprints.rs`・`package.rs` フットプリント / `parts.rs` `place_part` / `netlist.rs` 回路図→基板の取込
- `requirements.rs` 回路図の「設計の要件」の表を基板に取り込み(ネットクラス・電流・組・距離)、組の広がりと離す距離を点検する。自動配置は組を1つの塊にする(`ccad board req`)
- `step.rs`・`mesh.rs` メーカーの STEP を三角形のメッシュ(`Mesh3d`)にして、部品の3Dの形にする(`PartDef.model3d` → `Board.models3d` → `model3d.rs` の `part_meshes`)
- `svg.rs` 基板の1つの面の SVG(Gerber を読み戻して描く。画面のファイルメニューと `ccad board svg`)
- `link.rs`(app)回路図・基板・3D で選んだ部品とネットの連動(`App::sync_focus`。設定でオフにできる。詳細 `docs/code-notes/app.md`)
- `case.rs` 箱のケースの内寸に、基板が入るか・背の高い部品がないか(3D 表示の「内寸」と `ccad board case`)
- `dxf_in.rs` ケースの図面(DXF)から外形と穴を取る(画面のファイルメニューと `ccad board dxf_import`)
- `dxf.rs` ケース設計用の DXF(外形・穴・パッドのドリル・部品の箱。画面のファイルメニューと `ccad board dxf`)
- `shops.rs` 購入リスト(店の型番・検索リンク。`ccad board shopping`、`*-shopping.csv`)
- `fab.rs` 発注先と発注前チェック / `export.rs` 製造一式 / `gerber.rs` / `excellon.rs` / `assembly.rs` BOM・配置CSV / `gerber_view.rs` 書き出しの読み戻し / `strokefont.rs` シルク文字
- model3d.rs の形の追加(E1 の第2段階、2026-11-08): `Kind` に `Shell`(USB・DC ジャック・RJ45 の金属シェル)・`Terminal`(端子台)・`Transformer`(EI トランス)・`Buzzer` を足した。見分けはフットプリント名(`usb`・`jack`・`barrel`・`rj45`・`terminalblock`・`transformer`・`buzzer`)。シェルは部品番号が J・P・X のときだけ(USB の保護 IC を コネクタにしないため)。シェルは低い黒い土台の上に銀色の箱(土台は高さの12%、0.8〜1.6 mm)。端子台は緑の箱の上にパッドごとのねじ(円柱+溝の細い箱)。トランスは鉄心(高さいっぱい、パッドの長い辺の両端を12%ずつ削る)と、その両端にはみ出す銅色のコイル(高さの70%)。ブザーは黒い円柱(直径はパッド間隔の1.6倍、最小9 mm)の上に音の穴の黒い円。**高さは `TALL_FOOTPRINTS` の目安**(`usb` 7.0・`usb_micro` 2.8・`usb_c` 3.3・`usb_b` 11・`barrel` 11・`jack` 10・`buzzer` 9)で、実物とは数 mm 違いうる(部品の `height` か STEP が優先)。ケースの高さの確認(`part_total_height`)にも効く。同梱の部品で形が変わるのは USB_DEMO_TH・端子台(AC3P)・トランス(TR100 など)。ジャック・ブザーの同梱部品はまだ無く、名前で形が出るだけ。
