//! The context hint shown at the bottom of the window.

use super::*;

impl SchEditor {
    /// A tip for what to try next, based on what is on screen.
    pub fn hint(&self) -> String {
        if self.noise_pick {
            return "ノイズを測りたいノードをクリックします。Shift+クリックで基準ノード、Esc で中止".into();
        }
        if self.sch.components.is_empty() {
            return "まず部品を置きましょう。左の一覧から選んで画面をクリック。「ファイル → 開く」で docs/examples のサンプル回路も試せます".into();
        }
        if self.pending.is_some() {
            return "配線中: 次の点をクリックして線を延長、ダブルクリックか右クリックで終了します".into();
        }
        match self.tool {
            Tool::Place if !self.has_wave() => return "クリックで部品を置きます。R キーで回転、右クリックで中断。置いた後は「選択」で値を変えられます".into(),
            Tool::Wire => return "部品のピン (赤い点) から点へクリックしてつなぎます。T字に分岐する場合は、線の途中に別の線の端を置きます".into(),
            Tool::Label => return "ラベル名を決めて回路図の点をクリック。同じ名前の場所は線なしでつながります (電源 VCC や GND に便利)".into(),
            Tool::Select | Tool::Place => {}
        }
        if let Some(Sel::Comp(i)) = self.selected
            && let Some(c) = self.sch.components.get(i) {
                return match c.kind {
                    Kind::Resistor => "値を書き換えるか「スライダー」を動かしてみましょう。「自動で再計算」をオンにすると、電圧や波形が動きに合わせて変わります".into(),
                    Kind::Capacitor => "コンデンサは直流では電流を通さず、周波数が高いほど通しやすくなります。AC解析で効き方を見てみましょう".into(),
                    Kind::VSource => "電源の種類は「直流 / 交流 / パルス」のボタンで切り替えられます。「AC解析の入力にする」をオンにすると周波数特性の測定に使えます (ノイズ解析・DCスイープの入力にも指定できます)".into(),
                    Kind::Npn => "トランジスタはベース電流で大きなコレクタ電流を制御します。ベースに抵抗を付けて電圧を見てみましょう".into(),
                    _ => "選んだ部品の値は左の欄で変えられます。R キーで回転、Delete で削除".into(),
                };
            }
        if self.sweep.is_some() && self.wave_mode == WaveMode::Sweep {
            return "出力 = 入力の線と曲線が交わる所が切替電圧です。VIL・VIH の間が「どちらとも言えない」領域で、狭いほどスパッと切り替わります。電源電圧や部品を変えて再実行してみましょう".into();
        }
        if self.noise.is_some() && self.wave_mode == WaveMode::Noise {
            return "ノイズ一覧の素子名にカーソルを合わせると原因と対策が出ます。抵抗値を変えて再実行すると、雑音が √(抵抗値) に比例して変わるのが分かります".into();
        }
        if self.ac.is_some() && self.wave_mode == WaveMode::Ac {
            return "ボード線図: 上が大きさ(dB)、下が位相。-3 dB になる周波数がカットオフ周波数です。グラフはドラッグで移動、ホイールで拡大縮小".into();
        }
        if self.tran.is_some() && self.wave_mode == WaveMode::Tran {
            return "波形はドラッグで移動、ホイールで拡大縮小、ダブルクリックで全体表示。左の一覧で表示する信号を選べます".into();
        }
        if self.mc.is_some() && self.wave_mode == WaveMode::Mc {
            return "同じ回路でも、部品の誤差で実物ごとに電圧が変わります。「効いている部品」の上位を許容差の小さい部品にすると、ばらつきが小さくなります".into();
        }
        if self.sweep.as_ref().is_some_and(|s| s.temp) && self.wave_mode == WaveMode::Sweep {
            return "温度が上がるとダイオードやトランジスタの電圧が下がります (約 -2 mV/℃)。この変化で回路の動作点がどれだけずれるかを見ましょう".into();
        }
        if self.dc.is_none() {
            return "回路が描けたら「DC動作点を解析」で各点の電圧と部品の電流 (配線は低電圧が青、高電圧が赤) を見てみましょう。ERC で配線ミスの点検もできます".into();
        }
        "電圧が想定と違う所から疑ってみましょう。次は過渡解析 (波形)・AC解析 (周波数特性)・ノイズ解析を試せます。回路が完成したら「基板パターン」タブへ".into()
    }
}
