# 大学の最初の回路の授業 (1〜2 年) の単元の順と実験の組み立て

調べた日: 2026-11-28。対象: MIT 6.002 (OCW 2007)・MIT 6.200 (今の後継)・Berkeley EECS 16A/16B・Stanford ENGR 40M・Georgia Tech「Linear Circuits 1」(Coursera)・日本の大学の「電気回路」。
注意: MIT 6.200 と Berkeley の過去の学期のページはログインが要り、週ごとの中身は読めなかった。日本のシラバスは PDF が 404 で、検索結果の抜き書きにとどまる (下の Gaps)。

## 1. 単元の順 (DC・KVL/KCL・節点法・テブナン・非線形素子・従属電源/MOSFET のスイッチ・デジタルの抽象化・オペアンプ・C/L・1 次と 2 次の過渡・正弦波定常・フィルタ・電力)

### Takeaway
骨格はどこも「電圧・電流・オームの法則 → KVL/KCL → 体系的な解き方 (節点法・重ね合わせ・テブナン) → C と L → 1 次の過渡 → 2 次 (RLC) → 正弦波定常/インピーダンス → フィルタ (と周波数特性)」。違いは非線形素子とデジタルの置き場所で、MIT 6.002 と Stanford ENGR 40M は抵抗回路のすぐ後 (C/L より前) に「デジタルの抽象化・MOSFET のスイッチ」を入れ、Georgia Tech と日本の電気回路は線形回路だけで進み、MOSFET やデジタルは別の科目に回す。

### Cited Findings
**MIT 6.002 (Spring 2007, Agarwal、OCW) の 25 回の講義の順**
- L1 集中定数の抽象化・KVL・KCL → L2 節点解析 → L3 線形性・重ね合わせ・テブナン → **L4 デジタルの抽象化** → L5 MOS スイッチ (S・SR モデル)・MOS のゲート → L6–7 非線形抵抗・小信号解析 → L8 従属電源・増幅 → L9 MOS の SCS モデルと増幅器 → (小テスト 1) → L10–12 増幅器の大信号・小信号・小信号モデル → L13–14 コンデンサ・1 次回路・直観的な解析 → **L15 デジタルの記憶・状態** → L16–17 2 次系の過渡・減衰 → L18 正弦波定常・周波数特性 → (小テスト 2) → L19 インピーダンス法 → L20 フィルタ・Q・ラジオの同調 → L21–23 オペアンプの抽象化・帰還・反転/非反転・積分/微分・安定性・発振 → L24 エネルギーと電力 → L25 抽象化の壁を破る — [MIT OCW 6.002 Calendar](https://ocw.mit.edu/courses/6-002-circuits-and-electronics-spring-2007/pages/calendar/)
- 到達目標に「集中定数モデル、節点法・重ね合わせ・テブナン、ブール代数とゲートの設計 (静的規律・雑音余裕)、MOSFET のスイッチ-抵抗モデルと小信号増幅、1 次と 2 次の時間応答、複素インピーダンスの周波数応答、負帰還のオペアンプ、CMOS の消費電力」。教科書は Agarwal & Lang『Foundations of Analog and Digital Electronic Circuits』(Morgan Kaufmann, 2005) — [MIT OCW 6.002 Syllabus](https://ocw.mit.edu/courses/6-002-circuits-and-electronics-spring-2007/pages/syllabus/)
- オペアンプは 6.002 では終わり近く (L21–23)、エネルギーと電力も最後 (L24) — [MIT OCW 6.002 Calendar](https://ocw.mit.edu/courses/6-002-circuits-and-electronics-spring-2007/pages/calendar/)

**MIT 6.200 (今の後継、Fall 2025 / Spring 2026)**
- 推奨書は同じ Agarwal & Lang (MIT の図書館で無料) と Bose & Stevens『Introductory Network Theory』。必須の教科書は無い — [6.200 F25 Basic Course Information](https://circuits.mit.edu/F25/info/basics)
- 週ごとの講義の題目はログインの中で読めなかった。検索結果の抜き書きでは Fall 2025 の実験は週 1 ゲームパッド → 週 3 D/A 変換器 → 週 4 MOSFET のオーディオアンプ → 週 5 静電容量のタッチセンサー — [circuits.mit.edu F25 (検索結果の要約)](https://circuits.mit.edu/F25)。一次資料で確かめていない。

**Stanford ENGR 40M (Horowitz・Bell、ASEE 2018 の論文の「典型的な日程」)**
- 週 1: 電圧と電流・I-V 曲線・抵抗とオームの法則・回路を「力ずくで」解く・電力の行き先・理想ダイオード → 週 2: 実際のダイオード・太陽電池・近道 (直列/並列・分圧/分流・テブナン/ノートン)・重ね合わせ → 週 3: ステートマシンとデジタル論理・スイッチで論理・ブレッドボード・MOS トランジスタ・CMOS 論理 → 週 4: Arduino のプログラム・論理で計算 → 週 5: 中間試験・光とダイオード・ピンより多い LED を点ける (多重化) → 週 6: C と L・定常・エネルギー・RC/RL → 週 7: 周波数領域・フーリエ級数・C/L の周波数特性・受動フィルタ・ボード線図 → 週 8: 心電図・理想オペアンプ・増幅器・オペアンプのフィルタ → 週 9: 絶縁・デジタルフィルタ・降圧/昇圧コンバータ → 週 10: 復習 — [Bell & Horowitz, "Rethinking Non-major Circuits Pedagogy for Improved Motivation", ASEE 2018](https://peer.asee.org/30936.pdf)
- 論文自身が「基本の線形回路 → トランジスタとデジタル論理 → C・L・オペアンプ・フィルタとアナログに戻る、という少し変わった道」と書く。改編前の ENGR 40 は「KVL/KCL・オーム → 抵抗回路の解き方 → C と L → オペアンプ → 正弦波 → 受動・能動フィルタ」の伝統的な順だった — [Bell & Horowitz 2018](https://peer.asee.org/30936.pdf)
- 設計の原則: 「線形回路ではなく入門エレクトロニクスを教える」(LED・ダイオード・トランジスタを早く)、「デジタルとアナログを混ぜる」(4 つの作品のうち 3 つにマイコン)、「実際の応用に焦点」、「実験は概念の繰り返しでなく技能を教える場」 — [Bell & Horowitz 2018](https://peer.asee.org/30936.pdf)

**Berkeley EECS 16A/16B**
- 16A (過去の学期、2015–2023) は Imaging・Touchscreen・Acoustic Positioning (APS) の 3 つのモジュールで、回路は Touchscreen のモジュール。Fall 2017 は「Circuits Introduction: KVL/KCL, Touchscreens」の講義から抵抗膜式、その後に静電容量式 (電荷の分配・コンパレータ) — [EE16A 各学期のページ (検索結果の要約)](https://inst.eecs.berkeley.edu/~ee16a/fa17)。過去の学期のページは今はログインが要り、本文は確かめていない。
- **Spring 2026 の 16A (eecs16a.org) は中身が変わっていて、回路の単元が見当たらない**: ベクトルと信号・内積 → 複素指数・オイラーの式 → DTFS → 最小二乗 → PageRank → 固有値 → SVD/PCA → 離散時間 LTI。実験は Python Bootcamp・Shazam・APS・VR1/VR2 — [EECS16A (eecs16a.org)](https://eecs16a.org/)
- 16B (eecs16b.org に出ていたのは Spring 2024): 週 1 コンデンサ・RC・1 階の微分方程式 → 週 2 インダクタ・RL・CMOS トランジスタの RC モデル・DAC/ADC → 週 3 トランジスタ II・RLC と 2 階の微分方程式 → 週 4 インピーダンス・周波数領域・複素指数・伝達関数 → 週 5 ボード線図・極と零点 → 週 6 以降 状態空間・自由応答/強制応答・離散時間・システム同定・安定性と帰還制御・可制御性 → SVD・PCA・線形化 — [EECS16B (eecs16b.org)](https://eecs16b.org/)

**Georgia Tech「Linear Circuits 1: DC Analysis」(Coursera、Ferri ほか)**
- 7 モジュール: 1 電流・電圧・電力とエネルギー・オームの法則・回路図 → 2 KVL/KCL・直並列・分圧/分流・**DC のオペアンプ**・従属電源 → 3 網目法・節点法 (スーパーノード含む) → 4 テブナン/ノートン・電源変換・線形性と重ね合わせ・センサーの抵抗・ホイートストンブリッジ → 5 C と L・エネルギー → 6 過渡 (スイッチ・1 次の RC/RL) → 7 2 次回路 (RLC の自由応答、微分方程式は任意) — [Coursera: Linear Circuits 1](https://www.coursera.org/learn/linear-circuits-dcanalysis)
- 検索結果では Class Central の 4 モジュールの説明と食い違うが、Coursera の今のページは 7 モジュール — [Class Central](https://classcentral.com/course/linear-circuits-dcanalysis-8233)

**日本の大学の「電気回路」(検索結果の抜き書き)**
- 京都工芸繊維大学の電気回路: 正弦波交流をフェーザ (複素数) で表し、インピーダンス・アドミタンス、交流の直並列回路、交流電力、共振回路、変成器、そして過渡現象の解析法 — [京都工芸繊維大学 シラバス 2026 (検索結果)](https://www.kit.ac.jp/wp/wp-content/uploads/sy2026/12161007.pdf)
- 同大学の回路解析: 重ね合わせ・等価電源の定理・有効電力、ラプラス変換による過渡現象、三相交流、フーリエ級数 — [京都工芸繊維大学 シラバス 2026 (検索結果)](https://www.kit.ac.jp/wp/wp-content/uploads/sy2026/12160022.pdf)
- 長崎大学の講義ノート (辻峰男): キルヒホッフから始まり、フェーザの交流計算 (6〜8 章)、回路網方程式、回路の諸定理 — [長崎大学 電気回路講義ノート (検索結果)](https://nagasaki-u.repo.nii.ac.jp/record/5525/files/Electric%20Circuit%20Lecture%20Note.pdf)
- 新潟大学 (菊池久和) の回路学: キルヒホッフ・重ね合わせ・テブナン・ミルマン・フェーザ — [新潟大学 回路学の資料 (検索結果)](https://www.eng.niigata-u.ac.jp/~msiplab/files/ProfKikuchi/CircuitTheoryPdf.pdf)

### Inferences
- 共通の背骨は「KVL/KCL → 節点法・重ね合わせ・テブナン → C/L → 1 次 → 2 次 → 正弦波定常・インピーダンス → フィルタ」。この部分の順はどの授業でもほぼ同じ。
- 分かれ目は 3 つ: (a) デジタル・MOSFET を C/L より前に置くか (MIT 6.002・Stanford は前、Georgia Tech・日本は入れない)、(b) オペアンプの時期 (Georgia Tech は DC の段で早く、MIT・Stanford は終盤)、(c) 交流をどこで始めるか (日本の電気回路は直流の後すぐフェーザ・交流電力・共振に重点。米国の入門は過渡の後に正弦波定常)。
- 日本の「電気回路 I・II」は電力系の伝統 (交流電力・三相・変成器・ラプラス) が強く、MOSFET・デジタル・オペアンプは「電子回路」など別の科目に回る傾向 (抜き書きから見た推測)。
- Berkeley は 2026 年に 16A から回路を外した可能性がある (今の 16A のページに回路が無い)。16B は C から始まり、回路は前半 5 週ほどで、後半は制御・線形代数。

### Gaps
- MIT 6.200 の週ごとの講義の題目 (ログインが要る)。実験の 4 件は検索結果の要約だけ。
- Berkeley 16A の回路の単元の週ごとの順 (過去の学期のページはログインが要る)。今の 16A から回路がどこへ移ったか (16B か別の科目か) は確かめていない。
- 日本の国立大学の「電気回路 I・II」の 15 回ごとの授業計画は、PDF が 404 で読めず、検索の抜き書きだけ。I と II の分け目 (直流+交流の基礎 / 過渡・ラプラス・三相など) は一次資料で確かめていない。
- MIT 6.01・6.101 (OCW 2007 の入門アナログ実験) は時間内に読めなかった。

## 2. 実験の中身・道具・時期

### Takeaway
MIT 6.002 は 5 回 (Lab 0〜4) で、最後はアナログとデジタルの音声再生装置の設計。Stanford ENGR 40M は「作品」中心 (はんだ付けのラジオ → 太陽電池の USB 充電器 → 役に立たない箱 (スイッチ → Arduino と H ブリッジ) → LED キューブ → 心電図) で、2 週目から実物を作る。Berkeley は実物の装置 (タッチスクリーン、16B はロボットカー) と DataHub の notebook を組み合わせる。オンラインの Georgia Tech は実験の動画だけ。

### Cited Findings
- MIT 6.002: Lab 0 は L1 で出て L3 の前日まで、Lab 1 は L4 の後〜L8、Lab 2 は L11〜R15、Lab 3 は L15〜R20、Lab 4 は L18〜R23 — [MIT OCW 6.002 Calendar](https://ocw.mit.edu/courses/6-002-circuits-and-electronics-spring-2007/pages/calendar/)
- MIT 6.002: 道具はオシロスコープ・マルチメータ・信号発生器。TA が実験室で「できたこと」を確認し、予習・復習を実験ノートに。最終目標はアナログとデジタルの部品で音声再生装置を設計・製作・試験。宿題 11 は 2 週間の必須の設計課題で Lab 4 の予習を兼ねる。実験を済ませないと成績が 1 段下がる (配点は実験 5 %) — [MIT OCW 6.002 Syllabus](https://ocw.mit.edu/courses/6-002-circuits-and-electronics-spring-2007/pages/syllabus/)
- MIT 6.200: 毎週 1 回の実験で、その週の内容を実物の部品で作り、分析から設計へ移る。実験の始めに 15 分のオンラインの小テスト (nanoquiz)。宿題に予習 (pre-lab) と、週によってハードウェアの課題 — [6.200 F25 Basic Course Information](https://circuits.mit.edu/F25/info/basics)
- Stanford ENGR 40M (2018 の論文): 週 1 はんだ付けの導入 (AM/FM ラジオのキット。講義の知識なしでできる) → 週 2 太陽電池の携帯充電器 (DC-DC を黒箱として負荷ごとの電力と効率を測る。目的はマルチメータに慣れること) → 週 3 役に立たない箱 (モーター・DPDT・リミットスイッチ) → 週 4 Arduino 互換の Metro Mini とパワー MOSFET の H ブリッジで制御、PWM で「性格」 → 週 5–7 LED キューブ (4×4×4、20 本未満の I/O で 64 個を時分割の多重化、はんだ付け→配線とプログラム→自由課題) → 週 8–9 心電図 (計装アンプ+オペアンプのフィルタ、ファンクションジェネレータとオシロの使い方、正弦波で試してから Arduino の AD で取り、ノート PC で表示。安全のため 100 kΩ 超の抵抗・電池駆動のノート PC・絶縁チップの 3 重の絶縁) — [Bell & Horowitz 2018](https://peer.asee.org/30936.pdf)
- 毎回の実験は「予習 (回路を解析・設計、実験の 24 時間前に提出) → 実験 3 時間 → 復習 (分析の問いと 3 つの振り返りの問い)」。採点は完成度と「作りの質」(部品の整った並び・色分けした線・はんだ・直しやすさ) をルーブリックで — [Bell & Horowitz 2018](https://peer.asee.org/30936.pdf)
- Stanford ENGR 40M の 2025 秋 (Summer 2026 の版のシラバス、Horowitz): Lab 0 (週 2) → Lab 1 (週 3) → Lab 2A (週 4) → 中間試験 (週 5) → Lab 2B → 3A → 3B → 4A → 4B (週 10)。部品のキット (約 100 ドル) を一人ずつ配る。反転授業 (短い動画か読本 → Canvas の確認問題、授業中は問題演習)。資料に Arduino のガイド・はんだ付けの動画・EveryCircuit (回路シミュレータ) — [ENGR 40M Syllabus Summer 2026](https://edit-summer.stanford.edu/sites/summer/files/media/file/engr_40m_syllabus_summer_2026.pdf)
- Stanford の 2017 夏の実験の一覧: 週 2 太陽電池の USB 充電器、週 3–4 役に立たない箱 (基本・賢い)、週 5–7 LED キューブ、週 8 心電図 (LM4250・INA126、Arduino と Processing で波形) — [ENGR 40M Labs (Summer 2017)](https://web.stanford.edu/class/archive/engr/engr40m.1178/labs.html)。2025 年の学生のブログの要約では LED は手で組むキューブでなく 8×8 の専用の基板に 64 個をはんだ付けし Arduino Mini で駆動 (検索結果の要約で一次資料は未確認)。
- Berkeley 16B (Spring 2024): Lab 1 S1XT33N の導入 → Lab 2 アナログとデジタルのインターフェース → Lab 3 動きと速さの検出 → Lab 4–5 音声の検出 → Lab 6 システム同定 → Lab 7 制御 → Lab 8 分類 → Lab 9 統合・最終デモ。オシロのチートシート、DataHub の notebook — [EECS16B](https://eecs16b.org/)
- Berkeley 16A (過去の学期): Touchscreen 1「ハードウェアの基礎」から抵抗膜式のタッチスクリーン、のちに静電容量式 — [EE16A (検索結果の要約)](https://inst.eecs.berkeley.edu/~ee16a/fa17)、ラボの資料 [touch1.pdf (Fall 2022)](https://inst.eecs.berkeley.edu/~eecs16a/fa22/lab/touch1.pdf)
- Georgia Tech (Coursera): 実験は動画のデモ (部品・基本回路・RC・RLC)。部品のキットは求めない — [Coursera: Linear Circuits 1](https://www.coursera.org/learn/linear-circuits-dcanalysis)

### Inferences
- 道具の導入の順は「マルチメータ (DC) → ブレッドボード → マイコン → ファンクションジェネレータとオシロ (交流・過渡)」で、回路の単元の順と一致する (Stanford で特にはっきり)。
- 実験の時期は大きく 2 通り: MIT 6.002 は講義 4〜5 回ごとに 1 件の重い実験、MIT 6.200 と Stanford は毎週の実験 (6.200 は毎回の小テストで予習を確かめる)。
- 回路シミュレータ (EveryCircuit) を予習で使い、実物で確かめる流れが Stanford にある。

### Gaps
- MIT 6.002 の各 Lab の中身 (題目) は calendar に無く、PDF は読んでいない。
- MIT 6.200 の実験の全一覧 (ログインが要る)。
- 日本の大学の「電気回路」に付く学生実験 (別の科目の「電気電子工学実験」が普通) の中身と時期は調べられなかった。

## 3. 学生がつまずく所と、その手当て

### Takeaway
一次資料で確かめられたのは Stanford の報告: 符号の約束 (受動符号規約・電圧電流の向き)、「開いた所にも電圧がある」、「MOSFET はゲートから電流が流れる出力ではなく電圧で操るスイッチ」の 3 つ。手当ては、作品を中心にして毎週の講義を翌週の実験に要る分にする・作りの質の採点・振り返りの問い。MIT は「抽象化」(集中定数・デジタル・オペアンプ) を軸にし、測った結果とモデルの食い違いを説明させる。

### Cited Findings
- ENGR 40M で繰り返し出たつまずき: (1) 受動符号規約と電圧・電流の向き。符号の約束の目的が分からず、簡単な問題以外で符号を間違える。(2) 開放の所に電圧があり得ることが分からず、オームの法則を何にでも当てはめる (「電流が無いから電圧は 0」)。(3) MOSFET を入力で決まる「出力」と考え、ゲートからドレイン・ソースへ電流が流れると思う。宿題と授業の演習で手当てしているが、もっと研究が要ると著者 — [Bell & Horowitz 2018](https://peer.asee.org/30936.pdf)
- 実験で苦労する学生の多くは、なぜ苦労しているか自分で分からず、やり方を変えない (メタ認知)。復習の振り返りの問いと、実験の途中で解いた問題を共有する話し合いを試した — [Bell & Horowitz 2018](https://peer.asee.org/30936.pdf)
- 改編前の ENGR 40 は「濃くて退屈」と見られていた。作品中心に変えてから、1〜2 年生が多く取るようになり (未決定の学生が 30 % → 46 %)、評価は実験に好意的 (ただし時間がかかる)、EE の学部生はほぼ倍に (ほかの要因もあると著者) — [Bell & Horowitz 2018](https://peer.asee.org/30936.pdf)
- 作品中心の代償: 2 週目に充電器を作るため最初の 4 回で電圧・電流・KVL/KCL・オーム・電力・ダイオード・太陽電池・マルチメータの使い方を全部入れる必要がある。太陽電池・計装アンプのように入門では普通扱わない話も要る — [Bell & Horowitz 2018](https://peer.asee.org/30936.pdf)
- MIT 6.002 の考え方: 集中定数のモデル・デジタル (ブール) の抽象化・オペアンプのモデルという単純なモデルを積み、数式と測った振る舞いをつなげ、食い違いを説明させる。講義・演習・宿題・実験すべてが要るとする — [MIT OCW 6.002 Syllabus](https://ocw.mit.edu/courses/6-002-circuits-and-electronics-spring-2007/pages/syllabus/)
- MIT 6.200: 講義は前の週の宿題に基づく例題、演習はハードウェアのデモも。宿題はすぐ答えが返る「ドリル」・自動採点の演習・記述・予習の 4 種類 — [6.200 F25 Basic Course Information](https://circuits.mit.edu/F25/info/basics)

### Inferences
- 「デジタルを先に」(6.002 の L4) の利点は、MOSFET をスイッチとして先に見せ、増幅・小信号はその後にすること。Stanford もスイッチとしての MOSFET を Arduino と H ブリッジの前に置くが、それでも「ゲートから電流が流れる」誤解が残る。
- 符号の約束と開放端の電圧は最初の 2〜3 週の話で、ここでのつまずきがその後すべてに響く。回路の上に電圧・電流の向きと値を出して見せる道具が効きそう (推測)。

### Gaps
- MIT 6.002/6.200 自身が「どの単元が一番難しいか」を書いた一次資料は見つけられなかった。
- Berkeley 16A/16B・Georgia Tech・日本の授業のつまずきの報告は調べられなかった (授業評価や教育の論文は未調査)。

## 4. 数学 (複素数・微分方程式) の量と、どこで入れるか

### Takeaway
MIT 6.002 は微分方程式 (18.03) を前提にする。今の 6.200 は 8.02 だけが正式な前提で、微積分の基本は求めるが微分方程式は前提にしない。Berkeley 16B は 1 週目に 1 階の微分方程式、3 週目に 2 階、4 週目に複素指数とインピーダンスを授業の中で教える (補助の notes あり)。Stanford ENGR 40M は文系も含む非専門の学生向けで、微分方程式を避けて直観的に RC/RL を扱う。日本の電気回路は最初から複素数 (フェーザ) が中心で、後半でラプラス変換。

### Cited Findings
- MIT 6.002 の前提: 8.02 (電磁気) と 18.03 (微分方程式)。エネルギーを蓄える回路の時間の振る舞いに微分方程式を使う — [MIT OCW 6.002 Syllabus](https://ocw.mit.edu/courses/6-002-circuits-and-electronics-spring-2007/pages/syllabus/)
- MIT 6.200 の前提: 正式には 8.02 だけ。一次関数と方程式・対数・SI 単位と接頭語・微積分 (微分・積分・微積分の基本定理) に慣れていることを勧める — [6.200 F25 Basic Course Information](https://circuits.mit.edu/F25/info/basics)
- MIT 6.002 では複素インピーダンスは L18–19 (正弦波定常とインピーダンス法) で、2 次の過渡 (L16–17) の後 — [MIT OCW 6.002 Calendar](https://ocw.mit.edu/courses/6-002-circuits-and-electronics-spring-2007/pages/calendar/)
- Berkeley 16B: 週 1 で 1 階の微分方程式 (RC)、週 3 で 2 階 (RLC)、週 4 で複素指数・インピーダンス・伝達関数、週 5 でボード線図・極と零点。複素数の補助ノート (Note j) と参考書『Differential Equations for Engineers』 — [EECS16B](https://eecs16b.org/)
- 今の Berkeley 16A は 2 週目で複素指数・オイラーの式を入れる (信号の文脈) — [EECS16A](https://eecs16a.org/)
- Stanford ENGR 40M: 周波数領域とフーリエ級数・C と L の周波数特性・ボード線図は週 7。RC/RL は週 6 — [Bell & Horowitz 2018](https://peer.asee.org/30936.pdf)
- Georgia Tech: 2 次回路のモジュールで微分方程式の導き方は「任意」の扱い — [Coursera: Linear Circuits 1](https://www.coursera.org/learn/linear-circuits-dcanalysis)
- 日本: 正弦波交流を複素数と対応させてフェーザと呼ぶ回から始め、インピーダンス・アドミタンスで交流回路を扱う (京都工芸繊維大学)。回路解析の科目でラプラス変換で過渡を解く — [京都工芸繊維大学 シラバス (検索結果)](https://www.kit.ac.jp/wp/wp-content/uploads/sy2026/12161007.pdf)、[同 回路解析 (検索結果)](https://www.kit.ac.jp/wp/wp-content/uploads/sy2026/12160022.pdf)

### Inferences
- 米国の新しい授業 (6.200・16B) は、微分方程式を前提にせず授業の中で必要な分だけ教える方向。非専門向け (ENGR 40M・Coursera) は微分方程式を任意か直観に留める。
- 複素数の入れ方は 2 通り: 米国は「過渡 (微分方程式) → 正弦波定常で複素インピーダンス」、日本の電気回路は「直流の定理 → すぐフェーザ」で、過渡とラプラスは後。初学者向けの順としては、過渡を数値シミュレーションで見せてから複素数に入る米国の順の方が、シミュレータのあるソフトと相性が良さそう (推測)。

### Gaps
- 日本の大学で数学 (複素数・線形代数・微分方程式) をどの学期に並行して教えるかの一次資料は見つけていない。
- MIT 6.200 で微分方程式・複素数を講義のどの週で入れるかはログインの中で読めなかった。
