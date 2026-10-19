# ccad: 画面なしで使う (AIエージェント・自動化向け)

`ccad` は circuitCad の計算部分を、窓を開かずに使うコマンドです。**1行に1コマンド**の短いスクリプトを渡すと、**1コマンドにつき1行のJSON**で答えます。回路は座標ではなく**部品名とピン名**で作るので、AI(LLM)が書いても、つながらない事故が起きにくくなっています。

```bash
cargo run -p cli -- script.ccad        # ファイルから
cargo run -p cli -- -c 'new\nadd R 1k' # その場で (\n で改行)
cat script.ccad | cargo run -p cli -- - # 標準入力から
```

- 失敗した行で止まります(終了コード1)。続けるには `--keep-going`。
- 部品ライブラリは、アプリと同じ場所を読みます。別の場所は `--lib DIR`。
- `ccad help` で、コマンドの一覧が出ます。

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
| `parts [語...]` / `part 名前` | 部品ライブラリの検索と、ピン(番号と名前)・シミュレーションの有無。 |
| `add 種類 [値] [as=U1] [rot=0..3] [at=X,Y]` | 部品を置く。種類は `R C L D LED V NPN HEADERn` か、ライブラリの部品名。位置は自動。 |
| `value REF 値` / `remove REF` | 値の変更 / 部品の削除。 |
| `net 名前 REF.PIN ...` | ピンを名前でつなぐ。ピンは番号でも名前でもよい。別の名前がついたピンは拒否。 |
| `list` | 部品・ピン・各ピンのネット。 |
| `erc` / `spice` | 配線の点検(原因と直し方つき) / SPICEネットリスト。 |
| `dc` | 動作点: 各ネットの電圧、各素子の電流・電圧・電力。 |
| `tran 終了 刻み [probe=V(out),...] [samples=6] [uic]` | 過渡解析の要約(最小・最大・平均・最終値・周期・デューティ比)。 |
| `board new [幅 高さ]` / `import` / `autoplace` / `route [net=名]` / `drc` / `status` | 基板(mm)。取り込み・配置・自動配線・DRC。 |
| `board save F` / `open F` / `fab DIR` | `.pcbjson` の保存・読込、ガーバー・ドリル・BOM・部品位置・zip の書き出し。 |

## 注意
- 答えの数値は、アプリと同じエンジンの結果です。精度と限界は [accuracy.md](accuracy.md) にあります(実物との一致は未確認です)。
- AIが作った回路は、**`erc` と、`dc`/`tran` の答えを必ず確かめてください。** 部品のピン配置・モデルは概算です。
- 発注の実績はまだありません。`board fab` の出力を使う前に、発注先のプレビューで確かめてください。
- 作りの詳細は [code-notes/cli.md](code-notes/cli.md)。
