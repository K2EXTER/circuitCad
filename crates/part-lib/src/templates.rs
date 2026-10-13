//! Model templates: fill in a few numbers from the datasheet and get the pins and a behavioural
//! `.subckt` (written with B sources) for a part. The first template is a Li-ion battery charger.

use crate::PinDef;
use sch_core::PinSide;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Topology {
    /// The charge current is passed straight from VIN to BAT (TP4056, MCP73831).
    Linear,
    /// A switching converter with an external inductor on the LX pin (TP5100, IP2332).
    Buck,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CurrentSetting {
    /// The datasheet gives one current (no pin to set it).
    Fixed,
    /// A resistor on the PROG pin sets it: I = offset + k / R[ohm].
    Resistor,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChargerTemplate {
    pub topology: Topology,
    /// Number of cells in series; the float voltage is `cells * cell_voltage`.
    pub cells: u32,
    pub cell_voltage: f64,
    pub setting: CurrentSetting,
    /// Charge current in A (`Fixed`), or the value used when the PROG pin is open (`Resistor`).
    pub current: f64,
    /// `Resistor`: I = offset + k / R.
    pub k: f64,
    pub offset: f64,
    /// Below this voltage per cell the current is reduced (pre-charge); 0 = no pre-charge.
    pub precharge_per_cell: f64,
    pub precharge_fraction: f64,
    /// The cell counts as full when the current falls under this fraction of the set current.
    pub termination_fraction: f64,
    /// The charger needs VIN above this (and above the cell by `dropout`).
    pub vin_min: f64,
    /// 0 = no over-voltage stop.
    pub vin_max: f64,
    pub dropout: f64,
    /// Temperature input: charging runs while its voltage (100 uA through the NTC) is between the two.
    pub ntc: Option<(f64, f64)>,
    /// CHRG and STDBY open-drain outputs.
    pub status: bool,
}

impl Default for ChargerTemplate {
    fn default() -> Self {
        ChargerTemplate {
            topology: Topology::Linear,
            cells: 1,
            cell_voltage: 4.2,
            setting: CurrentSetting::Resistor,
            current: 0.0,
            k: 1200.0,
            offset: 0.0,
            precharge_per_cell: 2.9,
            precharge_fraction: 0.1,
            termination_fraction: 0.1,
            vin_min: 4.0,
            vin_max: 0.0,
            dropout: 0.3,
            ntc: None,
            status: true,
        }
    }
}

#[derive(Debug)]
pub struct Generated {
    pub pins: Vec<PinDef>,
    pub sim_name: String,
    pub sim_text: String,
    /// Pin numbers in the order of the `.subckt` ports.
    pub sim_pins: Vec<String>,
    pub description: String,
    /// What the generated model leaves out (shown next to the part).
    pub limits: Vec<String>,
}

/// A number written so that the SPICE reader takes it back unchanged.
fn num(x: f64) -> String {
    let s = format!("{x}");
    if s.contains('e') { format!("{x:e}") } else { s }
}

impl ChargerTemplate {
    pub fn float_voltage(&self) -> f64 {
        self.cells as f64 * self.cell_voltage
    }

    fn check(&self) -> Result<(), String> {
        let bad = |why: &str| Err(format!("充電ICのテンプレート: {why}"));
        if !(1..=8).contains(&self.cells) {
            return bad("セル数は 1〜8 にしてください");
        }
        if !(2.0..=5.0).contains(&self.cell_voltage) {
            return bad("1セルの満充電電圧は 2〜5V の範囲で入力してください (リチウムイオンは 4.2V、リン酸鉄リチウムは 3.6V)");
        }
        if self.setting == CurrentSetting::Fixed && self.current <= 0.0 {
            return bad("充電電流を 0 より大きい値で入力してください (データシートの「充電電流」)");
        }
        if self.setting == CurrentSetting::Resistor && (self.k <= 0.0 || self.offset < 0.0 || self.current < 0.0) {
            return bad("電流の式 I = 定数 + k ÷ R の k は 0 より大きく、定数と開放時の電流は 0 以上にしてください");
        }
        if !(0.0..=1.0).contains(&self.precharge_fraction) || !(0.0..1.0).contains(&self.termination_fraction) {
            return bad("プリチャージ電流と終了電流の割合は 0〜1 の間にしてください (例: 0.1 は 10%)");
        }
        if self.vin_min <= 0.0 || (self.vin_max != 0.0 && self.vin_max <= self.vin_min) {
            return bad("入力電圧の下限は 0 より大きく、上限は下限より大きい値 (または 0 = 上限なし) にしてください");
        }
        if let Some((lo, hi)) = self.ntc
            && !(lo >= 0.0 && hi > lo) {
                return bad("温度入力の正常な範囲は「下限 < 上限」にしてください (例: 0.3V〜2.73V)");
            }
        Ok(())
    }

    pub fn generate(&self, name: &str) -> Result<Generated, String> {
        self.check()?;
        let name = name.trim();
        if name.is_empty() {
            return Err("部品名を入力してください (モデルの名前に使います)".into());
        }
        // pins: (name, side)
        let mut pins: Vec<(&str, PinSide)> = vec![("VIN", PinSide::Top), ("GND", PinSide::Bottom), ("BAT", PinSide::Right)];
        if self.topology == Topology::Buck {
            pins.push(("LX", PinSide::Right));
        }
        if self.setting == CurrentSetting::Resistor {
            pins.push(("PROG", PinSide::Left));
        }
        if self.ntc.is_some() {
            pins.push(("NTC", PinSide::Left));
        }
        if self.status {
            pins.push(("CHRG", PinSide::Right));
            pins.push(("STDBY", PinSide::Right));
        }
        let has = |n: &str| pins.iter().any(|p| p.0 == n);
        let pin_defs: Vec<PinDef> = pins.iter().enumerate().map(|(i, (n, s))| PinDef { number: (i + 1).to_string(), name: n.to_string(), side: Some(*s), sim_only: false }).collect();
        let ports: Vec<String> = pins.iter().map(|p| p.0.to_ascii_lowercase()).collect();

        let vfl = self.float_voltage();
        let vtk = self.precharge_per_cell * self.cells as f64;
        let mut t: Vec<String> = vec![];
        let mut line = |s: String| t.push(s);
        let sub = name.replace(' ', "_");
        line(format!(".subckt {sub} {}", ports.join(" ")));
        line(format!("* {name}: Li-ion charger template ({} cell, {} V float, {})", self.cells, num(vfl), if self.topology == Topology::Buck { "averaged buck" } else { "linear" }));

        // run condition (0..1): input range, headroom over the cell, temperature window
        let mut on = format!("step(V(vin)-{},0.02)*step(V(vin)-V(bat)-{},0.01)", num(self.vin_min), num(self.dropout));
        if self.vin_max > 0.0 {
            on += &format!("*step({}-V(vin),0.02)", num(self.vin_max));
        }
        if let Some((lo, hi)) = self.ntc {
            on += &format!("*step(V(ntc)-{},0.01)*step({}-V(ntc),0.01)", num(lo), num(hi));
            line("* temperature input: 100 uA through the NTC (limited at 3.4 V) gives the voltage that is checked".into());
            line("Bntc 0 ntc I=100u-1m*max(V(ntc)-3.3,0)".into());
            line("Rntc ntc 0 100Meg".into());
        }
        line("* run condition: 1 while the input is in range, above the cell, and the cell temperature is fine".into());
        line(format!("Bon on 0 V={on}"));

        // set current
        match self.setting {
            CurrentSetting::Fixed => {
                line(format!("Bicc icc 0 V={}", num(self.current)));
            }
            CurrentSetting::Resistor => {
                line("* PROG: a 1 V reference through 1 ohm; x = the current in the PROG resistor (A)".into());
                line("Bref ref 0 V=step(V(vin)-1,0.02)".into());
                line("Rm ref prog 1".into());
                line("Bx x 0 V=V(ref)-V(prog)".into());
                let k = format!("{}+{}*V(x)", num(self.offset), num(self.k));
                // an open PROG pin carries no current: the open-pin value applies
                line(format!("Bicc icc 0 V=if(V(x)<0.5u,{},{k})", num(self.current)));
            }
        }
        // current command: pre-charge below the threshold, and the voltage loop near the float voltage
        let pre = if self.precharge_per_cell > 0.0 && self.precharge_fraction < 1.0 {
            format!("*(1-{}*step({}-V(bat),0.01))", num(1.0 - self.precharge_fraction), num(vtk))
        } else {
            String::new()
        };
        line("* current command: the set current (reduced when pre-charging), and a voltage loop that tapers it near the float voltage".into());
        line(format!("Bcmd cmd 0 V=V(on)*max(0,min(V(icc){pre},V(icc)*{}*({}-V(bat))))", num(50.0 / self.cells as f64), num(vfl)));

        match self.topology {
            Topology::Linear => {
                line("* pass element: the command current from VIN into the cell".into());
                line("Bpass vin pm I=V(cmd)".into());
                line("Vsn pm bat 0".into());
            }
            Topology::Buck => {
                line("* LX = cell voltage + 3 V/A x (command - current) + an integral term, limited to 0 .. VIN".into());
                line("Bint 0 cint I=0.02*(V(cmd)-I(Vsn))".into());
                line("Cint cint 0 1u".into());
                line("Rint cint 0 10k".into());
                line("Blx ls 0 V=limit(V(bat)+3*(V(cmd)-I(Vsn))+V(cint),0,V(vin))".into());
                line("Vsn ls ls2 0".into());
                line("* the switch never pushes current back into VIN".into());
                line("Sd ls2 lx ls2 lx SWD".into());
                line("Rlx lx 0 100Meg".into());
                line("* input current: output power over the efficiency (90 %) and the input voltage".into());
                line("Bin vin 0 I=I(Vsn)*V(bat)/(0.9*max(V(vin),1))".into());
            }
        }
        line("Riq vin 0 20k".into());
        if self.status {
            line("* status (open drain): CHRG low while charging, STDBY low when the current has fallen under the end-of-charge level".into());
            line(format!(
                "Bfull full 0 V=V(on)*step({}*V(icc)-I(Vsn),0.005)*step(V(bat)-({}),0.005)",
                num(self.termination_fraction),
                num(vfl - 0.05 * self.cells as f64)
            ));
            line("Bchg chg 0 V=V(on)*(1-V(full))".into());
            line("Sch chrg 0 chg 0 SWO".into());
            line("Ssb stdby 0 full 0 SWO".into());
            line("Rch chrg 0 100Meg".into());
            line("Rsb stdby 0 100Meg".into());
        }
        if self.topology == Topology::Buck {
            line(".model SWD SW(RON=0.17 ROFF=1G VT=0 VW=0.002)".into());
        }
        if self.status {
            line(".model SWO SW(RON=60 ROFF=1G VT=0.5 VW=0.1)".into());
        }
        line(format!(".ends {sub}"));

        let cur = match self.setting {
            CurrentSetting::Fixed => format!("充電電流 {} A", num(self.current)),
            CurrentSetting::Resistor => format!(
                "充電電流 = {}{} ÷ R_PROG[Ω] [A] (PROG 開放時 {} A)",
                if self.offset > 0.0 { format!("{} + ", num(self.offset)) } else { String::new() },
                num(self.k),
                num(self.current)
            ),
        };
        let description = format!(
            "充電IC (テンプレート): {}セル {} V、{}、{cur}。{}{}{}動作モデルは外から見た動きだけを再現します (損失・リップル・熱・タイマは扱いません)",
            self.cells,
            num(vfl),
            if self.topology == Topology::Buck { "降圧スイッチング (LX にコイルをつなぐ)" } else { "リニア" },
            if self.precharge_per_cell > 0.0 { format!("{} V/セル未満はプリチャージ。", num(self.precharge_per_cell)) } else { String::new() },
            if has("NTC") { "温度入力あり。" } else { "" },
            if has("CHRG") { "CHRG/STDBY は充電中/満充電で Low。" } else { "" }
        );
        let sim_pins: Vec<String> = (1..=pins.len()).map(|i| i.to_string()).collect();
        let mut limits = vec![
            "データシートの数字から作ったテンプレートのモデルです。外から見た動きだけを再現します。充電の3段階 (プリチャージ・定電流・定電圧) と状態表示が動きます。".to_string(),
            "扱わないもの: 損失、リップル、熱による制限、充電タイマ、再充電のしきい値。電流が終了電流を下回ると状態表示が「満充電」になりますが、充電そのものは止まらず、電圧が下がれば自然に電流が増えます。".to_string(),
            "電池は、小さなコンデンサで代用してください (値に IC=3.2 と書くと、その電圧から始まります)。".to_string(),
        ];
        limits.push(match self.topology {
            Topology::Linear => "入力電流は出力電流と同じです (リニア式は、電圧差が熱になります)。熱による電流の低下は扱いません。".to_string(),
            Topology::Buck => "降圧のスイッチングは「LX の平均の電圧」に置き換えてあります。入力電流は、出力の電力 ÷ 効率 (90%) ÷ 入力電圧です。".to_string(),
        });
        if self.ntc.is_some() {
            limits.push("温度入力は、100µA を流した電圧が決めた範囲の外のとき、充電を止めるだけです (低温で電流を減らすような段階的な動きはしません)。".to_string());
        }
        Ok(Generated { pins: pin_defs, sim_name: sub, sim_text: t.join("\n"), sim_pins, description, limits })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_follow_the_options() {
        let g = ChargerTemplate::default().generate("MyChg").unwrap();
        let names: Vec<&str> = g.pins.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["VIN", "GND", "BAT", "PROG", "CHRG", "STDBY"]);
        assert_eq!(g.sim_pins, ["1", "2", "3", "4", "5", "6"]);
        assert!(g.sim_text.starts_with(".subckt MyChg vin gnd bat prog chrg stdby"));
        let t = ChargerTemplate { topology: Topology::Buck, setting: CurrentSetting::Fixed, current: 1.0, ntc: Some((0.3, 2.7)), status: false, ..Default::default() };
        let g = t.generate("X").unwrap();
        let names: Vec<&str> = g.pins.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["VIN", "GND", "BAT", "LX", "NTC"]);
    }

    #[test]
    fn bad_numbers_are_explained() {
        for (t, hint) in [
            (ChargerTemplate { cells: 0, ..Default::default() }, "セル数"),
            (ChargerTemplate { setting: CurrentSetting::Fixed, current: 0.0, ..Default::default() }, "充電電流"),
            (ChargerTemplate { vin_min: 5.0, vin_max: 4.0, ..Default::default() }, "入力電圧"),
            (ChargerTemplate { ntc: Some((2.0, 1.0)), ..Default::default() }, "下限 < 上限"),
            (ChargerTemplate { termination_fraction: 1.5, ..Default::default() }, "0〜1"),
        ] {
            let e = t.generate("X").unwrap_err();
            assert!(e.contains(hint), "{e}");
        }
        assert!(ChargerTemplate::default().generate("  ").unwrap_err().contains("部品名"));
    }

    #[test]
    fn the_description_states_what_was_chosen() {
        let g = ChargerTemplate { cells: 2, topology: Topology::Buck, ..Default::default() }.generate("X").unwrap();
        assert!(g.description.contains("2セル 8.4 V") && g.description.contains("降圧") && g.description.contains("1200"), "{}", g.description);
    }
}
