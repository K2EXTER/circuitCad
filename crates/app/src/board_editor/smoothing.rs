//! "配線を整える" section of the board panel: the finish of the copper (round corners, teardrops, the round end of
//! a notch). It is a setting of the board that is applied when the board is shown and made, so the tracks stay as
//! drawn; the buttons bake the track part of it into the tracks.

use super::*;
use pcb_core::smooth::{self, Finish, Finished, SmoothReport};
use std::rc::Rc;

impl BoardEditor {
    /// The copper with the board's finish applied (`None` while the finish is off). Worked out again only when
    /// the copper or the settings change.
    fn finish_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let f = &self.board.finish;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        pcb_core::zonefill::geometry_key(&self.board).hash(&mut h);
        (f.corners, f.tapers, f.forks, f.radius.to_bits()).hash(&mut h);
        for c in &self.board.net_classes {
            let o = &c.finish;
            (&c.nets, o.corners, o.tapers, o.forks, o.radius.map(f64::to_bits)).hash(&mut h);
        }
        h.finish()
    }

    /// Where the tracks and vias are (what a finish is made from).
    fn tracks_key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for t in &self.board.tracks {
            (t.a.x.to_bits(), t.a.y.to_bits(), t.b.x.to_bits(), t.b.y.to_bits(), t.width.to_bits(), t.side).hash(&mut h);
        }
        for v in &self.board.vias {
            (v.pos.x.to_bits(), v.pos.y.to_bits(), v.diameter.to_bits()).hash(&mut h);
        }
        h.finish()
    }

    /// [`finished`](Self::finished) for the picture: when the copper has changed, the new finish is worked out on another thread (a
    /// quarter of a second on a board of this size, and it would freeze the window at the end of every move) and the picture keeps the
    /// last one until it is ready. The very first one is worked out at once, there being nothing to keep.
    pub(super) fn finished_async(&self, ctx: &egui::Context) -> Option<Rc<Finished>> {
        if !smooth::finish_active(&self.board) {
            return None;
        }
        // a group with tracks in it is being moved: the plain tracks follow the pointer, the finish is made once it is dropped
        if self.moving_group() && !self.moving_parts_only() {
            return None;
        }
        let key = self.finish_key();
        // an answer that came in
        let mut done: Option<(u64, Option<Finished>)> = None;
        {
            let mut job = self.finish_job.borrow_mut();
            if let Some((jk, rx)) = job.as_ref() {
                match rx.try_recv() {
                    Ok(f) => {
                        done = Some((*jk, f));
                        *job = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(30)),
                    Err(_) => *job = None,
                }
            }
        }
        if let Some((k, Some(f))) = done {
            *self.finish_cache.borrow_mut() = Some((k, Rc::new(f)));
            self.finish_tracks.set(self.finish_job_tracks.get());
        }
        let have = self.finish_cache.borrow().clone();
        match have {
            Some((k, v)) if k == key => return Some(v),
            Some((_, v)) if self.moving_parts_only() => return Some(v),
            None => return self.finished(),
            Some(_) => {}
        }
        if self.finish_job.borrow().is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let board = self.board.clone();
            std::thread::spawn(move || {
                let _ = tx.send(smooth::finished(&board));
            });
            *self.finish_job.borrow_mut() = Some((key, rx));
            self.finish_job_tracks.set(self.tracks_key());
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        // the last finish stands until the new one is ready (unless the tracks have moved: then the plain ones are drawn)
        if self.finish_tracks.get() != self.tracks_key() {
            return None;
        }
        self.finish_cache.borrow().as_ref().map(|(_, v)| v.clone())
    }

    pub(super) fn finished(&self) -> Option<Rc<Finished>> {
        if !smooth::finish_active(&self.board) {
            return None;
        }
        let key = self.finish_key();
        let mut cache = self.finish_cache.borrow_mut();
        if let Some((k, v)) = cache.as_ref()
            && (*k == key || self.moving_parts_only())
        {
            return Some(v.clone());
        }
        let fresh = Rc::new(smooth::finished(&self.board)?);
        self.finish_tracks.set(self.tracks_key());
        *cache = Some((key, fresh.clone()));
        Some(fresh)
    }

    pub(super) fn panel_smooth(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("配線を整える").on_hover_text("市販の基板のように、配線の角を丸め、太さが変わる所をなだらかにし、パッドに入る根元を垂直にそろえ、鋭角で分かれる2本の間のくぼみの先を丸くします。配線そのものは変えずに、表示とガーバーだけに反映します。ここは基板全体の設定です。ネットごとに変えるときは、ネットの設定窓 (銅をクリック) のネットクラスで");
        let mut f: Finish = self.board.finish;
        let mut changed = false;
        ui.horizontal(|ui| {
            changed |= ui.checkbox(&mut f.corners, "角を丸める").on_hover_text("同じ太さの配線がつながる角を、円弧にします。角が丸いと電流が流れやすく、高周波でも反射が減ります。直線に近い角や、短くて入らない角は小さくなるか、そのままです").changed();
            changed |= ui.add_enabled(f.corners, egui::DragValue::new(&mut f.radius).speed(0.05).range(0.1..=5.0).prefix("半径 ").suffix(" mm"))
                .on_hover_text("角の丸みの半径。配線が短いと、入る大きさまで自動で小さくなります")
                .changed();
        });
        changed |= ui.checkbox(&mut f.tapers, "太さの段差をなだらかに").on_hover_text("太い配線と細い配線がつながる所と、配線がパッド・ビアに入る根元に、中間の太さの短い配線を挟みます (パッドより細い配線は、根元がパッドの幅から配線の太さへ徐々に細くなります)。電流が急に絞られず、配線の根元が割れにくくなります").changed();
        changed |= ui.checkbox(&mut f.forks, "分かれ目を丸める").on_hover_text("1つの点から2本の配線が鋭い角度で出ている所の、細いくぼみ (エッチング液がたまりやすい所) を銅で埋めて、先を凹んだ丸い形にします。T字の内側の角や、同じネットの並走が1本にまとまる所のすき間も同じです。配線の経路は変わらず、表示とガーバーにだけ働きます (確定はできません)").changed();
        if changed {
            self.checkpoint_once();
            self.board.finish = f;
        }
        if ui.button("パッドの根元をそろえる").on_hover_text("すべての配線の、四角いパッドへの根元を、パッドの辺に垂直な短い区間にそろえます。配線の経路が、パッドの近くだけ少し変わります。ルール違反が増える所と、接続が切れる所は変えません。整えを切ったネットクラスのネットは除きます。自動配線の直後には、新しい配線に自動で行います (②の「パッドに垂直に出入り」が入っているとき)。元に戻すで取り消せます").clicked() {
            self.align_pad_entries_button();
        }
        let any = f.corners || f.tapers;
        if ui.add_enabled(any, egui::Button::new("整えた形を配線に確定する")).on_hover_text("角の丸みと段差を、配線そのものに書き込みます (確定すると、ほかの CAD にも渡せます。あとから配線を動かしても、丸めた角は自動では直りません)。ルール違反が増える所と、接続が切れる所は変えません。分かれ目の丸みとパッドの根元の肉づけ (凹んだ円弧) は、線では表せないので確定できません (パッドの根元は段の涙型になります)。これらを残したいときは確定せず、「配線を整える」の設定を入れたままにします。元に戻すで取り消せます").clicked() {
            self.smooth_tracks();
        }
    }

    /// Select track `i` (for screenshots).
    pub fn select_track(&mut self, i: usize) {
        self.selected = (i < self.board.tracks.len()).then_some(Item::Track(i));
    }

    /// Turn all the finish off or back on (for screenshots).
    pub fn set_finish(&mut self, on: bool) {
        let f = &mut self.board.finish;
        (f.corners, f.tapers, f.forks) = (on, on, on);
    }

    pub fn smooth_tracks(&mut self) {
        self.checkpoint();
        let opts = self.board.finish;
        let r = smooth::smooth_tracks(&mut self.board, &opts);
        self.report_smooth(r);
    }

    fn report_smooth(&mut self, SmoothReport { corners, tapers, .. }: SmoothReport) {
        self.violations.clear();
        self.status = if corners + tapers == 0 {
            self.undo.pop(); // nothing changed: no undo step
            "確定できる所がありませんでした (角がない、短くて入らない、またはルール違反になる所だけです)".into()
        } else {
            format!("角を {corners} か所丸め、太さの段差を {tapers} か所なだらかにして、配線に確定しました (分かれ目の丸みとパッドの肉づけは確定できないので、残すなら、そのままに)")
        };
    }
}
