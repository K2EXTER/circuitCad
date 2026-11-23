//! "データシートの読み方" (Q10): the order to read a data sheet in, for a beginner. The data sheet window walks through these steps
//! on the PDF the learner opened (its "読み方の案内": each step searches its heading), and the learning path has them as a checklist
//! (`bench_guide::DATASHEET`). The bundled parts' PDFs cannot be shipped, so the guide follows whatever PDF is open.

/// One step: what to look at, why, and the words of its heading (English and Japanese) to find it in the PDF.
pub struct ReadStep {
    pub title: &'static str,
    pub detail: &'static str,
    /// What to check on that page, one by one.
    pub checks: &'static [&'static str],
    pub terms: &'static [&'static str],
}

pub const STEPS: &[ReadStep] = &[
    ReadStep {
        title: "1. 何の部品か (表紙と特長)",
        detail: "1 ページ目で、型番と、何をする部品か (Description・概要)、電源電圧の範囲、パッケージを確かめます。特長 (Features) に書いた数値は、いちばん良い条件での値のことが多いので、あとで表の条件と見比べます。",
        checks: &["型番と、何をする部品かを確かめた", "電源電圧の範囲と、パッケージ (足の形) を確かめた"],
        terms: &["features", "description", "特長", "特徴", "概要"],
    },
    ReadStep {
        title: "2. 絶対最大定格 (これを超えると壊れる)",
        detail: "一瞬でも超えると壊れたり、寿命が縮んだりする限界です。「使ってよい値」ではありません。電源電圧・入力の電圧・電流・損失 (電力)・温度を見て、ふだんはこれより十分低く (2〜3 割以上の余裕を見て) 使います。入力の電圧は「電源 + 0.3V」のように電源に対して決まっていることが多いので、電源が入っていないときに入力に電圧をかけないようにも気をつけます。",
        checks: &["電源電圧の最大値を見た", "入力のピンにかけてよい電圧の範囲を見た", "最大の電流と損失 (電力) を見た"],
        terms: &["absolute maximum ratings", "absolute maximum rating", "maximum ratings", "絶対最大定格", "絶対最大"],
    },
    ReadStep {
        title: "3. 推奨動作条件 (ちゃんと動く範囲)",
        detail: "メーカーが「この範囲なら表どおりに動く」と決めた範囲です。電源電圧・入力の範囲・周囲の温度は、ここに入るように設計します。絶対最大定格との間は、壊れはしないけれど、性能は保証されない範囲です。",
        checks: &["推奨の電源電圧の範囲に、自分の電源が入っている", "動作温度の範囲を確かめた"],
        terms: &["recommended operating conditions", "recommended operating", "operating conditions", "推奨動作条件", "推奨動作範囲", "動作条件"],
    },
    ReadStep {
        title: "4. 電気的特性 (min・typ・max と条件)",
        detail: "部品の性能の表です。typ は代表値 (たくさん作ったときの真ん中あたり) で、保証ではありません。どの部品でも守られるのは min と max なので、設計は min・max で余裕を見ます (空欄は「決めていない」)。同じ行の「条件」(電源電圧・温度・負荷・周波数) が違うと値も変わるので、自分の使い方に近い条件の行を読みます。表の上に、全部の行に共通の条件 (TA = 25℃、VCC = 5V など) が書いてあることも多いです。",
        checks: &["typ だけでなく、min か max の値も見た", "その値の条件 (電源電圧・温度・負荷) が、自分の使い方に近いことを確かめた", "表の上の共通の条件 (温度など) を見た"],
        terms: &["electrical characteristics", "電気的特性", "電気特性"],
    },
    ReadStep {
        title: "5. 特性のグラフ (条件による変わり方)",
        detail: "表の 1 点だけではわからない、電圧・電流・温度・周波数による変わり方です。グラフのそばに書いた条件 (TA = 25℃ など) と、ほとんどが typ (代表値) であることに気をつけます。軸が対数のこともあります。circuitCad の「グラフから点を読む」で、曲線の値を読めます。",
        checks: &["グラフの条件 (温度・電源電圧) を見た", "縦軸・横軸の単位と、対数かどうかを見た"],
        terms: &["typical characteristics", "typical performance characteristics", "typical performance", "characteristic curves", "代表特性", "特性図", "特性曲線", "代表的な特性"],
    },
    ReadStep {
        title: "6. ピン配置 (番号と名前)",
        detail: "ピンの番号と名前、それぞれの働きの表です。図が上から見た図 (Top View) か、下から見た図 (Bottom View) かを必ず確かめます。NC (どこにもつながっていない)、裏の放熱パッド (GND につなぐか、つながないか) の扱いも書いてあります。",
        checks: &["上から見た図か下から見た図かを確かめた", "電源と GND のピンを見つけた", "NC や放熱パッドの扱いを見た"],
        terms: &["pin configuration", "pin assignment", "pin functions", "pin description", "pinout", "ピン配置", "端子配列", "ピン構成", "端子機能", "ピン説明"],
    },
    ReadStep {
        title: "7. 応用回路と推奨の部品",
        detail: "メーカーがすすめる使い方の回路です。入力・出力のコンデンサの容量や種類、抵抗の値の決め方 (式) が書いてあります。はじめは、この回路をそのまままねると確実です。コンデンサは、できるだけピンの近くに置くように書いてあることも多いです。",
        checks: &["応用回路を見つけた", "推奨のコンデンサの容量と種類を確かめた"],
        terms: &["typical application", "application information", "application circuit", "applications", "応用回路", "アプリケーション", "代表的な応用", "使用例"],
    },
    ReadStep {
        title: "8. 外形寸法とランドパターン",
        detail: "部品の大きさと、基板に作るパッドの推奨寸法です。単位 (mm かインチか)、どの数が最大・最小か、ピン 1 の印の位置を確かめます。基板を作るときは、ここの推奨のパッドを使うと、はんだ付けがうまくいきます。",
        checks: &["寸法の単位を確かめた", "ピン 1 の印の位置を確かめた", "推奨のパッド (ランド) の寸法を見た"],
        terms: &["package outline", "package dimensions", "mechanical data", "outline dimensions", "land pattern", "recommended footprint", "外形寸法", "外形図", "パッケージ寸法", "ランドパターン", "推奨ランド"],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_step_has_words_to_find_and_things_to_check() {
        for s in STEPS {
            assert!(!s.terms.is_empty() && !s.checks.is_empty(), "{}", s.title);
            assert!(s.detail.ends_with('。'), "{}", s.title);
            assert!(s.terms.iter().all(|t| datasheet::Query::new(t).is_some()), "{}: a word that searches nothing", s.title);
        }
    }
}
