# 電気回路の誤概念・学習の困難と、効果が示された教え方 (物理・工学教育研究の調査ノート)

調査日: 2026-11-28。検索と一部のページの取得で集めた。多くは要旨・二次資料での確認で、全文を読めたものは少ない (各節の「Gaps」に書いた)。

## 1. 最も多く、強固な誤概念は何か (出典つき)

### Takeaway
1980 年代から繰り返し確かめられているのは、「電流は使われて減る (消費説)」「電池は一定の電流を出す (定電流源)」「変化は流れの向きに順に伝わる (逐次的な推論)」「変えた所だけを見る (局所的な推論)」「電圧と電流の混同」、そしてその根にある「回路の概念モデルが無い」こと。これらは指導の後も、高校生でも大学生でも、日本の小中学生でも残る。

### Cited Findings
- **定電流源としての電池**: Cohen, Eylon & Ganiel (1983, Am. J. Phys. 51(5), 407–412) が「constant current」(外の回路を変えても電池の出す電流は変わらないという考え) を定義した。後の総説が、電池を定電圧源でなく定電流源とみる傾向の出典として Cohen ら (1983) と Heller & Finley (1992) を挙げている — [Wainwright, Toward Learning and Understanding Electricity (総説)](https://fg.ed.pacificu.edu/wainwright/Publications/MisconceptionsArticle.06.pdf)
- **局所的な推論**: 回路を互いにつながった系として見ず、1 点で起きることだけを見る。回路を変えると変えた点に注目し、ほかの部分も変わることに気づかない (Cohen ら 1983、Heller & Finley による) — [Wainwright 総説](https://fg.ed.pacificu.edu/wainwright/Publications/MisconceptionsArticle.06.pdf)
- **電流の消費**: 電荷が「使われる」ので電流が部品を通るたびに減る、という直観。電池を「充電する」「空になる」という日常の言い方がこれを強める — [Wainwright 総説](https://fg.ed.pacificu.edu/wainwright/Publications/MisconceptionsArticle.06.pdf)
- **逐次的な推論・電流のモデルの違い・電圧と電流の混同**: Shipstone (1984, European Journal of Science Education 6(2), 185–198) は 11〜18 歳 (大学 1 年を含む) を年齢別に調べ、電流の流れについての異なるモデル、逐次的な推論、電圧と電流の混同を報告した — [PER Central の Shipstone 1984 の書誌](https://www.per-central.org/items/detail.cfm?ID=2889)
- **McDermott & Shaffer (1992)**: Am. J. Phys. 60(11) の 2 部作 (Part I pp. 994–1003、Part II pp. 1003–1013)。回路の理解の研究を教材作りの指針に使う。1993 年に Part I の正誤表 (61, 81) — [ComPADRE](https://www.compadre.org/portal/items/detail.cfm?ID=2722)
- 同じグループ (ワシントン大学) の 2013 年の資料が、根強い困難として「電池は定電流源」「電流は回路で使い切られる」と、根本の困難「電気回路の概念モデルが無い」を挙げ、標準的な計算問題が解けることは機能的な理解の十分な基準ではないとしている — [McDermott, AAPT New Faculty Workshop 資料](https://aapt.org/Conferences/newfaculty/upload/McDermott_1306_NFW.pdf)
- 指導後でも、微積分ベースの入門物理で直流回路を習った後、回路の豆電球の明るさを正しく順位づけできるのは約 15% だけで、この困難は教える人の熟練や従来型の回路の実験を受けたかに関係なく残った (ワシントン大学の Tutorials の研究からの記述) — [検索結果の論文 (arXiv physics/0608240)](https://arxiv.org/pdf/physics/0608240)
- **Engelhardt & Beichner (2004)** の DIRECT による調査: 面接で最も多く使われた考えは「電池は一定の電流の源」。問題を電流中心で解く、用語を混同する (電流の性質を電圧や抵抗に当てはめる)、指導後も複数の誤概念を同時に持つ (特に女子学生)。実際の配線の絵を回路図に直すことはやさしくできるが、仕組みの理解は乏しい。高校生と大学生で同じような困難 — [arXiv physics/0304040 (Am. J. Phys. 72, 98–115, 2004)](https://arxiv.org/abs/physics/0304040)
- **日本の小中学生**: 公立の小 3〜中 3 の 822 名の調査 (立命館産業社会論集、2019) で、小 5 までは科学的な概念が進むが、小 6 で素朴な「減衰モデル」(消費説に当たる) へ戻る傾向。中 3 の始めでも誤った概念の生徒が 47.7% — [立命館大学の論文 PDF](https://www.ritsumei.ac.jp/file.jsp?id=429529) (数値は検索結果の要約による。PDF 本文は取得したが文字を読めなかった)
- 日本の小学校で「電圧」を扱わないことが、児童も教師も電圧と電流を混同する一因と指摘 — [千葉大学の論文 (雑誌 第 73 巻)](https://opac.ll.chiba-u.jp/da/curator/900123226/S13482084-73-P447.pdf)
- 教員養成系の学生が「学生なりの電流・電圧ルール」を 4 種類持つ。回路の学習内容は素朴概念と科学的概念の隔たりが大きい — [科研費 17K01021 成果報告](https://kaken.nii.ac.jp/ja/file/KAKENHI-PROJECT-17K01021/17K01021seika.pdf)
- 小 5〜大学生まで学年を追って電気の概念の一貫性の変化を調べた研究がある (横浜国立大学「教育デザイン研究」2023・2024) — [第 2 報 (中学生〜大学生)](https://ynu.repo.nii.ac.jp/record/2000486/files/15-1-17.pdf)

### Inferences
- 誤概念の中心は「電流」を主役にした考え方 (電流が出て、減り、順に流れる) で、「電位差 (電圧) が全体で決まる」という見方が欠けている。ソフトで教えるなら、電圧 (電位) を目に見える形にし、1 か所を変えると回路全体の値が同時に変わる様子を見せるのが的を射ている。
- 計算問題が解けても理解しているとは限らないので、定性的な予想 (明るさの順位、スイッチを入れたら何が変わるか) を問う課題が要る。
- 日本では小学校で電圧を扱わないことが混同の原因として挙がっており、日本語の初学者向けには最初から電圧 (電位) を出す価値がある。

### Gaps
- 短絡 (ショート) と並列の合成抵抗 (「抵抗を足すと全体の抵抗が増える」と思う誤り) について、一次資料を直接確認できなかった。DIRECT や McDermott & Shaffer に含まれるはずだが、全文を読めていない。
- Cohen ら (1983)、Shipstone (1984)、McDermott & Shaffer (1992) は全文を読めず、要旨と二次資料での確認。人数・割合などの数値は書いていない。
- 立命館の論文の著者名を確認できなかった。

## 2. 効果が示された教え方は何か (POE、電流が見えるシミュレーション、たとえ、電位を高さ・圧力とするモデル、メーターを使う実験)

### Takeaway
最も強い証拠は、電位 (電圧) を「圧力」や「高さ」として最初から主役にするカリキュラム (ドイツの電子ガスモデル、790 名の比較で有意に改善) と、研究にもとづく探究型の教材 (McDermott らの Tutorials / Physics by Inquiry、CASTLE)。POE は小規模な研究で効果ありの報告が多いが、規模が小さい。たとえ (水・人の群れ) は、1 つのたとえだけでは別の種類の問題で誤って使われるという限界が示されている。

### Cited Findings
- **電子ガスモデル (電位を「電気の圧力」とする)**: 電位を導体中の一種の電気的な圧力とし、電圧を圧力差として導入する。空気の圧力の直観と結びつける。フランクフルトの中等学校の生徒 790 名の比較研究で、従来の授業より回路の理解が有意に改善した — [Goethe 大学フランクフルトのプレスリリース](https://www.uni-frankfurt.de/95798554/Newly_developed_curriculum_improves_students__understanding_of_electric_circuits_in_schools)、[Tübingen 大学のニュース](https://uni-tuebingen.de/en/university/news-and-publications/newsfullview-news/article/newly-developed-curriculum-improves-students-understanding-of-electric-circuits-in-schools/)
- 理論の全体は Burde & Wilhelm, "Teaching electric circuits with a focus on potential differences", Physical Review Physics Education Research (2020)。博士論文は Burde (2018, Logos-Verlag)。14 名の教師との設計研究では、学習効果は高いと評価されたが、直列回路の説明 (過渡状態を使う部分) に改善の余地 — [ComPADRE](https://www.compadre.org/portal/items/detail.cfm?ID=14571)、[PhyDid B の論文](https://ojs.dpg-physik.de/index.php/phydid-b/article/download/760/909)
- **CASTLE** (Capacitor-Aided System for Teaching and Learning Electricity、Steinberg & Wainwright 1993, The Physics Teacher): 電荷を圧縮できる流体、電位を「圧力」とみなし、大きなコンデンサの充放電で豆電球の光り方を見せる。Predict/Observe/Explain の形で定性的な推論を重視。現場の試行で生徒の自信が上がり、女子で効果が大きかったと報告 — [Modeling Instruction の CASTLE のページ](https://www.modelinginstruction.org/?p=7987)、[ComPADRE](https://www.compadre.org/portal/items/detail.cfm?ID=2918) (評価の人数・効果量は確認できず)
- **POE (予想 → 観察 → 説明)**: タイの高校 2 年 (grade 11) で直流回路を POE で教え、事後テストが事前より 1% 水準で有意に高く、正規化ゲイン <g> = 0.35 (中程度) — [Juntana & Wuttiprom (検索結果)](https://ejournals.swu.ac.th/index.php/JSTEL/article/download/5340/5020/17658)。マルタの小規模研究 (並列回路の電位差) でも POE の面接で科学的な見方へ移った — [Borg Marks 2013](https://www.um.edu.mt/library/oar/handle/123456789/23758)。ただし多くが 1 クラス規模で、有意でも誤解が多く残った例もある
- **たとえ**: Gentner & Gentner (1983)「Flowing Waters or Teeming Crowds」は水の流れと人の群れの 2 つのたとえを比べ、1 つのたとえを学んだ学生はある種の問題は解けるが、別の種類の問題に不適切に使った — [Gentner & Gentner 1983 (PDF)](https://groups.psych.northwestern.edu/gentner/papers/GentnerGentner83.pdf)、[Stanford の interactive analogies 論文](https://web.stanford.edu/~danls/interactive%20analogies.pdf)。同論文は「電池は高い所の水槽のようなもの」と言うだけではほとんどの学生は回路の問題を解けず、対応づけを段階的に作る必要があるとする
- **日本**: 小学生に電圧、中学生に電位の概念を教える試行授業で一定の効果。現行の標準的な学習の流れでは正しい電圧の理解に届きにくい — [科研費 19K03109 成果報告](https://kaken.nii.ac.jp/en/file/KAKENHI-PROJECT-19K03109/19K03109seika.pdf)
- **McDermott & Shaffer (1992)** の Part II が研究にもとづく教材 (後の Tutorials in Introductory Physics) の作り方を示す。研究と教材の開発が互いを豊かにした — [ComPADRE](https://www.compadre.org/portal/items/detail.cfm?ID=2722)

### Inferences
- 「電位を高さ・圧力として色や高さで見せる」と「電流の流れを動く点で見せる」を組み合わせるのが、研究と合う。電子ガスモデルの成功は「電流より先に電位差」を示唆する。
- POE は手順として安く、シミュレーションと相性が良い (予想を入力 → 走らせる → 食い違いを説明)。ただし効果の大きさの証拠は弱めなので、それだけに頼らない。
- たとえは 1 つだけでなく、使える範囲と破綻する所をはっきり示すべき。

### Gaps
- 「ロープのたとえ」の効果を測った研究は見つけられなかった。
- 水のたとえ単体の効果を対照群で測った研究は見つからなかった。
- McDermott らの Tutorials の効果の数値 (事前・事後の正答率) は一次資料で確認できなかった。

## 3. シミュレーションと実機の実験について研究は何と言うか

### Takeaway
Finkelstein ら (2005) は、PhET の Circuit Construction Kit (電子の流れが見える) で実機の代わりをした組が、概念の理解でも、実際の回路を組んで説明する課題でも上回り、実回路を組むのも速かったと報告した。その後の研究は、シミュレーションは実機と同等以上の概念の学習をもたらし、順番 (仮想 → 実機、またはその逆) の効果は混在する、という線。

### Cited Findings
- **Finkelstein ら (2005)**「When learning about the real world is better done virtually」Phys. Rev. ST Phys. Educ. Res. 1, 010103。大人数の入門物理の授業で、CCK を実験機器の代わりに一部の組で使った。シミュレーションの組は期末試験の概念の問題でも、実回路を組んで動作を説明する課題でも上回った — [DOAJ の書誌](https://doaj.org/article/a86a7ac0a72641f2a000e4e2c86e5d61)、[SERC の解説](https://serc.carleton.edu/sp/library/phet/examples/example2.html)
- ポスター版では、シミュレーションの組は実回路をより速く組めた (p < 0.01) — [Finkelstein のポスター](https://cdn.serc.carleton.edu/files/textbook/finkelstein_poster2.pdf)。2004 年の PERC 版の題は「Can Computer Simulations Replace Real Equipment in Undergraduate Laboratories?」— [要旨](https://spot.colorado.edu/~finkelsn/sims_abs.html)
- SERC の解説によると、CCK の研究は全体として、シミュレーション (電子を表示してもしなくても) が実機と同じだけの概念の学習をもたらすと結論している — [SERC](https://serc.carleton.edu/sp/library/phet/examples/example2.html) (二次資料。比べた範囲の詳細は未確認)
- **順番**: Chini ら (2012, PRST-PER) は物理・仮想・物理→仮想・仮想→物理の 4 群を比べ、両方の順番が同じように理解を支えた — [Chini & Rebello 2012](https://perg.phys.ksu.edu/papers/2012/chini-rebello-prst-per.pdf) (この研究は回路でなく力学の滑車だった可能性があるので、使う前に確認)
- **Zacharia & de Jong (2014, Cognition and Instruction)**: 回路で、複雑な回路では仮想の操作の方が実物より理解を助け、仮想 → 実物の順だと実物の段階で使う適切な概念モデルができた — [書誌](https://informahealthcare.com/doi/abs/10.1080/07370008.2014.887083)、[Twente 大学](https://research.utwente.nl/en/publications/the-effects-on-students-conceptual-understanding-of-electric-circ/)
- PhET の CCK を POE 形式のワークシートで使う研究 (ACORN tutorial) もある — [NSF の論文](https://par.nsf.gov/servlets/purl/10541253)

### Inferences
- 電流の流れを見せるシミュレーションは、少なくとも概念の学習で実機に劣らない。シミュレーション → 実機の流れは、実機の作業 (組み立て) の速さにも効く可能性がある。circuitCad の「回路図 → 試作のブレッドボード → 実機」の流れはこの結果と合う。
- 実機には測定・故障・ノイズなどシミュレーションに無い学習があるので (4 章の故障探し)、置き換えより「先にシミュレーション、後で実機」が無難。

### Gaps
- Finkelstein ら (2005) の人数・効果量は一次資料で確認できなかった (約 1/3 の組が CCK、という記述が検索の要約にあるが、SERC は半分と書いており食い違う)。
- メーターの使い方の上達でのシミュレーションと実機の比較は見つからなかった。

## 4. 実験の技能 (マルチメーター・ブレッドボードの誤り・故障探し)

### Takeaway
メーターの使い方だけを変数にした研究は見つからず、近い証拠は故障探し (トラブルシューティング) の研究。学生は道具 (メーター) を使う手順を取ることが少なく、無作為にいじる例も多い。モデルにもとづく推論 (期待値と測定値を比べる) が故障探しの核とされる。

### Cited Findings
- バージニア大学の 2 年生 53 名がブレッドボードの回路の故障を直す研究で、多かった方略は「たどる・試す・パターンの照合・領域の知識を得る」、少なかったものの 1 つが「道具を使う」— [Kinsel ら (ASEE)](https://peer.asee.org/circuit-troubleshooting-techniques-in-an-electrical-and-computer-engineering-laboratory)
- 2024 年の ASEE の論文: 成功した学生も失敗した学生も、基板上のハードウェアの故障を見つけるのに苦労し、38% が無作為にいじることに頼った — [ASEE 2024](https://nemo.asee.org/public/conferences/344/papers/41142/view)
- 故障探しの観察用のオンラインの道具を作った研究 (学生のすべての操作をブレッドボードの絵と一緒に記録) — [Fritz ら 2021](https://peer.asee.org/online-interactive-tool-for-studying-how-students-troubleshoot-circuits.pdf)
- 故障探しの方略を教えて評価する 2 つの枠組み (ブレッドボード・電源・オシロスコープ・オペアンプ) — [Crockett ら 2025](https://peer.asee.org/byoe-teaching-and-assessing-troubleshooting-strategies-in-circuits-courses.pdf)
- Dounas-Frazer, Van De Bogart, Stetzer & Lewandowski: 8 組の学生が壊れた回路を声に出しながら直す研究で、故障探しの認知タスク分析とモデルにもとづく推論 (回路のモデルの予想と測定を比べて直す) を枠組みにした — [arXiv 1603.03684](https://ar5iv.labs.arxiv.org/html/1603.03684)。協同の故障探しのメタ認知 (Phys. Rev. Phys. Educ. Res. 13, 020116, 2017) — [arXiv 1701.02267](https://arxiv.org/abs/1701.02267)。指導者 20 名の面接 — [arXiv 1610.01564](https://www.arxiv.org/abs/1610.01564)
- メーターの典型的な誤り (実験の手引きから。研究ではない): 使い方を誤ってヒューズを飛ばす (電流のレンジで電圧を当てるなど)、レンジが高すぎる、電池切れ、プローブが金属でなく樹脂に当たっている — [Andrews 大学の電子工作の手引き](https://www.andrews.edu/services/physicsenterprises/products/lab-kits/physics2-resources/electronics-reference-guide.pdf)。入門の実験で、メーターを壊さずに抵抗・電圧・電流を測る、ブレッドボードの回路で測ることを教える — [UMBC CMPE212 Lab 1](https://courses.cs.umbc.edu/undergraduate/CMPE212/labs/lab1)

### Inferences
- 「期待する値 (シミュレーション) と実測を並べて比べる」画面は、故障探しの研究が核とするモデルにもとづく推論を直接支える。故障を仕込んだ練習 (circuitCad の修理の練習) は研究の課題の形と同じ。
- 初学者は「測る」を飛ばしがちなので、次にどこを測るかを促す仕組みが効くと考えられる (効果を測った研究は未確認)。

### Gaps
- ブレッドボードの誤り (行と列のつながりの誤解、中央の溝をまたがない IC、電源レールの切れ目) の頻度を数えた査読つきの研究は見つからなかった。
- メーターを電流計として直列に入れられない・並列に入れて壊す、の頻度の研究は見つからなかった。

## 5. 概念のテスト (DIRECT・CCII など) と何を測るか

### Takeaway
公開されて広く使われているのは DIRECT (直流の抵抗回路、29 問 × 2 版)。ECCE (45 問、直流と交流、コンデンサ・コイルも一部) は早い時期の回路専用のテストで、配布は教員に限られる。「CCII」という名のテストは見つからず、近いのは工学向けの CCI (Circuit Concept Inventory)。

### Cited Findings
- **DIRECT** (Engelhardt & Beichner 2004、Am. J. Phys. 72(1), 98–115, DOI 10.1119/1.1614813): 直流の抵抗回路の理解を調べる。29 問の 2 版。高校と大学が対象。誤概念を誤答の選択肢に組み込んだ。指導者が学生の進み具合と困難を追い、教材を評価するため — [arXiv physics/0304040](https://arxiv.org/abs/physics/0304040)、[KSU の PDF](https://perg.phys.ksu.edu/papers/2004/englehardt-beichner-ajp-1-04.pdf)。正式名の表記は資料で揺れる ("Determining and Interpreting Resistive Electric circuits Concepts Test")
- 他の研究での DIRECT の使用で事前テストが約 50%、Engelhardt & Beichner の大学の平均 53% を引用 — [DCU の調査](https://www.dcu.ie/sites/default/files/smec/pdfs/Surveying_prior_conceptual_understanding_of_DC_Circuits.pdf)
- **ECCE** (Electric Circuits Conceptual Evaluation、Sokoloff、1996 年の版): 45 問の選択式で直流と交流の回路、一部にコンデンサとコイル、一部の問いで説明を求める。大学入門・高校、約 60 分。PhysPort で教員のみ入手 — [PhysPort](https://www.physport.org/assessments/assessment.cfm?A=ECCE)、[UMD PERG](https://www.physics.umd.edu/perg/tools/diags.htm)。回路専用で最初に出たテストとされる — [NEMO ASEE 論文](https://nemo.asee.org/public/conferences/365/papers/46052/download)
- 回路の概念テストには CCI (Circuit Concept Inventory)、DIRECT、IBCDC (Inventory of Basic Conceptions – DC circuits) などがある。比べた 4 つのうち公開されているのは DIRECT だけ。ACIIEC (Adaptive Concept Inventory in Introductory Electric Circuits) 以外のテストは問いの大半がキルヒホッフの法則に関係する — [NEMO ASEE 論文](https://nemo.asee.org/public/conferences/365/papers/46052/download)、[ASEE の比較分析 (題目)](https://peer.asee.org/circuits-concept-inventories-a-comparative-analysis)
- 2022 年に高校生向けの直観に反する直流回路のテスト (CBECT、26 問) — [NEMO ASEE 論文](https://nemo.asee.org/public/conferences/365/papers/46052/download)
- 適応型のテスト (ACIIEC) の統計の分析 — [ASEE](https://strategy.asee.org/statistical-analysis-of-an-adaptive-concept-inventory-in-introductory-electric-circuits-for-students-and-instructors.pdf)

### Inferences
- アプリ内のクイズや学習の評価には、公開の DIRECT の項目の型 (明るさの順位・スイッチの開閉で何が変わるか・配線の絵 → 回路図) が手本になる (問題文そのものは転載せず型だけ使う)。
- 交流・過渡まで含む公開のテストはほぼ無いので、上級の内容の評価は自前で作る必要がある。

### Gaps
- 「CCII」はこの名では見つからなかった。依頼側の意図するテスト (CCI の誤記か、別のもの) を確認したい。
- CCI (工学向け) の作者・年・問題数は一次資料で確認できなかった (比較分析のページは 403 で読めなかった)。
- DIRECT の信頼性係数・下位の目標の一覧は全文で確認できていない。

## 6. 後で出てくる困難 (交流とフェーザ、過渡、トランジスタ、オペアンプの帰還、グラウンド・基準)

### Takeaway
入門の直流の後も困難は続く。RC の過渡とコンデンサ、トランジスタ回路 (直流のバイアスの考えを信号の解析に誤って使う)、オペアンプ (「ゴールデンルール」の機能的な理解が無い、電流の扱い) が研究で確かめられている。フェーザ・交流と、グラウンド・基準の誤解についてはまとまった研究を見つけられなかった。

### Cited Findings
- **RC 回路・コンデンサ**: 理工系の学部生の多くが静電容量と RC 回路の振る舞いで概念的・認識的な困難を持ち、静電容量を他の量と混同する (評価のテストを作る研究) — [GIREP サマースクール資料](https://indico.cern.ch/event/1593627/contributions/7132808/attachments/3289615/5881650/GIREP%20Summer%20School.pdf)
- 最も基本的な RC 回路でも定性的な振る舞いの予想と動作の概念の理解の困難が残る。ラプラス変換の扱いが他の回路解析と切り離されて教えられ、統合しにくい — [Kelly 2015, European Journal of Physics](https://hull-repository.worktribe.com/output/441305)
- 電流の微視的なモデルの指導を受けていない学生は、過渡の推論で暗記した式の意味を誤って読みやすい。ただし微視と巨視をつなぐ指導がいつも理解を改善するとは限らない — [arXiv physics/0207029](https://arxiv.org/pdf/physics/0207029)
- RC の微分方程式を解く足場かけのチュートリアル (多くの電磁気の授業は RC の定量問題に時間を割かない) — [Zavala & Martinez-Torteya (ASEE)](https://peer.asee.org/35249.pdf)
- **トランジスタ**: Van De Bogart & Stetzer (2018, arXiv 1806.00404)。指導後も多くの学生がバイポーラトランジスタ回路の頑健な理解を持たない。入力信号への影響を問うと、直流のバイアスの解析に適した推論を使う。的を絞った問いでは基本の振る舞いを知っていることを示す — [arXiv 1806.00404](https://arxiv.org/pdf/1806.00404)
- **オペアンプ**: Papanikolaou, Tombras, Van De Bogart & Stetzer, "Investigating student understanding of operational-amplifier circuits", Am. J. Phys. 83, 1039–1050 (2015)。多くの学生がオペアンプのゴールデンルールの機能的な理解を持たず、オペアンプ回路の振る舞い (電流など) に困難がある — [UMaine RiSE Center](https://umaine.edu/risecenter/?p=9650)、[Stetzer らの資料](https://www.per-central.org/items/perc/4255.pdf)。Van De Bogart の博士論文: すべての指導の後も多くの重要な回路について一貫した機能的な理解ができていない — [UMaine](https://umaine.edu/risecenter/?p=9650)
- オペアンプの機能の限界 (理想からのずれ) に焦点を当てた実習の研究 (Ríos ら, Phys. Rev. Phys. Educ. Res. 15, 010140, 2019) — [arXiv 1901.02423](https://ar5iv.labs.arxiv.org/html/1901.02423)

### Inferences
- 上級の困難も「どの見方 (直流のバイアス・小信号・理想オペアンプ) をいつ使うか」の切り替えの問題が多い。ソフトでは「直流の動作点」と「信号の変化分」を分けて見せる表示が役に立つと考えられる。
- 過渡では式より先に、コンデンサの電圧が連続であることを波形で見せる方が研究の指摘に合う。

### Gaps
- 交流・フェーザ (位相のずれ、複素インピーダンス、実効値) の学生の誤概念のまとまった研究は今回の検索では見つからなかった (ECCE が一部の交流を含むことのみ確認)。
- グラウンド・基準電位 (「グラウンドは電流が消える所」「電圧は 1 点の量」) の誤解の研究は見つからなかった。電位を主役にする電子ガスモデル (2 章) が関係するが、直接の研究は未確認。
- トランジスタ・オペアンプの研究の効果のある指導法 (介入の結果) は確認できなかった。
