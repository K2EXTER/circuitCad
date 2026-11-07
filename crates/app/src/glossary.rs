//! The glossary ("用語集・索引"): what a beginner meets in the lessons, with the reading, a short explanation and the lessons in which the word comes up.
//! The index is not written by hand: the lessons that use a term are found by looking for its name (or an alias) in the texts of the lessons.

use eframe::egui;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A word of electronics.
    Circuit,
    /// A word of this program.
    Operation,
    /// A word of the board (printed circuit board) side.
    Board,
}

pub struct Term {
    /// The word as the lessons write it (also the heading in the list).
    pub name: &'static str,
    /// In hiragana: the sort order of the index and a way to search.
    pub reading: &'static str,
    /// Other ways to write it (symbols, English, abbreviations). One-letter ones are not looked for in the lessons.
    pub aliases: &'static [&'static str],
    pub kind: Kind,
    pub short: &'static str,
}

const fn t(name: &'static str, reading: &'static str, aliases: &'static [&'static str], kind: Kind, short: &'static str) -> Term {
    Term { name, reading, aliases, kind, short }
}

use Kind::{Board as B, Circuit as C, Operation as O};

pub const TERMS: &[Term] = &[
    // ---- the basics of a circuit
    t("電圧", "でんあつ", &["ボルト"], C, "電気を押し出す力。回路の 2 点の間の「高さの差」で、単位はボルト (V)。"),
    t("電流", "でんりゅう", &["アンペア"], C, "流れる電気の量。単位はアンペア (A)。1A = 1000mA。"),
    t("抵抗", "ていこう", &["オーム"], C, "電流の流れにくさ。この働きをもつ部品の名前でもあります。単位はオーム (Ω)。"),
    t("オームの法則", "おーむのほうそく", &["V = I × R", "I = V ÷ R"], C, "電圧・電流・抵抗の関係。V = I × R (電圧 = 電流 × 抵抗)。どれか 2 つが分かれば、残りが決まります。"),
    t("電力", "でんりょく", &["ワット"], C, "電気が 1 秒あたりにする仕事 (熱や光になる量)。P = V × I。単位はワット (W)。"),
    t("直列", "ちょくれつ", &[], C, "部品を 1 本の道に順に並べること。同じ電流が流れ、抵抗は足し算になります。"),
    t("並列", "へいれつ", &[], C, "部品の道を分けて並べること。同じ電圧がかかり、電流は足し算になります。"),
    t("分圧", "ぶんあつ", &[], C, "直列の抵抗で電圧を分けること。中点の電圧 = 入力 × R2 ÷ (R1 + R2)。"),
    t("合成の抵抗", "ごうせいのていこう", &["合成抵抗"], C, "2 本以上の抵抗を、1 本とみなしたときの抵抗値。直列は足し算、並列は (R1 × R2) ÷ (R1 + R2)。"),
    t("GND", "ぐらうんど", &["グラウンド", "基準"], C, "電圧を測る基準の点 (0V)。回路図では、GND の記号やラベルで表します。電圧は「GND からいくつ高いか」で言います。"),
    t("直流", "ちょくりゅう", &["DC"], C, "向きも大きさも変わらない電気。電池などです。"),
    t("交流", "こうりゅう", &["AC"], C, "向きと大きさが、周期的に入れ替わる電気。家庭のコンセントなど。"),
    t("周波数", "しゅうはすう", &["ヘルツ"], C, "交流が 1 秒間に繰り返す回数。単位はヘルツ (Hz)。50Hz は 1 秒に 50 回。"),
    t("実効値", "じっこうち", &["RMS"], C, "交流の大きさを、同じ熱を出す直流の電圧で表した値。家庭の 100V は実効値で、ピークは約 141V。"),
    t("定格", "ていかく", &[], C, "部品が安全に使える電圧・電流・電力などの上限の目安。データシートに書かれています。"),
    t("許容差", "きょようさ", &["誤差"], C, "表示の値と、実際の値とのずれが許される範囲。±5% など。"),
    t("Meg", "めが", &["単位の接頭語"], C, "値の大きさの印。k = 1000 倍、Meg = 100 万倍、m = 1/1000、u (µ) = 100 万分の 1、n = 10 億分の 1、p = 1 兆分の 1。シミュレーションでは M と m を区別せず、どちらも 1/1000 なので、メガは Meg と書きます。"),
    // ---- passive parts
    t("コンデンサ", "こんでんさ", &[], C, "電気をためる部品。直流は通さず、電圧が変わるあいだだけ電流が流れます。"),
    t("容量", "ようりょう", &["ファラド", "静電容量"], C, "コンデンサがためられる電気の量を表す値。単位はファラド (F)。µF (マイクロ)、nF (ナノ)、pF (ピコ) がよく使われます。"),
    t("コイル", "こいる", &["インダクタ"], C, "電流が流れると、磁界にエネルギーをためる部品。電流が急に変わるのをいやがります。"),
    t("インダクタンス", "いんだくたんす", &["ヘンリー"], C, "コイルの、電流の変わりにくさを表す値。単位はヘンリー (H)。mH (ミリ) や µH がよく使われます。"),
    t("時定数", "じていすう", &["τ"], C, "充電や、電流の増え方の速さを表す時間。抵抗とコンデンサで τ = R × C、抵抗とコイルで τ = L ÷ R。τ の時間で、最終値の約 63% に達します。"),
    t("トランス", "とらんす", &["変圧器"], C, "2 つのコイルで、交流の電圧を巻数の比で変える部品。一次側と二次側は電気的につながっていません。"),
    t("ESR", "いーえすあーる", &["等価直列抵抗"], C, "コンデンサの中にある抵抗。リップル電流が流れると、ここで熱が出ます。"),
    t("DCバイアス", "でぃーしーばいあす", &["直流バイアス", "容量抜け"], C, "セラミックコンデンサに直流の電圧をかけると、容量が下がる現象です。耐圧いっぱいに近いほど大きく下がり、16V 品の 22µF は 5V で約 60% 下がります。電源の平滑や安定化に使うときは、かける電圧での容量で考えます。部品の値に BIAS=60 のように書くと、シミュレーションに入ります。"),
    t("インピーダンス", "いんぴーだんす", &[], C, "交流に対する抵抗のようなもの。周波数で変わります。単位はΩ。"),
    // ---- diodes
    t("ダイオード", "だいおーど", &[], C, "電流を一方向 (アノードからカソード) にだけ流す部品。"),
    t("順方向電圧", "じゅんほうこうでんあつ", &["Vf"], C, "ダイオードや LED に、電流を流しているときの、両端の電圧。シリコンのダイオードで約 0.7V、LED で約 2V。"),
    t("LED", "えるいーでぃー", &["発光ダイオード"], C, "電流を流すと光るダイオード。明るさは、電圧ではなく電流で決まります。"),
    t("ツェナーダイオード", "つぇなーだいおーど", &["ツェナー"], C, "逆向きの降伏を使って、電圧を一定に保つダイオード。"),
    t("降伏", "こうふく", &[], C, "逆向きの電圧が、ある値を超えたとき、急に逆向きに電流が流れ出す現象。"),
    t("ショットキーダイオード", "しょっときーだいおーど", &["ショットキー"], C, "順方向電圧が低く、逆回復がほとんどないダイオード。"),
    t("逆回復", "ぎゃくかいふく", &["trr"], C, "ダイオードに、急に逆電圧をかけたとき、蓄えられた電荷が抜けるまで、短い間、逆向きに電流が流れる現象。"),
    // ---- transistors and amplifiers
    t("トランジスタ", "とらんじすた", &[], C, "小さな信号で、大きな電流を制御する部品。増幅やスイッチに使います。"),
    t("hFE", "えいちえふいー", &["電流増幅率"], C, "トランジスタの、ベースに流す電流に対する、コレクタに流れる電流の倍率。"),
    t("飽和", "ほうわ", &[], C, "それ以上は増えなくなる状態。トランジスタが完全に ON になり、負荷が電流の上限を決める状態など。"),
    t("MOSFET", "もすふぇっと", &[], C, "ゲートの電圧で電流を制御するトランジスタ。ゲートには、直流では電流が流れません。"),
    t("しきい値", "しきいち", &["Vth"], C, "切り替わる境目の電圧。MOSFET では、電流が流れ始めるゲートの電圧。ロジック IC では、High と Low を分ける電圧。"),
    t("オン抵抗", "おんていこう", &["Rds(on)"], C, "MOSFET やスイッチが ON のときの、内部の抵抗。小さいほど、損失が少なくなります。"),
    t("オペアンプ", "おぺあんぷ", &[], C, "2 つの入力の電圧の差を、とても大きな倍率で増やす部品。"),
    t("負帰還", "ふきかん", &["帰還"], C, "出力の一部を入力に戻すこと。倍率が、抵抗の比で決まる、安定した回路になります。"),
    t("仮想短絡", "かそうたんらく", &[], C, "負帰還がかかったオペアンプで、+ と − の入力の電圧が、ほぼ等しくなること。"),
    t("ゲイン", "げいん", &["倍率"], C, "出力が、入力の何倍かを表す値。"),
    t("帯域", "たいいき", &["帯域幅"], C, "信号を正しく扱える、周波数の範囲。"),
    t("GBW", "じーびーだぶりゅー", &["ゲイン帯域幅積"], C, "オペアンプで、倍率 × 帯域が、ほぼ一定になる値。LM358 は約 1MHz。"),
    t("スルーレート", "するーれーと", &[], C, "出力の電圧が、変われる速さの上限 (V/µs)。大きく速い信号は、これを超えるとゆがみます。"),
    t("ボルテージフォロワ", "ぼるてーじふぉろわ", &[], C, "出力を − 入力につないで、入力と同じ電圧を出す、倍率 1 倍の回路。"),
    t("dB", "でしべる", &["デシベル"], C, "大きさの比を表す単位。0dB = 1 倍、6dB ≒ 2 倍、20dB = 10 倍、40dB = 100 倍、−20dB = 1/10。"),
    t("クリップ", "くりっぷ", &[], C, "出力が、電源の電圧に届いて、波形の上や下が平らに切れること。"),
    // ---- power
    t("整流", "せいりゅう", &[], C, "交流を、一方向の電流 (直流) に直すこと。"),
    t("全波整流", "ぜんぱせいりゅう", &["半波整流"], C, "交流の両方の半周期を使う整流 (ブリッジ)。ダイオード 1 本で半周期だけ使うのは、半波整流。"),
    t("平滑コンデンサ", "へいかつこんでんさ", &["平滑"], C, "整流した電圧のゆれを、ならすコンデンサ。"),
    t("リップル", "りっぷる", &[], C, "電源の電圧に残る、小さな周期的なゆれ。"),
    t("ドロップアウト", "どろっぷあうと", &[], C, "レギュレータが、出力を保つのに必要な、入力と出力の最低の差。入力がそれを下回ると、出力が下がります。"),
    t("リニアレギュレータ", "りにあれぎゅれーた", &["LDO", "三端子レギュレータ"], C, "入力と出力の差を熱にして、出力を一定に保つ電源 IC。部品が少なく、ノイズが小さい。"),
    t("スイッチングレギュレータ", "すいっちんぐれぎゅれーた", &["降圧コンバータ"], C, "トランジスタを高速に入り切りして、熱にせずに電圧を変える電源。効率が高い。"),
    t("PSRR", "ぴーえすあーるあーる", &["リップル除去比"], C, "入力のゆれが、出力にどれだけ伝わるかの比 (dB)。大きいほど、ゆれを消せます。"),
    t("効率", "こうりつ", &[], C, "入力の電力のうち、出力に使えた割合。"),
    t("ヒューズ", "ひゅーず", &[], C, "決まった電流を超えると、溶けて切れ、回路を守る部品。一度切れたら、元に戻りません。"),
    t("突入電流", "とつにゅうでんりゅう", &[], C, "電源を入れた瞬間に、一時的に流れる大きな電流。空のコンデンサを充電するときなど。"),
    t("サージ", "さーじ", &[], C, "雷などで、一瞬だけ加わる、とても高い電圧や大きな電流。"),
    t("バリスタ", "ばりすた", &[], C, "電圧が高くなると、急に電流を流して、電圧の上昇を止める部品。サージから回路を守ります。"),
    t("バイパスコンデンサ", "ばいぱすこんでんさ", &["デカップリング"], C, "ICの電源ピンのそばに置いて、電源の細かいゆれを吸収するコンデンサ。"),
    // ---- switches, relays and logic
    t("リレー", "りれー", &[], C, "コイルに電流を流して、接点を動かす部品。コイルと接点は、電気的に離れています。"),
    t("接点抵抗", "せってんていこう", &[], C, "スイッチやリレーの接点が閉じているときに残る、小さな抵抗。"),
    t("バウンス", "ばうんす", &["チャタリング"], C, "スイッチを押した瞬間に、接点が細かく入り切りを繰り返すこと。"),
    t("アーク", "あーく", &[], C, "スイッチを切るときに、接点の間に出る火花。"),
    t("プルアップ", "ぷるあっぷ", &[], C, "信号の線を、抵抗で電源に引き上げておくこと。何もつながっていない入力を、High に決めます。"),
    t("オープンドレイン", "おーぷんどれいん", &["オープンコレクタ"], C, "足を GND に引き下げる働きだけを持つ出力。High は、外につけたプルアップ抵抗が作ります。"),
    t("ロジック IC", "ろじっくあいしー", &["ロジックIC"], C, "High と Low の 2 つの値で、論理を扱う IC。"),
    t("High", "はい", &["Low", "ハイ", "ロー"], C, "ロジックの 2 つの値。High は電源に近い電圧 (「1」)、Low は 0V に近い電圧 (「0」)。"),
    t("ヒステリシス", "ひすてりしす", &[], C, "上がるときと、下がるときで、切り替わる境目を変えること。ノイズで出力が揺れるのを防ぎます。"),
    t("シュミットトリガ", "しゅみっととりが", &[], C, "入力のしきい値に、ヒステリシスを持たせた回路。"),
    // ---- resonance and sensors
    t("共振", "きょうしん", &[], C, "特定の周波数で、よく振動したり、電気をよく通したりすること。"),
    t("Q 値", "きゅーち", &["共振の鋭さ", "Q (共振"], C, "共振の鋭さを表す値。大きいほど、共振の山が細くなります。"),
    t("水晶振動子", "すいしょうしんどうし", &["水晶"], C, "正確な周波数で共振する部品。マイコンのクロック (時計) の元になります。"),
    t("等価回路", "とうかかいろ", &[], C, "同じ働きをする、部品の組み合わせで表した回路。"),
    t("サンプルホールド", "さんぷるほーるど", &[], C, "信号の電圧を、コンデンサにためて、その値を保つ回路。"),
    t("A/D コンバータ", "えーでぃーこんばーた", &["ADC", "A/Dコンバータ"], C, "電圧を、数字に変える回路。"),
    t("絶縁", "ぜつえん", &[], C, "電気的に切り離すこと。つながっていないので、グラウンドも別にできます。"),
    t("フォトカプラ", "ふぉとかぷら", &[], C, "LED と光センサを 1 つにして、光で信号を伝える部品。入力と出力が絶縁されます。"),
    t("CTR", "しーてぃーあーる", &["電流伝達率"], C, "フォトカプラの、出力の電流 ÷ 入力の電流。"),
    t("レシオメトリック", "れしおめとりっく", &[], C, "出力が、電源電圧に比例すること。A/D コンバータの基準が同じ電源なら、電源が変わっても読みは変わりません。"),
    t("磁束密度", "じそくみつど", &["ガウス", "テスラ"], C, "磁界の強さ。単位はガウス (G) やテスラ (T)。1G = 0.1mT。"),
    t("ホールセンサ", "ほーるせんさ", &[], C, "磁界の強さを、電圧で出すセンサ。"),
    t("サーミスタ", "さーみすた", &["NTC"], C, "温度で抵抗が変わる部品。NTC は、温度が上がると抵抗が下がるタイプ。"),
    t("B 定数", "びーていすう", &["B定数"], C, "サーミスタの、温度による抵抗の変わり方を表す定数。"),
    t("感度", "かんど", &[], C, "入力の変化に対する、出力の変化の大きさ。"),
    t("ミラー期間", "みらーきかん", &["プラトー"], C, "MOSFET を切り替えるとき、ゲートの電圧が一定に止まる期間。この間にドレインの電圧が下がります。"),
    t("スイッチング損失", "すいっちんぐそんしつ", &[], C, "切り替えの最中に、電圧と電流が同時に大きいために出る熱。"),
    // ---- words of the older lessons (parts, power, microcontroller, measuring)
    t("データシート", "でーたしーと", &["仕様書"], C, "部品のメーカーが出す、性能・限界・使い方を書いた資料 (PDF)。部品を選ぶときと、使うときに、必ず見ます。"),
    t("マイコン", "まいこん", &["マイクロコントローラ"], C, "プログラムで動く、小さなコンピュータの IC。Arduino の ATmega328P など。"),
    t("Arduino", "あるでゅいーの", &[], C, "マイコンを載せた、電子工作の定番のボード。USB でパソコンにつなぎ、プログラムを書き込みます。"),
    t("ATmega328P", "えーてぃーめがさんにーはちぴー", &[], C, "Arduino Uno に使われているマイコンの IC。"),
    t("ブレッドボード", "ぶれっどぼーど", &[], C, "はんだ付けなしで、部品を差して回路を組める、穴の開いた板。実験に使います。"),
    t("テスタ", "てすた", &["マルチメータ"], C, "電圧・電流・抵抗を測る、手のひらサイズの測定器。"),
    t("オシロスコープ", "おしろすこーぷ", &["オシロ"], C, "電圧が時間とともに変わる様子を、波形で見る測定器。"),
    t("プローブ", "ぷろーぶ", &[], C, "オシロスコープなどを、回路につなぐための棒やコード。"),
    t("シャント抵抗", "しゃんとていこう", &["シャント"], C, "電流を、両端の電圧に変えて測るための、小さな抵抗。電圧 ÷ 抵抗で、電流が分かります。"),
    t("アノード", "あのーど", &["カソード"], C, "ダイオードの足の名前。電流は、アノードから入り、カソードから出ます。カソード側に、印の線があります。"),
    t("エミッタ", "えみった", &["ベース", "コレクタ"], C, "バイポーラトランジスタの 3 本の足 (ベース・コレクタ・エミッタ)。小さなベース電流で、コレクタからエミッタへの大きな電流を制御します。"),
    t("ドレイン", "どれいん", &["ゲート", "ソース"], C, "MOSFET の 3 本の足 (ゲート・ドレイン・ソース)。ゲートの電圧で、ドレインからソースへの電流を制御します。"),
    t("エミッタフォロワ", "えみったふぉろわ", &[], C, "出力が入力に付いて動き、電圧はほぼ変えずに、流せる電流を増やすトランジスタの回路。"),
    t("パストランジスタ", "ぱすとらんじすた", &[], C, "レギュレータで、入力から出力へ電流を通す側の、大きなトランジスタ。"),
    t("ボディダイオード", "ぼでぃだいおーど", &[], C, "MOSFET の中に、もともと入っているダイオード。ソースからドレインへ電流を通します。"),
    t("フォトダイオード", "ふぉとだいおーど", &[], C, "光が当たると、電流が流れるダイオード。光の強さに比例する電流が得られます。"),
    t("LDR", "えるでぃーあーる", &["CdS"], C, "光が当たると、抵抗が下がる部品。明るさのセンサ。変わり方はゆるやかです。"),
    t("オフセット", "おふせっと", &[], C, "本来は 0 のはずの所に残る、一定のずれ。"),
    t("負荷", "ふか", &[], C, "電源から電力を受けて、働く側の回路や部品。「負荷が重い」は、電流が大きいこと。"),
    t("電圧源", "でんあつげん", &[], C, "一定の電圧を出す電源の、回路上の表し方。理想は、電流を流しても電圧が変わりません。"),
    t("内部抵抗", "ないぶていこう", &[], C, "電池や電源の中にある抵抗。電流を流すと、ここで電圧が下がります。"),
    t("電圧降下", "でんあつこうか", &[], C, "電流が抵抗を流れるとき、その両端で電圧が下がること。V = I × R。"),
    t("基準電圧", "きじゅんでんあつ", &["リファレンス"], C, "正確で、一定の電圧。比べる基準や、電源の出力を決めるのに使います。"),
    t("短絡", "たんらく", &["ショート"], C, "つないではいけない所が、つながって、大きな電流が流れること。"),
    t("位相", "いそう", &[], C, "交流の波の、ずれ具合 (角度)。入力と出力の波が、どれだけずれているかを表します。"),
    t("位相余裕", "いそうよゆう", &[], C, "増幅回路が、不安定になる (勝手に振動する) までの余裕。小さいと、過渡の波形が揺れ、ゼロだと発振します。"),
    t("発振", "はっしん", &[], C, "入力がなくても、自分で周期的に振動すること。意図して使う (発振器) 場合と、増幅回路の不具合の場合があります。"),
    t("オーバーシュート", "おーばーしゅーと", &[], C, "出力が、目標の値を、一時的に行き過ぎること。"),
    t("リンギング", "りんぎんぐ", &[], C, "信号が急に変わったあとに残る、細かい揺れ。"),
    t("フィルタ", "ふぃるた", &["ローパスフィルタ"], C, "特定の周波数の信号を通し、ほかを弱める回路。ローパスは、低い周波数を通します。"),
    t("カットオフ周波数", "かっとおふしゅうはすう", &["カットオフ"], C, "フィルタで、信号が約 0.7 倍 (−3dB) に弱まる周波数。"),
    t("アナログ", "あなろぐ", &[], C, "電圧のように、連続して変わる量。"),
    t("デジタル", "でじたる", &[], C, "High と Low の 2 つの値 (0 と 1) で表す信号。"),
    t("ロジックレベル", "ろじっくれべる", &[], C, "High と Low にあたる電圧の値。5V の系と 3.3V の系があります。"),
    t("PWM", "ぴーだぶりゅーえむ", &["パルス幅変調"], C, "スイッチを入り切りする時間の割合 (デューティ) を変えて、平均の電圧や電力を調整する方法。"),
    t("デューティ", "でゅーてぃ", &[], C, "信号が High の時間の割合 (デューティ比)。50% は、High と Low が半々。"),
    t("クロック", "くろっく", &[], C, "マイコンなどの動きの基準になる、一定の周期の信号。"),
    t("リセット", "りせっと", &["RESET"], C, "マイコンを、最初の状態に戻す信号。RESET ピンを Low にすると、プログラムが最初から始まります。"),
    t("プルダウン", "ぷるだうん", &[], C, "信号の線を、抵抗で GND に引き下げておくこと。何もつながっていない入力を、Low に決めます。"),
    t("シリアル通信", "しりあるつうしん", &["UART", "シリアル"], C, "1 本の線で、1 ビットずつ順番にデータを送る通信。マイコンとパソコンの通信などに使います。"),
    t("ボーレート", "ぼーれーと", &[], C, "シリアル通信の速さ。1 秒に送る信号の数 (9600 や 115200)。送る側と受ける側で、合わせます。"),
    t("スタートビット", "すたーとびっと", &["ストップビット"], C, "シリアル通信で、1 文字の始まりと終わりを知らせる信号。"),
    t("USB", "ゆーえすびー", &["VBUS"], C, "パソコンと機器をつなぐ規格。5V の電源 (VBUS) と、2 本のデータ線 (D+、D−) があります。"),
    t("フルスピード", "ふるすぴーど", &["ロースピード", "ハイスピード"], C, "USB の通信の速さの種類。ロー 1.5Mbit/s、フル 12Mbit/s、ハイ 480Mbit/s。"),
    t("ESD", "いーえすでぃー", &["静電気"], C, "静電気による放電。USB の信号線などに触れて、部品を壊すことがあります。"),
    t("ESL", "いーえすえる", &["等価直列インダクタンス"], C, "コンデンサの中にある、コイルの成分。高い周波数では、これのせいで、コンデンサとして働かなくなります。"),
    t("セラミックコンデンサ", "せらみっくこんでんさ", &[], C, "セラミックで作ったコンデンサ。小さく、高い周波数に強い。電圧をかけると容量が減ります。"),
    t("電解コンデンサ", "でんかいこんでんさ", &["電解"], C, "大きな容量が作れるコンデンサ。極性 (+ と −) があり、逆にすると壊れます。寿命があります。"),
    t("負荷容量", "ふかようりょう", &[], C, "水晶の両端につける、コンデンサの容量 (22pF など)。発振の周波数を決めます。"),
    t("放熱板", "ほうねつばん", &["ヒートシンク"], C, "部品の熱を、空気に逃がすための、金属の板。"),
    t("熱抵抗", "ねつていこう", &[], C, "熱の伝わりにくさ (℃/W)。電力 × 熱抵抗が、温度の上がり方になります。"),
    t("絶対最大定格", "ぜったいさいだいていかく", &[], C, "超えると壊れるかもしれない、限界の値。ふだんの使い方では、ここまで使いません。"),
    t("逆起電力", "ぎゃくきでんりょく", &[], C, "コイルの電流を急に切ったとき、電流を保とうとして出る、高い電圧。"),
    t("フライバックダイオード", "ふらいばっくだいおーど", &["クランプダイオード"], C, "コイル (リレーやモータ) と並列に入れて、逆起電力を逃がし、トランジスタを守るダイオード。"),
    t("ソフトスタート", "そふとすたーと", &[], C, "電源を入れたとき、出力をゆっくり立ち上げて、突入電流を減らす機能。"),
    t("ハイサイド", "はいさいど", &[], C, "負荷の電源側 (+ 側) に、スイッチを入れる方式。"),
    t("TL431", "てぃーえるよんさんいち", &["シャントレギュレータ"], C, "2.495V の基準電圧を作る部品。2 本の抵抗で、好きな電圧の基準が作れます。"),
    t("LM317", "えるえむさんいちなな", &[], C, "出力電圧を、2 本の抵抗で決められる、可変の三端子レギュレータ。"),
    // ---- the board
    t("基板", "きばん", &["プリント基板", "PCB"], B, "部品をのせて、銅の配線でつなぐ板。回路図ができたら、これを作って、製造メーカーに発注します。"),
    t("パターン", "ぱたーん", &["配線パターン"], B, "基板の上の、銅でできた配線や、パッドの形。「基板パターン」は、その設計。"),
    t("配線", "はいせん", &["トラック"], B, "部品の足どうしをつなぐ、基板の上の銅の線。"),
    t("線幅", "せんはば", &["配線幅", "推奨幅"], B, "配線の太さ。太いほど、抵抗が小さく、大きな電流を流せます。電流から、必要な太さが分かります。"),
    t("間隔", "かんかく", &["クリアランス"], B, "配線やパッドの間の距離。近すぎると、ショートしたり、製造できなかったりします。"),
    t("パッド", "ぱっど", &["ランド"], B, "部品の足を、はんだ付けする、基板の上の銅の部分。"),
    t("ビア", "びあ", &["スルーホール"], B, "基板の表と裏の配線を、穴の中の銅でつなぐ所。"),
    t("未配線", "みはいせん", &["ラッツネスト"], B, "まだ配線でつながっていない、つなぐべき接続。細い線で、表示されます。"),
    t("自動配置", "じどうはいち", &[], B, "部品の位置を、ソフトが自動で決める機能。叩き台なので、人が手直しします。"),
    t("自動配線", "じどうはいせん", &["オートルータ"], B, "ソフトが、配線を自動で引く機能。通せない所は、人が手で引きます。"),
    t("DRC", "でぃーあーるしー", &["デザインルールチェック", "違反"], B, "基板の配線が、間隔・幅などの製造のルールを守っているかを調べる機能。守っていない所は、違反として、表示されます。"),
    t("発注前チェック", "はっちゅうまえちぇっく", &["発注"], B, "基板を製造メーカーに注文する前の点検。部品が基板の外にないか、配線が細すぎないかなどを調べます。"),
    t("ガーバー", "がーばー", &["ガーバーファイル", "Gerber"], B, "基板の製造メーカーに渡す、層ごとの図面のデータ。"),
    t("外形", "がいけい", &[], B, "基板を切り抜く輪郭の線。部品は、外形の内側に置きます。"),
    t("差動ペア", "さどうぺあ", &["差動"], B, "2 本の線で 1 つの信号を運ぶ組。USB の D+ と D−。並べて、同じ長さで、引きます。"),
    t("長さ合わせ", "ながさあわせ", &["蛇行"], B, "ペアや複数の線の長さをそろえるため、線をくねらせて長くすること。"),
    t("ヘッダ", "へっだ", &["ピンヘッダ", "コネクタ"], B, "ピンが並んだ部品。基板どうしや、ケーブルをつなぐのに使います。"),
    t("ピッチ", "ぴっち", &[], B, "ピンや穴の、中心どうしの間隔。0.5mm ピッチ、2.54mm ピッチなど。小さいほど、配線が難しい。"),
    t("浮遊容量", "ふゆうようりょう", &[], B, "配線やパッドの間に、意図せずできる、小さなコンデンサ。高い周波数の信号に影響します。"),
    t("ノイズ", "のいず", &["雑音"], B, "信号に混ざる、じゃまな、不要な変動。"),
    t("回路図", "かいろず", &[], B, "部品の記号と線で、回路のつながりを描いた図。基板を作る元になります。"),
    t("グリッド", "ぐりっど", &["格子"], B, "画面の点や線の、位置をそろえる格子。部品や配線は、格子の点に吸い付きます。"),
    // ---- this program
    t("DC動作点", "でぃーしーどうさてん", &[], O, "回路が落ち着いた状態の、各点の電圧と各部品の電流を計算する解析。「解析」タブの「DC動作点を解析」ボタン。"),
    t("過渡解析", "かとかいせき", &["トランジェント"], O, "時間が進むにつれて、電圧や電流がどう変わるかを計算する解析。波形がグラフに出ます。"),
    t("AC解析", "えーしーかいせき", &["周波数特性", "ボード線図"], O, "周波数を変えながら、小さな交流が、各点にどれだけ出てくるかを調べる解析。グラフは大きさ (dB) と位相。"),
    t("DCスイープ", "でぃーしーすいーぷ", &["DC スイープ"], O, "電源の電圧を、少しずつ変えながら、出力を調べる解析。"),
    t("温度スイープ", "おんどすいーぷ", &[], O, "回路の温度を、少しずつ変えながら、電圧がどう変わるかを調べる解析。"),
    t("ばらつき解析", "ばらつきかいせき", &["モンテカルロ"], O, "部品の誤差 (許容差) で、結果がどれだけ動くかを、たくさんの組み合わせで調べる解析。"),
    t("ERC", "いーあーるしー", &["電気ルールチェック"], O, "回路図の配線ミス (つながっていない、ショートなど) を点検する機能。"),
    t("ラベル", "らべる", &["ネットラベル"], O, "同じ名前の点を、線を引かずにつなぐ札。ラベルがついた点は、同じ名前のラベルとつながっています。"),
    t("階層設計", "かいそうせっけい", &["シートの記号", "チャンネルの繰り返し"], O, "使い回したい回路を別のシートに1つ描き、その「記号」(箱) を何個も置いて使う方法。シートの中を直すと、全部の記号に効きます。記号ごとに、部品番号 (R1_CH1) とネット名 (CH1.MID) が別になります。"),
    t("ポート", "ぽーと", &["シートの出入口"], O, "子のシートの出入口。ポートの名前が、そのシートを記号にしたときのピンの名前になります。"),
    t("モデル", "もでる", &["SPICEモデル"], O, "部品の働きを、式や簡単な回路で表したもの。シミュレーションは、これで計算するので、実物と細かく違う所があります。"),
    t("SPICE", "すぱいす", &[], O, "回路シミュレーションの、世界共通の書き方と計算の方法。値の書き方 (4.7k、100n) も、これに従います。"),
    t("ライブラリ", "らいぶらり", &["部品ライブラリ"], O, "あらかじめ用意された、部品の一覧。左の部品の欄から、置けます。"),
];

/// Katakana to hiragana and lower case: how a word is compared in a search.
fn norm(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'ァ'..='ン' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
            _ => c.to_ascii_lowercase(),
        })
        .collect()
}

/// The row of the 五十音 table of a reading (か for が, は for ぱ ...), or "英数" when it does not start with a kana.
pub fn row_of(reading: &str) -> &'static str {
    const ROWS: &[(&str, &str)] = &[
        ("あ", "ぁあぃいぅうぇえぉお"),
        ("か", "かがきぎくぐけげこご"),
        ("さ", "さざしじすずせぜそぞ"),
        ("た", "ただちぢっつづてでとど"),
        ("な", "なにぬねの"),
        ("は", "はばぱひびぴふぶぷへべぺほぼぽ"),
        ("ま", "まみむめも"),
        ("や", "ゃやゅゆょよ"),
        ("ら", "らりるれろ"),
        ("わ", "わをん"),
    ];
    let first = reading.chars().next().unwrap_or(' ');
    ROWS.iter().find(|(_, set)| set.contains(first)).map_or("英数", |(row, _)| row)
}

/// The terms that match a search text (name, reading, an alias), in the order of the table; all of them for an empty text.
pub fn search(query: &str) -> Vec<&'static Term> {
    let q = norm(query.trim());
    TERMS.iter().filter(|t| q.is_empty() || norm(t.name).contains(&q) || t.reading.contains(&q) || t.aliases.iter().any(|a| norm(a).contains(&q))).collect()
}

/// The vowel of a kana, for "ー" (the long vowel mark goes with the vowel before it in the order of the index).
fn vowel(c: char) -> char {
    const COLUMNS: [(char, &str); 5] = [
        ('あ', "あぁかがさざただなはばぱまやゃらわ"),
        ('い', "いぃきぎしじちぢにひびぴみり"),
        ('う', "うぅくぐすずつづっぬふぶぷむゆゅる"),
        ('え', "えぇけげせぜてでねへべぺめれ"),
        ('お', "おぉこごそぞとどのほぼぽもよょろを"),
    ];
    COLUMNS.iter().find(|(_, set)| set.contains(c)).map_or(c, |(v, _)| *v)
}

/// The key of the index order: the reading, with "ー" read as the vowel before it.
fn sort_key(reading: &str) -> String {
    let mut last = 'あ';
    reading
        .chars()
        .map(|c| {
            if c == 'ー' {
                last
            } else {
                last = vowel(c);
                c
            }
        })
        .collect()
}

/// The terms in the order of the index (by reading).
pub fn sorted(mut v: Vec<&'static Term>) -> Vec<&'static Term> {
    v.sort_by_key(|t| sort_key(t.reading));
    v
}

/// Whether `text` uses the term: its name or an alias of two characters or more is in it.
pub fn occurs_in(term: &Term, text: &str) -> bool {
    std::iter::once(term.name).chain(term.aliases.iter().copied()).any(|w| w.chars().count() >= 2 && text.contains(w))
}

/// The terms a text uses (for the lesson window).
pub fn terms_in(text: &str) -> Vec<&'static Term> {
    sorted(TERMS.iter().filter(|t| occurs_in(t, text)).collect())
}

/// A lesson that the glossary can start: a lesson of the schematic editor (its index, see `schematic::lesson_ids`) or of the board editor
/// (the number that `start_board_lesson` takes).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Schematic(usize),
    Board(usize),
}

struct Entry {
    target: Target,
    title: &'static str,
    text: String,
}

/// Every lesson with everything it says: the schematic lessons in their order, then the board lessons.
fn all_lessons() -> &'static Vec<Entry> {
    static LESSONS: OnceLock<Vec<Entry>> = OnceLock::new();
    LESSONS.get_or_init(|| {
        let titles = crate::schematic::lesson_titles();
        let mut v: Vec<Entry> = crate::schematic::lesson_texts().into_iter().enumerate().map(|(i, text)| Entry { target: Target::Schematic(i), title: titles[i], text }).collect();
        v.extend(crate::board_editor::lesson::lesson_texts().into_iter().map(|(which, title, text)| Entry { target: Target::Board(which), title, text }));
        v
    })
}

/// The lessons that use the term, with their titles.
pub fn lessons_of(term: &Term) -> Vec<(Target, &'static str)> {
    all_lessons().iter().filter(|e| occurs_in(term, &e.text)).map(|e| (e.target, e.title)).collect()
}

#[derive(Default)]
pub struct GlossaryWindow {
    pub open: bool,
    query: String,
    selected: Option<&'static str>,
}

impl GlossaryWindow {
    /// Open the window on a term (an empty or unknown name only opens it).
    pub fn open_at(&mut self, name: &str) {
        self.open = true;
        self.query.clear();
        self.selected = TERMS.iter().find(|t| t.name == name).map(|t| t.name);
    }

    /// Draw the window; the answer is the lesson the learner chose to start.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Target> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut start = None;
        egui::Window::new("用語集・索引").open(&mut open).default_size([760.0, 520.0]).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("さがす:");
                ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("名前・読み・別名 (例: ぶんあつ、dB、ADC)").desired_width(300.0))
                    .on_hover_text("ひらがな・カタカナ・英字のどれでも探せます");
                if ui.small_button("消す").clicked() {
                    self.query.clear();
                }
            });
            let found = sorted(search(&self.query));
            ui.small(format!("{} 件 (全 {} 語)。語を押すと、説明と、その語が出てくるレッスンが右に出ます", found.len(), TERMS.len()));
            ui.separator();
            ui.columns(2, |cols| {
                egui::ScrollArea::vertical().id_salt("glossary_list").show(&mut cols[0], |ui| {
                    let mut row = "";
                    for term in &found {
                        let r = row_of(term.reading);
                        if r != row {
                            row = r;
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(format!("【{row}】")).strong().color(crate::theme::WARN));
                        }
                        let mark = match term.kind {
                            Kind::Operation => "  (操作)",
                            Kind::Board => "  (基板)",
                            Kind::Circuit => "",
                        };
                        if ui.selectable_label(self.selected == Some(term.name), format!("{} ({}){mark}", term.name, term.reading)).clicked() {
                            self.selected = Some(term.name);
                        }
                    }
                    if found.is_empty() {
                        ui.label("見つかりませんでした。別の書き方 (ひらがな・英字) でも試してください");
                    }
                });
                egui::ScrollArea::vertical().id_salt("glossary_detail").show(&mut cols[1], |ui| {
                    let Some(term) = self.selected.and_then(|n| TERMS.iter().find(|t| t.name == n)) else {
                        ui.label("左の一覧から、語を選んでください。");
                        return;
                    };
                    ui.heading(term.name);
                    ui.small(format!(
                        "読み: {}　{}",
                        term.reading,
                        match term.kind {
                            Kind::Operation => "(このソフトの言葉)",
                            Kind::Board => "(基板の言葉)",
                            Kind::Circuit => "(回路の言葉)",
                        }
                    ));
                    if !term.aliases.is_empty() {
                        ui.small(format!("別名: {}", term.aliases.join("、")));
                    }
                    ui.add_space(4.0);
                    ui.label(term.short);
                    ui.add_space(8.0);
                    let lessons = lessons_of(term);
                    if lessons.is_empty() {
                        ui.small("この語が出てくるレッスンは、まだありません。");
                    } else {
                        crate::widgets::section_heading(ui, "この語が出てくるレッスン");
                        for (target, title) in lessons {
                            if ui.button(title).on_hover_text("このレッスンを始めます").clicked() {
                                start = Some(target);
                            }
                        }
                    }
                });
            });
        });
        if !open {
            self.open = false;
        }
        start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ids of the schematic lessons that use a term.
    fn schematic_ids_of(term: &Term) -> Vec<&'static str> {
        let ids = crate::schematic::lesson_ids();
        lessons_of(term).iter().filter_map(|(t, _)| if let Target::Schematic(i) = t { Some(ids[*i]) } else { None }).collect()
    }

    #[test]
    fn names_are_unique_and_the_readings_are_hiragana() {
        let mut names: Vec<&str> = TERMS.iter().map(|t| t.name).collect();
        names.sort();
        let n = names.len();
        names.dedup();
        assert_eq!(names.len(), n, "a name is listed twice");
        for t in TERMS {
            assert!(!t.reading.is_empty() && t.reading.chars().all(|c| ('ぁ'..='ん').contains(&c) || c == 'ー'), "{}: reading {} is not hiragana", t.name, t.reading);
            assert!(t.short.chars().count() >= 12, "{}: the explanation is too short", t.name);
        }
    }

    #[test]
    fn every_term_is_used_by_a_lesson() {
        let missing: Vec<&str> = TERMS.iter().filter(|t| lessons_of(t).is_empty()).map(|t| t.name).collect();
        assert!(missing.is_empty(), "in no lesson (the index would be empty): {missing:?}");
    }

    #[test]
    fn a_search_finds_by_the_reading_the_name_and_the_alias() {
        let names = |q: &str| search(q).iter().map(|t| t.name).collect::<Vec<_>>();
        assert!(names("ぶんあつ").contains(&"分圧") && names("ブンアツ").contains(&"分圧"), "hiragana and katakana");
        assert!(names("db").contains(&"dB") && names("デシベル").contains(&"dB"), "name and alias");
        assert!(names("adc").contains(&"A/D コンバータ"), "an English alias");
        assert_eq!(search("").len(), TERMS.len(), "an empty search lists all");
        assert!(names("ぜんぜんないことば").is_empty());
    }

    #[test]
    fn the_index_groups_by_the_row_of_the_reading() {
        assert_eq!(row_of("ぶんあつ"), "は");
        assert_eq!(row_of("ぐらうんど"), "か");
        assert_eq!(row_of("ぴーえすあーるあーる"), "は");
        assert_eq!(row_of("でしべる"), "た");
        assert_eq!(row_of("abc"), "英数");
        let order = ["あ", "か", "さ", "た", "な", "は", "ま", "や", "ら", "わ", "英数"];
        let rows: Vec<usize> = sorted(search("")).iter().map(|t| order.iter().position(|r| *r == row_of(t.reading)).unwrap()).collect();
        assert!(rows.windows(2).all(|w| w[0] <= w[1]), "the index is in the order of the rows");
    }

    #[test]
    fn a_long_vowel_mark_sorts_with_the_vowel_before_it() {
        assert_eq!(sort_key("いーえすあーる"), "いいえすあある");
        let names: Vec<&str> = sorted(search("")).iter().map(|t| t.name).collect();
        let pos = |n: &str| names.iter().position(|x| *x == n).unwrap();
        assert!(pos("ERC") < pos("インダクタンス") || pos("インダクタンス") < pos("ERC"), "both are listed");
        // いーあーるしー (ERC) comes before いんだくたんす: "いいあああるしし" < "いんだくたんす"
        assert!(pos("ERC") < pos("インダクタンス"));
    }

    #[test]
    fn the_lessons_of_a_term_are_found_in_their_texts() {
        let term = TERMS.iter().find(|t| t.name == "分圧").unwrap();
        let lessons = schematic_ids_of(term);
        assert!(lessons.contains(&"intro_read"), "{lessons:?}");
        assert!(terms_in("分圧とリップルの話").iter().any(|t| t.name == "リップル"));
    }

    #[test]
    fn the_words_of_a_lesson_are_listed_for_its_window() {
        let ids = crate::schematic::lesson_ids();
        let texts = crate::schematic::lesson_texts();
        let i = ids.iter().position(|l| *l == "intro_read").unwrap();
        let names: Vec<&str> = terms_in(&texts[i]).iter().map(|t| t.name).collect();
        for want in ["分圧", "ラベル", "GND", "Meg", "DC動作点"] {
            assert!(names.contains(&want), "{want} is not among {names:?}");
        }
    }

    #[test]
    fn words_of_the_older_lessons_lead_to_them() {
        let find = |name: &str| schematic_ids_of(TERMS.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("{name}")));
        assert!(find("データシート").contains(&"datasheet"));
        assert!(find("プルアップ").contains(&"pullup"));
        assert!(find("フライバックダイオード").contains(&"flyback"));
        assert!(find("TL431").contains(&"tl431_ref"));
        assert!(find("ESD").iter().any(|l| l.starts_with("usb")));
    }

    #[test]
    fn words_of_the_board_lessons_lead_to_them() {
        let boards = |name: &str| -> Vec<usize> {
            let term = TERMS.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("{name}"));
            lessons_of(term).iter().filter_map(|(t, _)| if let Target::Board(w) = t { Some(*w) } else { None }).collect()
        };
        assert!(boards("DRC").contains(&2), "the pre-order check lesson");
        assert!(boards("差動ペア").contains(&4) && boards("差動ペア").contains(&6));
        assert!(boards("未配線").contains(&0));
    }
}
