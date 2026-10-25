//! Whole-board actions: autorouting and the DRC.

use super::*;

/// The auto-router's grid pitch for an editor grid of `grid` mm: a divisor of it, so that every point the editor snaps
/// to is also a point the router can use (0.5 mm -> 0.25 mm, 2.54 mm -> 0.231 mm). Without a grid: 0.2 mm.
pub(super) fn route_pitch(grid: f64) -> f64 {
    if grid < 0.1 {
        return 0.2;
    }
    grid / (grid / 0.25).ceil().max(1.0)
}

impl BoardEditor {
    /// Route the airwires of every net, or only of `net`. One undo step. When no currents are declared yet, the schematic's
    /// DC currents are asked for first (so that the nets that carry a lot of current get a track width that fits) and the
    /// route is made when `App` has answered (`route_after_currents`); not during a guided lesson.
    pub fn autoroute(&mut self, net: Option<&str>) {
        if self.board.outline.len() >= 3 && self.board.net_currents.is_empty() && !self.currents_tried && self.lesson.is_none() && !ratsnest(&self.board).is_empty() {
            self.currents_tried = true;
            self.route_after_currents = Some(net.map(str::to_string));
            self.currents_request = Some(CurrentSource::Dc);
            return;
        }
        self.autoroute_now(net);
    }

    /// [`Self::autoroute`] without asking for the currents first.
    pub fn autoroute_now(&mut self, net: Option<&str>) {
        if self.board.outline.len() < 3 {
            self.status = "先に基板の外形を決めてください".into();
            return;
        }
        if !ratsnest(&self.board).iter().any(|w| net.is_none_or(|n| w.net == n)) {
            self.status = "自動配線: 未配線はありません".into();
            return;
        }
        self.checkpoint();
        // neighbouring pads of one net (the two LX pins of a regulator) become one piece of copper first, so that one trunk leaves them
        let bridged = if net.is_none() { pcb_core::bridge::bridge_pads(&mut self.board) } else { 0 };
        let tracks_before = self.board.tracks.len();
        let opt = RouteOptions { track_width: self.track_width, via_diameter: self.via_dia, via_drill: self.via_drill, grid: route_pitch(self.grid), follow: None, clearance: None, pad_lead: self.pad_lead.then_some(self.pad_lead_len) };
        // differential pairs first, side by side; the rest afterwards
        let mut pair_routed = 0;
        let mut pair_failed = vec![];
        for d in self.board.diff_pairs.clone() {
            if net.is_none_or(|n| n == d.p || n == d.n) {
                let r = autoroute::route_pair(&mut self.board, &d, &opt);
                pair_routed += r.routed;
                pair_failed.extend(r.failed);
            }
        }
        // nets with a declared current ("配線の太さ (電流から)") are routed with the width that current needs
        let mut report = autoroute::autoroute_sized(&mut self.board, &opt, net);
        report.routed += pair_routed;
        // a pair connection that failed is tried again by the plain router above; count only what is still open
        let _ = pair_failed;
        self.violations.clear();
        if report.routed == 0 && bridged == 0 {
            self.undo.pop(); // nothing changed: no undo step
        }
        // the new tracks leave and enter the pads at a right angle, like those drawn by hand
        let aligned = if self.pad_lead && report.routed > 0 { self.align_pad_entries(Some((tracks_before..self.board.tracks.len()).collect())) } else { 0 };
        // the router and the pad leads cut a route into short pieces: join the ones that run in a line
        let joined = if report.routed > 0 { pcb_core::merge::merge_collinear(&mut self.board, tracks_before) } else { 0 };
        // joined into longer runs, more pad ends have room for a lead (and for the move onto the pad's centre line)
        let aligned = aligned + if self.pad_lead && report.routed > 0 { self.align_pad_entries(Some((tracks_before..self.board.tracks.len()).collect())) } else { 0 };
        let joined = joined + if report.routed > 0 { pcb_core::merge::merge_collinear(&mut self.board, tracks_before) } else { 0 };
        // a via or a track end left a hair from a pad of its own net makes a slit the fab may flag: fill it
        let filled = if report.routed > 0 { drc::fill_slits(&mut self.board) } else { 0 };
        // the GND pour of the bottom is there: fill the top between the wires with ground as well
        let top_note = if self.pour_top && report.routed > 0 && self.board.zones.iter().any(|z| z.net == "GND" && z.side == Side::Bottom) {
            match pcb_core::groundpour::top_pour(&mut self.board, None) {
                Ok(true) => "表面も GND のベタで埋めました。",
                Ok(false) => "",
                Err(_) => "表面の GND ベタは、ルール違反が増えるので置きませんでした。",
            }
        } else {
            ""
        };
        self.status = if report.failed.is_empty() {
            let mut tail = if aligned > 0 { format!("パッドの根元 {aligned} か所を垂直にそろえました。") } else { String::new() };
            tail += top_note;
            if bridged > 0 {
                tail += &format!("同じネットの隣り合うパッド {bridged} 組を、先に銅でつなぎました。");
            }
            if filled > 0 {
                tail += &format!("同じネットの銅どうしの狭いすき間 {filled} か所を、銅で埋めました。");
            }
            if !report.wide.is_empty() {
                let list: Vec<String> = report.wide.iter().map(|(n, w)| format!("{n} {w:.2} mm")).collect();
                tail += &format!("電流が大きいネットは、必要な太さで引きました ({})。", list.join("、"));
            }
            if joined > 0 {
                tail += &format!("一直線につながる配線 {joined} か所を1本にまとめました。");
            }
            format!("自動配線: {} 本を配線しました。{tail}(格子 {:.3} mm。手で足すときは、配線や端の点に吸い付きます)。DRC で確認しましょう", report.routed, opt.grid)
        } else {
            format!(
                "自動配線: {} 本を配線、{} 本は通せませんでした (黄色の線のまま)。部品の位置を動かすか、手動で配線してください",
                report.routed,
                report.failed.len()
            )
        };
    }

    /// Pack the parts into rows (connected ones together) and fit the outline. One undo step.
    pub fn autoplace(&mut self) {
        self.checkpoint();
        match pcb_core::autoplace::autoplace(&mut self.board) {
            Ok(r) => {
                self.violations.clear();
                self.selected = None;
                self.multi.clear();
                self.fit_pending = true;
                let mut poured = if r.poured.is_empty() { String::new() } else { format!("ベタのある {} は配置の計算から外し、部品の間隔を詰めました。ベタも新しい外形に合わせました。", r.poured.join("・")) };
                if r.repoured > 0 {
                    poured += &format!("ベタへのビア {} 個は置き直しました。", r.repoured);
                }
                self.status = format!("自動配置: {} 個を並べ、外形を {} × {} mm にしました。{poured}位置は手で動かして調整できます", r.placed, r.width, r.height);
            }
            Err(e) => {
                self.undo.pop();
                self.status = e;
            }
        }
    }

    /// Rip up and route again the nets that detour or carry vias that are not needed (`pcb_core::reroute`), among all the
    /// wiring or just `net`. One undo step; nothing changes when nothing gets better.
    pub fn shorten_routes(&mut self, net: Option<&str>) {
        if self.board.tracks.is_empty() {
            self.status = "配線がまだありません。先に配線してください".into();
            return;
        }
        self.checkpoint();
        let opt = RouteOptions { track_width: self.track_width, via_diameter: self.via_dia, via_drill: self.via_drill, grid: route_pitch(self.grid), follow: None, clearance: None, pad_lead: self.pad_lead.then_some(self.pad_lead_len) };
        let (len, vias) = (self.board.tracks.iter().map(|t| t.a.dist(t.b)).sum::<f64>(), self.board.vias.len());
        let r = pcb_core::reroute::improve_routes(&mut self.board, &opt, None, net);
        if r.improved == 0 {
            self.undo.pop();
            self.status = "配線を短くする: これ以上短くできる配線は見つかりませんでした (引き直して、短く・きれいになるものだけ採用します)".into();
            return;
        }
        self.violations.clear();
        let after = self.board.tracks.iter().map(|t| t.a.dist(t.b)).sum::<f64>();
        self.status = format!(
            "配線を短くする: {} 本を引き直しました。配線の長さ {:.0} → {:.0} mm、ビア {} → {} 個。DRC で確認しましょう (気に入らなければ「元に戻す」)",
            r.improved,
            len,
            after,
            vias,
            self.board.vias.len()
        );
    }

    /// The GND pour on the bottom layer with a via for every surface ground pad (see `pcb_core::groundpour`). One undo step.
    pub fn ground_pour(&mut self) {
        self.checkpoint();
        let opt = pcb_core::groundpour::PourOptions { net: None, track_width: self.track_width.max(0.3), via_diameter: self.via_dia, via_drill: self.via_drill, top: self.pour_top, side: Side::Bottom };
        match pcb_core::groundpour::ground_pour(&mut self.board, &opt) {
            Ok(r) if r.zone_added || r.vias > 0 => {
                self.violations.clear();
                let left = if r.left > 0 { format!("ビアを置けなかったパッドが {} 個あります (自動配線が線でつなぎます)。", r.left) } else { String::new() };
                self.status = format!(
                    "GND ベタ: 裏面{}を GND の銅箔にして、表面の GND パッド {} 個を短い線とビアでつなぎました ({} 個は元からつながっていました)。{left}このあと「自動配線」で GND 以外の線を引きます",
                    if r.top_added { "と表面の空いた所" } else { "" }, r.vias, r.already
                );
            }
            Ok(_) => {
                self.undo.pop(); // nothing changed: no undo step
                self.status = "GND ベタは、もう置いてあります".into();
            }
            Err(e) => {
                self.undo.pop();
                self.status = e;
            }
        }
    }

    pub fn run_drc(&mut self) {
        self.violations = drc::check_final(&self.board);
        self.violations.extend(pcb_core::diffpair::check_pairs(&self.board));
        self.violations.extend(drc::slit_checks(&self.board));
        self.drc_ran = true;
        self.status = format!("DRC: 違反 {} 件", self.violations.len());
    }
}
