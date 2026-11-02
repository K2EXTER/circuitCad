//! The headings a data sheet has, in English and Japanese, to jump to the page that has the table or drawing wanted.

use crate::text::Query;

/// A part of a data sheet that is read when entering a part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heading {
    PinConfig,
    LandPattern,
    Package,
    AbsMax,
    Electrical,
}

pub const HEADINGS: &[Heading] = &[Heading::PinConfig, Heading::LandPattern, Heading::Package, Heading::AbsMax, Heading::Electrical];

impl Heading {
    /// The name shown on the button.
    pub fn label(self) -> &'static str {
        match self {
            Heading::PinConfig => "ピン配置",
            Heading::LandPattern => "ランドパターン",
            Heading::Package => "外形寸法",
            Heading::AbsMax => "絶対最大定格",
            Heading::Electrical => "電気的特性",
        }
    }

    /// What it is for, for the hover text.
    pub fn help(self) -> &'static str {
        match self {
            Heading::PinConfig => "ピンの番号と名前の表 (③ ピン に使います)",
            Heading::LandPattern => "基板のパッドの推奨寸法 (④ 部品の形 に使います)",
            Heading::Package => "部品本体の外形寸法 (④ 部品の形 の本体・高さに使います)",
            Heading::AbsMax => "超えると壊れる電圧・電流・電力 (① 基本の定格 に使います)",
            Heading::Electrical => "条件つきの代表的な値 (① 基本の定格 に使います)",
        }
    }

    /// How data sheets write it, in English and in Japanese.
    pub fn terms(self) -> &'static [&'static str] {
        match self {
            Heading::PinConfig => &["pin configuration", "pin assignment", "pin functions", "pin description", "pinout", "ピン配置", "端子配列", "ピン構成", "端子構成", "ピン機能", "端子機能", "ピン説明", "端子説明"],
            Heading::LandPattern => &["land pattern", "recommended footprint", "recommended pad", "recommended solder", "ランドパターン", "ランド パターン", "ランド・パターン", "推奨パッド", "推奨ランド", "実装パターン", "パッド寸法", "推奨はんだ"],
            Heading::Package => &["package outline", "package dimensions", "mechanical data", "outline dimensions", "外形寸法", "外形図", "パッケージ寸法", "パッケージ外形", "パッケージ情報", "メカニカル", "機械的データ"],
            Heading::AbsMax => &["absolute maximum ratings", "absolute maximum rating", "絶対最大定格", "絶対最大"],
            Heading::Electrical => &["electrical characteristics", "電気的特性", "電気特性"],
        }
    }

    /// The searches that find it (any of them).
    pub fn queries(self) -> Vec<Query> {
        self.terms().iter().filter_map(|t| Query::new(t)).collect()
    }
}
