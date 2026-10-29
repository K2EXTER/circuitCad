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
- **`coverage`** は、シミュレーションが何を知らないかの一覧(`ignored`=モデルがなく計算に入っていない部品、`generic`=ダイオード・LED・NPN が型番ではなく標準の値で計算されているもの、`caveats`=ライブラリのモデルの注意書き、`test_sources`=シミュレーション専用の部品、`total`・`clean`)。`dc`・`tran`・`ac` の答えの `models` に、その数(`ignored` は部品番号)が付く。モデルがない部品は回路にないものとして計算されるので、`models.ignored` が空でないときは結果を疑う。
`dc` の答えには、**回路が「動いた」だけでなく「使える」かを見るための知らせ**が付きます。`ratings` は定格のチェック(アプリと同じ。抵抗・トランジスタ・LED などが、形ごとの電力の目安を超えている/近い。`level` は `over`/`caution`。データシートではなく目安)、`warnings` には、オペアンプの IN+ と IN- の電圧が違うとき(負帰還が効いていない = 出力が電源の手前で頭打ち)の注意が出ます。コンパレータとして使うオペアンプなら無視してかまいません。`tran` にも同じ形で付きます(`ratings` と `warnings`。後半の半分だけを見て、立ち上がりは除きます): 抵抗の消費電力(実効値)、LED の最大電流、電解コンデンサのリップル電流(部品のデータシートの定格がある場合。アプリと同じ計算)、オペアンプの入力差(時間の2割を超えて続くとき)。発振回路やコンパレータのオペアンプには、警告が出ることがあります(文に「無視してよい」と書いてあります)。トランジスタ・ダイオードの電力は、`dc` だけが見ます。
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
| `value REF 値` / `remove REF` | 値の変更 / 部品の削除。値にスペースが入る電源(`PULSE(0 5 1m 1u 1u 2m 5m)`、`0 AC 1`)は、引用符なしで書ける。 |
| `simonly REF [off]` | その部品を**シミュレーション専用**にする(試験用の電源、測る相手の代わりに置いた抵抗など)。`board import`・部品表・購入リストに出ず、`dc`/`tran` などでは使われる。`list` に `sim_only: true` と出る。`off` で元に戻す。アプリでは、部品の編集欄の「シミュレーション専用 (基板に載せない)」。実際に作る回路図とシミュレーションの試験用の源を、1 つのファイルで両立させるため。 |
| `sheet` / `sheet add [名前]` / `sheet use N\|名前` / `sheet name 名前` / `sheet move REF… to=N\|名前` / `sheet tidy [all]` | 複数シート。引数なしで一覧(番号・名前・部品数・ラベル数)。`add` は空のシートを作って、それを操作対象にする。`move` は部品と、そのピンにあるネットラベルを、別のシートへ動かす(`net` で作った回路は配線がなくラベルだけなので、機能ごとにシートを分けられる)。`tidy` は、操作対象のシート(`all` なら全シート)の部品を、横が縦の約 1.7 倍の詰めた段に並べ直す(ネットとラベルは変わらない。`add` は 1 段 110 マスで折り返すので、部品が多いと縦に長く伸びる)。シートをまたぐ接続は、同じ名前のネットラベル・電源記号。`add`・`net`・`remove`・`list` は操作対象のシートだけ。`dc` などの解析は全シートを見る。 |
| `lib [reload]` | 部品ライブラリのフォルダと部品数。`lib reload` は読み直す(`part import` したあと、またはファイルを直接書き足したあと、`ccad mcp` を再起動しなくてよい)。 |
| `part import FILE [name=] [subckt=] [pins=A,B,..] [map=1,2,..] [package=部品名]` | メーカーの SPICE モデル(平文の `.subckt`)から部品を作ってライブラリに書く。**そのサブサーキットから呼ばれる別の `.subckt` と、ファイル先頭の `.model` / `.param` / `.func` の行も一緒に取り込む。**`pins` は記号のピン名(サブサーキットのノード順)、`map` はそのピン番号、`package` はフットプリントを借りる部品(既定 TL072 = SOIC-8)。**暗号化モデル(LTspice の `<Binary File>` など)は断る。** シミュレータが読めない素子があれば、その理由を返す。ピンの位置(辺)は名前から推測した仮のもの。 |
| `net 名前 REF.PIN ...` | ピンを名前でつなぐ。ピンは番号でも名前でもよい。別の名前がついたピンは拒否。 |
| `list` | 部品・ピン・各ピンのネット。 |
| `erc` / `spice` | 配線の点検(原因と直し方つき。`errors` と `circuit_ok` を見る) / SPICEネットリスト。 |
| `dc [full] [probe=V(out),I(R1),V(a,b)]` | 動作点: 各ネットの電圧、各素子の電流・電圧・電力(部品モデルの内部は `full` のとき)。`probe=` を付けると、その値だけを `probes` で返す(`voltages`/`elements` は省く)。 記憶素子(フリップフロップなど、`Element::Seq`)があると、答えに `memory`(名前→0/1)と警告が付き、`ic=名前=1,名前=0`(名前の一部でよい)で初期の状態を決められる。**ゲートで組んだラッチ・カウンタ(74HC74 などのモデル)は答えが2つ以上ある**: `guess=NET=V,NET=V`(SPICE の `.nodeset`。gmin を下げる間、ネットをその電圧に引く)で近い答えを選び、`states` を付けると、全部のネットを高い方・低い方に置いて解き直して、別の電圧に落ちるネットを `other_states` で返す(解き直しが2回ぶんかかる。大きい回路では分単位)。 |
| `sweep REF 開始 終了 [steps=11] [log=1] probe=V(out),I(R1)`(または `sweep REF list=1,2,5 probe=…`) | 1つの部品(V 電源・R・C・L)の値を振って、値ごとに動作点を求める。値ごとに1行(収束しない値は `error`)。部品の値は元に戻る。最大 200 点。 |
| `sens probe=V(out) [step=1] [parts=R1,R2 \| kinds=R,V,I,C,L] [top=15]` | 感度解析: 部品の値を 1 部品ずつ `step` % だけ動かして動作点を解き直し、プローブがどれだけ動くかを大きい順に返す(`per_percent` = 1 % あたりの変化量、`elasticity` = 部品の相対変化に対するプローブの相対変化。分圧なら R2 が R1/(R1+R2)、電源が 1)。既定は R・V・I の素子(ライブラリ部品は除く)。`parts=` で名前を指定できる。150 部品を超えると断る(名前を指定する)。値が数字で始まらない電源(`PULSE(...)`)は動かさない。 |
| `tran 終了 [刻み] [probe=V(out),...] [samples=6] [uic]` | 過渡解析の要約(最小・最大・平均・最終値・周期・デューティ比)。刻みは最大の刻みで、省くと 終了/1000(誤差の大きい所は自動で細かくなる)。 `settle=V(out)[,..] [from=20u] [tol=1e-2,1e-3,1e-4]` を付けると、`from`(刺激を入れた時刻)から、最終値の「ステップの tol 倍」の帯に入ったままになるまでの時間(`settled_within`。終わりまでに入らなければ null)・初期値・最終値・行き過ぎ(`overshoot_pct`)を、全点から計算して返す。`ic`・`guess` は `dc` と同じ。 |
| `models [語...] [zip=PATH] [size=0603] [diel=X7R] [cmin=1u] [cmax=10u] [vmin=16]` / `models model=型番` | メーカーが配るSPICEモデルのzip(村田の「SPICE netlist」など。規約で再配布できないので自分でダウンロードする)から探す(別名 `murata`)。`zip=` を省くと、アプリで追加したzipをすべて使う。型番の語は空白区切りで全部を含むもの。`size`(インチ表記: `0603`=1608)・`diel`・`cmin`/`cmax`・`vmin` は、内容を読み取れるモデル(今は村田・日本ケミコン・ニチコン。サイズ・温度特性は村田だけ)にだけ効く。`model=` は等価回路と注意(村田は25℃・DC 0Vの値で、直流電圧による容量低下は入っていない)を返す。`models use 型番` は、そのモデルをこの実行のライブラリの部品にして(2端子のモデルだけ。ピン1・2、電解は `+`/`-`、コンデンサの記号、値・定格電圧・組み込みフットプリント `C_0603`/`C_0805` は読み取れたときだけ)、続けて `add 型番 [値] as=C1` で置ける。答えの `todo` に、まだ自分でやること(大きさの情報がなく基板に使えない、値と定格電圧が不明、など)が出る。ライブラリのファイルには保存しない(実行のあいだだけ)。 |
| `calc [名前 数値...]` | 値の計算(LEDの抵抗・分圧・RCフィルタ・降圧/昇圧・オペアンプ・電池の持ち時間など14種。アプリの「値の決め方」と同じ式)。名前に `_mA` `_kHz` などが付く入力は、その単位の素の数字で書く(`10`。`10m` は拒否)。LEDの電圧など入力に使う値は、`dc` で使う部品モデルと少し違うので、電流は計算と数%ずれる。引数なしで一覧と入力の順番。答えは値・手順・注意・故障時の様子。 |
| `ac 開始Hz 終了Hz [ppd=10] [probe=V(out)]` | 周波数応答(電源の値に `AC 1` が要る: `value V1 "0 AC 1"`)。1デケードごとのゲインと位相、-3dB点(周波数の点を補間して求めるので、`calc` の理論値と0.1%ほどずれる)、位相余裕。 |
| `noise 出力 開始Hz 終了Hz [ppd=10] [input=V1] [temp=27] [top=8]` | 雑音解析(熱雑音・ショット雑音)。出力はネット名(`A,B` で2つの差)。帯域の rms、1デケードごとの密度(V/√Hz)、雑音の大きい部品(寄与の割合つき)。`input=V1` を付けると、その電源(V か I)に換算した入力換算雑音も返す。ライブラリ部品のマクロモデルの内部の抵抗も出る(`U1.RN1` など)。1/f 雑音は扱わない。 |
| `spec 種類 対象 下限 上限 [at=Hz]` | 直前の `dc`/`tran`/`ac` の答えが範囲に入っているか(`pass`)。`-` は制限なし。引数なしで種類の一覧。回路を編集すると答えは消える。TARGET は `dc_voltage` がネット名(`MID`)、`dc_current` が部品名(`D1`、単位はA: `8m 12m`)、`tran_*`/`ac_*` が信号名(`V(OUT)`)。`at=` は `1k` のように書ける。`pass:false` でもコマンドは `ok:true`(実行できたという意味)。 |
| `req` / `req net 名前 nets=A,B,GN* [role=] [width=mm] [clearance=mm] [current=A] [links=CB1-U7:0.6,U7-U4:0.3] [メモ…]` / `req suggest [apply=1]`(DC解析のネットごとの電流から、表の空の電流と、行のない大電流のネットを提案。`apply=1` で表に入れる。書いてある電流とスイッチングの行は変えない) / `req group 名前 parts=R1,R2 [max=mm]` / `req apart 名前 a=.. b=.. min=mm` | 設計の要件の表(全シートで1つ。回路図に保存される)。`req` で一覧と、回路にない名前の指摘(`problems`)。`net` はネットの役割・線幅・間隔・電流と、`links=` の部品間の線幅(2つの部品の間の線だけ別の幅。`部品-部品:幅mm` をカンマ区切り。自動配線はその2部品の間を先にその幅で引き、残りをあとからつなぐ。`links=` で全部置き換え)(名前の末尾の `*` は前方一致。役割は power / switching / sense / analog / logic / none)、`group` は近くに置く部品の組(`max` はいちばん離れた2部品の中心距離の上限 mm。0 はチェックなし)、`apart` は離す組(`a` と `b` は部品番号かネット名で、いちばん近い2パッドの中心距離の下限 mm)。既にある行は、書いた項目だけを変える。`remove=1` で行を消す。各行の理由は `reason=文`(`note=` でも、名前のあとの言葉でも同じ)。`req` の答えの `unexplained` は、理由が空の行(あとで読む人に「なぜ」が伝わらない行)。`board import` が表を取り込む(行ごとにネットクラス+電流、組と距離は基板に保存)。 |
| `board new [幅 高さ]` / `import` / `layers N` / `pour [net=名] [layer=bottom|inner1…]` / `autoplace` / `move REF X Y [rot=0..3]` / `route [net=名] [lead=mm] [currents=0] [improve=1]` / `improve [net=名]` / `labels` / `class` / `impedance` / `drc` / `status` | 基板(mm)。取り込み・ベタ・配置・自動配線・DRC。`move` は1個の部品を指定の位置(部品の原点、mm)へ動かす(配線は動かない)。`route` は `layers=outer`(内層に線を引かない。ベタの層を割らない)と `vias=0`(ビアを使わない。検出線用)を取る。`route` は `grid=0.1`(配線の格子 mm。既定 0.2。0.5 mm ピッチの IC の足の間に通すとき細かくする)も取る。`guard net=A guard=GRDA [gap=0.35] [width=0.3]` は、検出する(高インピーダンスの)ネットの表の配線の両側に、ガードのネット(バッファの出力)の細い線を、角でつないで置く(まわりの銅に近い所は置けない。配線ごとに DRC が増えないものだけ。線の連なり 1 つにつきガードのネットのビア 1 個を置くので、そのあと `route net=GRDA` で、バッファの出力へつなぐ)。`mode=pour` は、ベタで囲む旧い方法(ベタが検出線で切れて、ガードのネットに触れない部分は島として消えるため、ほとんど囲めない)。`copper net=DRN rect=x0,y0,x1,y1 [layers=top,bottom] [vias=6]` は、ネットの銅箔のベタ(パワートランジスタのドレインなど)と、層をつなぐビア(パッドの近くから)を置く。`nudge [budget=秒] [grid=] [layers=outer]` は、全部配線したあとも残った接続の近くの小さい部品(3 パッド以下、固定していない)を 1〜4 mm 動かし(その部品の配線は消して引き直す)、未配線が減って DRC が増えないものだけ残す(答えは `moved`・`open_before`・`open_after`・`tried`)。`req` は、`req` の表の組と距離を、いまの配置で調べる(答えは `groups`・`apart`・`checked`・`violations`・`findings`・`missing`)。`breakout [width=0.2]` は、配線の前に、足の間隔が 0.8 mm 以下の IC の足から出口の短い線を引く(辺の真ん中の足はまっすぐ外へ、両端の足は外へ向けて曲げる。端に近い足ほど早く曲げるので、真ん中の足の出口をふさがない。DRC が増える IC には引かない。答えは `stubs`・`parts`・`dropped`)。`route` は、通らない接続が残ったとき、この出口の線を引いてやり直し、通らない接続が少ないほうを採る(`board route` の中で自動。0.5 mm ピッチの QFN などで効く)。`slits` は、同じネットの銅箔の間の狭いすき間(最小間隔より狭い)を、短い銅で埋める(答えは `filled`)。`layers N` は銅箔の層数(2・4・6・8。内層の配線やベタが消えるときは断る)。`pour layer=inner1` は、ベタを内層に置く(先に `layers 4`)。表面のパッドごとにビアを置くのは同じ(4 層のネットのベタ用)。`pour` はアプリの「GND ベタ」(裏面を銅箔のベタにして、表面のパッドごとにビア。答えは `zone_added`・`vias`・`already`・`left`)で、`import` のあと `autoplace` の前に使う。`autoplace [sheets=1] [arrays=0] [groups=0]` は、`groups=0` で、`req group` の組を1つの塊にして並べる(既定オン)のを切る。`apart=0` で、`req apart` の離す距離を並べるときに守る(既定オン)のを切る。`req bypass IC caps=C1,C2 [reason=文]`(どの IC のバイパスコンデンサか。`remove=1` で消す)、`req suggest bypass=1 [apply=1]`(対応のない容量を、回路から推測した提案。DC 解析は使わない)。`board req` の `findings` の `kind` に `bypass` も出る。**`req waive group|apart|bypass 行の名前 [item=コンデンサ] reason=文 [remove=1]`** は、点検の指摘を「承知した例外」にする(理由が要る。理由のない例外は `req` の `unexplained` に出る)。`board req` の答えでは、承知した指摘は `violations`・`findings` に数えず、`waived`(理由つき)に出る。発注前チェックには「承知した例外が N 件」と理由が出る(情報)。例: `req waive apart 降圧から離す reason=基板が小さいので承知`。答えの `group_notes` は、組の塊に入れられなかった部品(コピーか IC のブロックにまとまっているもの)の知らせ。`sheets=1` で複数シートの回路をシートごとの塊にして並べる(既定はオフ。基板が大きくなる)。同じ構造のコピー(レンジ回路など)は、同じ並べ方で格子に並べる(`arrays=0` で切る)。ベタ(ネットのあるゾーン)がある基板では、そのネットを計算から外して部品を詰め(`poured_nets_ignored`)、ベタを新しい外形に合わせ、ベタのビアと短い線は置き直す(`repoured_vias`。ほかの配線があると断る)。`route` はアプリと同じく、パッドの辺に垂直に `lead`(既定0.5mm。0で切る)出入りするよう根元をそろえ(`aligned_pads`)、一直線につながる配線を1本にまとめ、まとめた数を `joined_tracks` で返す。電流がまだ決まっていない基板では、回路の動作点(DC)の電流を先に基板に入れ(`currents_declared`)、必要な太さが `track_width` より太いネットは、その太さで先に引く(`wide_nets`。引けなければ普通の太さで引いて、`board check` が細いと警告する)。`currents=0` で切る。`pour` は既定で表面の空いた所もGNDのベタで埋める(`top_added`。`top=0` で切る。ルール違反が増えるときは置かない)。`route` は、裏面にGNDベタがあって表面にまだなければ、配線のあとに表面を埋める(`top_pour`。`top=0` で切る。**`net=` でネットを指定したときは埋めない**。基板のほかの所を変えないため。`top=1` で埋める)。`status` などの `unrouted` は、画面の「未配線」と同じ数(銅でつながっていない接続 + ベタが分断されて銅がつながっていないパッド)。`improve=1` を付けると、引いたあと遠回りした配線やビアを引き直して短くする(`rerouted_nets`・`saved_mm`。自動配線の直後は得が小さいので、既定では切ってある)。`improve` は基板全体(手で引いた配線も)に同じ引き直しをかけ、配線の長さ・ビア・本数の前後を返す。`impedance` は配線の太さ⇄インピーダンスの目安(教科書の式、±10%): `board impedance width=0.35` で太さから Z0、`space=` を付けると差動も、`target=50` で 50 Ω の線幅、`diff=90 space=0.2` で差動 90 Ω の線幅、`diff=90 width=0.2` で差動 90 Ω の間隔。`er=`・`height=`(外層から基準面)・`gap=`(内層の上下のベタの間)・`copper=`(µm)で層構成を決める(基板に保存される)、`layer=inner` で内層。`class` はネットクラス(`role=power|switching|sense|analog|logic|none` で役割も付けられる。自動配線は役割の順に引き、検出のネットはビアなし。`route` は、通らない接続が残ると、格子を 1/2・1/4 にして引き直す。`route ladder=0` で切る): `board class` で一覧、`board class 名前 [width=mm] [clearance=mm] [via=径/穴] [nets=A,B]` で作成・変更(ネットは、ほかのクラスから移る。基板の最小線幅・最小間隔より小さい値は断る)、`board class 名前 remove=1` で削除。クラスは、`route` の線幅・ビア・間隔と DRC に効く(基板ファイルに保存される)。`orient [dry=1] [parts=U1,U2] [minpads=3]` は、配線の付いていない部品を、未配線が短く・交差が少なくなる向き(0/90/180/270°)に回す(答えは `turned`・`tried`。ほかの部品や配線にぶつかる向きは選ばない。自動配置のあとでは得は小さい)。`swap [dry=1]` は、74HC32 のように同じゲートが並ぶ IC で、配線の付いていないゲートの割り当て(と OR ゲートの 2 つの入力)を入れ替えて、未配線を短く・交差を少なくする(答えは `swapped`・`locked`。`dry=1` は見るだけ。入れ替えは基板に覚えられ、`import` をやり直しても残る)。**`board backannotate`** は、その入れ替えを回路図へ反映する(回路図の配線の端とネットラベルを、入れ替えたピンへ動かす。部品は描き直さないので、ピンの間で線が交差して見えることがある。つながりは基板と同じ。ほかのネットが混ざるときは、何も変えずに理由を返す。反映した部品の覚えは基板から消える。答えは `parts`・`moved`・`left`)。`status` の答えの `quality` は、配線の見た目の数字(長さ・層ごとの長さ・ビア・線分・折れ・短い線・45° の長さ・並走の長さ)。`quality [top=10] [sort=bends|vias|detour|length|segments] [net=A,B] [poured=1]` は、その数字と、悪いネット(折れ・ビア・最短の木に対する遠回りの比・層ごとの長さ)を並べる(ベタのネットは `poured=1` で出す)。`status` の答えの `open_nets` は、未配線が残っているネットごとの本数。`labels` は、配線・パッド・ビア・穴・ほかの文字に重なっている部品番号のシルク文字を、部品の上・下・横の重ならない場所へ動かす(答えは `moved`・`still_touching`。手で動かした文字はそのまま)。 |
| `board picture [layer=top\|bottom\|inner1…] [rect=x0,y0,x1,y1] [px=1600] [net=A,B] [marks=1] [others=1] [grid=mm] [out=file.png]` | 基板の**絵**(PNG)を書く。答えは `file`・`width`・`height`・`legend`(色と番号の意味、範囲と 1 mm あたりの px)。1 つの銅箔の層のベタ・配線・パッド・ビア・穴・シルクと、mm の格子(数字は上の縁が x、左の縁が y)を描く。`net=` はそのネットを別々の色(黄・水色・ピンク…)で、`marks=1` は DRC 違反に番号を付け(`legend` に位置と内容)未配線を黄色の線で、`others=1` は他の層をうすく描く。`rect=` で一部を拡大(範囲が狭いほど 1 mm あたりの px が増える)。`ghost=A,B` は、そのネットを(今ある配線を外した複製で)ルータが引いたらこうなる、という経路を**影**(ネットの色の太い半透明の線と中心線。ビアは丸)で重ねる(`ghostvias=1` でビアを許す。基板は変わらない。凡例に長さとビア数、引けなかったネット)。`ghostguard=A:GRDA,B:GRDB` は、影の検出線の両側に `board guard` で置くガード配線の案(`guardgap=0.25`・`guardwidth=0.3`)も、ガードのネットの色の影で描く。AI は出た PNG を見て、配線の形・ガードの囲み・窓・部品の重なりを目で確かめる。配線は設計どおりの形(角丸め・涙型の仕上げは描かない)。 |
| `board picture` の続き | `names=1` はパッドのそばにネット名を、`part=R1,U2` は部品に輪と名前を描く。**MCP(`ccad_run`)では、絵が画像として答えに付いてくる**(2.5 MB まで。大きいときは `px` を小さく)。`out=` を省くと、一時フォルダの `ccad-picture-<層>.png` に書く。 |
| `board net NAME` / `board track add x1,y1 x2,y2 … [layer=top] [width=0.25] [net=NAME] [float=1]` / `board track remove net=NAME [layer=] [vias=1] \| rect=x0,y0,x1,y1 \| index=N` / `board via add x,y [net=] [dia=] [drill=]` / `board via remove x,y [r=0.3]` | 基板の配線を、AI が手で直すための命令。`net` は、ネットのパッド(場所)・層ごとの配線(本数・長さ・太さ)・ビア・ベタ・未配線を返す(絵と数字を突き合わせる)。`track add` は、点を結ぶ配線を引き(答えは `laid`・`length_mm`・`net`・`drc_violations`)、**`net=` を付けると、そのネットのパッド・配線・ビアに触れていなければ取り消す**(別のネットに触れる = 短絡、何にも触れない = 浮いた配線、も取り消す。`float=1` で浮いた配線を許す)。`track remove` はネット・範囲・番号で配線を消す(`vias=1` でそのネットのビアも)。`via add` は、触れたものか `net=` のネットのビアを置く。 |
| `board zones [merge\|bake]` / `board keepout layer=inner1 [net=NET] rect=x0,y0,x1,y1 \| circle=x,y,r \| pad=REF[,REF] [margin=1.0]` / `board keepout clear layer=… [net=…]` | ベタ(ゾーン)は**図形のリスト**(足す図形と引く図形。円弧は円弧のまま)でできている。`board zones` は一覧(層・ネット・図形ごとの足す/引く・頂点の数・円弧の数・面積)。`merge` は、同じネット・同じ層で重なる・接するベタを 1 枚にまとめて頂点に確定し、`bake` は全ベタの図形を最終の頂点(直線だけ)に確定する(どちらも余分な頂点を取り、0.5 mm² 未満の引く図形を落とす。答えは `merged`・`baked`・`points_removed`・`shapes_dropped`)。`keepout` は、その層のベタ(`net=` があればそのネットだけ)に、引く図形(銅を置かない穴・窓)を足す: 四角、円(円弧の図形)、またはパッドのまわり(パッドの半径 + `margin`)。`clear` で、引く図形を全部消す。内層のベタを、検出線のスルーホールから離すときなどに使う。DRC・ガーバー・画面に効く(`irdrop` の抵抗の計算は、ベタの穴を見る)。 |
| `board thermal` | 放熱パッド(QFN・ESOP の裏のパッド)でサーマルビアが足りないものに、直径 0.3 mm のビアの格子を置く(アプリの「サーマルビアを置く」)。答えは `pads_lacking`・`vias_added`・`pads_still_lacking`・`drc_violations`。置けば、同じネットの裏の銅(`board pour`)につながって熱が逃げる。 |
| `board currents [tran] [from=F] [widen]` | `dc` と同じ動作点(`tran` を付けると、直前の `tran` の後半 F 以降(既定0.5)の実効値。スイッチング電源など DC で電流が決まらない回路用。ピークは `stats` に出すが太さには使わない。`spiky:true` はピークが実効の10倍超=起動サージやスパイクの可能性)から各ネットの電流を求め、基板のネット電流に入れて、必要な配線の太さ(IPC-2221、温度上昇10℃・35µm)と今の最細の太さを返す。`widen` で細い配線を必要な太さに太くする(あとで `board drc`)。 |
| `nc REF.PIN [REF.PIN …] [remove=1]` | 意図して未接続のピン(使わない出力、データシートが「開放」と言うピン)に印を付ける。ERC はそのピンを「未接続」と言わなくなり、あとで配線やラベルがつながると食い違いを警告する。ピンは番号でも名前でも(同じ名前の全ピンに付く)。`remove=1` で印を外す。ピンを書かないと印の一覧。答えは `changed` と `no_connects`。回路図に × で出る。 |
| `pinnames REF 名前1,名前2,…` / `board pinlabels [height=1.0]` | ピンヘッダ(コネクタ)の各ピンの名前(F+、SA、GND など。1 番ピンから。`-` で消す)を決める。記号のそばに出て、`board pinlabels` が基板のシルクに、パッドのそばへ(基板の中央に近い側に)書く。もう一度実行すると書き直す。名前はネットや配線に影響しない。 |
| `board postlayout on\|off [pads=RS1,RNA.1]` | `pads=` で、その部品(またはピン)の抵抗・インダクタンスを `pads` に返す。`on` にすると以後の `dc`/`tran`/`ac` に、配線した基板の銅の抵抗とインダクタンス(電源の入口パッドから各パッドまで)がパッドの手前に直列に入る(レイアウト後シミュレーション)。答えに `largest`(抵抗の大きいパッド。`nanohenry` も。`couplings`/`strongest_couplings` は別ネットの配線どうしの磁気結合 k。反映中に 1 MHz を超える `ac` を実行すると、`warnings` に「配線の抵抗は直流の値(表皮効果で約 300 kHz 以上は増える)」の注意が出る。行きと帰りが近いほどループのインダクタンスが小さくなる)。ピンの電圧は `ac`/`tran` の probe に `V(R1.1.wire)` と書く。`dc`/`tran` の答えの `post_layout` で効いているか分かる。ネットの電圧は入口側の値。 |
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
