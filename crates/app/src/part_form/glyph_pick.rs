//! The "記号の形" choice: which symbol a part is drawn with (shared by the part form and the KiCad import window).

use super::*;

const GATE_TIP: &str = "ゲートが複数入った IC。箱とピン名はそのまま、中央にゲートの記号を1つ描きます";
const TWO_PIN_TIP: &str = "左右に1本ずつの2ピン部品だけ";

/// (glyph, label, what it is and which pin names it needs).
pub const GLYPH_CHOICES: &[(&str, &str, &str)] = &[
    ("", "自動 (名前とピンから決める)", "部品名とピン名から、合う記号を選びます。同梱の部品はこれで記号になります。当てはまらなければ箱です"),
    ("box", "箱 (ピン名つきの四角)", "名前やピンに関わらず、四角の箱にピン名を並べます"),
    ("diode", "ダイオード", "整流・スイッチング。ピン名 A K"),
    ("schottky", "ショットキーダイオード", "棒の両端が S 形。ピン名 A K"),
    ("zener", "ツェナーダイオード", "棒の両端が Z 形。ピン名 A K"),
    ("tvs", "TVS ダイオード", "ピン名 A K"),
    ("led", "LED", "ピン名 A K"),
    ("photodiode", "フォトダイオード", "ピン名 A K (光は L)"),
    ("npn", "NPN トランジスタ", "ピン名 B C E (左・上・下に1本ずつ)"),
    ("pnp", "PNP トランジスタ", "ピン名 B C E (左・上・下に1本ずつ)"),
    ("nmos", "Nch MOSFET", "ピン名 G D S (左・上・下に1本ずつ)"),
    ("pmos", "Pch MOSFET", "ピン名 G D S (左・上・下に1本ずつ)"),
    ("opamp", "オペアンプ (三角形)", "ピン名 IN+ IN- OUT (電源は上下)"),
    ("comparator", "コンパレータ (三角形+段の印)", "ピン名 IN+ IN- OUT (電源は上下)"),
    ("regulator", "3端子レギュレータ (箱+DC記号)", "ピンの形は箱のまま。ピン名 IN OUT GND なら自動でもこの記号になります"),
    ("gate_and", "ゲート AND (箱の中に記号)", GATE_TIP),
    ("gate_or", "ゲート OR (箱の中に記号)", GATE_TIP),
    ("gate_not", "ゲート NOT (箱の中に記号)", GATE_TIP),
    ("gate_nand", "ゲート NAND (箱の中に記号)", GATE_TIP),
    ("gate_nor", "ゲート NOR (箱の中に記号)", GATE_TIP),
    ("gate_xor", "ゲート XOR (箱の中に記号)", GATE_TIP),
    ("gate_schmitt", "ゲート シュミットNOT (箱の中に記号)", GATE_TIP),
    ("gate_buffer", "ゲート バッファ (箱の中に記号)", GATE_TIP),
    ("ff_d", "フリップフロップ (箱の中に記号)", "D型フリップフロップなどの IC。箱とピン名はそのまま、中央にクロックの三角つきの小さな箱を描きます"),
    ("counter", "カウンタ (箱の中に記号)", "カウンタ IC。箱とピン名はそのまま、中央にクロックの三角と階段を描きます"),
    ("shift_reg", "シフトレジスタ (箱の中に記号)", "シフトレジスタ IC。箱とピン名はそのまま、中央にクロックの三角と矢印を描きます"),
    ("shunt_reg", "シャントレギュレータ (TL431形)", "ピン名 CATHODE REF ANODE"),
    ("transformer", "トランス", "ピン名 P1 P2 S1 S2"),
    ("battery", "電池", "ピン名 + -"),
    ("crystal", "水晶振動子", "ピン名 X1 X2 (2端子なら 1 2)"),
    ("ldr", "光センサ (LDR)", "ピン名 A B (光は L)"),
    ("motor", "モーター", "ピン名 + - (回転数は SPD)"),
    ("capacitor", "コンデンサ (2ピン専用)", TWO_PIN_TIP),
    ("electrolytic", "電解コンデンサ (2ピン専用)", TWO_PIN_TIP),
    ("resistor", "抵抗 (2ピン専用)", TWO_PIN_TIP),
    ("inductor", "コイル (2ピン専用)", TWO_PIN_TIP),
];

fn label_of(glyph: &str) -> &'static str {
    GLYPH_CHOICES.iter().find(|c| c.0 == glyph).map_or("(不明な記号)", |c| c.1)
}

/// A drop-down for `glyph`. `sym` is the part the symbol is for: a note says when its pins do not suit the choice.
pub fn glyph_combo(ui: &mut egui::Ui, id: &str, glyph: &mut String, sym: Option<&sch_core::CustomSymbol>) {
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(id)
            .selected_text(label_of(glyph))
            .width(260.0)
            .show_ui(ui, |ui| {
                for (g, label, tip) in GLYPH_CHOICES {
                    ui.selectable_value(glyph, g.to_string(), *label).on_hover_text(*tip);
                }
            })
            .response
            .on_hover_text(i18n::tr("回路図で、この部品を何の記号で描くか。ピンの位置は変わらないので、配線はそのままです。「自動」のままで合っていれば、選ぶ必要はありません"));
        if let Some(sym) = sym.filter(|s| !crate::schematic::glyph_fits(glyph, s)) {
            let need = GLYPH_CHOICES.iter().find(|c| c.0 == glyph.as_str()).map_or("", |c| c.2);
            ui.colored_label(crate::theme::WARN, i18n::tr("ピンが合わないので、箱で描きます"))
                .on_hover_text(i18n::trf!("この記号が使うピンの名前・数と合っていません ({need})。ピン名を直すか、別の記号を選んでください (部品: {})", sym.name));
        }
    });
}
