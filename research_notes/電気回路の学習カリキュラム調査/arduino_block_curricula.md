# マイコンとブロックプログラミングによる初学者向け電子工作カリキュラム (Arduino Starter Kit / CTC GO! / Tinkercad / micro:bit / mBlock / 日本の学校教育)

調査日: 2026-11-28。一次資料のうち取得できたもの: Arduino Projects Book の本文 (テキスト化された写し)、Weintrop & Wilensky 2019 の論文 PDF、microbit.org の単元ページ、文部科学省の研修用教材 PDF。Tinkercad・CTC GO!・mBlock は販売ページと二次資料しか得られなかった (各節の Gaps 参照)。

## 1. 回路の概念をどの順に出すか (LED+抵抗 → スイッチとプルアップ/プルダウン → 可変抵抗とアナログ入力 → PWM → センサ → モータとトランジスタ → サーボ → シリアルモニタ → チャタリング)

### Takeaway
Arduino Starter Kit は「マイコン無しの回路 (直列・並列・ブレッドボード)」→ デジタル入出力 (プルダウン抵抗) → アナログ入力とシリアルモニタ → PWM (アナログ出力) → サーボ → … → トランジスタとモータ (別電源・ダイオード・GND 共通) → H ブリッジ、の順で、電流の大きい負荷は後半 (9〜10 番) に置く。micro:bit (MakeCode) は基板に LED マトリクス・ボタン・センサが載っているので外付けの回路を出さずに「出力 → 入力 → 変数 → センサとしきい値 → 乱数」と、プログラミングの概念の順に並ぶ。日本の中学校技術科は回路の細部よりも「センサ → 判断 → アクチュエータ」の計測・制御システムとしての見方を重視する。

### Cited Findings
**Arduino Starter Kit (Projects Book, 15 プロジェクト)**
- 15 件の順: 01 Get to Know Your Tools / 02 Spaceship Interface / 03 Love-O-Meter / 04 Color Mixing Lamp / 05 Mood Cue / 06 Light Theremin / 07 Keyboard Instrument / 08 Digital Hourglass / 09 Motorized Pinwheel / 10 Zoetrope / 11 Crystal Ball / 12 Knock Lock / 13 Touchy-Feel Lamp / 14 Tweak the Arduino Logo / 15 Hacking Buttons — [Pimoroni](https://shop.pimoroni.com/products/arduino-starter-kit-1); [Mouser](https://www.mouser.in/new/arduino/arduino-starter-kit/)
- 各プロジェクトの「Discover」(その回に学ぶこと) と主な部品 — [Arduino Projects Book (テキスト化した写し)](https://huggingface.co/datasets/Vikhrmodels/programming_books_eng/blob/main/Arduino/Arduino_Projects_Book.pdf.txt)
  - 01: 電気の基礎理論・ブレッドボードのしくみ・直列と並列。スイッチ・LED・220Ω。**Arduino は 5V の電源として使うだけでスケッチは無い**
  - 02: デジタル入出力・最初のプログラム・変数。LED 3 個・スイッチ・220Ω・**10kΩ のプルダウン抵抗**。本はプルダウンを「スイッチが開いているときにピンを GND につなぐ」と説明
  - 03: **アナログ入力・シリアルモニタ** (TMP36 温度センサ、LED 3 個)
  - 04: アナログ出力 (PWM)・値の変換 (map)。フォトレジスタ 3 個と 10kΩ (分圧)、RGB LED
  - 05: 値の変換・**サーボ**・ライブラリ。**可変抵抗**はここで初めて出る。サーボが動き始めに電流を多く引くので 100µF のコンデンサで電圧の落ち込みをならす
  - 06: tone() で音・アナログセンサの校正 (フォトレジスタ+10kΩ、圧電ブザー)
  - 07: 抵抗のはしご (複数のスイッチを 1 本のアナログ入力で読む)・配列
  - 08: long 型・タイマ (millis()、unsigned long)。傾きスイッチ
  - 09: **トランジスタ・大電流/高電圧の負荷**。モータ・トランジスタ・ダイオード・9V 電池
  - 10: **H ブリッジ** (モータの向きを変える)
  - 11: LCD・switch/case・random()
  - 12: **圧電素子を入力に**・自作の関数 (サーボ・1MΩ)
  - 13: 外部ライブラリの導入・タッチ (静電容量) センサ
  - 14: Processing とのシリアル通信
  - 15: **フォトカプラ**・他の機器とつなぐ (はんだごてが要る)
- 9 番の説明 (要旨): モータは Arduino のピンが出せる以上の電流を要るので、トランジスタを電子スイッチとして使い、小さな信号で別の 9V 電源のモータを入り切りする。止めたときにモータが返す電圧から守るため、電源と逆向きにダイオードを並列に入れる。電圧の違う 2 つの回路は電源の線を分けたまま GND だけをつなぐ (共通の GND) — [Arduino Projects Book](https://huggingface.co/datasets/Vikhrmodels/programming_books_eng/blob/main/Arduino/Arduino_Projects_Book.pdf.txt)
- チャタリング (debounce) を正面から説明する箇所は、取得した本文の範囲では見つからなかった (8 番の millis() のタイマを 12 番が再利用する形) — 同上
- スケッチは Arduino IDE の File / Examples / 10.StarterKit に入っている — [The Pi Hut](https://thepihut.com/products/arduino-starter-kit)

**Arduino CTC GO! (Core Module, 高校向け)**
- 8 つのレッスン → 8 つのガイド付きプロジェクト → 6 つの自分で進めるプロジェクト、の 3 段。ガイド付きプロジェクトでレッスンで得た知識を使って作る。45 分 × 20 回、24 人、14〜17 歳。UNO WiFi Rev2 と Education Shield 8 組 — [Arduino Store](https://store.arduino.cc/ctc-go-core-module); [Arduino Education CTC GO!](https://arduino.cc/education/ctc-go)

**BBC micro:bit (MakeCode、microbit.org の単元「First lessons with MakeCode and the micro:bit」、7〜11 歳)**
- 順: 1 名札 (表示・アルゴリズム・繰り返し) → 2 鼓動するハート (順次・時間・ループ) → 3 気分バッジ (ボタン入力・選択) → 4 歩数計 (加速度センサ・変数・命令の順序) → 5 常夜灯 (光センサ・if/else・forever・制御システム) → 6 じゃんけん (乱数・else if・公平さの評価) — [micro:bit Educational Foundation](https://www.microbit.org/teach/lessons/first-lessons-with-makecode-and-the-microbit)

**Tinkercad Circuits**
- Arduino の「スターター」(Blink など) を開くとコードの欄にブロックと生成されたコードが出る。よくある進み方は、LED の点滅 → 繰り返しブロックで複数の LED → 押しボタンのデジタル入力 — [Lynda (LinkedIn Learning)](https://www.lynda.com/Tinkercad-tutorials/Writing-code-Arduino/2839010/2363089-4.html); [Udemy の講座の説明](https://www.udemy.com/course/learn-arduino-using-tinkercad-for-beginners/)
- 基本の回路と Arduino の対話型レッスンは横のパネルに手順を出す形。ASU の授業案は「3 つのシミュレーションのレッスン、続けると全 24 件」と書く — [ASU STEM lesson plan](https://stemteachers.asu.edu/stem-lesson-plans/tinkercad-building-basic-circuits)

**日本の中学校 技術分野 D(3)「計測・制御のプログラミングによる問題の解決」**
- 必要なセンサやアクチュエータ (電気エネルギーを動作に変える装置 [モータなど]) を選び、計測・制御システムを構想し、センサで計った値をもとにアクチュエータを制御するプログラムを作る。例は土中水分センサ (A0) の値が 40 より小さくなったらリレー (D6) で電動ポンプを動かす「自動灌水システム」(Scratch で) と「生活支援ロボット」 — [文部科学省 研修用教材「D 情報の技術」](https://www.mext.go.jp/content/20210419-mxt_jogai01-000006333_001.pdf)
- 2017 (平成 29) 年告示では、前回から必修だった計測・制御に加えて、ネットワークを使った双方向性のあるコンテンツのプログラミングが加わった — 同上
- 旭川市の実践はセンサ・アクチュエータの「しきい値や動作」の理解を知識・技能の目標にする。安中市の実践は micro:bit と「TECH 未来」で、条件分岐の設定値を変えて動作の違いを確かめる — [IPSJ 会誌の記事](https://www.ipsj.or.jp/magazine/kdecuq00000017kq-att/IPSJ-MGN670807.pdf) (検索結果の要約から。全文は未確認)

**日本の小学校 (MESH)**
- 6 年理科「電気の利用」で、人がいるときだけ明かりをつけるプログラムを班で考え MESH (人感・明るさのセンサのブロック) で作る形が各地にある (東京都・大田区・大分県・長野県) — [東京都教育委員会](https://infoedu.metro.tokyo.lg.jp/file/5e47e304526dc16c1469008b5edb4a7a.pdf); [文部科学省 未来の学びコンソーシアム](https://www.mext.go.jp/miraino_manabi/content/135.html); [長野県](https://www.pref.nagano.lg.jp/kyoiku/kyogaku/kyoshokuin/shiryo/documents/programing_sho6.pdf)

### Inferences
- Arduino Starter Kit の順は「流れる電流が小さく壊しにくいもの → 大きいもの」と「デジタル → アナログ入力 → アナログ出力」の 2 本の軸で並んでいる。可変抵抗は入力の 1 番目ではなく 5 番 (サーボと一緒)、アナログ入力の最初は温度センサとシリアルモニタ (03)。プルダウンが 02 で最初に出る (プルアップと INPUT_PULLUP は前面に出ない)。
- micro:bit と MESH は「回路を組まない」ことで電気の失敗 (浮いた入力・電流制限) を避け、概念を「入力・判断・出力」に絞っている。回路の理解はカリキュラムの外に出されている。
- circuitCad の試作のタブ (ブレッドボード + Arduino のスケッチ + ブロック) は、Starter Kit の回路の順と、Tinkercad のブロック/コード併記の両方に近い位置にある。

### Gaps
- Tinkercad の公式 Learn (Circuits / Arduino) のレッスン名の今の一覧は得られなかった (公式ページを取得していない)。
- CTC GO! Core のレッスン 8 件の題名と順は、販売ページに無く、カリキュラム資料 (要申請) で未確認。Arduino Education Science Kit は調べられなかった。
- Starter Kit の本でチャタリングを扱うかは、本文の取得範囲では確認できなかった。

## 2. ブロックからテキストへの橋渡しと、その効果

### Takeaway
MakeCode・Tinkercad・mBlock はどれも「ブロックと文のコードを同時に見せ、片方を変えるともう片方が変わる」形で橋をかける。研究 (Weintrop & Wilensky) では、ブロックで始めた高校生は 5 週後に文の環境より伸びたが、Java に移って 10 週後にはその差が消えた。つまり「ブロックの伸びは自動では文に移らない」ので、移る段を意図して支える必要がある。

### Cited Findings
- MakeCode はブラウザにエディタ・シミュレータ・デバッガ・コンパイラを持ち、Blockly と Static TypeScript を自動で相互変換するので編集の形を切り替えられる。ただしクラスなど Blockly で編集できない構文がある — [Microsoft Research (SPLASH-E 2019)](https://microsoft.com/en-us/research/uploads/prod/2019/10/splashws19splashemain-p8-p-41a6cf2-42682-final.pdf)
- micro:bit は Blocks・JavaScript・Python の 3 つで書け、研究ではブロックと文のエディタを行き来する「ハイブリッド」の環境と特徴づけられる — [arXiv 2110.00812](https://arxiv.org/pdf/2110.00812)
- ブロックのエディタから JavaScript に切り替えるボタンで、生徒は 2 つの表し方の対応を見比べられる — [UBC Digital Sandbox](https://scarfedigitalsandbox.teach.educ.ubc.ca/bbc-microbit/)
- MakeCode には 9〜15 歳向けの 14 週の入門コースがある — [Microsoft MakeCode resources](https://www.microsoft.com/makecode/resources?azure-portal=true)
- Tinkercad の Arduino のコード欄は、ブロック・文 (C++)・ブロック+文の 3 つの見え方を持つ。ブロックは色で分類 (灰=コメント、青=出力、橙=制御) — [Lynda](https://www.lynda.com/Tinkercad-tutorials/Writing-code-Arduino/2839010/2363089-4.html)
- mBlock は Scratch 3.0 をもとにし、ブロックのコードを 1 クリックで Python に変え、並べて同期して見せる。Makeblock は「ブロック → MicroPython → Arduino C」の連続した段を掲げる — [Makeblock Education Computer Science](https://education.makeblock.com/computer-science/); [mblock.cc](https://mblock.cc) (いずれも販売元の主張)
- Weintrop & Wilensky (2017, ACM TOCE): 高校の 5 週の準実験で、ブロックの群は文の群より学習の伸びと得点が大きかった (両群とも事前・事後で伸びた) — [Weintrop & Wilensky 2017](https://ccl.northwestern.edu/2017/BlockingProgress.pdf)
- Weintrop & Wilensky (2019, Computers & Education 142, 103646): 初心者が 5 週ブロックか文の入門環境を使い、その後 Java に移って 10 週、計 15 週を追った。5 週目のブロック群の概念理解の差は Java の 10 週後に消え、Java でのプログラミングの仕方にも群の差は無く、態度の差も 15 週後には消えた。著者は「ブロックで得た概念の伸びは文の言語へ自動的には移らない。移行を助ける必要がある」と結論する。教師・カリキュラム・時間をそろえた準実験は初めてと述べる — [Weintrop & Wilensky 2019 (PDF)](https://www.terpconnect.umd.edu/~weintrop/papers/Weintrop&Wilensky_2019_C&E.pdf)
- 小学生の放課後で micro:bit のブロックの群と Python の群を比べた研究 (Veng & Mouza, SITE 2024) では、フィジカルコンピューティングの群の方が態度が肯定的だった — [NSF PAR](https://par.nsf.gov/servlets/purl/10536459) (抜粋のみ確認)

### Inferences
- 「ブロックと文を並べて見せる」は今の主流だが、それだけで移行が保証される証拠は見当たらない。Weintrop らの結果は「伸びは消えない (悪くもならない) が、自動では残らない」と読むのが正確。
- 回路の学習ソフトとしては、ブロック → 同じ意味の文、の対応を行ごとに光らせる・文で直すとブロックに戻る、のような「行き来」を残し、後半の課題で文だけを書く段を用意するのが研究の示唆に沿う。

### Gaps
- 電子工作 (マイコン) に限ったブロック → 文の移行の効果を測った研究は見つからなかった。Weintrop らは画面上のプログラミング (Pencil Code など) の研究。
- Tinkercad・mBlock の併記表示の効果を測った研究は見つからなかった。

## 3. 回路 (電気の理解) とプログラムの組み合わせ方、学ぶ人がつまずく所

### Takeaway
Starter Kit は毎回「ブレッドボードの絵 + 回路図 + コード」をそろえ、電気の背景説明は最小限にして作りながら学ばせる。つまずきとして資料が明示するのは、配線の間違い・部品の間違いや抜け・ブレッドボードへの差し込み不良 (Booth ら)、micro:bit のシールドの向きと接続するピンの取り違え、しきい値の決め方。浮いた入力やプルアップを特定のつまずきとして挙げた研究は見つからなかった。モータの電源と共通 GND はStarter Kit が 9 番で、サーボの電流は 5 番でコンデンサとともに扱う。

### Cited Findings
- Starter Kit の本は電子工作とプログラミングの背景説明を最小限にし、プロジェクトが前のものの上に積み上がる。各プロジェクトにブレッドボードの絵と回路図の両方がある — [Arduino Projects Book](https://huggingface.co/datasets/Vikhrmodels/programming_books_eng/blob/main/Arduino/Arduino_Projects_Book.pdf.txt)
- モータはピンの電流では足りない → トランジスタ・別電源 (9V)・逆向きのダイオード・GND だけを共通に (9 番)。サーボは動き始めに電流を多く引く → 100µF (5 番) — 同上
- Booth ら (CHI 2016): 温度センサを Arduino につなぎ値を読むプログラムを書く課題で起きた問題を分類。よくある誤りとして配線違い・部品違い・部品の抜け・差し込み不良が挙げられ、電子工作の経験があってもハードの誤りは避けがたい、とまとめられている — [Booth et al. CHI 2016](https://openaccess.city.ac.uk/id/eprint/14844/12/booth-chi2016-cr.pdf) (本文はボット防御で取得できず、特許文書の二次的な要約による: [USPTO](https://image-ppubs.uspto.gov/dirsearch-public/print/downloadPdf/11301605))
- 初めて Arduino を使う 31 人がブロックのプログラミング環境と、普通のブレッドボード・モジュール式の BitBlox で作業した質的研究がある — [Trials and tribulations of novices working with the Arduino](https://redi.cedia.edu.ec/document/483223)
- フィジカルコンピューティングでのデバッグを詳しく調べた研究は少ない (K-12 の e-textiles の研究の指摘) — [NSF PAR](https://par.nsf.gov/servlets/purl/10309411)
- 広島大学附属の指導案: micro:bit を I/O シールドにつなぐときは向きがあるので注意、センサを接続するピンを間違えない — [広島大学](https://home.hiroshima-u.ac.jp/cyata/dc/guide3.pdf) (検索結果の要約から)
- 文部科学省の教材: 全体の構想とプログラムの流れを生徒自身が整理しないと、その後のプログラミングとデバッグがスムーズに行かない。フローチャートやアクティビティ図で思考を見えるようにする。センサの値はつないだ端子 (例 A0) に合わせて変える必要がある — [文部科学省 研修用教材](https://www.mext.go.jp/content/20210419-mxt_jogai01-000006333_001.pdf)
- MESH の小学校の実践の課題として、教科と結び付けた授業の難しさ、機器のトラブルに教師 1 人で対応しにくいこと。教員が電気電子部品の知識に不慣れな点も研究で指摘 — [文部科学省 未来の学び](https://www.mext.go.jp/miraino_manabi/content/135.html); [宮城教育大学関連の資料 (検索結果)](https://researchmap.jp/NAKAYAMA_Shinya/misc/41685529/attachment_file.pdf)

### Inferences
- 教材側は「回路図とブレッドボードの絵をそろえて見せる」「電流の大きい負荷は後に」「センサ値はまずシリアルや画面に出して見る (03 番・灌水の例の 5 秒ごとの表示)」で回路とプログラムを結ぶ。
- 浮いた入力・ピンの電流制限は、教材では「プルダウン抵抗を最初から図に入れる」「LED には必ず 220Ω」で起こさせない設計になっており、つまずきとして観察・測定されていない (=研究の空白)。circuitCad の ERC・シミュレーションで「抵抗を外したら/プルダウンを外したら何が起きるか」を見せられれば、既存教材が扱わない所を埋められる。
- Booth らの誤りの種類 (配線違い・差し込み不良) は回路図とブレッドボードの照合で見つけられる種類で、circuitCad の照合の機能が直接対応する。

### Gaps
- プルアップ/プルダウン・浮いた入力・ピンの電流制限・共通 GND の失敗の頻度を測った研究は見つからなかった。
- Booth らの論文本文 (誤りの割合など数字) は取得できなかった。

## 4. 評価と「チャレンジ」の組み立て

### Takeaway
多くは「ガイド付きで作る → 自分で変える/組み合わせる最後の課題」の段で、点数の試験より作ったものの動作と振り返りで見る。日本の技術科は「課題の設定 → 構想 (フローチャート/アクティビティ図) → 制作・デバッグ → 評価・改善」の問題解決の過程そのものを観点別 (知識・技能 / 思考・判断・表現 / 主体的に学習に取り組む態度) で評価する。

### Cited Findings
- CTC GO! は 8 レッスン → 8 ガイド付きプロジェクト → 6 自分で進めるプロジェクト。自分で進める分はレッスンとガイド付きを終えてから — [Arduino Store](https://store.arduino.cc/ctc-go-core-module)
- micro:bit の単元は最後の「じゃんけん」がそれまでの技能を合わせる課題で、ゲームの公平さを評価して終わる。常夜灯は自分の部屋の明るさで試して直す (反復的なテスト)。単元のページには正式な評価方法の記述は無い — [microbit.org](https://www.microbit.org/teach/lessons/first-lessons-with-makecode-and-the-microbit)
- Starter Kit の本は各プロジェクトを積み上げ式にし、作ったあとに変えてみる余地を残す (本文の構成) — [Arduino Projects Book](https://huggingface.co/datasets/Vikhrmodels/programming_books_eng/blob/main/Arduino/Arduino_Projects_Book.pdf.txt)
- 技術科 D(3) は PDCA の課題解決のサイクルで生徒が主体的に取り組むことが重要とし、「課題の設定」から「成果の評価」までの各過程の要点を押さえて指導計画を立てる。研修教材は学習評価の章を「知識・技能」「主体的に学習に取り組む態度」の観点ごとに持つ — [文部科学省 研修用教材](https://www.mext.go.jp/content/20210419-mxt_jogai01-000006333_001.pdf)
- 熊本県の学習構想案は「計測・制御システムの仕組みを理解し、安全・適切なプログラムの制作、動作の確認及びデバッグ等ができる」を目標に置く — [熊本県](https://www.pref.kumamoto.jp/uploaded/attachment/125752.pdf) (検索結果の要約から)
- 和歌山県の資料は 3 年で計測・制御のプログラミングによる問題解決を扱い、3 年間を積み上げる — [和歌山県](https://www.pref.wakayama.lg.jp/prefg/501500/d00213120_d/fil/kinokuniICT_jhs2104a.pdf) (検索結果の要約から)

### Inferences
- 「手順どおりに作る回 → 1 つ変える → 自分の課題」の 3 段は CTC GO!・micro:bit・技術科で共通。circuitCad のレッスン → チャレンジ (答えが通るテスト付き) の構成はこれに対応する。
- 技術科の評価は「動いたか」より「構想を図で示し、デバッグし、改善したか」を見るので、フローチャートや「センサの値 → しきい値 → 出力」を図で示す仕掛けが学校で使われるときの鍵になりうる。

### Gaps
- Tinkercad・mBlock の課題の自動採点や評価の仕組みの具体は確認できなかった。
- micro:bit の 11〜14 歳向け単元 (Oak National Academy の Year 7「Physical computing using the micro:bit」6 レッスンなど) の中身は未確認 — [Oak National Academy](https://www.thenational.academy/teachers/curriculum/computing-secondary-ocr/units/physical-computing-using-the-micro-bit)
