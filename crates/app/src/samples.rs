//! Sample circuits built into the program, for "ファイル → サンプル回路を開く": a beginner can try a finished
//! circuit at once, without finding the `docs/examples` folder. They are the same files as in `docs/examples`.

pub struct Sample {
    pub title: &'static str,
    /// One or two sentences: what the circuit shows, and what to press next.
    pub about: &'static str,
    pub json: &'static str,
}

macro_rules! sample {
    ($title:expr, $about:expr, $file:expr) => {
        Sample { title: $title, about: $about, json: include_str!(concat!("../../../docs/examples/", $file, ".schjson")) }
    };
}

/// Roughly from the easiest to the most involved.
pub const SAMPLES: &[Sample] = &[
    sample!("LED と電流制限抵抗", "電池・抵抗・LED の一番かんたんな回路。「解析」タブの「DC動作点を解析」で、電流が出ます", "led"),
    sample!("12V から 5V を作って LED を光らせる", "三端子レギュレータ (LM7805) の回路。「DC動作点を解析」で、各点の電圧と電流が回路図に重なります", "regulator_led"),
    sample!("電源と負荷を2枚のシートに分ける", "上の「12V から 5V」と同じ回路を、電源のシートと負荷のシートに分けたもの。シートが違っても、同じ名前のネットラベル (5V) は同じ配線です。上のタブで切り替え、「DC動作点を解析」で1枚のときと同じ答えを確かめます", "two_sheets"),
    sample!("RC ローパスフィルタ", "抵抗とコンデンサで高い周波数を通しにくくする回路。「AC解析を実行」で、ボード線図が出ます", "rc_lowpass"),
    sample!("RLC のパルス応答", "パルスを入れると波形が振動して落ち着く回路。「過渡解析を実行」で、波形が出ます", "rlc_pulse"),
    sample!("ツェナーで電圧を一定にする", "もっとも簡単な安定化。入力を変えても出力があまり動きません", "zener_regulator"),
    sample!("ブリッジ整流 (交流を直流にする)", "ダイオード 4 個と平滑コンデンサ。「過渡解析を実行」で、リップルが見えます", "bridge_rectifier"),
    sample!("3相電源と Y 結線の負荷", "3相電源 (A・B・C が 120° ずつずれた交流) に抵抗を3つつなぎます。「過渡解析を実行」で、3つの波形のずれが見えます", "three_phase_wye"),
    sample!("3相ブリッジ整流", "3相電源をダイオード 6 個で直流にします。単相のブリッジ整流より、リップルがずっと小さくなります。「過渡解析を実行」で、V(VDC) がほぼ平らな直流になります", "three_phase_rectifier"),
    sample!("エミッタフォロワ (電流を増やす)", "トランジスタで、電圧はそのまま流せる電流を増やす回路です", "emitter_follower"),
    sample!("オペアンプで信号を大きくする", "非反転増幅。抵抗の比でゲインが決まります", "opamp_amp"),
    sample!("直列レギュレータ (オペアンプ + 基準電圧)", "出力を測って、ずれたら直す (帰還) 安定化電源。電圧安定化の本質です", "series_regulator"),
    sample!("保護つきの電源 (逆接続ダイオード + 78xx)", "電源を逆につないでも壊れにくくする回路です", "power_supply"),
    sample!("MOSFET スイッチ", "小さな信号で大きな電流を入り切りする回路です", "mosfet_switch"),
    sample!("NE555 の点滅回路", "タイマー IC で LED を点滅させます。「過渡解析を実行」で、充放電の波形が見えます", "ne555_blinker"),
    sample!("降圧コンバータ (スイッチング電源)", "熱にしないで電圧を下げる電源。スイッチの波形とリップルを見られます", "buck_converter"),
    sample!("総合デモ (電源 + 555 + カウンタ + 温度アラーム + アンプ)", "部品 41 個 の大きな回路。「過渡解析を実行」で、時計 (CLK) と 1/2・1/4 に割ったカウンタの波形が見えます。計算に 数十秒かかります", "mixed_signal"),
    sample!("電源ラインのインピーダンス (バイパスコンデンサの効き)", "ケーブルの先の電源ラインに、電解・10µF・100nF のコンデンサをつけた回路。「解析」タブの「インピーダンス」で、ネットに RAIL と入れて実行すると、谷 (コンデンサがよく効く周波数) と山 (電圧が揺れやすい周波数) が出ます", "rail_impedance"),
    sample!("アンプのひずみ (出力が頭打ちになる)", "オペアンプの増幅回路 (ゲイン 11) に 0.4V の正弦波を入れると、出力が電源の範囲を超えて頭が削られます。「過渡解析を実行」→ 波形で「ひずみ率 (THD)」を開くと約 16% と出ます。V1 を 0.1V にすると、ほぼ 0% になります", "amp_clipping"),
    sample!("充電 IC (TP5100)", "電池を充電する IC の回路です。ライブラリ部品の使い方の例になります", "tp5100_charger"),
    sample!("Arduino 互換ボード (最小構成)", "ATmega328P + 16MHz 水晶 + シリアル書き込み用ヘッダ + 13番ピンの LED。マイコンの中はシミュレーションできませんが、電源・リセット・水晶の周りは確かめられます。そのまま「基板」タブで部品を取り込み、配置・配線・発注データまで進められます", "arduino_minimal"),
    sample!("Arduino Mega 互換ボード (USB つき、シールドなし)", "ATmega2560 (TQFP-100) と、USB-シリアル変換 CH340G、USB コネクタ、水晶 2 つ、リセット、ICSP、ポートごとの引き出しヘッダ。部品 36 個の大きな回路です。マイコンの中はシミュレーションできませんが、回路図 (ERC) と、基板づくり (配置・配線) の練習に使えます。基板は 100 ピンの IC のため、自動配線では通らない線が残ります (手で引くか、配置を工夫します)", "arduino_mega_usb"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sample_is_a_circuit_with_parts_and_titles_are_unique() {
        let mut titles = std::collections::HashSet::new();
        for s in SAMPLES {
            assert!(titles.insert(s.title), "duplicate title {}", s.title);
            let sch: sch_core::Schematic = serde_json::from_str(s.json).unwrap_or_else(|e| panic!("{}: {e}", s.title));
            assert!(!sch.components.is_empty(), "{} is empty", s.title);
        }
    }

    #[test]
    fn a_sample_can_be_opened_and_is_not_saved_over_the_original() {
        let mut ed = crate::schematic::SchEditor::default();
        assert!(ed.open_sample(SAMPLES[1].title, SAMPLES[1].json));
        assert!(!ed.sch.components.is_empty());
        assert!(ed.status.contains(SAMPLES[1].title), "{}", ed.status);
        assert!(!ed.open_sample("broken", "{ not json"));
    }
}
