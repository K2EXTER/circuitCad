//! Lesson 86: the same circuit used several times (sheet symbols and ports).

use super::conditions::*;
use super::*;

/// Every part of the circuit, on the sheet being edited and on the others.
fn all_parts(e: &SchEditor) -> impl Iterator<Item = &Component> {
    e.sch.components.iter().chain(e.sch.sheets.iter().flat_map(|s| s.components.iter()))
}

/// The learner has opened the sheet inside the symbols ("RCステージ").
fn stage_sheet_opened(e: &SchEditor) -> bool {
    e.sch.sheet_count() >= 2 && e.sch.active_sheet != 0
}

/// C1 of the stage sheet is not 159n any more, and the AC run is fresh (it ran for the circuit as it is now).
fn stage_c1_changed_and_ac(e: &SchEditor) -> bool {
    all_parts(e).any(|c| c.refdes == "C1" && c.value.trim() != "159n") && ac_fresh(e)
}

/// A fourth sheet symbol is on a sheet.
fn four_symbols(e: &SchEditor) -> bool {
    all_parts(e).filter(|c| c.sub.is_some()).count() >= 4
}

pub(super) const HIERARCHY: Lesson = Lesson {
    id: "hierarchy",
    title: "86. 同じ回路を何回も使う (階層)",
    goal: "1つのシートに描いた回路を、記号 (箱) にして何回も使います。中を直すと全部に効くことと、使うたびに部品とネットの名前が別になることを確かめます",
    summary: "同じ形の回路が何個も要るとき (チャンネルの繰り返し、フィルタの段)、部品をコピーして描くと、直すときに全部を直さなければなりません。階層にすると、使い回したい回路を別のシートに1つだけ描き、出入口に「ポート」を置きます。そのシートを記号にして置くと、ポートが記号のピンになります。記号を何個置いても、中身は同じシートのコピーです。コピーの部品番号は R1_CH1・R1_CH2 のように記号の名前が付き、ネットの名前も CH1.MID・CH2.MID と、コピーごとに別になります (VCC と GND だけは全部共通です)。この回路は、RC ローパスを「RCステージ」のシートに1つ描き、3つの記号 CH1〜CH3 で3段に重ねたものです。",
    limits: &[
        "コピーは、すべて同じ回路です。チャンネルごとに部品の値を変えることはできません (3つの C1 は、いつも同じ値です)。値を変えたいときは、別のシートに分けて描きます。",
        "回路図の上に重ねる電圧・電流の表示は、最初のコピーのものだけです。ほかのコピーの値は、波形の信号名 (V(CH2.OUT) など) や、基板に取り込んだあとの部品番号で見ます。",
        "コピーの部品を基板で入れ替えた (ゲートやピンの入れ替え) とき、回路図には戻しません。1つ直すと全部のコピーに効いてしまうためで、入れ替えは基板だけに残ります。",
        "ポートの向き (入力・出力・入出力) は、記号のピンが左右のどちらに出るかを決めるだけです。向きが違う線をつないでも、電気的には何も止めません。",
    ],
    real: &[],
    circuit: include_str!("../../../../../docs/examples/hierarchy_rc_stages.schjson"),
    setup: |_| {},
    steps: &[
        Step {
            tab: PanelTab::Draw,
            title: "「RCステージ」のシートを開く",
            detail: "回路図の上のタブから「RCステージ」を開きます (回路図の CH1 の箱をダブルクリックしても開きます。戻るときは、タブの横の「親へ戻る」)。R1 と C1 が1つずつあり、左右に旗の形の「ポート」(IN・OUT) が付いています。このポートの名前が、箱のピンの名前になります。",
            done: stage_sheet_opened,
        },
        Step {
            tab: PanelTab::Analyze,
            title: "AC解析で、3段重ねた特性を見る",
            detail: "「AC解析を実行」を押します。1段の RC (R=1k、C=159n) のカットオフは約 1kHz ですが、3段重ねると約 190Hz まで下がり、その先は1段より急に落ちます (段をつなぐと、次の段が前の段の負荷になるためです)。ボード線図の信号は V(VOUT) です。",
            done: ac_done,
        },
        Step {
            tab: PanelTab::Draw,
            title: "C1 を 1.59u にして、もう一度解析する",
            detail: "「RCステージ」のシートで C1 を選び、値を 1.59u (10倍) にして「AC解析を実行」を押します。C1 を直したのは1か所だけですが、3つの箱ぜんぶが変わるので、カットオフは約 1/10 に下がります。同じ形の回路は、1か所を直せば全部に効くのが、階層の良いところです。",
            done: stage_c1_changed_and_ac,
        },
        Step {
            tab: PanelTab::Draw,
            title: "箱を1つ増やして、4段にする",
            detail: "回路図のシート (タブの「シート1」) に戻り、「RCステージ」のタブを右クリックして「いまのシートに、記号として置く」を選びます。CH4 の箱が置かれます。続けて、CH3 の OUT につないだラベル VOUT を CH4 の OUT に移し、CH3 の OUT と CH4 の IN を、ラベル S3 で結べば、4段になります (部品は、R1_CH4 と C1_CH4 が自動で増えます)。",
            done: four_symbols,
        },
    ],
};
