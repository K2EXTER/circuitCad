//! Device equations: junction limiting, the transistor and MOSFET models.

use super::*;

pub(crate) fn exp_safe(x: f64) -> f64 {
    x.min(80.0).exp()
}

/// SPICE3 junction voltage limiting. Returns the limited voltage.
pub(super) fn pnjlim(vnew: f64, vold: f64, vt: f64, vcrit: f64) -> f64 {
    if vnew > vcrit && (vnew - vold).abs() > 2.0 * vt {
        if vold > 0.0 {
            let arg = (vnew - vold) / vt;
            if arg > 0.0 {
                vold + vt * (2.0 + (arg - 2.0).ln())
            } else {
                vold - vt * (2.0 + (2.0 - arg).ln())
            }
        } else {
            vt * (vnew / vt).ln()
        }
    } else {
        vnew
    }
}

/// Transistor in "NPN space": currents into C, B, E and the partial derivatives
/// with respect to (vbe, vbc).
pub(crate) struct BjtEval {
    pub(crate) ic: f64,
    pub(crate) ib: f64,
    pub(crate) ie: f64,
    pub(crate) dic: (f64, f64),
    pub(crate) dib: (f64, f64),
    pub(crate) die: (f64, f64),
}

pub(crate) fn bjt_eval(m: &BjtModel, vbe: f64, vbc: f64) -> BjtEval {
    let (nfvt, nrvt) = (m.nf * VT, m.nr * VT);
    let ef = exp_safe(vbe / nfvt);
    let er = exp_safe(vbc / nrvt);
    let (iff, ir) = (m.is * (ef - 1.0), m.is * (er - 1.0));
    let (gf, gr) = (m.is / nfvt * ef, m.is / nrvt * er);
    // Early effect: the transport current is scaled by k = 1 - vbc/vaf - vbe/var (kept above 0.05)
    let (dk_be, dk_bc) = (if m.var.is_finite() { -1.0 / m.var } else { 0.0 }, if m.vaf.is_finite() { -1.0 / m.vaf } else { 0.0 });
    let k_raw = 1.0 + dk_be * vbe + dk_bc * vbc;
    let (k, dk_be, dk_bc) = if k_raw > 0.05 { (k_raw, dk_be, dk_bc) } else { (0.05, 0.0, 0.0) };
    let it = iff - ir;
    let ic = it * k - ir / m.br;
    let ib = iff / m.bf + ir / m.br;
    let dic = (gf * k + it * dk_be, -gr * k + it * dk_bc - gr / m.br);
    let dib = (gf / m.bf, gr / m.br);
    BjtEval { ic, ib, ie: -(ic + ib), dic, dib, die: (-(dic.0 + dib.0), -(dic.1 + dib.1)) }
}

/// Level-1 MOSFET in NMOS orientation: `(id, gm, gds)` with `id` flowing drain to source.
/// The device is symmetric, so for `vds < 0` source and drain swap roles.
pub(crate) fn mos_eval(m: &MosModel, vgs: f64, vds: f64) -> (f64, f64, f64) {
    pub(super) fn forward(m: &MosModel, vgs: f64, vds: f64) -> (f64, f64, f64) {
        let vov = vgs - m.vto.abs();
        if vov <= 0.0 {
            return (0.0, 0.0, 0.0);
        }
        let clm = 1.0 + m.lambda * vds;
        if vds < vov {
            let id0 = m.kp * (vov * vds - 0.5 * vds * vds);
            (id0 * clm, m.kp * vds * clm, m.kp * (vov - vds) * clm + id0 * m.lambda)
        } else {
            let id0 = 0.5 * m.kp * vov * vov;
            (id0 * clm, m.kp * vov * clm, id0 * m.lambda)
        }
    }
    if vds >= 0.0 {
        forward(m, vgs, vds)
    } else {
        let (id, gm, gds) = forward(m, vgs - vds, -vds);
        (-id, -gm, gm + gds)
    }
}
