# 他の電気CADとの機能比較

2026-11-02 に、各社の公式ドキュメント・機能紹介ページを調べて作った。比較の相手は KiCad 9、EasyEDA(Standard / Pro)、Altium Designer、Quadcept。

**読み方**
- 「確認」= 下の出典の公式ページに書いてあったこと。
- 「未確認」= 今回の調べでは書いてあるページに行き着かなかった、または機能紹介の文だけで細部が分からないもの。「無い」という意味ではない。
- circuitCad の列は `CLAUDE.md` と `docs/candidates.md` の記載による。
- 機能紹介は各社の宣伝が中心。何かを判断に使うときは、リンク先の最新の説明を読み直すこと。

## 比較表

| 機能 | circuitCad | KiCad 9 | EasyEDA | Altium Designer | Quadcept |
|---|---|---|---|---|---|
| 回路図 → 基板 → 製造データ | あり | 確認 | 確認 | 確認 | 確認(回路図 Circuit Designer、基板 PCB Designer/3D) |
| SPICE シミュレーション | あり(解析の種類が多い) | 確認(ngspice を回路図エディタに統合。モデル付きの記号で実行し波形表示) | 未確認(ngspice の記述はあるが、深さは出典で食い違う) | 確認(SPICE シミュレーションの機能ページあり) | 未確認(検索では記載なし) |
| 自動配線 | あり | 未確認(本体の機能としては今回見つけていない) | Pro は内蔵、Standard は外部プラグイン(確認) | 未確認 | セミオートルーター(確認) |
| 差動ペア・等長配線 | あり | 未確認 | 未確認 | 未確認 | 確認 |
| 発注前の点検(DFM) | あり(`fab.rs`) | 未確認 | 未確認 | 未確認 | DFM チェック(確認) |
| 製造データの形式 | Gerber / Excellon / BOM / 配置CSV | Gerber・配置リストなど(確認) | 未確認 | 未確認 | ODB++ に対応(確認)。circuitCad は対象外 |
| 部品店との連携 | 購入リスト(LCSC の型番14件) | 未確認 | LCSC の部品に直接リンク、基板製造と一体(確認。JLCPCB・LCSC・EasyEDA は同じ会社グループ) | 未確認(Octopart 系のページはある) | 未確認 |
| 3D モデルの取り込み | STEP は部品1個ずつ(部品フォーム) | 3D 出力あり(確認)。取り込みの形式は未確認 | OBJ・WRL は両版、STEP は Pro だけ(確認) | STEP の双方向(確認) | 3D 機能あり。取り込み形式は未確認 |
| 3D の書き出し | なし(E3) | 3D 出力あり(確認)。9 で STEP 出力の改善(プレリリース記事の記述) | 未確認 | STEP / Parasolid / 3D PDF(確認) | 未確認 |
| 複数基板(マルチボード) | なし(E5) | 未確認 | 未確認 | あり: 接続の照合(Connection Manager)、3D の嵌合(mating)、システムBOM(確認) | 未確認 |
| 共同作業 | なし | 未確認(Git 運用は一般的だが今回は確認していない) | チーム・権限・ライブラリ共有は両版。ロック・チェックイン/アウトは Pro。リアルタイム共同編集は「Pro の 2025 版で対応」と注記(確認) | 確認(バージョン管理・権限は競合の記述。公式の詳細は未確認) | クラウドベース(確認) |
| 無償利用 | 無償 | 無償・オープンソース(確認) | 未確認 | 有償 | 非商用に限り無償(パッド数・層数に制限あり。確認) |
| 初学者向けの学習機能 | あり(レッスン・チャレンジ・ホバー・学習パス) | 公式には見当たらず。コミュニティの教材が多い(確認) | 未確認 | 未確認 | 未確認 |
| CLI / AI からの操作 | あり(`ccad`、MCP) | 未確認 | EasyEDA 向けの MCP サーバーが第三者から出ている(確認。公式ではない) | 未確認 | 未確認 |

## 分かったこと

**circuitCad が強いところ**
- 回路図・シミュレーション・基板が1本で、解析が多い(過渡・AC・ノイズ・極零・ループゲイン・最悪ケース・モンテカルロ・故障・自己発熱)。KiCad は ngspice を統合しているが、解析ごとの専用画面や測定は circuitCad のほうが多い、という見立て(KiCad 側の細部は未確認)。
- 学習機能と日本語UI。調べた範囲で、公式の機能として持っているものは見つからなかった。
- CLI と MCP で、AI が回路を作って検証できる。

**差が出ているところ**
- 3D と STEP。Altium は STEP/Parasolid の入出力と、機械設計ツールとの同期(MCAD CoDesigner)まである。EasyEDA も Pro なら STEP を取り込める。circuitCad は部品1個の STEP 読み込みだけ(E1〜E4)。
- マルチボード。Altium は「接続の照合 → 3D で嵌合 → システム全体の部品表」の3本立て(E5 の根拠)。
- 発注との一体感。EasyEDA は部品店と基板製造まで同じ会社グループ。circuitCad は購入リストまで(A2)。
- 共同作業。EasyEDA は権限・ロック・チェックイン/アウトまである。circuitCad は個人利用の前提。

**ここから言える次の手**
- 「初学者が基板を発注して終わるまで」を考えると、効きそうなのは **発注との一体感(A2 の型番確認、購入リスト)** と **3D(E1〜E3)**。マルチボードは3Dのあと(`candidates.md` の方針どおり)。
- 共同作業は、個人向けの方針とは別の話なので、今は候補にしない。

## 未確認として残したもの(次に調べるなら)
- KiCad: 自動配線、差動ペア・等長配線、STEP の取り込み、DFM に近いチェック(公式のマニュアルの該当章)。
- Altium: SPICE の解析の種類、自動配線、差動ペア・等長配線(機能ページと詳細のドキュメント)。
- EasyEDA: シミュレーションの種類と深さ(Pro の公式ドキュメント)。
- Quadcept: シミュレーション、STEP の入出力(公式の機能一覧)。

## 出典
- KiCad 9.0 紹介: https://docs.kicad.org/9.0/en/introduction/introduction.html
- KiCad 9 の新機能(紹介記事): https://www.protoexpress.com/blog/kicad-version-9-exploring-the-new-features/
- EasyEDA Standard と Pro の機能比較: https://prodocs.easyeda.com/en/introduction/std-vs-pro/
- EasyEDA について: https://easyeda.com/page/about
- LCSC のブログ(EasyEDA と KiCad の連携): https://lcsc.com/blog/smarter-pcb-design-easyeda/
- Altium Designer のマルチボード: https://www.altium.com/altium-designer/features/multi-board
- Altium Designer の機能一覧: https://altium.com/altium-designer/de/features
- Quadcept Community(窓の杜): https://forest.watch.impress.co.jp/library/software/quadcept/
- Quadcept(MONOist): https://monoist.itmedia.co.jp/mn/articles/1302/28/news133.html
