# part-lib のメモ

## `kicad_sch.rs` — KiCad の回路図 (`.kicad_sch`) の読み取り(2026-11-08、第1段階)

`parse_schematic(text) -> KicadSch`。部品(`SchPart`: 参照名・値・`lib_id`・フットプリント・その他の欄・ピン)と、ネット(`SchNet`: 名前・つながるピン)を返す。絵は読まない(KiCad の記号とこのソフトの記号は形が違うので、部品は並べ直し、ネットラベルでつなぐ予定)。読まなかったもの(子シート・バス・定義のない記号・名前の違うラベルが同じ配線にあること)は `notes` に日本語で入る。

- 対応: ファイル形式 6〜9(`(kicad_sch ...)`)。記号の定義は、ファイルの中の `lib_symbols` から読む(`lib_name` があればそれ)。
- 接続の規則は KiCad と同じ: 配線は両端をつなぐ。配線の端・ピン・ラベル・ジャンクションが別の配線の**途中**にあれば、その配線につながる(T 字)。**ただ交差するだけの配線はつながらない**(交差の上にジャンクションがあればつながる)。ラベル(ローカル・グローバル・階層)と電源記号は、同じ名前なら1つのネット。名前は「電源記号 > グローバルラベル > ローカルラベル」の順で、同順位ならファイルの先頭のもの。名前のないネットは `N001`、`N002` …。
- 電源記号(`lib_symbols` の中に `(power)` がある記号、または参照名が `#` で始まるもの)は部品にせず、ピンのあるネットに名前を付けるだけ。`#FLG` は無視。
- 複数ユニットの部品(74HC00 の A〜D など)は、参照名が同じなら1つの部品にまとめ、ピンを全部持たせる。ユニットが 0 の副記号は全ユニット共通。スタイル 2 (ド・モルガン) のピンは読まない。
- ピンの位置: 記号の座標は y が上向き、シートは y が下向き。`place()` は「鏡像(`mirror x` は y を、`mirror y` は x を反転)→ 反時計回りに回転 → シートの座標へ(y を反転)」の順(KiCad は鏡像を先に掛ける)。座標はマイクロメートルの整数にして、同じ点かどうかを比べる。
- NC のバツ(`no_connect`)は、その位置にあるピンを `no_connects` に入れる。
- 検証: `ECL-OR.kicad_sch`(ngspice の例に付属)と、KiCad が書き出した `ECL-OR.cir` のネットが、ネットごとの部品の組み合わせで一致する(テスト `a_real_kicad_sheet_gives_the_nets_kicad_wrote`。この PC の `F:\Tools
gspice` にあるときだけ実行され、無ければ何もせず通る)。この実物は回転 0 度で、鏡像 y を含む。**回転 90〜270 度は実物で確かめていない**(自作のシートのテストで、KiCad の仕様から導いた位置と比べているだけ)。KiCad が入っている環境で `kicad-cli sch export netlist` と比べるのが確実。
- 大文字小文字: KiCad のネット名は区別する (`IN2` と `In2` は別のネット)。SPICE は区別しないので、`.cir` では同じに見える。ここでは別のまま。
## kicad_import.rs — `KicadSch` から `Schematic` を組み立てる(I1 第2段階)
- 入口は `import(&KicadSch) -> Imported{sch, notes, basic, custom}`。画面(ファイル → KiCad の回路図を開く… / 起動引数 `*.kicad_sch`)は `app/schematic/file.rs` の `load_kicad_sch`、CLI は `import_kicad FILE`。
- 絵は引き継がない。部品を CLI の `add` と同じ行詰めで並べ直し、つながりはピンの先端のネットラベルで表す(`net` コマンドと同じ)。ネット名 `GND`・`0` は `GND` にして接地記号を1つ置く。
- 基本の部品に変えるもの(`basic_kind`): `R`/`R_Small`/`R_US`、`C`/`C_Small`、`L`/`L_Small`、`D`/`D_Small`/`D_Schottky`/`D_Zener`、`LED`、`Q_NPN*`(ピン名 B/C/E)、`VDC`/`VSIN`/`VPULSE`(`Sim.Type`・`Sim.Params` から `5`・`SIN(...)`・`PULSE(...)` の値にする。パラメータが足りないものは推測せず、記号から作った部品にする)。ダイオードの KiCad のピン1=K・2=A。値の `4k7`→`4.7k`、`2R2`→`2.2`。
- それ以外は `kicad::to_part(sym, None)` の記号(ピン番号はそのまま)。モデルは付かないので `coverage` に「モデルなし」と出る。取り込みの案内にも参照名を並べる。
- フットプリントは、同じ名前か `_NNNNMetric` を外した名前(`R_0805_2012Metric`→`R_0805`)がこのソフトにあれば付ける。なければ案内に出す。DNP は `sim_only` にする。
- 参照名が `?` や重複のものは新しい名前にして案内に出す。
- 検証: 自作の分圧回路 `docs/examples/kicad-divider.kicad_sch`(9 V→4.5 V。`ccad` でも通る)と、実物 `ECL-OR.kicad_sch`(ネットごとの部品が KiCad と一致)。
- 未対応: 上の3つ以外の電源(EXP・PWL など。電流源は IDC・ISIN・IPULSE を 2026-11-09 から取り込む)、極性つきコンデンサの極性の記号(`C_Polarized`・`CP` は普通のコンデンサになり、案内に「ピン1が+」と出る)、`Spice_Model` 欄、子シート(第1段階の案内)。


## ltspice.rs / ltspice_import.rs / ltspice_expr.rs — LTspice の回路図 (`.asc`) の取り込み(2026-11-09、候補 J17)
- 入口は `ltspice_import::import_file(path, lib) -> Imported`(`Imported.directives` に SPICE の指示)。画面は「ファイル → LTspice の回路図を開く…」/ 起動引数 `*.asc`(`--dc` `--tran` `--ac` は取り込んだ設定で実行)で `app/schematic/file.rs` の `load_ltspice`、CLI は `import_ltspice FILE`。並べ直し・ネットのラベル・GND 記号は KiCad と共通(`kicad_import.rs` の `place_in_row`・`label_nets`)。
- 読み取り(`ltspice.rs`): 文字コードは UTF-16(印あり・なし)・UTF-8・Latin-1(古い版の `µ`)を見分ける(`decode`)。`WIRE`・`FLAG`(`0` は GND)・`IOPIN`・`SYMBOL`・`SYMATTR`・`TEXT`(`!` で始まるものが指示、`
` で行が分かれ、`;` の行は捨てる)。ネットは、線の両端をつなぎ、線の途中に乗った点(線の端・ピン・旗)もつなぐ(交差だけではつながない)。同じ名前の旗は 1 つのネット。`jumper` は部品ではなく、両端をつなぐ線として扱う。
- 記号の向き(`place`): `R90` は (x, y)→(−y, x)、`M` は回したあと左右反転(`M90` は (y, x))。LTspice 付属の例(Educational の 82 個)で、基本の部品のピン 2654 本のうち、どこにもつながらないのは 9 本(わざと開けたもの)まで確かめた(テスト `ltspices_own_examples_have_their_pins_on_wires`。LTspice が入っていないと何もしない)。
- 記号のピン: `.asy` を、`.asc` と同じフォルダ → `ltspice_symbol_dirs()`(`%LOCALAPPDATA%\LTspice\lib\sym`、`Documents\LTspiceXVII` など)の順に探す。名前が `OpAmps\LT1001` でも `LT1001` だけでも見つかるよう、下のフォルダまで索引を作る(`symbol_index`、LTspice と同じ)。基本の記号(res・cap・ind・diode・zener・schottky・led・voltage・current・npn・pnp・nmos・pmos)は内蔵(`builtin_symbol`)なので、LTspice がなくても読める。
- 値(`ltspice_import.rs`): `µ`→`u`、`""`→空、`{式}` は `.param`/`.params`(後ろで決まる値も使えるよう、決まらなくなるまで繰り返す。`.step param` は最初の値)から計算して数にする(`ltspice_expr.rs`: SPICE の接尾辞、`+ - * / ** ^`、`pi`、`sqrt` などの関数、`mc(x,tol)` は x、`4k7` は 4.7k)。電源は `SINE(...)`→`SIN(...)`(7 つ目の周期数は捨てる)、`AC 1` だけなら `0 AC 1`、`Value2` を後ろに足す。コンデンサの `Rser`/`Lser`/`IC` は `ESR=`/`ESL=`/`IC=`、コイルの `Rser` は `DCR=`。コイルに `Rser` が無ければ `DCR=1m`(LTspice の既定。これが無いと、電源とコイルのループが解けない)。定格(`V=` `Irms=` など)は捨て、それ以外の未対応の設定は案内に出す。
- 部品の決め方: res/cap/ind/voltage は基本の部品。それ以外は、値か記号の名前がライブラリの部品名と同じならその部品(ピンは名前で突き合わせ、ダイオードの `+`/`-` は A/K)。無ければ diode 類→基本のダイオード、led→LED、npn→基本の NPN、pnp・nmos・pmos は 2N3906・2N7000・AO3401A で代用(案内)。式・制御の電源(e・g・f・h・bv・bi)は取り込まず案内。電流源(current)は `Kind::ISource` に(ピンを入れ替える。向きは `app/analysis.md` の「電流源と負荷の急変」)。それ以外は `.asy` のピンから記号を作る(`kicad::to_part`、モデルなし)。同じ案内は部品をまとめて 1 行にする。
- 解析の指示(`analyses`): `.tran Tstop` / `.tran Tstep Tstop [Tstart [dTmax]] [uic]`(刻みは dTmax か Tstep、0 なら自動)と `.ac dec|oct|lin N f0 f1`(oct は ×3.32、lin は 50 点/デケード)を画面の欄に入れる。`.model`・`.lib`・`.include`・`K` などは案内に出して使わない。
- 検証: 付属の Educational 82 個のうち 76 個が取り込んで `dc` まで通る(通らないのは、音声ファイルの電源・非線形のコア・パルス発生器の記号・プローブ専用の電源・GND が IC の中にしかないもの)。全 4385 個が落ちずに読めることを `every_ltspice_example_reads_without_a_panic`(`--ignored`、約 1 分)で確かめた。サンプル `docs/examples/ltspice-rc.asc`(RC ローパス、`.param R=1.5k`、`.ac`)は画面のテスト `an_ltspice_sheet_opens_with_its_analysis_and_simulates` で -3dB点 1.06 kHz まで確かめる。
- データシートとの比べの表(`model_checks.rs`、`model_checks.json` を埋め込む): 部品の名前ごとに、測ったモデルの文の指紋(FNV-1a。行の前後の空白と空行は無視)・出典・行(項目・データシート・比べた相手 `target`・条件・モデル・差・`tested`)。書くのは cli のテスト `opamp_models.rs` `regulator_models.rs` の `write_*`(`tests/common/checks.rs`)。`for_model(name, text)` は指紋が合うときだけ返す。`bundled_origin(name, text)` は、出どころの欄が無い写し(回路図・ユーザーのライブラリ)に、同じモデルの同梱の部品の `sim.origin` を返す。出どころの記録が無い同梱の部品は `UNRECORDED` に並べる(テスト `every_bundled_model_says_where_it_comes_from_or_is_listed_as_unrecorded`)。モデルの出どころ `sim.origin` は sch-core の `ModelOrigin`(`OriginKind`)。部品作成フォームは、モデルの文が記録したときと同じ間だけ `origin` を残す(`PartForm::sim_origin`)。
- 同梱部品の入れ替え(`lib.rs` の `install_examples`): `.bundled` に記録した部品は二度と上書きしない。例外は1回きりの段で、鍵を `.bundled` に残す(`+model`・`+limits`・`+ratings`・`+shops` など)。モデルを作り直した部品は `+model-<同梱のモデルの指紋>`(`rebuilt_key`。作り直すたびに1回ずつ。2026-11 の版は `+model2026-11` で記録していた): ユーザーのファイルのモデルの文が `legacy_models.json`(6 部品の前の版のモデルと説明文。git の履歴から集めた)のどれかと同じときだけ、`sim` を新しい版に替え、説明文は前の版と同じときだけ替え、データシートのリンクは空のときだけ入れる(`with_rebuilt_model`)。ほかの欄はユーザーのまま。テスト `op_amps_with_an_earlier_bundled_model_get_the_data_sheet_model`。モデルを作り直したら、コミットの前に `python tools/collect_legacy_models.py` を回す(HEAD とモデルが違う同梱の部品について、git の履歴の前の版のモデルと説明文をこのファイルに足す。`--since REV`・部品名の指定・`--check` もある)。名前の付け替えは追わない(git の付け替えの検出が、似た部品から写した新しい部品を同じファイルとみなすため。同梱の部品のファイルは一度も名前を変えていない)。

## `series.rs` — メーカーのシリーズ (値ごとの部品に展開、2026-11-12、候補 P1・P2)

- `SeriesDef` (`crates/part-lib/series/*.series.json`、形式は `docs/part-format.md` の「シリーズ」) から、値ごとの `PartDef` を `part(v)` で作る (JSON を組んで `PartDef` に読み込む。`PartDef` に欄を足していないので、ほかの場所の `PartDef { .. }` はそのまま)。同梱は `BUNDLED`: Panasonic の ±1 % の厚膜チップ抵抗 4 つの大きさ (ERJ-1GJF 0201・ERJ-2RKF 0402・ERJ-3EKF 0603 は 10 Ω〜1 MΩ で 571 値、ERJ-6ENF 0805 は 10 Ω〜2.2 MΩ で 607 値。合わせて 2320 個)。E24 と E96 の共通は 1.0・1.1・1.3・1.5・2.0・7.5 の 6 つだけなので、1 桁に 114 値。データシートの ±1 % の表にあるほかの品は入れていない: ERJ-XGNF (01005、AEC-Q200 でない)、ERJ-1GNF (0201、1GJ と同じ大きさで AEC-Q200 でない)、ERJ-2RCF (0402 の 1〜9.76 Ω、温度係数が −100〜+600 ppm/K と片寄っていて `tc_ppm` 1 つで書けない)、ERJ-8EN・14N・12N・12S・1TN (新規設計に勧めない NRFND)。包装の符号は大きさで決まる (1GJ は C、2RK は X、3EK・6EN は V)。
- `lib.rs` の `load_library(dir)` = `load_dir(dir)` + 同梱のシリーズの部品 (同じ名前のファイルがあればファイルが勝つ)。シリーズの部品はライブラリのフォルダに書かない (571 個のファイルにしない。消す・直すの仕組み `.bundled` とも無関係)。アプリと CLI の読み込みはすべて `load_library`。テストと取り込みの試験は `load_dir` のまま。
- 名前 → (シリーズ, 値) は `member(name)` (初回に索引を作る)。`by_name("ERJ-3EKF")`、`nearest(v)` (いちばん近い値と、ちょうどか)、`neighbours(v)` (上下の値)、`default_value()` (10 kΩ があればそれ)。値の読み取り `parse_ohms` は `1M` をメガとして読む (SPICE の `M` = ミリとは違う。抵抗で 1 mΩ を書く人はいないため)。`4k7`・`2M2`・`100Ω` も読む。
- 型番の符号 `sig3`: 有効数字 3 桁 + 0 の数、100 Ω 未満は `R` が小数点 (Panasonic のデータシートの「1002 → 10 kΩ」「Decimal point is expressed by R」)。KOA RK73H・ROHM MCR も同じ形なので、次のシリーズはデータの JSON だけで足せる見込み。Vishay CRCW (`10K0` の形) は符号を足す。
- 値ごとの部品: モデルは `R1` だけの `.subckt` (名前は型番の `-` を `_` に) と `* spread R1 下限 上限 resistance` (許容差)。`ratings` は `p_max`、`v_max` = min(√(P·R), 素子の最高電圧)、表の行 (許容差・温度係数・定格電力と下げ方・素子の最高電圧・使える温度・過負荷電圧)。説明に品質の印・本体の寸法・ランドの出どころ。`origin` は `equivalent` で、確かめた資料と日付。
- ほかの大きさの数字も同じ表から: ERJ1GJ 0.05 W・25 V・50 V・±200・−55〜125 ℃ (下げ方も 70 ℃から 125 ℃で 0)・Grade 1・0.60 × 0.30 × 0.23 mm、ERJ2RK 0.1 W・50 V・100 V・±100・Grade 0・1.00 × 0.50 × 0.35 mm、ERJ6EN 0.125 W・150 V・200 V・±100・10〜2.2 M・Grade 0・2.00 × 1.25 × 0.60 mm。ランドは DMM0000COL17 の 0603 (0201)・1005 (0402)・2012 (0805) の行の真ん中。
- ERJ-3EKF の数字の出どころ: データシート AOA0000C304 (29-May-25) の Ratings の表 (ERJ3EK: 0.1 W・75 V・150 V・±1 %・10〜1 M (E24,E96)・±100・−55〜155・Grade 0)、Dimensions (ERJ3E: 1.60 × 0.80 × 0.45 mm)、型番の説明 (±1 % の ERJ 3EK F 1002 V)、Power derating curve (70 ℃から 155 ℃で 0)。ランドはランドパターンの資料 DMM0000COL17 の 1608 (a 0.7〜0.9・b 2.0〜2.2・c 0.8〜1.0) の真ん中。Panasonic のサイトは curl と Python の取得を受け付けなかった (時間切れ) ので、Node の fetch で取った。PDF の表は文字の抜き出しで崩れるので、`datasheet` クレートで絵にして読んだ。
- 店の番号 (`shops`) はまだ入れていない (DigiKey の番号は型番と違う。確かめたものだけ入れる方針)。部品の説明・モデルの限界の文は値ごとに作るので、英語の表 (`i18n`) には入っていない (英語の画面でも日本語のまま)。
- テスト: `series::tests` (型番の作り方・値の数と E 系列・全部品の `validate`・spread と定格電圧・ランドが推奨の真ん中・値の読み取りと近い値)。
