//! Editing a part that is already in the library: a `PartDef` read back into the form.

use super::*;

/// A number the form's fields read back with `sim_core::parse_value`.
fn num(v: f64) -> String {
    format!("{v}")
}

fn side_word(s: PinSide) -> &'static str {
    match s {
        PinSide::Left => "left",
        PinSide::Right => "right",
        PinSide::Top => "top",
        PinSide::Bottom => "bottom",
    }
}

impl PartForm {
    /// A form holding `def`, to change it and save it again. Saving under the same name replaces the part;
    /// a new name adds a copy. What the form has no field for (shop numbers, pin trading rules, simulation
    /// parameters, a pad list written by hand) is kept from `def` when the form is turned back into a part.
    pub fn from_def(def: &PartDef) -> Self {
        let mut f = PartForm {
            name: def.name.clone(),
            category: def.category.clone(),
            prefix: def.prefix.clone(),
            value: def.value.clone().unwrap_or_default(),
            description: def.description.clone(),
            datasheet: def.datasheet.clone(),
            glyph: def.glyph.clone(),
            height: def.height.unwrap_or(0.0),
            land_unverified: def.land_unverified,
            model3d: def.model3d.clone(),
            orig: Some(def.clone()),
            ..PartForm::default()
        };
        // the saved 3D shape is the base the buttons turn and place again (without it, turning would clear the shape)
        if def.model3d.is_some() {
            f.step_base = def.model3d.clone();
            f.step_file = "保存済みの3Dの形".into();
        }
        f.pins_text = def
            .pins
            .iter()
            .map(|p| match p.side {
                Some(s) => format!("{} {} {}", p.number, p.name, side_word(s)),
                None => format!("{} {}", p.number, p.name),
            })
            .collect::<Vec<_>>()
            .join("\n");
        match &def.package {
            PackageSpec::Chip2 { pitch, pad_len, pad_width } => {
                (f.pkg, f.pitch, f.len, f.width) = (PkgKind::Chip2, *pitch, *pad_len, *pad_width);
            }
            PackageSpec::Dual { pitch, row_spacing, pad_len, pad_width, drill, .. } => {
                (f.pkg, f.pitch, f.row, f.len, f.width) = (PkgKind::Dual, *pitch, *row_spacing, *pad_len, *pad_width);
                f.tht = drill.is_some();
                f.drill = drill.unwrap_or(f.drill);
            }
            PackageSpec::Quad { pitch, span, pad_len, pad_width, .. } => {
                (f.pkg, f.pitch, f.span, f.len, f.width) = (PkgKind::Quad, *pitch, *span, *pad_len, *pad_width);
            }
            PackageSpec::SingleRow { pitch, pad_d, drill, .. } => {
                (f.pkg, f.pitch, f.pad_d, f.drill) = (PkgKind::Row, *pitch, *pad_d, *drill);
            }
            PackageSpec::DoubleRow { pitch, row_spacing, pad_d, drill, .. } => {
                (f.pkg, f.pitch, f.row, f.pad_d, f.drill) = (PkgKind::Row2, *pitch, *row_spacing, *pad_d, *drill);
            }
            PackageSpec::Explicit { .. } => f.keep_package = true,
        }
        if let Some(b) = &def.body {
            f.body_on = true;
            f.body_circle = b.shape == "circle";
            (f.body_w, f.body_h, f.body_x, f.body_y, f.body_mark, f.body_pin) = (b.w, b.h, b.x, b.y, b.mark, b.mark_pin.clone());
        }
        if let Some(m) = &def.sim {
            f.sim = if m.kind == SimKind::Model { SimChoice::Model } else { SimChoice::Subckt };
            f.sim_element = m.element.clone();
            f.sim_name = m.name.clone();
            f.sim_text = m.text.clone();
            f.sim_pins = m.pins.join(" ");
            f.sim_limits = m.limits.join("\n");
        }
        if let Some(r) = &def.ratings {
            f.rt_v = r.v_max.map(num).unwrap_or_default();
            f.rt_i = r.i_max.map(num).unwrap_or_default();
            f.rt_p = r.p_max.map(num).unwrap_or_default();
            f.rt_rows = r.rows.iter().map(|row| format!("{} | {} | {}", row.name, row.value, row.condition)).collect::<Vec<_>>().join("\n");
            if let Some(rp) = &r.ripple {
                f.rt_ripple = num(rp.i_rms);
                f.rt_ripple_f = num(rp.freq_hz);
                f.rt_ripple_table = rp.coeffs.iter().map(|(hz, m)| format!("{} {}", num(*hz), num(*m))).collect::<Vec<_>>().join("\n");
            }
            if let Some(l) = &r.life {
                f.rt_life_h = num(l.hours);
                f.rt_life_t = num(l.temp_c);
            }
        }
        f
    }

    /// The part this form was opened from (None = a new part).
    pub fn editing(&self) -> Option<&str> {
        self.orig.as_ref().map(|d| d.name.as_str())
    }

    /// The number of pads the opened part's package has, while the pin table still lists the same pins in
    /// the same kind of package (a part may have more pads than listed pins, e.g. the unused pins of a gate IC).
    pub(super) fn orig_pad_count(&self, pins: &[PinDef]) -> Option<usize> {
        let o = self.orig.as_ref()?;
        let same_pins = pins.len() == o.pins.len() && pins.iter().zip(&o.pins).all(|(a, b)| a.number == b.number);
        let (kind, n) = match &o.package {
            PackageSpec::Dual { pins, .. } => (PkgKind::Dual, *pins),
            PackageSpec::Quad { pins, .. } => (PkgKind::Quad, *pins),
            PackageSpec::SingleRow { pins, .. } => (PkgKind::Row, *pins),
            PackageSpec::DoubleRow { pins, .. } => (PkgKind::Row2, *pins),
            _ => return None,
        };
        (same_pins && kind == self.pkg && self.thermal_pin.trim().is_empty() && self.extras.trim().is_empty()).then_some(n)
    }

    /// Carries over what the form cannot show, so that editing a part does not drop it.
    pub(super) fn keep_hidden(&self, def: &mut PartDef) {
        let Some(o) = &self.orig else { return };
        def.shops = o.shops.clone();
        def.swap = o.swap.clone();
        def.programming = o.programming.clone();
        if def.footprint_name.is_none() {
            def.footprint_name = o.footprint_name.clone();
        }
        if let (Some(m), Some(om)) = (def.sim.as_mut(), o.sim.as_ref()) {
            m.params = om.params.clone();
        }
        for p in &mut def.pins {
            p.sim_only = o.pins.iter().any(|q| q.number == p.number && q.sim_only);
        }
    }
}
