//! A pairing URI as QR code modules, for the window to draw.

use serde::Serialize;

/// A square of `size` × `size` modules, row by row; `true` is dark.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Qr {
    pub size: usize,
    pub modules: Vec<bool>,
}

impl Qr {
    pub fn of(text: &str) -> qrcode::types::QrResult<Qr> {
        let code = qrcode::QrCode::new(text.as_bytes())?;
        let modules = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
        Ok(Qr { size: code.width(), modules })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_has_its_finder_patterns_in_three_corners() {
        let qr = Qr::of("clipsync://pair?v=1").unwrap();
        assert_eq!(qr.modules.len(), qr.size * qr.size);
        let dark = |x: usize, y: usize| qr.modules[y * qr.size + x];
        // A finder pattern is a dark 7×7 ring around a light ring around a dark 3×3 centre.
        for (x0, y0) in [(0, 0), (qr.size - 7, 0), (0, qr.size - 7)] {
            for i in 0..7 {
                assert!(dark(x0 + i, y0) && dark(x0 + i, y0 + 6) && dark(x0, y0 + i) && dark(x0 + 6, y0 + i));
            }
            assert!(!dark(x0 + 1, y0 + 1) && dark(x0 + 3, y0 + 3));
        }
        assert!(!dark(qr.size - 4, qr.size - 4), "no finder pattern bottom right");
    }
}
