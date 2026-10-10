//! The "操作ガイド" window: every mouse / key operation in one readable place, so the editors do
//! not need lines of tiny help text. Opened from the ヘルプ menu, with F1, or from the small
//! "操作" buttons next to a panel.

use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelpTab {
    Schematic,
    Waveform,
    Board,
    View3d,
}

const TABS: &[(HelpTab, &str)] = &[
    (HelpTab::Schematic, "回路図"),
    (HelpTab::Waveform, "波形"),
    (HelpTab::Board, "基板"),
    (HelpTab::View3d, "3D表示"),
];

/// (operation, what it does) rows of one page, in groups with a heading.
type Page = &'static [(&'static str, &'static [(&'static str, &'static str)])];

const SCHEMATIC: Page = &[
    (
        "部品と配線",
        &[
            ("左クリック", "選んでいる部品を置く / 配線の点を置く (斜めは自動で L 字になります)"),
            ("ダブルクリック・右クリック", "描いている配線を確定します"),
            ("右クリック (何も描いていないとき)", "選択ツールに戻ります。選択ツールでは部品の上で右クリックすると、その部品の設定の小窓が開きます"),
            ("R", "置く前・選んだ部品を 90° 回転します"),
            ("Delete", "選んだものを消します"),
            ("Ctrl+Z / Ctrl+Y", "元に戻す / やり直し (値の変更も戻せます)"),
        ],
    ),
    (
        "選ぶ",
        &[
            ("左ドラッグ (選択ツール)", "完全に囲んだものだけを選びます"),
            ("右ドラッグ (選択ツール)", "触れたものも選びます。Shift を押しながらで、いまの選択に追加します"),
            ("中ドラッグ", "画面の移動 (配線中などは右ドラッグでも可)"),
        ],
    ),
    (
        "画面",
        &[
            ("ホイール", "拡大縮小"),
            ("F", "回路全体が見えるように表示します"),
            ("Esc", "選択を外す / 配線や「回路図で選ぶ」をやめる"),
        ],
    ),
    (
        "部品の値",
        &[
            ("値のスライダー", "選んだ部品 (抵抗・コンデンサ・コイル) の値を動かします。「幅」で動かせる範囲を変えられます"),
            ("「自動で再計算」", "オンにすると、値を動かすたびに開いている解析をやり直します。計算できない値では、直前の結果が残ります"),
        ],
    ),
];

const WAVEFORM: Page = &[
    (
        "波形の見方",
        &[
            ("ドラッグ", "波形を動かします (横も縦も)"),
            ("ホイール", "時間軸 (横軸) の拡大縮小。Ctrl を押しながらで縦軸"),
            ("縦軸の数字の上でホイール", "縦軸だけを拡大縮小します (ポインタの位置が中心です)"),
            ("横軸の数字の上でホイール", "横軸だけを拡大縮小します"),
            ("ダブルクリック", "全体表示に戻し、測定カーソルを消します"),
        ],
    ),
    (
        "測る",
        &[
            ("クリック", "測定カーソル A、次に B を置きます。2 点の差 (Δt) と周期、各波形の値が出ます。以降のクリックは近いほうのカーソルを動かします"),
            ("波形にポインタを置く", "その位置の値が、波形の色で表示されます"),
            ("左のパネルの数値", "平均・最小・最大・振幅・周波数・デューティ比は、いま表示している範囲の値です。ズームすると、見ている部分だけの値になります"),
        ],
    ),
    (
        "範囲の欄と「自動」",
        &[
            ("開始 〜 終了 の欄", "表示範囲を数字で入れます (10m, 2u など)。Enter で反映します"),
            ("「全体」", "横軸を最初の範囲に戻します"),
            ("「自動」", "縦軸をいま見えている範囲に合わせます。手で動かすと切れます"),
        ],
    ),
    (
        "信号を選ぶ",
        &[
            ("左の一覧のチェック", "見たい信号を選びます。電圧は上、電流は下の別の枠に出ます"),
            ("「回路図で選ぶ」", "押してから回路図をクリックします。ピンや配線 = その点の電圧、部品の本体 (または Shift+ピン) = その部品の電流を追加します。Esc か右クリックで終わります"),
        ],
    ),
    (
        "AC解析 (ボード線図)",
        &[
            ("ゲインの基準", "0 dB = 1 V (または 1 A)。入力の電源を「AC 1」にすると、出力がそのまま伝達関数になります"),
        ],
    ),
];

const BOARD: Page = &[
    (
        "配置・配線",
        &[
            ("左クリック", "ツールに応じて、部品や配線の点を置く・選ぶ"),
            ("右クリック", "描いているものを確定します。描いていないときは選択ツールに戻ります"),
            ("Enter", "外形やゾーンの多角形を確定します"),
            ("Space", "配線の折れ曲がる向きを切り替えます"),
            ("V", "配線中に押すと、カーソルまで引いてビアを置き、反対の面で続けます"),
            ("L", "作業する層 (表面 / 裏面) を切り替えます"),
        ],
    ),
    (
        "選ぶ・動かす",
        &[
            ("左ドラッグ (選択ツール)", "完全に囲んだものを選びます"),
            ("右ドラッグ (選択ツール)", "触れたものも選びます。Shift で追加"),
            ("ドラッグ (選んだ部品)", "部品を動かします。複数選んでいれば、まとめて動きます"),
            ("中ドラッグ", "画面の移動 (他のツールでは右ドラッグでも可)"),
            ("R", "選んだ部品を回転します"),
            ("B", "選んだ部品を表と裏で反転します"),
            ("Delete", "選んだものを消します (部品は消せません)"),
            ("Esc", "選択や、描いているものをやめます"),
        ],
    ),
    (
        "画面と編集",
        &[
            ("ホイール", "拡大縮小"),
            ("F", "基板全体が見えるように表示します"),
            ("Ctrl+Z / Ctrl+Y", "元に戻す / やり直し"),
        ],
    ),
];

const VIEW3D: Page = &[(
    "3Dで見る",
    &[
        ("左ドラッグ", "基板を回します"),
        ("右ドラッグ (または中ドラッグ)", "基板を動かします"),
        ("ホイール", "拡大縮小"),
        ("ダブルクリック", "初期の向きに戻します"),
        ("部品にポインタ", "部品の名前を表示します"),
        ("部品をクリック", "基板のエディタでその部品が選ばれます"),
    ],
)];

fn page(tab: HelpTab) -> Page {
    match tab {
        HelpTab::Schematic => SCHEMATIC,
        HelpTab::Waveform => WAVEFORM,
        HelpTab::Board => BOARD,
        HelpTab::View3d => VIEW3D,
    }
}

/// State of the window.
#[derive(Default)]
pub struct Help {
    pub open: bool,
    tab: Option<HelpTab>,
}

impl Help {
    pub fn show(&mut self, tab: HelpTab) {
        self.open = true;
        self.tab = Some(tab);
    }

    pub fn window(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = true;
        let current = self.tab.unwrap_or(HelpTab::Schematic);
        let mut next = current;
        egui::Window::new("操作ガイド").open(&mut open).default_width(640.0).default_height(560.0).vscroll(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (t, name) in TABS {
                    ui.selectable_value(&mut next, *t, *name);
                }
                ui.label("(F1 でいつでも開けます)");
            });
            ui.separator();
            for (heading, rows) in page(current) {
                ui.add_space(8.0);
                ui.heading(*heading);
                egui::Grid::new(("help", current as u8, *heading)).num_columns(2).min_col_width(210.0).spacing([16.0, 8.0]).striped(true).show(ui, |ui| {
                    for (op, what) in *rows {
                        ui.label(egui::RichText::new(*op).strong());
                        ui.add(egui::Label::new(*what).wrap());
                        ui.end_row();
                    }
                });
            }
        });
        self.tab = Some(next);
        if !open {
            self.open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_has_rows_with_both_columns_filled() {
        for (t, name) in TABS {
            let p = page(*t);
            assert!(!p.is_empty(), "{name}");
            for (heading, rows) in p {
                assert!(!heading.is_empty() && !rows.is_empty(), "{name}: {heading}");
                for (op, what) in *rows {
                    assert!(!op.is_empty() && !what.is_empty(), "{name}: {heading}: {op}");
                }
            }
        }
    }
}
