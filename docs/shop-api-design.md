# 部品店の API で在庫・値段・候補を取る(J5 の計画)

2026-11-11 調べた。まだ作っていない。**作る前に、ここの「未確認」を公式のページで確かめ直す。**

きっかけ: DMM の試作(bench/)で、注文画面に行くまで在庫切れ(ADR4525BRZ)・作り手の違い(BAV199 が Slkormicro 製で逆電流が 16 倍)・生産が続いているか(TQ2-L2-5V)が分からなかった。

## 店ごとの調べた結果

### DigiKey(本命)

- 出典: developer.digikey.com の Documentation、Product Information v4 の ProductSearch・KeywordSearch のページと Swagger、API User Agreement(2026-11-11 に読んだ)。
- 認証: OAuth 2.0。アプリを登録すると Client ID と Client Secret が出る。2-legged(client credentials)で、利用者のログインなしにトークンが取れる。トークンは 10 分で切れる。要求には `X-DIGIKEY-Client-Id` と `Authorization: Bearer …` を付ける。Sandbox は `sandbox-api.digikey.com`。
- 地域: `X-DIGIKEY-Locale-Site` / `-Language` / `-Currency` / `-ShipToCountry` の見出しで指定する。**JP・ja・JPY が使えるかは未確認。**
- 回数の上限(Product Information): **1 分に 120 回、1 日に 1000 回**。超えると 429。答えの見出し `X-RateLimit-Remaining` で残りが分かる。
- 使う所:
  - `GET /products/v4/search/{型番}/productdetails`: 1 つの型番の詳しい情報
  - `POST /products/v4/search/keyword`: キーワード・型番・絞り込み。本文は `Keywords`・`Limit`・`Offset`・`FilterOptionsRequest`(作り手・分類・状態・包装・`MinimumQuantityAvailable`・`ParameterFilterRequest`)・`SortOptions`
  - ほかに `pricing`、`substitutions`(代わりの品)、`recommendedproducts`、`media`
- 答えの `Product` に入るもの: `ManufacturerProductNumber`、`Manufacturer`、`QuantityAvailable`、`ProductStatus`、`Discontinued`、`EndOfLife`、`DateLastBuyChance`、`ManufacturerLeadWeeks`、`DatasheetUrl`、`ProductUrl`、`Parameters`(仕様の一覧)、`ProductVariations`(DigiKey の型番・包装・`StandardPricing`・最小注文数)。KeywordSearch では `MyPricing` は空。
- **仕様での絞り込みは ID で指定する**: 分類の ID、仕様の項目の `ParameterId`、値の `ValueId`。値は自由な数ではなく、前の検索の答えの `FilterOptions` に出た値から選ぶ。そのため「出力 4.5 V・SOT-23-5 の LDO」は、(1) キーワードと分類で検索 → (2) 答えの `FilterOptions` から値を選んで絞る、の 2 段になる。**ID の対応表をソフトに持たない**(変わるかもしれない)。答えの `Parameters` の文字(例 "4.5V")を手元で読んで比べる方が丈夫。
- 規約(API User Agreement)で効く所:
  - 使ってよい目的は、自分の公開サイトと、承認された他のサイト、それに「DigiKey からの購入を自動化し、よくするための自分の社内アプリ(Internal Application)」。**利用者が自分の鍵で自分の circuitCad から引くのは、この Internal Application に当たると読める**が、DigiKey の承認の扱いは未確認。
  - **DigiKey のデータを第三者に配らない**(3.2 (iv))。→ 取った結果を同梱の部品データやリポジトリに入れない。手元の保存は、その利用者の手元だけ。
  - **出どころを示す**(3.1.4・5.1 (b)(c))。ほかの店のデータと区別せずに混ぜない。→ 表示に「DigiKey」と出し、店ごとに行を分ける。
  - 90 日使わないと止められることがある。
  - **Client Secret はソフトに埋め込めない**(配ると第三者に渡すことになる)。→ 利用者がそれぞれアプリを登録して、自分の ID と Secret を入れる。

### Mouser(2 番手)

- 出典: mouser.com の API のページ(検索の要約。ページそのものは自動では読めなかった: 403)。**ブラウザで読み直す。**
- 鍵: MyMouser のアカウントから Search API の鍵を申し込む。承認待ちになることがある。
- 上限: 1 回 50 件まで、**1 分に 30 回、1 日に 1000 回**。
- 型番の検索(1 回に 10 件まで)とキーワードの検索。仕様での絞り込みは無いようだ(未確認)。在庫・値段・データシートの URL が取れる。
- 規約: 自分のアプリの中で表示するための、取り消せる限定の許可(見つかった写しは 2013 年のもの。**今の版は未確認**)。

### Octopart / Nexar(補助)

- 複数の店をまとめて引ける GraphQL。無料の枠は「合わせて 100〜1000 部品」と出典によって違い、はっきりしない。**今は作らない**(枠が小さい・規約を別に読む必要)。

### 秋月電子

- 公開の API は見つからない。ページを自動で読むことはしない。今の検索リンク(`pcb-core/shops.rs`)のまま。

## ほかのソフトのやり方(2026-11-11 に検索で調べた。細部は未確認)

| ソフト | やり方 | 鍵 |
|---|---|---|
| EasyEDA | 同じ会社グループの LCSC の在庫・値段を直接出す | 要らない(自社のデータ) |
| Altium Designer | Altium の自社サーバー経由(Manufacturer Part Search)。Altium は Octopart を持っていて、在庫・値段・仕様を無料で出す。深い分析は有料の別サービス | 要らない(サインインも不要との記述) |
| KiCost(KiCad の部品表の値段) | 最初は店のページを読んでいたが、店のページが変わるたびに壊れて Octopart → 各店の公式 API(DigiKey・Mouser・Farnell)と Nexar に移った。DigiKey の v4 にも対応 | **利用者が自分で登録した鍵** |
| InvenTree(部品の在庫管理)のプラグイン | DigiKey の Client ID・Secret、Mouser の検索用の鍵を設定に入れる。利用者ごとの鍵を持てるものと、チームで1つの鍵のものがある。DigiKey は Sandbox ではなく Production のアプリを作るよう案内 | **利用者(か、その組織)が自分で登録した鍵** |
| KC2PK など | Octopart の鍵で在庫と値段 | 利用者の鍵。Octopart の無料の鍵は減っているという声 |

分かったこと:

- 鍵なしで使えるのは、**データを自社で持っている会社のソフト**(EasyEDA = LCSC、Altium = Octopart)だけ。サーバーを持たない無料のソフトは、どれも「利用者が自分の鍵を入れる」形。circuitCad もこちら。
- **店のページを読む方式は壊れやすく、捨てられた**(KiCost の経験)。公式の API だけを使う。
- 店を差し替えられる形にしておく(KiCost は 3 回データ源を替えている)。

## 作り方の決まり

1. **鍵は利用者のもの**: 利用者が DigiKey(と Mouser)に登録して、自分の ID・Secret・鍵を入れる。ソフトにもリポジトリにも鍵は入れない。保存場所は設定のフォルダ(`%APPDATA%\circuitCad`)で、画面には伏せ字で出す。手順は日本語で説明する(登録の画面の場所、Production のアプリを作る、Product Information を購読する)。
2. **送るのは型番と検索の言葉だけ**。回路図そのものや利用者の情報は送らない。
3. **無くても動く**: 鍵が無い・オフライン・上限に達した、のときは今までどおり(検索リンク)。その理由を日本語で出す。
4. **日付つきで手元に残す**: 在庫はすぐ変わるので、答えに取った日時を付ける。同じ型番は一定時間(例 1 時間)は保存した答えを使い、上限を使い切らない。保存は利用者の手元だけで、ファイルを共有しても入らない場所に置く。
5. **出どころを出す**: 表示に店の名前を出し、店ごとに分ける。
6. **部品の `shops` 欄には自動で書かない**: 本人が「これにする」と選んだ型番だけ書く(`docs/part-format.md` の「確かめた型番だけ書く」を守る)。

## 段階

1. **ccad で型番の確かめ**: `stock 型番 [shop=digikey|mouser]` → 在庫・状態(生産中止・最終購入日)・作り手・値段の段・データシートの URL。`board shopping` の各行にも在庫を付けられるようにする。テストは記録した答え(手で作った JSON)で行い、テストからは店に接続しない。
2. **ccad で候補探し**: `find 分類 キーワード [条件…]` → DigiKey の KeywordSearch で候補を取り、答えの `Parameters` を手元で読んで条件(電圧・パッケージ・在庫あり)に合うものを並べる。BAV199 のように同じ型番で作り手が違うものは、作り手ごとに分けて、仕様の違い(漏れ電流など)を並べる。
3. **画面**: 部品表(BOM)の横に在庫・値段・取った日時。部品作成フォームの型番欄の横に「店で確かめる」。設定に鍵の入力と登録の手順。

## 未確認(作る前に確かめる)

- DigiKey: 日本(JP・JPY)で引けるか。個人の利用で Production のアプリが作れるか、承認が要るか。Internal Application の解釈(個人が自分の鍵で引く使い方で問題ないか。必要なら api.contact@digikey.com に問い合わせる)。
- Mouser: 今の規約、日本の通貨、絞り込みの有無。
- HTTP の部品(今の依存クレートに無ければ、足すと `THIRD_PARTY_LICENSES.txt` の作り直しが要る)。
