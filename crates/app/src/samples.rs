//! Sample circuits built into the program, for "ファイル → サンプル回路を開く": a beginner can try a finished
//! circuit at once, without finding the `docs/examples` folder. They are the same files as in `docs/examples`.

pub struct Sample {
    pub title: &'static str,
    /// One or two sentences: what the circuit shows, and what to press next.
    pub about: &'static str,
    pub json: &'static str,
    /// Opens in "デジタル回路を動かしながら見る" (`schematic/live_logic.rs`).
    pub live: bool,
}

macro_rules! sample {
    ($title:expr, $about:expr, $file:expr) => {
        Sample { title: $title, about: $about, json: include_str!(concat!("../../../docs/examples/", $file, ".schjson")), live: false }
    };
    (live $title:expr, $about:expr, $file:expr) => {
        Sample { title: $title, about: $about, json: include_str!(concat!("../../../docs/examples/", $file, ".schjson")), live: true }
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
    sample!("セラミックコンデンサの DC バイアス (同じ 10µF でも容量が減る)", "同じ積層セラミックコンデンサ (村田 GRM188R61C106MA73、10µF・16V・X5R) を使った RC ローパスを 2 つ並べ、片方は直流 0V、もう片方は 12V をかけています。「AC解析を実行」して OUT0 と OUT12 を比べると、12V 側は -3dB 点が約 160Hz から約 1.1kHz に上がります。直流電圧で容量が 1/7 ほどに減るためです (データシートの DC バイアス特性)。耐圧に余裕のある品や大きいサイズを選ぶ理由です", "mlcc_dc_bias"),
    sample!("トランジスタが自分の熱で電流を増やす (自己発熱)", "同じ 2N3904 (SOT-23) を 2 つ、約 5mA で使っています。Q1 はベースの電圧を分圧で決めただけ、Q2 はエミッタに 100Ω を入れています。「解析」タブの「自己発熱を計算」を押すと、Q1 は 33K ほど温まり、温まると同じベースの電圧でも電流が増える (1℃ で約 2mV 少ない電圧で流れる) ので、コレクタの電流が 5.5mA から 12mA に増えます (C1 が 10.2V → 7.9V)。Q2 は電流が増えるとエミッタの電圧が上がってベースを押し戻すので、4% ほどしか変わりません。R2 を大きくすると、Q1 はさらに熱くなります", "bjt_self_heat"),
    sample!("レギュレータに放熱板を付ける (熱抵抗を接合部・ケースに分ける)", "同じ LM7805 (TO-220) を 2 つ、どちらも 12V から 5V を作り 0.5A を流します (7V × 0.5A = 3.5W が熱になります)。「解析」タブの「自己発熱を計算」で、放熱板のない U1 は 50 K/W で 177K も温まり、接合部の上限を超えます。U2 は放熱板 (10 K/W) 付きで 48K です。熱は 接合部 → ケース → 空気 と流れ、放熱板はケースから先にしか効かないので、θjc (約 5 K/W) の分は残ります。表の θ の横の「放熱」で θjc と放熱板を変えられます", "regulator_heatsink"),
    sample!("RLC のパルス応答", "パルスを入れると波形が振動して落ち着く回路。「過渡解析を実行」で、波形が出ます", "rlc_pulse"),
    sample!("ツェナーで電圧を一定にする", "もっとも簡単な安定化。入力を変えても出力があまり動きません", "zener_regulator"),
    sample!("電源の揺れを減らす (RC フィルタ + ツェナー)", "9V の電源を、RC フィルタ (R1・C1) とツェナー (D1) で 5.1V にします。「解析」タブの「電源除去比 (PSRR)」で、揺らす電源を V1、見る点を VOUT にして計算すると、電源の揺れが VOUT にどれだけ届くかが周波数ごとに出ます (見る点を FILT にすると、フィルタだけの効き)。C1 や C2 を小さくして計算し直すと、揺れが届きやすくなるのが分かります", "psrr_filter"),
    sample!("差動アンプの同相除去比 (CMRR)", "オペアンプ U1 と 4 本の抵抗で、入力 INP と INN の差を 10 倍にする差動アンプ。「解析」タブの「同相除去比 (CMRR)」で出力を OUT にして計算すると、2 つの入力に共通の揺れをどれだけ打ち消すかが周波数ごとに出ます。抵抗がそろっていれば計算ではほぼ無限ですが、「抵抗の誤差 %」を 1 にすると、1% の抵抗では 50〜70 dB ほどまで下がるのが分かります。R4 を 101k にして計算し直すと、1 本のずれの効き方も見られます", "cmrr_diffamp"),
    sample!("負荷の急変 (電流源で負荷を増やす)", "ツェナーで 5.1V を作る回路に、電流源 I1 (負荷) とコンデンサ C1 を足したもの。I1 は 1ms で 0 → 10mA に増え、2ms 後に戻ります。「過渡解析を実行」して、波形の左の「負荷の急変」タブを押すと、電圧がどれだけ下がるか (約 77mV、ツェナーの内部抵抗の分) と、戻るまでの時間が出ます。C1 を 100u にすると、ゆっくり下がるようになりますが、落ち着く先は同じです", "load_step"),
    sample!("モーターを回す (MOSFET + 負荷のトルク)", "小型の DC モーター M1 を、MOSFET Q1 で 5ms からオンにします。D1 はフライバックダイオード。M1 の SPD の電圧が回転数 (1V = 1000rpm) で、電流源 I1 が 50ms から 1mN·m の負荷をかけます (1mA = 1mN·m)。「過渡解析を実行」すると (終了 100ms はサンプルに入っています)、起動の大きな電流と、負荷で回転が落ちて電流が増える様子が見えます。故障シミュレーションで部品 M1.CJ (軸) の「モーターの軸がロック」を選んで「連鎖故障を追う」と、Q1 が壊れ、巻線が焼ける順が出ます", "motor_driver"),
    sample!("ブリッジ整流 (交流を直流にする)", "ダイオード 4 個と平滑コンデンサ。「過渡解析を実行」で、リップルが見えます", "bridge_rectifier"),
    sample!("3相電源と Y 結線の負荷", "3相電源 (A・B・C が 120° ずつずれた交流) に抵抗を3つつなぎます。「過渡解析を実行」で、3つの波形のずれが見えます", "three_phase_wye"),
    sample!("3相ブリッジ整流", "3相電源をダイオード 6 個で直流にします。単相のブリッジ整流より、リップルがずっと小さくなります。「過渡解析を実行」で、V(VDC) がほぼ平らな直流になります", "three_phase_rectifier"),
    sample!("エミッタフォロワ (電流を増やす)", "トランジスタで、電圧はそのまま流せる電流を増やす回路です", "emitter_follower"),
    sample!("オペアンプで信号を大きくする", "非反転増幅。抵抗の比でゲインが決まります", "opamp_amp"),
    sample!("アクティブフィルタ (4 次バターワース ローパス 1kHz)", "オペアンプ TL072 のサレンキーの段を 2 つ重ねたローパス。「値の決め方」の「アクティブフィルタ」で計算し、「回路図に置いて AC で確かめる」で置いたものです。「AC解析を実行」すると、点で描いた目標の特性に V(FOUT) が重なります (1kHz で −3dB、10kHz で −80dB)。部品の値を E24 に変えたり、TL072 を LM358 に変えたりして、どこがずれるか比べられます", "active_filter"),
    sample!("直列レギュレータ (オペアンプ + 基準電圧)", "出力を測って、ずれたら直す (帰還) 安定化電源。電圧安定化の本質です", "series_regulator"),
    sample!("保護つきの電源 (逆接続ダイオード + 78xx)", "電源を逆につないでも壊れにくくする回路です", "power_supply"),
    sample!("電源の予算表 (9V → 5V → 3.3V、あと何 mA つなげるか)", "9V から LM7805 で 5V、そこから MCP1700 で 3.3V を作り、5V に 150mA、3.3V に 120mA の負荷をつないだ回路。解析の「電源の予算表」で、レールの木 (VIN → 5V → 3V3)、それぞれの電流と定格、あと何 mA つなげるか、レギュレータの発熱と効率が出ます。LM7805 は (9 − 5)V × 270mA ≒ 1.1W が熱になって約 80℃、あと約 220mA で、決め手は定格 (1.5A) ではなく温度だと分かります", "power_budget"),
    sample!("突入電流 (電源を入れた瞬間の大電流)", "12V から逆接続保護のダイオード 1N4007 を通して 1000µF のコンデンサと LM7805 につないだ回路。解析の「突入電流」で、電源を入れた瞬間に空のコンデンサへ数十 A が流れ、ダイオードの定格 1A を大きく超えることが分かります。電源の抵抗を大きくする (細い配線・NTC サーミスタ) と、ピークがどう下がるかも試せます", "inrush"),
    sample!("電池の持ち時間 (18650 と 3.3V レギュレータ、間欠で動くマイコン)", "リチウムイオン電池 18650 から MCP1700 で 3.3V を作り、ふだん 1mA で眠って 100ms ごとに 5ms だけ 100mA 流れるマイコン (無線の送信) を負荷にした回路。解析の「電池の持ち時間」で、平均 5.96mA・約 17 日もつこと、電池が 3.2V まで下がると 3.3V が保てず止まることが出ます。U1 を AMS1117-3.3 に変えると、ドロップアウトが大きくて 3.2V に届きません", "battery_life"),
    sample!("最初の電源投入 (電流制限の値と正常な電流)", "9V から逆接続防止のダイオードと 7805 で 5V を作り、LED と負荷をつないだ小さな基板。「学ぶ → 最初の電源投入のガイド」を開くと、9V の電源から流れるはずの電流 (約 17mA。抵抗の許容差 5% と 7805 の出力電圧のばらつきで 16.0〜19.0mA) と、電源に入れる電流制限の値 (30mA) が出ます。測った電流に 60 を入れると「ずっと多い」と、疑う所が並びます", "power_on"),
    sample!("押しボタンでブザーを鳴らす", "押しボタン SW1 を押している間だけ、5V が電子ブザー BZ1 にかかって鳴ります。試作のタブで電源を入れて SW1 をクリックすると、スピーカーから音が出ます。電源の電圧を下げると小さくなり、約 2.5V より下では鳴りません。ブザーを逆向きに差すと鳴りません", "buzzer_button"),
    sample!("ボリュームで LED の明るさを変える", "ボリューム VR1 の両端を 5V と GND につなぎ、真ん中の足の電圧でトランジスタを動かして LED を光らせます。選んで右の「つまみ」を回すと、LED の電流が 0〜約 16mA で変わります (半分より下では消えます: トランジスタの 0.7V と LED の約 2V を越えないため)。LED をボリュームの真ん中の足に直接つなぐと、ボリュームが焦げることも確かめられます。試作のタブでは、電源を入れたまま回せます", "pot_dimmer"),
    sample!("MOSFET スイッチ", "小さな信号で大きな電流を入り切りする回路です", "mosfet_switch"),
    sample!("NE555 の点滅回路", "タイマー IC で LED を点滅させます。「過渡解析を実行」で、充放電の波形が見えます", "ne555_blinker"),
    sample!("カウンタとデコーダ (LED が順に流れる)", "74HC161 の 4 ビットカウンタと 74HC138 のデコーダ。クロック 1kHz で数えて、LED が 1 つずつ順に光ります。「過渡解析を実行」で、QA〜QD と Y0〜Y7 の波形が見えます", "counter_decoder"),
    sample!("シフトレジスタで LED を順に光らせる", "74HC595 に 1 ビットずつデータを送ると、そのビットが QA から QH へ 1 つずつ進み、LED が順に光ります。「過渡解析を実行」で、QA〜QH の波形が見えます", "shift_register_leds"),
    sample!(live "半加算器を動かす (スイッチで 0 と 1)", "開くと「デジタル回路を動かす」になります。スライドスイッチ SW1・SW2 をクリックして A と B を切り替えると、XOR (S) と AND (C) の LED が光り、右下の真理値表が埋まります。配線は High が赤、Low が青。上の帯の「ゆっくり」で、ゲートの遅れが見えます", "half_adder"),
    sample!(live "SR ラッチを動かす (押しボタンで覚える)", "開くと「デジタル回路を動かす」になります。押しボタン SW1 (セット) と SW2 (リセット) をクリックすると、NAND 2 つのラッチが、離したあとも最後に押した方を覚えます。Shift + クリックで押したままにできます", "sr_latch"),
    sample!(live "カウンタを動かす (クロックで 1 つずつ数える)", "開くと「デジタル回路を動かす」になります。上の帯の「1 クロック」で 74HC161 が 1 つずつ数え、4 つの LED が 2 進数で 0〜15 を表します。▶ で動かすと、下のロジックアナライザに Q0〜Q3 (バス Q、16 進) が流れます", "live_counter"),
    sample!(live "7 セグメントで数える (0〜9 のカウンタ)", "開くと「デジタル回路を動かす」になります。74HC161 が数え、10 になった瞬間に NAND がクリアして 0〜9 をくり返し、CD4511B がその数を 7 セグメント表示器の形に変えます。74HC161 の上の「記憶」に中の 4 ビットが出ます。「ゆっくり」で 9 → 0 を見ると、一瞬だけ 10 (表示が消える) を通るのが分かります (非同期クリアのグリッチ)", "seg7_counter"),
    sample!("カウンタと 7 セグメント表示 (数字が 0 から 9 まで進む)", "74HC161 の 4 ビットカウンタの出力を、CD4511B (BCD → 7 セグメントのデコーダ) に入れて、7 つの LED (a〜g) を光らせます。0 から 9 まで数字の形に光り、10 以上は消えます。「過渡解析を実行」で、各セグメントの波形が見えます", "counter_7seg"),
    sample!("正弦波をデジタルで作る (カウンタ → ROM → DAC → ADC)", "74HC4040 のカウンタが ROM32x8 (正弦波 1 周期の表) のアドレスを進め、出てきた 8 ビットの値を R-2R のはしごが電圧に変えます (DA 変換)。その電圧を 4 ビットの AD コンバータ ADC4_FLASH が読み取ります。「過渡解析を実行」で、階段状の正弦波 V(N7) と、AD 変換の値 V(Y0)〜V(Y3) が見えます", "dac_adc_sine"),
    sample!("双方向バス (74HC245 で向きを切り替える)", "同じ 4 本の線 A1〜A4 で、データを両方向に送ります。DIR が High の間 (0〜8ms) は、カウンタ U1 の値を 3 ステートのバッファ U2 が A 側に出し、74HC245 がそれを B 側に写します。DIR が Low になると (8ms〜)、U2 が A 側を手放し、245 が向きを変えて、B 側の抵抗 (DIP スイッチの代わり) が決める値 5 を A 側に出します。「過渡解析を実行」して波形の「デジタル表示」を入れると、A[1..4] と B[1..4] の値が見えます。U3 (NDIR) を外して U2 の 1OE を GND にすると、8ms からぶつかり合いの警告が出ます", "bus_transceiver"),
    sample!("AD コンバータの 2 つの方式 (フラッシュ型と逐次比較型)", "同じゆっくりした正弦波 AIN を、2 つの 4 ビット AD コンバータで読みます。U1 (フラッシュ型) は比較器 15 個で、クロックごとに答えが出ます。U2 (逐次比較型) は比較器 1 個で、START を下げた瞬間の AIN を保持し (サンプルホールド)、そのあと 1 クロックに 1 ビットずつ、上の桁から「1 にしてみて、AIN を超えたら 0 に戻す」を繰り返し、5 クロック目に EOC が High になります。「過渡解析を実行」して波形の「デジタル表示」を入れ、F[0..3] と S[0..3] を比べてください。1 回の変換 (80µs) を拡大すると、S の値が上の桁から 1 ビットずつ決まっていく様子が見えます (答えが 10 なら 8 → 8 → 10 → 10)", "sar_adc"),
    sample!("バスで 8 本の信号をまとめる", "ヘッダの 8 本の信号 D0〜D7 を、太い青のバスの線 D[0..7] でまとめて、8 個の LED につなぎます。バスの線は何もつなぎません。つなぐのは各ピンの D0〜D7 のラベルで、ラベルツールで D[0..7] と書いてピンを順にクリックすると、D0 D1 … と順に付きます。バスの範囲と合わない番号があると、点検(ERC)が教えてくれます", "bus_leds"),
    sample!("セットアップ時間の違反 (データがクロックに間に合わない)", "74HC04 を6個つないで遅らせたデータが、74HC74 のクロックの 7ns 前にしか届かない回路です。「過渡解析を実行」で、足りない時間の警告が出ます。違反したクロックでは、Q がデータどおりにならないことがあります (実物では 0 か 1 か分からないので、ここでは乱数で決めます)。VCK の遅れ (150n) を 180n に変えてもう一度実行すると、警告が消え、Q がデータに従います", "timing_check"),
    sample!("降圧コンバータ (スイッチング電源)", "熱にしないで電圧を下げる電源。スイッチの波形とリップルを見られます", "buck_converter"),
    sample!("総合デモ (電源 + 555 + カウンタ + 温度アラーム + アンプ)", "部品 41 個 の大きな回路。「過渡解析を実行」で、時計 (CLK) と 1/2・1/4 に割ったカウンタの波形が見えます。計算は 1 秒ほどです", "mixed_signal"),
    sample!("電源ラインのインピーダンス (バイパスコンデンサの効き)", "ケーブルの先の電源ラインに、電解・10µF・100nF のコンデンサをつけた回路。「解析」タブの「インピーダンス」で、ネットに RAIL と入れて実行すると、谷 (コンデンサがよく効く周波数) と山 (電圧が揺れやすい周波数) が出ます", "rail_impedance"),
    sample!("アンプのひずみ (出力が頭打ちになる)", "オペアンプの増幅回路 (ゲイン 11) に 0.4V の正弦波を入れると、出力が電源の範囲を超えて頭が削られます。「過渡解析を実行」→ 波形の左の「ひずみ率」タブを押すと約 16% と出ます。V1 を 0.1V にすると、ほぼ 0% になります", "amp_clipping"),
    sample!("極と零 (共振する LC フィルタ)", "10Ω・1mH・1µF のローパス。「解析」タブの「極と零」で、出力に OUT・入力に IN と入れて実行すると、5.03 kHz に一対の複素数の極 (Q = 3.16) が出て、当てはめの曲線が回路の応答に重なります。抵抗を 10Ω から 100Ω にすると、極が実数の2つに分かれて山が消えます", "rlc_poles"),
    sample!("帰還の安定性 (出力にコンデンサをつけた電源)", "直列レギュレータの出力に 10µF をつけた回路。「解析」タブの「ループゲイン」で、切る部品に R3 と入れて実行すると、位相余裕がわずか約 10° と出ます。グラフの上の「ナイキスト線図」を押すと、線が -1 のすぐ近くを通るのが見えます。出力のコンデンサを外す (消す) と、約 90° に戻ります", "loop_cap_load"),
    sample!("根軌跡 (ゲインを上げると発振するフィルタ)", "オペアンプのサレン・キー型ローパス (1.6 kHz)。ゲインは 1 + R3/R4 で、3 倍を超えると発振します。「解析」タブの「根軌跡」で、部品に R3・1k から 30k・出力 OUT・入力 IN として実行すると、R3 を大きくするにつれて 2 つの極が虚軸に近づき、約 20kΩ で右半面 (赤い所) に出るのが見えます", "sallen_key_locus"),
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
