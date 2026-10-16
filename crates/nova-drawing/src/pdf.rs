//! Vector PDF 1.4 writer for a sheet (one page, Helvetica text)

use crate::render::{text_width, Align, Item, Layer, Prim, Sheet2D, FONT_EM};
use glam::DVec2;
use std::fmt::Write as _;

const PT: f64 = 72.0 / 25.4;

fn n(v: f64) -> String {
    let s = format!("{:.3}", v * PT);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" {
        "0".into()
    } else {
        s
    }
}

/// Text as a PDF literal string in WinAnsi (Latin-1 for French accents)
fn pdf_string(text: &str) -> Vec<u8> {
    let mut out = vec![b'('];
    for c in text.chars() {
        let b = match c {
            '(' | ')' | '\\' => {
                out.push(b'\\');
                c as u8
            }
            c if (c as u32) < 0x20 => b' ',
            c if (c as u32) < 0x7f => c as u8,
            c if (0xa0..=0xff).contains(&(c as u32)) => c as u32 as u8,
            '€' => 0x80,
            '–' => 0x96,
            '—' => 0x97,
            '’' => 0x92,
            _ => b'?',
        };
        out.push(b);
    }
    out.push(b')');
    out
}

/// Bézier control points for a circular arc of at most 90°
fn arc_bezier(c: DVec2, r: f64, a0: f64, a1: f64) -> [DVec2; 4] {
    let k = 4.0 / 3.0 * ((a1 - a0) / 4.0).tan();
    let p0 = c + DVec2::new(a0.cos(), a0.sin()) * r;
    let p3 = c + DVec2::new(a1.cos(), a1.sin()) * r;
    let p1 = p0 + DVec2::new(-a0.sin(), a0.cos()) * r * k;
    let p2 = p3 - DVec2::new(-a1.sin(), a1.cos()) * r * k;
    [p0, p1, p2, p3]
}

fn arc_path(s: &mut String, c: DVec2, r: f64, start: f64, end: f64) {
    let (a0, mut a1) = (start.to_radians(), end.to_radians());
    while a1 <= a0 {
        a1 += std::f64::consts::TAU;
    }
    let segments = ((a1 - a0) / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = (a1 - a0) / segments as f64;
    for i in 0..segments {
        let b = arc_bezier(c, r, a0 + step * i as f64, a0 + step * (i + 1) as f64);
        if i == 0 {
            let _ = write!(s, "{} {} m ", n(b[0].x), n(b[0].y));
        }
        let _ = writeln!(s, "{} {} {} {} {} {} c", n(b[1].x), n(b[1].y), n(b[2].x), n(b[2].y), n(b[3].x), n(b[3].y));
    }
}

fn content(sheet: &Sheet2D) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"1 J 1 j\n");
    let mut current: Option<Layer> = None;
    for Item { prim, layer, .. } in &sheet.items {
        let mut s = String::new();
        if current != Some(*layer) {
            let [r, g, b] = layer.color().map(|c| c as f64 / 255.0);
            let dashes: Vec<String> = layer.dashes().iter().map(|d| n(*d)).collect();
            let _ = writeln!(s, "{} w [{}] 0 d {r:.3} {g:.3} {b:.3} RG {r:.3} {g:.3} {b:.3} rg", n(layer.width()), dashes.join(" "));
            current = Some(*layer);
        }
        match prim {
            Prim::Line(a, b) => {
                let _ = writeln!(s, "{} {} m {} {} l S", n(a.x), n(a.y), n(b.x), n(b.y));
            }
            Prim::Polyline(pts) => {
                for (i, p) in pts.iter().enumerate() {
                    let _ = write!(s, "{} {} {} ", n(p.x), n(p.y), if i == 0 { "m" } else { "l" });
                }
                s.push_str("S\n");
            }
            Prim::Circle { center, radius } => {
                arc_path(&mut s, *center, *radius, 0.0, 360.0);
                s.push_str("h S\n");
            }
            Prim::Arc { center, radius, start, end } => {
                arc_path(&mut s, *center, *radius, *start, *end);
                s.push_str("S\n");
            }
            Prim::Arrow { tip, dir } => {
                let d = dir.normalize_or_zero();
                let base = *tip - d * 3.0;
                let side = d.perp() * 0.5;
                let (p, q) = (base + side, base - side);
                let _ = writeln!(s, "[] 0 d {} {} m {} {} l {} {} l h f", n(tip.x), n(tip.y), n(p.x), n(p.y), n(q.x), n(q.y));
                // restore the layer's dash pattern
                current = None;
            }
            Prim::Text { at, height, text, align, angle } => {
                let w = text_width(text, *height);
                let shift = match align {
                    Align::Left => 0.0,
                    Align::Center => -w / 2.0,
                    Align::Right => -w,
                };
                let (sa, ca) = angle.to_radians().sin_cos();
                let origin = *at + DVec2::new(ca, sa) * shift;
                let _ = write!(
                    s,
                    "BT /F1 {} Tf {ca:.5} {sa:.5} {:.5} {ca:.5} {} {} Tm ",
                    n(*height * FONT_EM),
                    -sa,
                    n(origin.x),
                    n(origin.y)
                );
                out.extend_from_slice(s.as_bytes());
                out.extend_from_slice(&pdf_string(text));
                out.extend_from_slice(b" Tj ET\n");
                continue;
            }
        }
        out.extend_from_slice(s.as_bytes());
    }
    out
}

/// PDF file bytes for the sheet
pub fn write(sheet: &Sheet2D) -> Vec<u8> {
    let stream = content(sheet);
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>",
            n(sheet.size.x),
            n(sheet.size.y)
        )
        .into_bytes(),
        {
            let mut o = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
            o.extend_from_slice(&stream);
            o.extend_from_slice(b"\nendstream");
            o
        },
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec(),
        b"<< /Producer (NOVARIS CAD) >>".to_vec(),
    ];
    let mut out: Vec<u8> = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes(),
    );
    out
}
