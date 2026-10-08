# 部品定義ファイル (`*.part.json`)

1つの部品の **回路図記号・基板パッケージ・シミュレーションモデル** を1ファイルで定義します。
置き場所: `%APPDATA%\circuitCad\library\`（アプリの「ライブラリ部品 → JSONから追加…」でも追加できます）。
同梱の例: `crates/part-lib/examples/`。

データシートの PDF から作るときは、PDF を Claude に渡して「この部品の part.json を作って」と頼めます。
ピン表と推奨ランドパターン(Recommended Land Pattern)の寸法を読み取ります。
**読み取りの誤りがありうるので、ピン番号と寸法は必ずデータシートで確認してください。**
アプリは保存時に整合性(ピン番号とパッドの対応、モデルのノード数など)を検証します。

## フィールド

| フィールド | 必須 | 内容 |
|---|---|---|
| `name` | ○ | 部品名 |
| `category` | | ライブラリ一覧でのグループ名(例: `MOSFET`、`オペアンプ`)。省略すると「その他」 |
| `description`, `datasheet` | | 説明、データシートURL |
| `prefix` | | 部品番号の頭文字(既定 `U`) |
| `value` | | 回路図に出す値(既定 `name`) |
| `footprint_name` | | 基板上のフットプリント名(既定 `name`) |
| `pins[]` | ○ | `number`(データシートのピン番号、パッド名と一致させる)、`name`、`side`(`left/right/top/bottom`、省略で前半が左・後半が右)。同じ辺では配列の順に上→下/左→右へ並ぶ。名前が `NC` のピンは ERC で未接続を報告しない |
| `package` | ○ | 下記 |
| `sim` | | 下記 |

## package (単位 mm、パッドの `y` は上が正)

- `{"type":"chip2","pitch":..,"pad_len":..,"pad_width":..}` — 2端子チップ。`pitch` はパッド中心間
- `{"type":"dual","pins":8,"pitch":1.27,"row_spacing":5.4,"pad_len":1.55,"pad_width":0.6}` — SOIC/SSOP/TSSOP。`drill` を付けるとスルーホール(DIP)。ピン1は左上、反時計回り。`row_spacing` は2列の中心間距離、`pad_len` は列間方向、`pad_width` は列に沿う方向
- `{"type":"quad","pins":32,"pitch":0.8,"span":8.4,"pad_len":1.6,"pad_width":0.5}` — QFP 等。`span` は向かい合うパッド中心間
- `{"type":"single_row","pins":4,"pitch":2.54,"pad_d":1.7,"drill":1.0}` — 1列ヘッダ。ピン1は角パッド
- `{"type":"explicit","pads":[{"pin":"1","x":0,"y":0,"shape":"rect|circle|oval","w":1,"h":1,"drill":0.8}]}` — 座標を直接指定(サーマルパッド等)。
  同じ `pin` のパッドを複数書くと(SOT-223 のタブなど)、1つの接続として扱われ、すべてのパッドにネットが付きます

## sim (部品の特性)

```json
"sim": {
  "kind": "model",            // "model" = .model カード / "subckt" = .subckt ブロック
  "element": "D",             // model のとき SPICE 素子記号 (D, Q, M, J ...)
  "name": "1N4148",           // 実体化行で使うモデル名 / サブサーキット名
  "text": ".model 1N4148 D(IS=2.52n RS=0.568 N=1.752)",
  "pins": ["A", "K"],         // モデルが期待するノード順に並べた記号のピン番号
  "params": { "vf_typ": 0.72 } // 任意。自作シミュレータ用の特性値
}
```

使える SPICE の記述:
- `.model` カード: ダイオード `D(IS N RS BV IBV)`、バイポーラ `NPN` / `PNP`(`IS BF BR NF NR`)、
  MOSFET `NMOS` / `PMOS`(`VTO KP LAMBDA CGS CGD`。`element` は `M`、ノード順は ドレイン・ゲート・ソース)
- `.subckt` の本体: `R C L V I E G D Q M`。それ以外の素子を含むと、黙って無視せずエラーになります

SPICE ネットリストの書き出し時に、使われているモデルが1回だけ出力されます。`sim` が無い部品は
コメント行になり、シミュレーションには含まれません。
