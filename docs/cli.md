# ccad: 画面なしで使う (AIエージェント・自動化向け)

`ccad` は circuitCad の計算部分を、窓を開かずに使うコマンドです。**1行に1コマンド**の短いスクリプトを渡すと、**1コマンドにつき1行のJSON**で答えます。回路は座標ではなく**部品名とピン名**で作るので、AI(LLM)が書いても、つながらない事故が起きにくくなっています。

```bash
cargo run -p cli -- script.ccad        # ファイルから
cargo run -p cli -- -c 'new\nadd R 1k' # その場で (\n で改行)
cat script.ccad | cargo run -p cli -- - # 標準入力から
```

- 失敗した行で止まります(終了コード1)。続けるには `--keep-going`。
- **`"ok": true` は「コマンドが動いた」という意味で、「回路が正しい」という意味ではありません。** 回路の点検は `erc` の答えの `errors`(0なら配線の誤りなし。`circuit_ok` も同じ)で見ます。`--strict` を付けると、`erc` にエラーがあるとき `erc` が失敗して止まり、`dc`・`tran`・`board import` も実行を拒みます。AIが書くときは `--strict` を勧めます。
- 部品ライブラリは、アプリと同じ場所を読みます。別の場所は `--lib DIR`。
- `ccad help` で、コマンドの一覧が出ます。

## 約束事
- **名前が `GND` のネットが、0V の基準です。** どの回路にも1つ必要です(`net GND V1.- R1.2` のように作ります。最初の `GND` が、GNDの記号も置きます。手で置く必要はありません)。
- **ピンは番号でも名前でも書けて、大文字小文字は区別しません**(`U1.out` と `U1.OUT` は同じ)。部品のピンは、先に `part 部品名` で調べます。基本の部品のピン: `R`/`C`/`L` は `1` `2`、`D`/`LED` は `A` `K`、`V` は `+` `-`、`NPN` は `C` `B` `E`。
- 1つのピンに、別の名前のネットは付けられません(エラーになります)。直すには `remove REF` で部品を消して、置き直します。
- `add R` で値を省くと、既定値(R は 10k、C は 100n、LED は LED)が入ります。値は書くほうが安全です。
- `parts 語` は、部品名・カテゴリ・説明文(日本語が多い)を検索します。英語の機能名では見つからないことがあるので、`parts 7805` のように型番で探してください。
- `dc` は、ライブラリ部品のモデルの内部の素子とノード(`U1.Ex`、`U1.xh` のように名前に `.` があるもの)を隠します。内部の電圧や電力は回路の実際の値ではありません。見るときは `dc full`。
- `dc` の答えには、**回路が「動いた」だけでなく「使える」かを見るための知らせ**が付きます。`ratings` は定格のチェック(アプリと同じ。抵抗・トランジスタ・LED などが、形ごとの電力の目安を超えている/近い。`level` は `over`/`caution`。データシートではなく目安)、`warnings` には、オペアンプの IN+ と IN- の電圧が違うとき(負帰還が効いていない = 出力が電源の手前で頭打ち)の注意が出ます。コンパレータとして使うオペアンプなら無視してかまいません。`tran` にも同じ形で付きます(`ratings` と `warnings`。後半の半分だけを見て、立ち上がりは除きます): 抵抗の消費電力(実効値)、LED の最大電流、電解コンデンサのリップル電流(部品のデータシートの定格がある場合。アプリと同じ計算)、オペアンプの入力差(時間の2割を超えて続くとき)。発振回路やコンパレータのオペアンプには、警告が出ることがあります(文に「無視してよい」と書いてあります)。トランジスタ・ダイオードの電力は、`dc` だけが見ます。
- `dc`・`tran`・`board import` の答えにも `circuit_ok` が付きます。`false` なら、ERCにエラーが残っています(`warnings` に理由)。数値を信用する前に `erc` を見てください。
- **`tran` は、DC動作点から始めます**(コンデンサは最初から充電されています。直流電源につないだRCなら、最初から定常値で、変化しません)。**ゼロから立ち上がる様子(ステップ応答、発振)を見るときは `uic` を付けます。** 付けないと「壊れた回路」に見える結果になります。
- `tran` の `samples` は、0 から終了時間までを等間隔に取った点で、最後の点は終了時間です。特定の時刻の値が欲しいときは、その時刻を終了時間にして実行します。

## 例: LED を 5V から(いちばん小さい回路)
```
new
add V 5 as=V1
add R 270 as=R1
add LED as=D1
net VCC V1.+ R1.1
net LEDA R1.2 D1.A
net GND V1.- D1.K
erc
dc
```
`dc` の答えの `elements.D1.current` が、LED に流れる電流(約 11 mA)です。

## 例: RC フィルタのステップ応答(`uic` を付ける)
```
new
add V 1 as=V1
add R 1.6k as=R1
add C 100n as=C1
net IN V1.+ R1.1
net OUT R1.2 C1.1
net GND V1.- C1.2
erc
tran 160u 1u probe=V(OUT) samples=5 uic
```
時定数 R×C = 160 µs のとき、`V(OUT)` の `final` は 0.632 V(= 1 − 1/e)です。`uic` を付けないと、最初から 1 V のままになります。

## 例: 間違いを直す
ピン名を間違えると、使えるピンが答えに出ます。その一覧から選んで、書き直します。
```
net LEDA R1.2 D1.ANODE
→ {"ok":false,"error":"D1 has no pin 'ANODE' (pins: A K)"}
net LEDA R1.2 D1.A
```

## 例: 抵抗の分圧
```
new
add V 12 as=V1
add R 10k as=R1
add R 4.7k as=R2
net VIN V1.+ R1.1
net MID R1.2 R2.1
net GND V1.- R2.2
erc
dc
```
`dc` の答えの `voltages.MID` が 3.8367 V(= 12 × 4.7k / 14.7k)になります。

## 例: 555 の点滅を、基板と製造データまで
```
new
add V 5 as=V1
add NE555 as=U1
add R 1k as=R1
add R 68k as=R2
add C 10n as=C1
add C 100n as=C2
add LED as=D1
add R 330 as=R3
net VCC V1.+ U1.VCC U1.RESET R1.1
net GND V1.- U1.GND C1.2 C2.2 D1.K
net DIS R1.2 R2.1 U1.DISCH
net TIMING R2.2 C1.1 U1.TRIG U1.THRES
net CTL U1.CONT C2.1
net OUT U1.OUT R3.1
net LEDA R3.2 D1.A
erc
tran 5m 5u probe=V(OUT)
save blink.schjson
board import
board autoplace
board route
board drc
board fab out/
```
`tran` の答えには `period`(約 0.94 ms。理論値 0.693 × (R1 + 2 R2) × C1 = 0.95 ms)と `duty` が入ります。`blink.schjson` はアプリでも開けます。

## コマンド
| コマンド | 内容 |
|---|---|
| `new` / `open F` / `save F` | 回路図(`.schjson`)。 |
| `parts [語...]` / `part 名前` | 部品ライブラリの検索と、ピン(番号と名前)・シミュレーションの有無。`part` は基本の部品(`R` `LED` など)のピンも出す。 |
| `add 種類 [値] [as=U1] [rot=0..3] [at=X,Y]` | 部品を置く。種類は `R C L D LED V NPN HEADERn` か、ライブラリの部品名。位置は自動。 |
| `value REF 値` / `remove REF` | 値の変更 / 部品の削除。 |
| `net 名前 REF.PIN ...` | ピンを名前でつなぐ。ピンは番号でも名前でもよい。別の名前がついたピンは拒否。 |
| `list` | 部品・ピン・各ピンのネット。 |
| `erc` / `spice` | 配線の点検(原因と直し方つき。`errors` と `circuit_ok` を見る) / SPICEネットリスト。 |
| `dc [full]` | 動作点: 各ネットの電圧、各素子の電流・電圧・電力(部品モデルの内部は `full` のとき)。 |
| `tran 終了 [刻み] [probe=V(out),...] [samples=6] [uic]` | 過渡解析の要約(最小・最大・平均・最終値・周期・デューティ比)。刻みは最大の刻みで、省くと 終了/1000(誤差の大きい所は自動で細かくなる)。 |
| `models [語...] [zip=PATH] [size=0603] [diel=X7R] [cmin=1u] [cmax=10u] [vmin=16]` / `models model=型番` | メーカーが配るSPICEモデルのzip(村田の「SPICE netlist」など。規約で再配布できないので自分でダウンロードする)から探す(別名 `murata`)。`zip=` を省くと、アプリで追加したzipをすべて使う。型番の語は空白区切りで全部を含むもの。`size`(インチ表記: `0603`=1608)・`diel`・`cmin`/`cmax`・`vmin` は、内容を読み取れるモデル(今は村田・日本ケミコン・ニチコン。サイズ・温度特性は村田だけ)にだけ効く。`model=` は等価回路と注意(村田は25℃・DC 0Vの値で、直流電圧による容量低下は入っていない)を返す。`models use 型番` は、そのモデルをこの実行のライブラリの部品にして(2端子のモデルだけ。ピン1・2、電解は `+`/`-`、コンデンサの記号、値・定格電圧・組み込みフットプリント `C_0603`/`C_0805` は読み取れたときだけ)、続けて `add 型番 [値] as=C1` で置ける。答えの `todo` に、まだ自分でやること(大きさの情報がなく基板に使えない、値と定格電圧が不明、など)が出る。ライブラリのファイルには保存しない(実行のあいだだけ)。 |
| `calc [名前 数値...]` | 値の計算(LEDの抵抗・分圧・RCフィルタ・降圧/昇圧・オペアンプ・電池の持ち時間など14種。アプリの「値の決め方」と同じ式)。名前に `_mA` `_kHz` などが付く入力は、その単位の素の数字で書く(`10`。`10m` は拒否)。LEDの電圧など入力に使う値は、`dc` で使う部品モデルと少し違うので、電流は計算と数%ずれる。引数なしで一覧と入力の順番。答えは値・手順・注意・故障時の様子。 |
| `ac 開始Hz 終了Hz [ppd=10] [probe=V(out)]` | 周波数応答(電源の値に `AC 1` が要る: `value V1 "0 AC 1"`)。1デケードごとのゲインと位相、-3dB点(周波数の点を補間して求めるので、`calc` の理論値と0.1%ほどずれる)、位相余裕。 |
| `spec 種類 対象 下限 上限 [at=Hz]` | 直前の `dc`/`tran`/`ac` の答えが範囲に入っているか(`pass`)。`-` は制限なし。引数なしで種類の一覧。回路を編集すると答えは消える。TARGET は `dc_voltage` がネット名(`MID`)、`dc_current` が部品名(`D1`、単位はA: `8m 12m`)、`tran_*`/`ac_*` が信号名(`V(OUT)`)。`at=` は `1k` のように書ける。`pass:false` でもコマンドは `ok:true`(実行できたという意味)。 |
| `board new [幅 高さ]` / `import` / `pour [net=名]` / `autoplace` / `move REF X Y [rot=0..3]` / `route [net=名] [lead=mm] [currents=0] [improve=1]` / `improve [net=名]` / `labels` / `class` / `drc` / `status` | 基板(mm)。取り込み・ベタ・配置・自動配線・DRC。`move` は1個の部品を指定の位置(部品の原点、mm)へ動かす(配線は動かない)。`pour` はアプリの「GND ベタ」(裏面を銅箔のベタにして、表面のパッドごとにビア。答えは `zone_added`・`vias`・`already`・`left`)で、`import` のあと `autoplace` の前に使う。`autoplace` は、ベタ(ネットのあるゾーン)がある基板では、そのネットを計算から外して部品を詰め(`poured_nets_ignored`)、ベタを新しい外形に合わせ、ベタのビアと短い線は置き直す(`repoured_vias`。ほかの配線があると断る)。`route` はアプリと同じく、パッドの辺に垂直に `lead`(既定0.5mm。0で切る)出入りするよう根元をそろえ(`aligned_pads`)、一直線につながる配線を1本にまとめ、まとめた数を `joined_tracks` で返す。電流がまだ決まっていない基板では、回路の動作点(DC)の電流を先に基板に入れ(`currents_declared`)、必要な太さが `track_width` より太いネットは、その太さで先に引く(`wide_nets`。引けなければ普通の太さで引いて、`board check` が細いと警告する)。`currents=0` で切る。`pour` は既定で表面の空いた所もGNDのベタで埋める(`top_added`。`top=0` で切る。ルール違反が増えるときは置かない)。`route` は、裏面にGNDベタがあって表面にまだなければ、配線のあとに表面を埋める(`top_pour`。`top=0` で切る)。`improve=1` を付けると、引いたあと遠回りした配線やビアを引き直して短くする(`rerouted_nets`・`saved_mm`。自動配線の直後は得が小さいので、既定では切ってある)。`improve` は基板全体(手で引いた配線も)に同じ引き直しをかけ、配線の長さ・ビア・本数の前後を返す。`class` はネットクラス: `board class` で一覧、`board class 名前 [width=mm] [clearance=mm] [via=径/穴] [nets=A,B]` で作成・変更(ネットは、ほかのクラスから移る。基板の最小線幅・最小間隔より小さい値は断る)、`board class 名前 remove=1` で削除。クラスは、`route` の線幅・ビア・間隔と DRC に効く(基板ファイルに保存される)。`labels` は、配線・パッド・ビア・穴・ほかの文字に重なっている部品番号のシルク文字を、部品の上・下・横の重ならない場所へ動かす(答えは `moved`・`still_touching`。手で動かした文字はそのまま)。 |
| `board currents [tran] [from=F] [widen]` | `dc` と同じ動作点(`tran` を付けると、直前の `tran` の後半 F 以降(既定0.5)の実効値。スイッチング電源など DC で電流が決まらない回路用。ピークは `stats` に出すが太さには使わない。`spiky:true` はピークが実効の10倍超=起動サージやスパイクの可能性)から各ネットの電流を求め、基板のネット電流に入れて、必要な配線の太さ(IPC-2221、温度上昇10℃・35µm)と今の最細の太さを返す。`widen` で細い配線を必要な太さに太くする(あとで `board drc`)。 |
| `board postlayout on\|off` | `on` にすると以後の `dc`/`tran`/`ac` に、配線した基板の銅の抵抗とインダクタンス(電源の入口パッドから各パッドまで)がパッドの手前に直列に入る(レイアウト後シミュレーション)。答えに `largest`(抵抗の大きいパッド。`nanohenry` も。`couplings`/`strongest_couplings` は別ネットの配線どうしの磁気結合 k。反映中に 1 MHz を超える `ac` を実行すると、`warnings` に「配線の抵抗は直流の値(表皮効果で約 300 kHz 以上は増える)」の注意が出る。行きと帰りが近いほどループのインダクタンスが小さくなる)。ピンの電圧は `ac`/`tran` の probe に `V(R1.1.wire)` と書く。`dc`/`tran` の答えの `post_layout` で効いているか分かる。ネットの電圧は入口側の値。 |
| `board shopping [boards=N] [spare=0] [file=PATH]` | 買うものの一覧。値とフットプリントごとに `qty`(`boards` 枚分)・`spare`(予備。チップの抵抗・コンデンサ・コイル・LED だけ、2個か全体の1割の多いほう。`spare=0` で切る)・`buy`(合計)と、店(`akizuki`・`digikey`・`lcsc`)ごとの `number`(部品JSONに型番があるとき)か `search`(店の検索ページへのリンク)を返す。`file=` を付けるとCSVも書く。`board fab` の出力にも `*-shopping.csv` として入る(列は `Value,Footprint,Designator,Qty,Spare,Buy` のあとに店ごとの型番とリンク)。 |
| `board route` の `bridge=0` | 自動配線の前に、同じ部品・同じネットで隣り合う表面実装パッド(レギュレータの2つのLXピンなど)を短い銅でつなぐ処理を止める。答えの `pads_bridged` がつないだ数。 |
| `board check [fab=jlcpcb\|pcbway\|oshpark\|standard]` | アプリの「発注の準備」と同じ発注前チェック。`ready`(エラーが無ければ true)、`errors`/`warnings`/`info`/`ok` の数、`list`(ok 以外の項目の `level`・`title`・理由 `detail`・場所 `at`)を返す。未配線・DRC違反・基板の外の部品・配線の太さと電圧降下・シルク・鋭角・サーマルビア・バイパスコンデンサ・料金の枠(`fab=` を付けたとき)・**回路図と基板の食い違い**(取り込み後に回路図を直したとき)を調べる。`board fab` の前に実行し、`ready:false` なら直してから書き出す。 |
| `board save F` / `open F` / `fab DIR` | `.pcbjson` の保存・読込、ガーバー・ドリル・BOM・部品位置・zip の書き出し。 |

## MCPサーバーとして使う (Claude Code などから)

`ccad mcp` は、同じコマンドを [MCP](https://modelcontextprotocol.io/) のサーバーとして出します(標準入出力、依存の追加なし)。回路図と基板は**サーバーが動いている間、呼び出しをまたいで残る**ので、AIが1コマンドずつ答えを見ながら組めます。

| ツール | 内容 |
|---|---|
| `ccad_run` | `script`(1行1コマンド)を実行して、1行1個のJSONを返す。最初に失敗した行で止まり、`isError` が true になる(`keep_going: true` で続ける)。 |
| `ccad_help` | コマンドの一覧と約束事(GND、ピン名、`ok` の意味、`uic`)。最初に読ませる。 |

登録の例 (Claude Code。パスは自分の場所に直す):
```bash
cargo build -p cli --release
claude mcp add ccad -- /path/to/target/release/ccad --strict mcp
```
他のクライアントは、`command` に `ccad`、`args` に `["--strict", "mcp"]` を設定します (`--lib DIR` で部品ライブラリの場所も変えられます)。

- 手元で試すだけなら、`target/debug/ccad` でも動きます。
- **このサーバーは、AIが指定したパスにファイルを書きます**(`save`、`board save`、`board fab DIR`)。信頼できるクライアントだけにつなぎ、書き込み先に気をつけてください。
- `--strict` は、ERCにエラーがあるあいだ `dc`・`tran`・`board import` を拒みます。AIには勧めます。ただし `erc` 自身も失敗扱いになり、答えには最初のエラーだけが出ます。一覧とヒントが見たいときは、`--strict` なしの実行で `erc` を見てください。

## 注意
- 答えの数値は、アプリと同じエンジンの結果です。精度と限界は [accuracy.md](accuracy.md) にあります(実物との一致は未確認です)。
- AIが作った回路は、**`erc` と、`dc`/`tran` の答えを必ず確かめてください。** 部品のピン配置・モデルは概算です。
- 発注の実績はまだありません。`board fab` の出力を使う前に、発注先のプレビューで確かめてください。
- 作りの詳細は [code-notes/cli.md](code-notes/cli.md)。
