//! The sample circuits the browser version carries (built in: there are no files to open in the browser).

use sch_core::Schematic;

pub struct Sample {
    /// Japanese, shown through `i18n::tr`.
    pub title: &'static str,
    pub about: &'static str,
    json: &'static str,
}

impl Sample {
    pub fn schematic(&self) -> Result<Schematic, String> {
        serde_json::from_str(self.json).map_err(|e| format!("{}: {e}", self.title))
    }
}

macro_rules! sample {
    ($title:literal, $about:literal, $file:literal) => {
        Sample { title: $title, about: $about, json: include_str!(concat!("../../../docs/examples/", $file)) }
    };
}

pub const SAMPLES: &[Sample] = &[
    sample!("オームの法則", "電源と抵抗 1 本。電圧 ÷ 抵抗 = 電流", "intro_ohm.schjson"),
    sample!("LED を光らせる", "LED と電流を決める抵抗", "led_basics.schjson"),
    sample!("ダイオード", "順方向に流すと約 0.6 V 下がる", "diode_basics.schjson"),
    sample!("ツェナーダイオード", "決まった電圧で止める", "zener_basics.schjson"),
    sample!("トランジスタ", "小さいベース電流で大きいコレクタ電流", "transistor_basics.schjson"),
    sample!("分圧", "抵抗 2 本で電圧を分ける", "divider_tolerance.schjson"),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every sample opens and has a DC answer: the browser shows nothing else for it yet.
    #[test]
    fn every_sample_opens_and_solves() {
        for s in SAMPLES {
            let sch = s.schematic().unwrap();
            let circuit = sim_core::from_schematic(&sch).unwrap_or_else(|e| panic!("{}: {e:?}", s.title));
            sim_core::dc_op(&circuit).unwrap_or_else(|e| panic!("{}: {e}", s.title));
        }
    }
}
