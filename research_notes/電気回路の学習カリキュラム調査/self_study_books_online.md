# 独学用の電子工作・電子回路の教材 (本とオンライン) の章の順と実習のやり方

調べた日: 2026-11-28。出版社のページ (O'Reilly・Cambridge・gihyo) は一部 403 で読めず、目次は公式の目次ページ・出版社の説明・図書館の目録の抜粋から集めた。**出典が取れなかった章立ては「記憶による・未確認」と明記**し、Gaps に回した。

---

## 1. それぞれの話題の順と、実技 (ブレッドボード・テスター・はんだ付け・オシロ・データシート・基板の発注) をどこに置くか

### Takeaway
理論の順はほぼ共通で「電圧・電流・抵抗とオームの法則 → 直列・並列と分圧 → コンデンサ (RC) → ダイオード → トランジスタ → (オペアンプ) → デジタル (ゲート・フリップフロップ・カウンタ) → マイコン」。違うのは実技の置き場所で、Make: Electronics と SparkFun・Adafruit は**最初の数時間でテスターとブレッドボードを持たせる**。教科書型 (All About Circuits・Khan Academy) は実技を別の巻 (付録・プロジェクトの巻) に分けるか、まったく扱わない。基板の発注 (PCB) を本筋に入れる教材は、今回見た中には無かった。

### Cited Findings

**Learning the Art of Electronics (Hayes・Horowitz、第 2 版 2025 年 4 月、共著に David Abrams が加わった)**
- 章は「日」の番号で、各章に N (本文)・L (実習)・S (補足)・W (例題)・O (オンライン) が付く。毎日ラボを回す授業 (ハーバード大の講義) の形をそのまま本にしたもの — [LAoE 公式の目次](https://learningtheartofelectronics.com/table-of-contents/)
- 順: Part I アナログの受動素子 (1 DC 回路・2 RC 回路・3 ダイオード回路) → Part II 個別のトランジスタ (4 トランジスタ I・5 トランジスタ II) → Part III オペアンプ (6 オペアンプ I・7 理想からのずれ・8 正帰還・9 寄生発振とアクティブフィルタ・10 PID のモーター制御とロックインアンプ・11 電圧レギュレータ・12 MOSFET のスイッチと JFET・13 グループでオーディオの課題) → Part IV デジタル (14 論理ゲート・15 プログラマブルロジック (FPGA と Verilog)・16 フリップフロップ・17 カウンタ・18 メモリ・19 有限状態機械) → Part V (20 A/D・D/A と PLL・21 デジタルの課題) → Part VI マイコン (22〜27: 入門・スタックとタイマー・内蔵の周辺機能・タイマーと割り込み・シリアル通信・RTOS。ARM の SAMD21 ボードで C 言語) → 28 課題の案 — [LAoE 公式の目次](https://learningtheartofelectronics.com/table-of-contents/)
- 実技の付録: A 回路のデバッグ・B ピン配置・C 伝送線路・D **オシロスコープの使い方の助言** — [同上](https://learningtheartofelectronics.com/table-of-contents/)
- 26 章構成で、ラボはアナログとデジタルの釣り合いが取れているという書評 (初版について) — [Physics Today の書評](https://physicstoday.aip.org/reviews/learning-the-art-of-electronics-a-hands-on-lab-course)
- 第 1 章の実習は「Lab: DC circuits」 — [Cambridge Core](https://www.cambridge.org/core/books/learning-the-art-of-electronics/lab-dc-circuits/47BDD40EA8973CD2BA4C42B95EE9BA48)

**All About Circuits「Lessons in Electric Circuits」(6 巻)**
- 巻の順: I 直流 → II 交流 → III 半導体 → IV デジタル → V 参考資料 → VI DIY のプロジェクト — [All About Circuits 教科書の目次](https://www.allaboutcircuits.com/textbook/)
- I 直流の章: 電気の基本 → オームの法則 → **電気の安全 (3 章目)** → 指数表記と接頭語 → 直列と並列 → 分圧器とキルヒホッフの法則 → 直並列 → 直流の計器の回路 → 計装の信号 → 直流の回路網の解析 → 電池と電源 → 導体と絶縁体 → コンデンサ → 磁気 → インダクタ → RC と L/R の時定数 — [同上](https://www.allaboutcircuits.com/textbook/)
- II 交流: 交流の基本 → 複素数 → リアクタンスとインピーダンス (L・C・RLC) → 共振 → 複数の周波数 → フィルタ → 変圧器 → 多相 → 力率 → 交流の計器 → 交流モーター → 伝送線路 — [同上](https://www.allaboutcircuits.com/textbook/)
- III 半導体: 増幅器と能動素子 → 固体の理論 → ダイオードと整流 → バイポーラトランジスタ → JFET → MOSFET → サイリスタ → オペアンプ → 実用のアナログ回路 → 直流モーターの駆動 → 真空管 — [同上](https://www.allaboutcircuits.com/textbook/)
- IV デジタル: 数の表し方 → 2 進の演算 → 論理ゲート → スイッチ → リレー → ラダー図 → ブール代数 → カルノー図 → 組み合わせ回路 → マルチバイブレータ → 順序回路 → シフトレジスタ → D/A・A/D → デジタル通信 → メモリ → 計算機の原理 — [同上](https://www.allaboutcircuits.com/textbook/)
- V 参考資料に **SPICE の使い方・故障探し (理論と実践)・回路図記号** が入る。VI DIY は 入門 → 基本の課題と**測定器** → 直流 → 交流 → 個別半導体 → アナログ IC → デジタル IC → 555 の課題 — [同上](https://www.allaboutcircuits.com/textbook/)

**Khan Academy「Electrical engineering」の Circuit analysis**
- 単元: 回路素子 (理想と実際の R・C・L と電源)・抵抗の回路 (直列・並列・Δ-Y・分圧)・直流の回路解析 (キルヒホッフ・節点電圧法・網目電流法)・自然応答と強制応答 (RC・RL・RLC の時間変化)・交流の回路解析 (正弦波入力を微分方程式でなく交流の方法で) — [Khan Academy の静的な写し](https://zubairlk.github.io/ka-static-demo/Science/Electrical_engineering/Circuit_analysis/index.html) (本家 khanacademy.org は読み込めなかった。写しの並び順は本家と違う可能性あり)
- 動画には理想素子・理想電源・キルヒホッフの電圧則・節点電圧法・受動符号規約・並列抵抗がある — [検索結果の抜粋: Khan Academy](https://www.khanacademy.org/science/electrical-engineering/ee-circuit-analysis-topic/ee-resistor-circuits/v/ee-parallel-resistors-1)
- 作者 Willy McAllister は同じ内容を Spinning Numbers というサイトでも公開している — [Spinning Numbers](https://spinningnumbers.org/about/)

**Make: Electronics 第 3 版 (Charles Platt、2021 年秋、Make: Community)**
- 副題どおり「発見しながら学ぶ (learning through discovery)」を初版 (2009 年) から続けている。扱うのは個別の部品 (抵抗・コンデンサ・トランジスタ・インダクタ・ダイオード・IC) で、マイコンを使うときも必要だという立場 — [Make: の紹介ページ](https://make.co/?p=15067)
- 第 3 版の変更: 写真と回路図をほぼ全部差し替え、**ブレッドボードは全編で片側 1 本のバスのもの**、配置は写真でなく図で見せる、IC の中の図を描き直し、**Arduino を広げた (Arduino の課題が 3 つ新しく)**、サイコロ・防犯ブザー・反応時間計・コイルとコンデンサの音の合成器・簡単なオーディオアンプ・タイマーと発振器・電磁気の実演が新しいか改訂 — [同上](https://make.co/?p=15067)
- 章は 5 つのセクション (Section One: The Basics、Section Two: Switching …)。実験は番号付きで、実験 1 は「Taste the Power!」、実験 5 は「Let's Make a Battery」、第 3 版の実験 6 は「Getting Connected」。付録 A の表は 5 セクションの実験 1〜30 を対象にしていると書かれる — [検索結果の抜粋: O'Reilly 第 3 版](https://oreilly.com/library/view/make-electronics-3rd/9781680456868)
- 実験 1 は 9V 電池を舐める、実験 2 は電池を短絡させる (第 2 版の説明) — [DigiKey ブログ 実験 1](https://www.digikey.com/en/blog/taste-the-power-experiment-1); [DigiKey ブログ 実験 2](https://www.digikey.com/en/blog/lets-abuse-a-battery-experiment-2)
- 第 2 版では第 1 章 The Basics が実験 1〜5、実験 8 が「A Relay Oscillator」。初版の章名は「Experiencing Electricity」「Switching Basics and More」「Getting Somewhat More Serious」「Chips, Ahoy!」 — [検索結果の抜粋: O'Reilly 第 2 版](https://www.oreilly.com/library/view/make-electronics-2nd/9781680450255/ch01.html)
- 第 3 版専用の部品キットがある — [O'Reilly 第 3 版](https://oreilly.com/library/view/make-electronics-3rd/9781680456868)

**Practical Electronics for Inventors (Scherz・Monk、第 4 版 2016 年、McGraw Hill)**
- 第 4 版はプログラマブルロジック・半導体・オペアンプ・電圧レギュレータ・電源・デジタルを新しく扱う。第 3 版でセンサー・マイコン・モジュール (完成品の基板)・ソフトの道具の章が入った — [検索結果の抜粋: O'Reilly 第 4 版](https://www.oreilly.com/library/view/practical-electronics-for/9781259587559/)
- 第 1 章「Introduction to Electronics」、第 2 章「Theory」、第 17 章まである — [O'Reilly 第 1 章](https://www.oreilly.com/library/view/practical-electronics-for/9781259587559/xhtml/12_Chapter_01.xhtml); [第 17 章](https://www.oreilly.com/library/view/practical-electronics-for/9781259587559/xhtml/28_Chapter_17.xhtml)
- 第 5 版は見つからなかった (検索で出たのは第 3 版と第 4 版だけ) — [検索結果](https://www.oreilly.com/library/view/practical-electronics-for/9781259587559/)

**Getting Started in Electronics (Forrest Mims III)**
- 128 ページ、Master Publishing、第 3 版 2003 年。章の名は第 1 章 Electricity、第 2 章 Electronic Components (小売店の抜粋。全体は未確認)。回路は基本・光・デジタル・リニアに分けて載る — [TAMU 図書館の目録](https://libcat.library.tamu.edu/Record/in00002363321/TOC) (403 で読めず、検索の抜粋); [Internet Archive の写し](https://archive.org/details/getting-started-in-electronics/mode/2up)
- 「史上最高の電子工作の本」と呼ぶブロガーもいる (手書きの図と少ない数式が理由) — [MightyOhm](https://mightyohm.com/blog/2008/12/the-greatest-electronics-book-ever-written/)

**SparkFun Learn**
- 「Voltage, Current, Resistance, and Ohm's Law」を電子工作の 3 つの基本の部品と呼び、出発点に置く。はんだ付けは基本の技能 — [SparkFun Engineering Essentials](https://www.sparkfun.com/engineering-essentials)
- 「How to Use a Multimeter」(2015 年 1 月) は導通・電圧・抵抗・電流を扱い、今は保守されていない (archived) 扱い — [検索結果の抜粋: SparkFun](https://learn.sparkfun.com/tutorials/tags/electrical-engineering?page=all)
- 入門の入り口は「Where Do I Start?」で、概念のチュートリアルと SparkFun Inventor's Kit を勧める — [SparkFun Where Do I Start?](https://learn.sparkfun.com/tutorials/where-do-i-start) (403 で本文は読めず、検索の抜粋)
- 「How to Use a Breadboard」は家で学ぶ上位 15 本のまとめに入っている — [SparkFun News 3271](https://news.sparkfun.com/3271)

**Adafruit Learn**
- Learn Arduino シリーズ (18 回): 0 準備 → 1 Blink → 2 LED (**抵抗の値を変えて明るさを変える**) → 3 RGB LED → 4 74HC595 のシフトレジスタ → 5 シリアルモニタ → 6 デジタル入力 (押しボタン) → 7 RGB のフェーダー → 8 アナログ入力 (ボリューム) → 9 光センサー (CdS) → 10 圧電スピーカー → 11・12 LCD → 13 **トランジスタで DC モーター** → 14 サーボ → 15 L293 で正逆転 → 16 ステッピングモーター → 17 PIR とメール — [Adafruit Learn Arduino](https://learn.adafruit.com/series/learn-arduino)
- 「Adafruit Guide to Excellent Soldering」は道具から始まる: 25W 以上の鉛筆型、温度調節つきが良い、ブタン・電池式は非常用、はんだごてガン・トーチ・cold-heat は基板に使わない、こて台・60/40 のヤニ入りはんだ・換気・ニッパー、万力・ヘルピングハンド・吸い取り器と吸い取り線。次のページは準備 (preparation) — [Adafruit Guide to Excellent Soldering](https://learn.adafruit.com/adafruit-guide-excellent-soldering)

**日本の本: 『改訂新版 電子工作入門以前』(後閑哲也、技術評論社、2025 年)**
- 1 章 電気の発見からトランジスタまでの歴史 → 2 章 電子工作の始め方 (道具とコラム「はんだ付けのコツ」・パソコン・**ブレッドボードによる組み立て**・**計測器 (デジタルマルチメータ)**) → 3 章 回路設計の基礎 (**回路図が読めて描けるようになるには**・抵抗・コンデンサ・コイル・能動部品) → 4 章 電源 (電源の種類・太陽電池・実験用電源の製作・電源の不具合と対策・放熱) → 5 章 LED (基本・明るさ・電池 1 個で・ソーラーライト・赤外線リモコン・7 セグ時計) → 6 章 音 → 7 章 FM ラジオ → 8 章 動くもの (サーボ・ステッピング・リモコンカー) → 9 章 センサーの記録 (USB シリアル・Wi-Fi でクラウド) → 付録 Raspberry Pi Pico W・**表面実装 IC のはんだ付け** — [gihyo.jp](https://gihyo.jp/book/2025/978-4-297-15070-9)
- 目次にデータシートの読み方の節は無い — [同上](https://gihyo.jp/book/2025/978-4-297-15070-9)
- ブレッドボードは工具なしで回路を作れるのが利点、LED は砲弾型の 2 本足がブレッドボードで扱いやすい (PC Watch の連載「再入門! ブレッドボード電子工作」) — [PC Watch](https://pc.watch.impress.co.jp/docs/column/musashino_proto/517061.html)

### Inferences
- 実技の置き場所は 3 型に分かれる: (a) 最初に道具を持たせる型 (Make: Electronics・SparkFun・Adafruit・電子工作入門以前の 2 章) / (b) 毎回の理論の直後にラボを組む型 (LAoE: 章ごとに N の直後に L) / (c) 理論の巻と実習の巻を分ける型 (All About Circuits: I〜IV が理論、VI が DIY、V に SPICE と故障探し) / (d) 実技なし (Khan Academy)。
- オシロスコープを独立に扱うのは LAoE (付録 D) くらいで、入門書はテスター止まりが多い。データシートの読み方を章にした教材は、今回見た目次には無かった (Gaps)。
- 基板の発注・ガーバーは、どの教材の目次にも出てこない。circuitCad の「回路図 → シミュレーション → 基板 → 発注」の後半は、既存の独学ルートの外にある (空白がある) と言えそう。
- SPICE を教材に入れているのは All About Circuits (V 巻) だけで、シミュレーションを学習の道具として使う流れは本の側にはほとんど無い。

### Gaps
- Make: Electronics 第 3 版の全実験名と番号は取れなかった (O'Reilly は 403)。付録 A の「実験 1〜30」という記述と、第 2 版の 36 実験 (記憶による・未確認) が食い違う可能性がある。記憶では第 2 版は The Basics (1〜5) → Switching Basics (6〜11: スイッチ・リレー・リレーの発振・コンデンサと時間・トランジスタのスイッチ) → Getting Somewhat More Serious (12〜15: **はんだ付けはここで初めて**・LED を焼く・パルス) → Chips, Ahoy! (16〜24: 555・論理ゲート・フリップフロップ・カウンタ・サイコロ) → What Next? (25〜36: 磁気・発電・スピーカー・コイル・フィルタ・ファズ・ラジオ・マイコン) だが、出典で確かめていない。
- Practical Electronics for Inventors 第 4 版の 3〜16 章の名は出典で確かめられなかった。記憶では 3 基本の部品 → 4 半導体 → 5 光エレクトロニクス → 6 センサー → 7 **Hands-on Electronics (安全・ブレッドボード・はんだ付け・テスター・オシロ・PCB の作り方)** → 8 オペアンプ → 9 フィルタ → 10 発振器とタイマー → 11 レギュレータと電源 → 12 デジタル → 13 マイコン → 14 プログラマブルロジック → 15 モーター → 16 オーディオ → 17 モジュール。7 章が実技の章であることは要確認。
- Mims の全目次は取れなかった (目録が 403)。
- SparkFun の公式の「順番」を示すページは本文が読めず、順番は未確認。
- Khan Academy の本家の単元の並びは読めなかった (写しの並びは「直流解析 → 抵抗 → 素子 → 応答 → 交流」だが、本家の並びは素子が先の可能性がある)。
- 「電子工作の素」(書名どおりの本) と、トランジスタ技術の入門特集・CQ 出版の定番回路の本の目次は見つけられなかった。

---

## 2. 「発見しながら学ぶ」と「理論が先」: どう並べるか

### Takeaway
Make: Electronics は「まず壊して・触って・測ってから、何が起きたかを説明する」順で、電池を舐める・短絡させる・LED を焼くところから入る。理論が先の教材 (All About Circuits・Khan Academy) は数学と法則を積み上げ、実習は後ろの巻か無し。LAoE はその中間で、毎日「短い本文 → すぐラボ」。日本の『電子工作入門以前』は「歴史 → 道具 → 回路の基礎 → 作りたい物ごとの章」で、後半は目的 (光らせたい・音を出したい・動かしたい) の順に並ぶ。

### Cited Findings
- Make: Electronics は初版 (2009 年) から「learning through discovery」を続ける — [Make:](https://make.co/?p=15067)
- 実験 1 は 9V 電池を舐める、実験 2 は電池を短絡させる — [DigiKey 実験 1](https://www.digikey.com/en/blog/taste-the-power-experiment-1); [DigiKey 実験 2](https://www.digikey.com/en/blog/lets-abuse-a-battery-experiment-2)
- 第 3 版は配置を写真でなく図で見せる・片側 1 本のバスのブレッドボードで統一 (初学者が迷わないように形をそろえた) — [Make:](https://make.co/?p=15067)
- LAoE は各章 (= 1 日) に本文 N と実習 L が対になっている — [LAoE 目次](https://learningtheartofelectronics.com/table-of-contents/)
- All About Circuits は 1 巻の 3 章目に電気の安全を置き、理論の巻 (I〜IV) と DIY の巻 (VI) を分ける — [All About Circuits](https://www.allaboutcircuits.com/textbook/)
- Khan Academy は節点電圧法・網目電流法・微分方程式の応答・交流の解析と、大学の回路理論の順 — [Khan 静的な写し](https://zubairlk.github.io/ka-static-demo/Science/Electrical_engineering/Circuit_analysis/index.html)
- Adafruit の Arduino シリーズは課題ごとに部品を 1 つずつ足す (LED → RGB LED → シフトレジスタ → ボタン → ボリューム → 光センサー → モーター) — [Adafruit](https://learn.adafruit.com/series/learn-arduino)
- 『電子工作入門以前』は 5〜9 章を「LED を光らせたい」「音を出したい」「ラジオを作りたい」「動くものを作りたい」「センサデータを記録したい」と目的で並べる — [gihyo.jp](https://gihyo.jp/book/2025/978-4-297-15070-9)

### Inferences
- 発見型は「現象 → 測る → 名前と式」、理論型は「式 → 例題 → (後で) 作る」。発見型でも順は理論型とほぼ同じ (電池 → 抵抗 → スイッチ・リレー → コンデンサ → トランジスタ → IC) なので、違いは「各段で先に手を動かすか」だけ。
- circuitCad の場合、シミュレーションは「壊しても良い実験」を安全に提供できる (電池を短絡・LED を焼くのを画面の中で) ので、Make: の発見型の流れと相性が良い。
- 「目的ごとの章」(日本の本・Adafruit) は、部品の種類ではなく作りたい物で入口を選ばせる。学習パスやサンプルの並べ方の参考になる。

### Gaps
- Platt 本人が発見型の理由を述べた序文の文は読めなかった (O'Reilly の序章が 403)。
- 発見型と理論型の学習効果を比べた研究は今回探していない。

---

## 3. つまずく所として扱われている話題と、その扱い方

### Takeaway
目次から見える「わざわざ章や課題を立てている」つまずき所は、LED の電流制限抵抗、安全、トランジスタ (2 章に分ける・スイッチとして先に)、オペアンプの理想からのずれと発振、電源の不具合、回路図の読み方、デバッグ。扱い方は「わざと失敗させる」(Make: の LED を焼く)、「値を変えて見せる」(Adafruit の抵抗の値と明るさ)、「章を分けて段階的に」(LAoE のトランジスタ I・II、オペアンプ I〜V)。

### Cited Findings
- LED の抵抗: Adafruit の Lesson 2 は抵抗の値を変えて LED の明るさを変える — [Adafruit](https://learn.adafruit.com/series/learn-arduino)。『電子工作入門以前』は 5 章を LED の基本の使い方と明るさの変え方から始める — [gihyo.jp](https://gihyo.jp/book/2025/978-4-297-15070-9)
- 安全: All About Circuits は直流の巻の 3 章目に電気の安全 — [All About Circuits](https://www.allaboutcircuits.com/textbook/)。Adafruit のはんだ付けは換気・こて台・適したこてを最初に — [Adafruit Soldering](https://learn.adafruit.com/adafruit-guide-excellent-soldering)
- トランジスタ: LAoE は「Transistors I」「Transistors II」の 2 日に分け、MOSFET のスイッチは別の章 (12) — [LAoE 目次](https://learningtheartofelectronics.com/table-of-contents/)。Make: 第 2 版ではリレーを先にやってからトランジスタのスイッチへ (Switching の章) — [O'Reilly 第 2 版の抜粋](https://www.oreilly.com/library/view/make-electronics-2nd/9781680450255/ch01.html)。Adafruit はモーターを回すところで初めてトランジスタ (Lesson 13) — [Adafruit](https://learn.adafruit.com/series/learn-arduino)
- オペアンプ: LAoE は「理想からのずれ」「寄生発振」を別の章にしている — [LAoE 目次](https://learningtheartofelectronics.com/table-of-contents/)
- 電源・放熱: 『電子工作入門以前』4 章に「電源に関する不具合と対策」「放熱の方法と放熱器」 — [gihyo.jp](https://gihyo.jp/book/2025/978-4-297-15070-9)
- 回路図: 『電子工作入門以前』3-1「回路図が読めて描けるようになるには」、All About Circuits V 巻に回路図記号の章 — [gihyo.jp](https://gihyo.jp/book/2025/978-4-297-15070-9); [All About Circuits](https://www.allaboutcircuits.com/textbook/)
- デバッグ・故障探し: LAoE の付録 A 回路のデバッグ、All About Circuits V 巻の故障探し — [LAoE 目次](https://learningtheartofelectronics.com/table-of-contents/); [All About Circuits](https://www.allaboutcircuits.com/textbook/)
- 伝送線路 (グラウンド・配線の上級の話): LAoE 付録 C、All About Circuits II 巻 14 章 — [LAoE 目次](https://learningtheartofelectronics.com/table-of-contents/); [All About Circuits](https://www.allaboutcircuits.com/textbook/)
- はんだ付けの失敗: Adafruit は弱いこては部品を熱しすぎ、酸化膜ができ、イモはんだ (cold joint) の元になると説明 — [Adafruit Soldering](https://learn.adafruit.com/adafruit-guide-excellent-soldering)

### Inferences
- トランジスタは「バイアス (増幅)」より先に「スイッチ」として教える流れが入門書・Arduino 系では主流 (Make:・Adafruit)。バイアスを正面から扱うのは LAoE と All About Circuits III 巻の理論型。
- デカップリング (パスコン) とグラウンドを独立の章にした入門教材は、今回の目次には見当たらない。LAoE の寄生発振の章・電源の不具合の節で間接に出る程度と思われる。

### Gaps
- 「パスコン」「グラウンド」を各教材がどう説明しているかは本文が読めず未確認。
- Make: 第 3 版で LED を焼く実験 (第 2 版の「Broil an LED」、記憶による) が残っているかは未確認。

---

## 4. ほぼ全部にある話題 (共通の背骨) と、一部にしかない話題

### Takeaway
共通の背骨: 電圧・電流・抵抗とオームの法則、直列・並列・分圧、コンデンサと RC の時間、ダイオードと LED、トランジスタ。実技を扱う教材ではブレッドボードとテスターも背骨。一部だけ: オペアンプ・電源 (レギュレータ)・デジタル論理・マイコン・モーター・ラジオ (RF)・音・交流と変圧器・SPICE・プログラマブルロジック・基板の発注 (無し)。

### Cited Findings
- オームの法則・直列並列・分圧・コンデンサ・RC: All About Circuits I 巻、Khan Academy、LAoE 1〜2 章、SparkFun の Ohm's law — [All About Circuits](https://www.allaboutcircuits.com/textbook/); [Khan 写し](https://zubairlk.github.io/ka-static-demo/Science/Electrical_engineering/Circuit_analysis/index.html); [LAoE](https://learningtheartofelectronics.com/table-of-contents/); [SparkFun](https://www.sparkfun.com/engineering-essentials)
- オペアンプ: LAoE (5 章分)、All About Circuits III 巻、PEFI 第 4 版で新しく — [LAoE](https://learningtheartofelectronics.com/table-of-contents/); [All About Circuits](https://www.allaboutcircuits.com/textbook/); [O'Reilly PEFI 第 4 版](https://www.oreilly.com/library/view/practical-electronics-for/9781259587559/)。Khan Academy の Circuit analysis には無い — [Khan 写し](https://zubairlk.github.io/ka-static-demo/Science/Electrical_engineering/Circuit_analysis/index.html)
- 電源・レギュレータ: LAoE 11 章、PEFI 第 4 版、『電子工作入門以前』4 章 — [LAoE](https://learningtheartofelectronics.com/table-of-contents/); [O'Reilly PEFI](https://www.oreilly.com/library/view/practical-electronics-for/9781259587559/); [gihyo.jp](https://gihyo.jp/book/2025/978-4-297-15070-9)
- デジタル論理: LAoE Part IV、All About Circuits IV 巻、PEFI — [LAoE](https://learningtheartofelectronics.com/table-of-contents/); [All About Circuits](https://www.allaboutcircuits.com/textbook/); [O'Reilly PEFI](https://www.oreilly.com/library/view/practical-electronics-for/9781259587559/)。Mims はデジタルとリニアの回路を載せる — [検索の抜粋](https://archive.org/details/getting-started-in-electronics/mode/2up)
- マイコン: LAoE Part VI (SAMD21・C・RTOS)、Make: 第 3 版 (Arduino の課題 3 つ)、PEFI 第 3 版以降、Adafruit (全編)、『電子工作入門以前』(Pico W) — [LAoE](https://learningtheartofelectronics.com/table-of-contents/); [Make:](https://make.co/?p=15067); [O'Reilly PEFI](https://www.oreilly.com/library/view/practical-electronics-for/9781259587559/); [Adafruit](https://learn.adafruit.com/series/learn-arduino); [gihyo.jp](https://gihyo.jp/book/2025/978-4-297-15070-9)
- モーター: Adafruit (DC・サーボ・ステッピング)、All About Circuits (交流モーター・直流モーターの駆動)、LAoE 10 章 (PID のモーター制御)、『電子工作入門以前』8 章 — 各出典は上と同じ
- ラジオ・RF: 『電子工作入門以前』7 章 FM ラジオ、All About Circuits II 巻の伝送線路、LAoE の PLL — [gihyo.jp](https://gihyo.jp/book/2025/978-4-297-15070-9); [All About Circuits](https://www.allaboutcircuits.com/textbook/); [LAoE](https://learningtheartofelectronics.com/table-of-contents/)
- 磁気・コイル: All About Circuits I 巻 (磁気・インダクタ)、Make: 第 3 版の電磁気の実演とコイルとコンデンサの音の合成器 — [All About Circuits](https://www.allaboutcircuits.com/textbook/); [Make:](https://make.co/?p=15067)
- プログラマブルロジック (FPGA・Verilog): LAoE 15 章、PEFI 第 4 版 — [LAoE](https://learningtheartofelectronics.com/table-of-contents/); [O'Reilly PEFI](https://www.oreilly.com/library/view/practical-electronics-for/9781259587559/)
- 555 タイマー: All About Circuits VI 巻に 555 の課題の章 — [All About Circuits](https://www.allaboutcircuits.com/textbook/)。Make: 第 3 版の「タイマー回路と発振器」 — [Make:](https://make.co/?p=15067)
- SPICE: All About Circuits V 巻だけ — [All About Circuits](https://www.allaboutcircuits.com/textbook/)
- 表面実装のはんだ付け: 『電子工作入門以前』付録 2 — [gihyo.jp](https://gihyo.jp/book/2025/978-4-297-15070-9)
- センサーとデータの記録・クラウド: 『電子工作入門以前』9 章、PEFI 第 3 版以降のセンサーの章、Adafruit (光・PIR) — [gihyo.jp](https://gihyo.jp/book/2025/978-4-297-15070-9); [O'Reilly PEFI](https://www.oreilly.com/library/view/practical-electronics-for/9781259587559/); [Adafruit](https://learn.adafruit.com/series/learn-arduino)

### Inferences
- 背骨の並び (R → 分圧 → RC → ダイオード/LED → トランジスタ) は全教材でほぼ同じなので、circuitCad の学習パスもこの順にしておけば、どの本と並べて使っても迷わない。
- 背骨の後は 3 つに分かれる: アナログ深掘り (オペアンプ・電源・フィルタ)、デジタル (ゲート → FF → カウンタ → マイコン)、目的の工作 (光・音・モーター・ラジオ・センサー)。入門書・オンラインは後者 2 つに、LAoE と All About Circuits は前者 2 つに重みがある。
- 新しい版ほどマイコン (Arduino・Pico) を広げている (Make: 第 3 版、PEFI 第 3〜4 版、『電子工作入門以前』の改訂新版)。
- 基板の設計と発注・データシートの読み方・パスコンとグラウンドは、どの独学ルートでも薄い (今回の目次では見当たらない)。circuitCad が足せる余地。

### Gaps
- PEFI 第 4 版と Mims の目次を出典で確かめられなかったので、両者での電源・オペアンプ・モーターの扱いは一部未確認。
- トランジスタ技術の入門特集・CQ 出版の定番回路の本は見つけられなかった。
