//! Device equations: junction limiting, the transistor and MOSFET models.

use super::*;
use crate::circuit::DiodeModel;

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

/// Recombination current of a diode (`isr * (exp(v / (nr vt)) - 1)`) and its conductance.
pub(crate) fn diode_recomb(m: &DiodeModel, v: f64) -> (f64, f64) {
    if m.isr <= 0.0 {
        return (0.0, 0.0);
    }
    let nvt = m.nr * VT;
    let e = exp_safe(v / nvt);
    (m.isr * (e - 1.0), m.isr / nvt * e)
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
    // high injection: the transport current is divided by (1 + sqrt(1 + 4 q2)) / 2 with q2 = if/ikf + ir/ikr
    let (inv_f, inv_r) = (if m.ikf.is_finite() { 1.0 / m.ikf } else { 0.0 }, if m.ikr.is_finite() { 1.0 / m.ikr } else { 0.0 });
    let q2 = iff * inv_f + ir * inv_r;
    let (h, dh) = if q2 > 0.0 {
        let s = (1.0 + 4.0 * q2).sqrt();
        (2.0 / (1.0 + s), -4.0 / ((1.0 + s) * (1.0 + s) * s))
    } else {
        (1.0, 0.0)
    };
    // leakage currents of the two junctions
    let leak = |is: f64, n: f64, v: f64| if is > 0.0 { let e = exp_safe(v / (n * VT)); (is * (e - 1.0), is / (n * VT) * e) } else { (0.0, 0.0) };
    let ((ile, gle), (ilc, glc)) = (leak(m.ise, m.ne, vbe), leak(m.isc, m.nc, vbc));
    let it = iff - ir;
    let tk = it * k * h;
    let d_tk = (gf * k * h + it * dk_be * h + it * k * dh * gf * inv_f, -gr * k * h + it * dk_bc * h + it * k * dh * gr * inv_r);
    let ic = tk - ir / m.br - ilc;
    let ib = iff / m.bf + ile + ir / m.br + ilc;
    let dic = (d_tk.0, d_tk.1 - gr / m.br - glc);
    let dib = (gf / m.bf + gle, gr / m.br + glc);
    BjtEval { ic, ib, ie: -(ic + ib), dic, dib, die: (-(dic.0 + dib.0), -(dic.1 + dib.1)) }
}

/// Charge and capacitance (C = dQ/dV) of a junction's depletion layer at `v`: the usual
/// `cj0 / (1 - v/vj)^m` below `fc * vj` and its tangent line above (so the charge stays finite in forward bias).
pub(crate) fn depletion(cj0: f64, vj: f64, m: f64, fc: f64, v: f64) -> (f64, f64) {
    if cj0 <= 0.0 {
        return (0.0, 0.0);
    }
    let vx = fc * vj;
    let charge = |v: f64| if (m - 1.0).abs() < 1e-9 { -cj0 * vj * (1.0 - v / vj).ln() } else { cj0 * vj / (1.0 - m) * (1.0 - (1.0 - v / vj).powf(1.0 - m)) };
    if v < vx {
        (charge(v), cj0 / (1.0 - v / vj).powf(m))
    } else {
        // tangent at vx: C(v) = c0 + c1 (v - vx)
        let c0 = cj0 / (1.0 - fc).powf(m);
        let c1 = c0 * m / (vj * (1.0 - fc));
        let d = v - vx;
        (charge(vx) + c0 * d + 0.5 * c1 * d * d, c0 + c1 * d)
    }
}

/// Charge and capacitance of a diode junction at `v` (depletion plus the diffusion charge `tt * id`).
pub(crate) fn diode_q(m: &DiodeModel, nvt: f64, v: f64) -> (f64, f64) {
    let (qd, cd) = depletion(m.cj0, m.vj, m.m, m.fc, v);
    let ex = exp_safe(v / nvt);
    (qd + m.tt * m.is * (ex - 1.0), cd + m.tt * m.is / nvt * ex)
}

/// Stored charge and capacitance of the base-emitter junction (depletion plus the forward diffusion charge).
pub(crate) fn bjt_qbe(m: &BjtModel, vbe: f64) -> (f64, f64) {
    let (qd, cd) = depletion(m.cje, m.vje, m.mje, m.fc, vbe);
    let nfvt = m.nf * VT;
    let ef = exp_safe(vbe / nfvt);
    (qd + m.tf * m.is * (ef - 1.0), cd + m.tf * m.is / nfvt * ef)
}

/// Stored charge and capacitance of the base-collector junction.
pub(crate) fn bjt_qbc(m: &BjtModel, vbc: f64) -> (f64, f64) {
    let (qd, cd) = depletion(m.cjc, m.vjc, m.mjc, m.fc, vbc);
    let nrvt = m.nr * VT;
    let er = exp_safe(vbc / nrvt);
    (qd + m.tr * m.is * (er - 1.0), cd + m.tr * m.is / nrvt * er)
}

/// Level-1 MOSFET in NMOS orientation: `(id, gm, gds)` with `id` flowing drain to source.
/// The device is symmetric, so for `vds < 0` source and drain swap roles.
pub(crate) fn mos_eval(m: &MosModel, vgs: f64, vds: f64) -> (f64, f64, f64) {
    pub(super) fn forward(m: &MosModel, vgs: f64, vds: f64) -> (f64, f64, f64) {
        let vov = vgs - if m.jfet { m.vto } else { m.vto.abs() };
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
