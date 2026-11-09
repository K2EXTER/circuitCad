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
    sample!("同じ回路を3回使う (階層: RC を3段重ねる)", "RC ローパスの「ステージ」を別のシートに1つだけ描き、3つの記号 (CH1〜CH3) で使い回します。「RCステージ」のタブを開くと中身 (R と C、入口と出口のポート) が見えます。「解析」タブの「AC解析を実行」で、3段重ねた特性が出ます (1段より急に落ちます)。シートの中を直すと、3つ全部に効きます", "hierarchy_rc_stages"),
    sample!("RC ローパスフィルタ", "抵抗とコンデンサで高い周波数を通しにくくする回路。「AC解析を実行」で、ボード線図が出ます", "rc_lowpass"),
    sample!("実測と比べる (RC ローパス)", "上の RC ローパスに、作った基板で測った値の例を入れたもの (値は説明のために作った例です)。「AC解析を実行」すると測った点がボード線図に重なり、「過渡解析を実行」するとオシロの波形が重なります (波形の左の「実測を重ねる」)。「解析」タブのスペック表には、2 枚分の -3dB点の実測が入っています", "rc_lowpass_measured"),
    sample!("RLC のパルス応答", "パルスを入れると波形が振動して落ち着く回路。「過渡解析を実行」で、波形が出ます", "rlc_pulse"),
    sample!("ツェナーで電圧を一定にする", "もっとも簡単な安定化。入力を変えても出力があまり動きません", "zener_regulator"),
    sample!("電源の揺れを減らす (RC フィルタ + ツェナー)", "9V の電源を、RC フィルタ (R1・C1) とツェナー (D1) で 5.1V にします。「解析」タブの「電源除去比 (PSRR)」で、揺らす電源を V1、見る点を VOUT にして計算すると、電源の揺れが VOUT にどれだけ届くかが周波数ごとに出ます (見る点を FILT にすると、フィルタだけの効き)。C1 や C2 を小さくして計算し直すと、揺れが届きやすくなるのが分かります", "psrr_filter"),
    sample!("負荷の急変 (電流源で負荷を増やす)", "ツェナーで 5.1V を作る回路に、電流源 I1 (負荷) とコンデンサ C1 を足したもの。I1 は 1ms で 0 → 10mA に増え、2ms 後に戻ります。「過渡解析を実行」して、波形の左の「負荷の急変 (落ち込みと戻り)」を開くと、電圧がどれだけ下がるか (約 77mV、ツェナーの内部抵抗の分) と、戻るまでの時間が出ます。C1 を 100u にすると、ゆっくり下がるようになりますが、落ち着く先は同じです", "load_step"),
    sample!("モーターを回す (MOSFET + 負荷のトルク)", "小型の DC モーター M1 を、MOSFET Q1 で 5ms からオンにします。D1 はフライバックダイオード。M1 の SPD の電圧が回転数 (1V = 1000rpm) で、電流源 I1 が 50ms から 1mN·m の負荷をかけます (1mA = 1mN·m)。「過渡解析を実行」すると (終了 100ms はサンプルに入っています)、起動の大きな電流と、負荷で回転が落ちて電流が増える様子が見えます。故障シミュレーションで部品 M1.CJ (軸) の「モーターの軸がロック」を選んで「連鎖故障を追う」と、Q1 が壊れ、巻線が焼ける順が出ます", "motor_driver"),
    sample!("ブリッジ整流 (交流を直流にする)", "ダイオード 4 個と平滑コンデンサ。「過渡解析を実行」で、リップルが見えます", "bridge_rectifier"),
    sample!("3相電源と Y 結線の負荷", "3相電源 (A・B・C が 120° ずつずれた交流) に抵抗を3つつなぎます。「過渡解析を実行」で、3つの波形のずれが見えます", "three_phase_wye"),
    sample!("3相ブリッジ整流", "3相電源をダイオード 6 個で直流にします。単相のブリッジ整流より、リップルがずっと小さくなります。「過渡解析を実行」で、V(VDC) がほぼ平らな直流になります", "three_phase_rectifier"),
    sample!("エミッタフォロワ (電流を増やす)", "トランジスタで、電圧はそのまま流せる電流を増やす回路です", "emitter_follower"),
    sample!("オペアンプで信号を大きくする", "非反転増幅。抵抗の比でゲインが決まります", "opamp_amp"),
    sample!("直列レギュレータ (オペアンプ + 基準電圧)", "出力を測って、ずれたら直す (帰還) 安定化電源。電圧安定化の本質です", "series_regulator"),
    sample!("保護つきの電源 (逆接続ダイオード + 78xx)", "電源を逆につないでも壊れにくくする回路です", "power_supply"),
    sample!("MOSFET スイッチ", "小さな信号で大きな電流を入り切りする回路です", "mosfet_switch"),
    sample!("NE555 の点滅回路", "タイマー IC で LED を点滅させます。「過渡解析を実行」で、充放電の波形が見えます", "ne555_blinker"),
    sample!("カウンタとデコーダ (LED が順に流れる)", "74HC161 の 4 ビットカウンタと 74HC138 のデコーダ。クロック 1kHz で数えて、LED が 1 つずつ順に光ります。「過渡解析を実行」で、QA〜QD と Y0〜Y7 の波形が見えます", "counter_decoder"),
    sample!("シフトレジスタで LED を順に光らせる", "74HC595 に 1 ビットずつデータを送ると、そのビットが QA から QH へ 1 つずつ進み、LED が順に光ります。「過渡解析を実行」で、QA〜QH の波形が見えます", "shift_register_leds"),
    sample!("カウンタと 7 セグメント表示 (数字が 0 から 9 まで進む)", "74HC161 の 4 ビットカウンタの出力を、CD4511B (BCD → 7 セグメントのデコーダ) に入れて、7 つの LED (a〜g) を光らせます。0 から 9 まで数字の形に光り、10 以上は消えます。「過渡解析を実行」で、各セグメントの波形が見えます", "counter_7seg"),
    sample!("正弦波をデジタルで作る (カウンタ → ROM → DAC → ADC)", "74HC4040 のカウンタが ROM32x8 (正弦波 1 周期の表) のアドレスを進め、出てきた 8 ビットの値を R-2R のはしごが電圧に変えます (DA 変換)。その電圧を 4 ビットの AD コンバータ ADC4_FLASH が読み取ります。「過渡解析を実行」で、階段状の正弦波 V(N7) と、AD 変換の値 V(Y0)〜V(Y3) が見えます", "dac_adc_sine"),
    sample!("バスで 8 本の信号をまとめる", "ヘッダの 8 本の信号 D0〜D7 を、太い青のバスの線 D[0..7] でまとめて、8 個の LED につなぎます。バスの線は何もつなぎません。つなぐのは各ピンの D0〜D7 のラベルで、ラベルツールで D[0..7] と書いてピンを順にクリックすると、D0 D1 … と順に付きます。バスの範囲と合わない番号があると、点検(ERC)が教えてくれます", "bus_leds"),
    sample!("セットアップ時間の違反 (データがクロックに間に合わない)", "74HC04 を6個つないで遅らせたデータが、74HC74 のクロックの 7ns 前にしか届かない回路です。「過渡解析を実行」で、足りない時間の警告が出ます。VCK の遅れ (150n) を 180n に変えてもう一度実行すると、警告が消えます", "timing_check"),
    sample!("降圧コンバータ (スイッチング電源)", "熱にしないで電圧を下げる電源。スイッチの波形とリップルを見られます", "buck_converter"),
    sample!("総合デモ (電源 + 555 + カウンタ + 温度アラーム + アンプ)", "部品 41 個 の大きな回路。「過渡解析を実行」で、時計 (CLK) と 1/2・1/4 に割ったカウンタの波形が見えます。計算に 数十秒かかります", "mixed_signal"),
    sample!("電源ラインのインピーダンス (バイパスコンデンサの効き)", "ケーブルの先の電源ラインに、電解・10µF・100nF のコンデンサをつけた回路。「解析」タブの「インピーダンス」で、ネットに RAIL と入れて実行すると、谷 (コンデンサがよく効く周波数) と山 (電圧が揺れやすい周波数) が出ます", "rail_impedance"),
    sample!("アンプのひずみ (出力が頭打ちになる)", "オペアンプの増幅回路 (ゲイン 11) に 0.4V の正弦波を入れると、出力が電源の範囲を超えて頭が削られます。「過渡解析を実行」→ 波形で「ひずみ率 (THD)」を開くと約 16% と出ます。V1 を 0.1V にすると、ほぼ 0% になります", "amp_clipping"),
    sample!("極と零 (共振する LC フィルタ)", "10Ω・1mH・1µF のローパス。「解析」タブの「極と零」で、出力に OUT・入力に IN と入れて実行すると、5.03 kHz に一対の複素数の極 (Q = 3.16) が出て、当てはめの曲線が回路の応答に重なります。抵抗を 10Ω から 100Ω にすると、極が実数の2つに分かれて山が消えます", "rlc_poles"),
    sample!("帰還の安定性 (出力にコンデンサをつけた電源)", "直列レギュレータの出力に 10µF をつけた回路。「解析」タブの「ループゲイン」で、切る部品に R3 と入れて実行すると、位相余裕がわずか約 10° と出ます。出力のコンデンサを外す (消す) と、約 90° に戻ります", "loop_cap_load"),
    sample!("充電 IC (TP5100)", "電池を充電する IC の回路です。ライブラリ部品の使い方の例になります", "tp5100_charger"),
    sample!("ATtiny85 の書き込み用ヘッダ (ISP)", "ATtiny85 と、書き込み用の 6 ピンのヘッダ (標準の AVR ISP の並び) の回路です。書き込みに要るピン (RESET・MOSI・MISO・SCK・電源・GND) がヘッダまでつながっていることを、ERC が調べます。ラベル SCK の片方の名前を変えて ERC を実行すると、どのピンが足りないか分かります", "attiny_isp"),
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

    #[test]
    fn the_measured_sample_brings_its_spec_table_and_recordings_and_is_clean() {
        let s = SAMPLES.iter().find(|s| s.json.contains("\"bench\"")).expect("a sample with bench recordings");
        let mut ed = crate::schematic::SchEditor::default();
        assert!(ed.open_sample(s.title, s.json));
        assert_eq!(ed.spec_items().len(), 1);
        assert_eq!(ed.spec_items()[0].measured, "944, 1.01k");
        let b = ed.bench_data();
        assert_eq!((b.ac.len(), b.tran.len()), (1, 1));
        assert!(b.tran[0].x.len() > 300 && b.tran[0].shift == 1e-3);
        assert!(!ed.dirty(), "opening a sample is not a change");
        // the readings agree with the simulated -3 dB point (1.0 kHz) as a 6 % miss that a ±10 % capacitor explains
        let readings = sim_core::split_readings(&ed.spec_items()[0].measured, sim_core::parse_value).unwrap();
        let r = sim_core::compare_readings(sim_core::SpecKind::AcCutoff, 1001.0, &readings, Some((910.0, 1112.0))).unwrap();
        assert_eq!(r.level, sim_core::Agreement::Close);
    }
}

#[cfg(test)]
mod hierarchy_sample_tests {
    use super::*;

    #[test]
    fn the_three_stage_sample_expands_to_three_copies_and_has_no_error() {
        let s = SAMPLES.iter().find(|s| s.json.contains("\"sub\"")).expect("a sample that uses a sheet symbol");
        let sch: sch_core::Schematic = serde_json::from_str(s.json).unwrap();
        let flat = sch.flat();
        let mut refs: Vec<&str> = flat.components.iter().filter(|c| !c.kind.is_power_symbol()).map(|c| c.refdes.as_str()).collect();
        refs.sort();
        assert_eq!(refs, ["C1_CH1", "C1_CH2", "C1_CH3", "R1_CH1", "R1_CH2", "R1_CH3", "V1"]);
        assert!(sch_core::erc(&sch).iter().all(|p| p.severity != sch_core::Severity::Error), "{:?}", sch_core::erc(&sch));
        assert!(sim_core::from_schematic(&sch).is_ok(), "the expanded circuit simulates");
    }
}
