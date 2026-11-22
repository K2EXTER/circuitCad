//! The sample circuits the browser version carries (built in: there are no files to open in the browser).

use sch_core::Schematic;

pub struct Sample {
    /// Japanese, shown through `i18n::tr`.
    pub title: &'static str,
    pub about: &'static str,
    json: &'static str,
    /// Opens running as a digital circuit (`sim_core::live_logic`): the switches are tapped.
    pub live: bool,
}

impl Sample {
    pub fn schematic(&self) -> Result<Schematic, String> {
        serde_json::from_str(self.json).map_err(|e| format!("{}: {e}", self.title))
    }
}

macro_rules! sample {
    ($title:literal, $about:literal, $file:literal) => {
        Sample { title: $title, about: $about, json: include_str!(concat!("../../../docs/examples/", $file)), live: false }
    };
    (live $title:literal, $about:literal, $file:literal) => {
        Sample { title: $title, about: $about, json: include_str!(concat!("../../../docs/examples/", $file)), live: true }
    };
}

pub const SAMPLES: &[Sample] = &[
    sample!("オームの法則", "電源と抵抗 1 本。電圧 ÷ 抵抗 = 電流", "intro_ohm.schjson"),
    sample!("LED を光らせる", "LED と電流を決める抵抗", "led_basics.schjson"),
    sample!("ダイオード", "順方向に流すと約 0.6 V 下がる", "diode_basics.schjson"),
    sample!("ツェナーダイオード", "決まった電圧で止める", "zener_basics.schjson"),
    sample!("トランジスタ", "小さいベース電流で大きいコレクタ電流", "transistor_basics.schjson"),
    sample!("分圧", "抵抗 2 本で電圧を分ける", "divider_tolerance.schjson"),
    sample!(live "半加算器 (スイッチで 0 と 1)", "SW1・SW2 をタップして A と B を切り替えると、XOR (S) と AND (C) の LED が変わり、真理値表が埋まっていきます", "half_adder.schjson"),
    sample!(live "SR ラッチ (押しボタンで覚える)", "SW1 (セット) か SW2 (リセット) を押すと、指を離しても LED がそのまま。NAND 2 つで 1 ビットを覚えます", "sr_latch.schjson"),
    sample!(live "カウンタ (クロックで数える)", "「1 クロック」を押すたびに 74HC161 が 1 つ数え、4 つの LED が 2 進数で 0〜15 を表します", "live_counter.schjson"),
    sample!(live "7 セグメントで数える", "クロックで 0〜9 を数えて 7 セグメントに出します。10 になった瞬間にクリアするので、一瞬だけのグリッチも見えます", "seg7_counter.schjson"),
    sample!(live "シフトレジスタ", "SW1 で入れる 0 か 1 を決め、「1 クロック」を押すたびに LED の並びが 1 つ隣へ動きます", "shift_register.schjson"),
    sample!(live "信号機", "クロックごとに 74HC4017 の出力が 1 つずつ進み、青 4 回・黄 1 回・赤 5 回をくり返します", "traffic_light.schjson"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every sample opens: a DC answer, or the digital circuit runs (and has something to tap or a clock).
    #[test]
    fn every_sample_opens_and_solves() {
        for s in SAMPLES {
            let sch = s.schematic().unwrap();
            let circuit = sim_core::from_schematic(&sch).unwrap_or_else(|e| panic!("{}: {e:?}", s.title));
            if s.live {
                let ll = sim_core::live_logic::LiveLogic::new(&sch, circuit, String::new());
                assert!(ll.error.is_none(), "{}: {:?}", s.title, ll.error);
                assert!(!ll.inputs.is_empty() || !ll.clocks.is_empty(), "{}: nothing to work it with", s.title);
            } else {
                sim_core::dc_op(&circuit).unwrap_or_else(|e| panic!("{}: {e}", s.title));
            }
        }
    }
}
