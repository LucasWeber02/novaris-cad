//! DXF R12 (AC1009) writer: one layer per line type, dimensions exploded
//! into lines, arrows (SOLID) and texts. Units are millimetres.

use crate::render::{Align, Item, Layer, Prim, Sheet2D};
use glam::DVec2;
use std::fmt::Write as _;

fn layer_color(l: Layer) -> u8 {
    match l {
        Layer::Visible | Layer::Frame => 7,
        Layer::Hidden => 8,
        Layer::Tangent => 9,
        Layer::Center => 4,
        Layer::Dimension => 3,
        Layer::Error => 1,
        Layer::Thin => 7,
    }
}

fn layer_ltype(l: Layer) -> &'static str {
    match l {
        Layer::Hidden => "DASHED",
        Layer::Center => "CENTER",
        _ => "CONTINUOUS",
    }
}

struct W(String);

impl W {
    fn g(&mut self, code: i32, value: impl std::fmt::Display) {
        let _ = writeln!(self.0, "{code:>3}\n{value}");
    }

    fn f(&mut self, code: i32, v: f64) {
        self.g(code, format!("{v:.6}"));
    }

    fn pt(&mut self, base: i32, p: DVec2) {
        self.f(base, p.x);
        self.f(base + 10, p.y);
        self.f(base + 20, 0.0);
    }
}

/// DXF special codes for Ø and °; other characters stay as they are
fn dxf_text(t: &str) -> String {
    t.replace('Ø', "%%c").replace('°', "%%d")
}

/// DXF file bytes (Windows-1252, as declared by $DWGCODEPAGE)
pub fn write(sheet: &Sheet2D) -> Vec<u8> {
    let mut w = W(String::new());
    w.g(0, "SECTION");
    w.g(2, "HEADER");
    w.g(9, "$ACADVER");
    w.g(1, "AC1009");
    w.g(9, "$DWGCODEPAGE");
    w.g(3, "ANSI_1252");
    w.g(9, "$EXTMIN");
    w.pt(10, DVec2::ZERO);
    w.g(9, "$EXTMAX");
    w.pt(10, sheet.size);
    w.g(9, "$LTSCALE");
    w.f(40, 1.0);
    w.g(0, "ENDSEC");

    w.g(0, "SECTION");
    w.g(2, "TABLES");
    w.g(0, "TABLE");
    w.g(2, "LTYPE");
    w.g(70, 3);
    for (name, desc, pattern) in [
        ("CONTINUOUS", "Solid line", &[][..]),
        ("DASHED", "Dashed __ __ __", &[3.0, -1.5][..]),
        ("CENTER", "Center ____ _ ____", &[8.0, -1.5, 1.0, -1.5][..]),
    ] {
        w.g(0, "LTYPE");
        w.g(2, name);
        w.g(70, 0);
        w.g(3, desc);
        w.g(72, 65);
        w.g(73, pattern.len());
        w.f(40, pattern.iter().map(|p: &f64| p.abs()).sum());
        for p in pattern {
            w.f(49, *p);
        }
    }
    w.g(0, "ENDTAB");
    w.g(0, "TABLE");
    w.g(2, "LAYER");
    w.g(70, Layer::ALL.len());
    for l in Layer::ALL {
        w.g(0, "LAYER");
        w.g(2, l.name());
        w.g(70, 0);
        w.g(62, layer_color(l));
        w.g(6, layer_ltype(l));
    }
    w.g(0, "ENDTAB");
    w.g(0, "ENDSEC");

    w.g(0, "SECTION");
    w.g(2, "ENTITIES");
    for Item { prim, layer, .. } in &sheet.items {
        let l = layer.name();
        match prim {
            Prim::Line(a, b) => {
                w.g(0, "LINE");
                w.g(8, l);
                w.pt(10, *a);
                w.pt(11, *b);
            }
            Prim::Polyline(pts) => {
                w.g(0, "POLYLINE");
                w.g(8, l);
                w.g(66, 1);
                w.pt(10, DVec2::ZERO);
                w.g(70, 0);
                for p in pts {
                    w.g(0, "VERTEX");
                    w.g(8, l);
                    w.pt(10, *p);
                }
                w.g(0, "SEQEND");
                w.g(8, l);
            }
            Prim::Circle { center, radius } => {
                w.g(0, "CIRCLE");
                w.g(8, l);
                w.pt(10, *center);
                w.f(40, *radius);
            }
            Prim::Arc { center, radius, start, end } => {
                w.g(0, "ARC");
                w.g(8, l);
                w.pt(10, *center);
                w.f(40, *radius);
                w.f(50, start.rem_euclid(360.0));
                w.f(51, end.rem_euclid(360.0));
            }
            Prim::Arrow { tip, dir } => {
                let d = dir.normalize_or_zero();
                let base = *tip - d * 3.0;
                let side = d.perp() * 0.5;
                w.g(0, "SOLID");
                w.g(8, l);
                w.pt(10, *tip);
                w.pt(11, base + side);
                w.pt(12, base - side);
                w.pt(13, base - side);
            }
            Prim::Text { at, height, text, align, angle } => {
                w.g(0, "TEXT");
                w.g(8, l);
                w.pt(10, *at);
                w.f(40, *height);
                w.g(1, dxf_text(text));
                w.f(50, *angle);
                let h = match align {
                    Align::Left => 0,
                    Align::Center => 1,
                    Align::Right => 2,
                };
                if h != 0 {
                    w.g(72, h);
                    w.pt(11, *at);
                }
            }
        }
    }
    w.g(0, "ENDSEC");
    w.g(0, "EOF");
    // Windows-1252: Latin-1 range maps one to one
    w.0.chars().map(|c| if (c as u32) < 0x100 { c as u32 as u8 } else { b'?' }).collect()
}
