//! Panel edytora liczony matematyką z [`aurum`].
//!
//! To jest miejsce, w którym biblioteka łączy się z resztą projektu: rozmiar
//! panelu, pozycje i marginesy liczymy wektorami i macierzą z
//! [`aurum`](https://docs.rs/aurum), a wynik wysyłamy do VS Code jako komendy
//! `vscode/*`. Dzięki temu „interfejs Rustu” w edytorze jest liczony tą samą
//! matematyką, co reszta programu.
//!
//! ```
//! use aurum::math::Vec2;
//! use vscode_bridge::panel::PanelLayout;
//!
//! let layout = PanelLayout::new(Vec2::new(1280.0, 720.0), 320.0, 16.0);
//! let editor = layout.editor();
//!
//! // 1280 − 320 (panel) − 2×16 (marginesy) = 928 pikseli na edytor.
//! assert_eq!(editor.size.x, 928.0);
//! // Środek okna pomniejszony o margines.
//! assert_eq!(layout.center(), Vec2::new(640.0, 360.0));
//! assert_eq!(editor.center(), Vec2::new(480.0, 360.0));
//! ```

use aurum::math::scalar::clamp;
use aurum::math::Vec2;

use crate::lsp::Position;

/// Układ okna: panel boczny + marginesy, liczony wektorami.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelLayout {
    /// Rozmiar całego okna.
    okno: Vec2,
    /// Szerokość panelu bocznego.
    panel: f32,
    /// Margines wokół treści.
    margines: f32,
}

impl PanelLayout {
    /// Nowy układ dla okna o danym rozmiarze.
    ///
    /// Szerokość panelu jest przycinana do połowy okna, żeby edytor zawsze
    /// miał sensowne minimum szerokości.
    pub fn new(okno: Vec2, panel: f32, margines: f32) -> Self {
        let maksymalny_panel = okno.x * 0.5;
        Self {
            okno,
            panel: clamp(panel, 0.0, maksymalny_panel),
            margines: margines.max(0.0),
        }
    }

    /// Rozmiar okna.
    pub fn size(&self) -> Vec2 {
        self.okno
    }

    /// Środek obszaru roboczego (bez marginesów).
    pub fn center(&self) -> Vec2 {
        self.okno * 0.5
    }

    /// Obszar edytora — okno minus panel i marginesy.
    pub fn editor(&self) -> Rect {
        let szerokosc_edytora = (self.okno.x - self.panel - 2.0 * self.margines).max(0.0);
        let wysokosc_edytora = (self.okno.y - 2.0 * self.margines).max(0.0);
        Rect {
            origin: Vec2::new(self.margines, self.margines),
            size: Vec2::new(szerokosc_edytora, wysokosc_edytora),
        }
    }

    /// Pozycja panelu bocznego.
    pub fn panel(&self) -> Rect {
        let x = self.okno.x - self.panel;
        Rect {
            origin: Vec2::new(x, 0.0),
            size: Vec2::new(self.panel, self.okno.y),
        }
    }

    /// Środek edytora w układzie LSP (do `vscode/insertText`).
    pub fn editor_center_position(&self) -> Position {
        let srodek = self.editor().center();
        Position::new(srodek.y as u32, srodek.x as u32)
    }
}

/// Prostokąt w układzie ekranu (Y w dół, jak w WebGPU).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Lewy górny róg.
    pub origin: Vec2,
    /// Rozmiar.
    pub size: Vec2,
}

impl Rect {
    /// Środek prostokąta.
    pub fn center(&self) -> Vec2 {
        self.origin + self.size * 0.5
    }

    /// Czy punkt mieści się w prostokącie.
    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.origin.x
            && point.x <= self.origin.x + self.size.x
            && point.y >= self.origin.y
            && point.y <= self.origin.y + self.size.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn okno() -> PanelLayout {
        PanelLayout::new(Vec2::new(1280.0, 720.0), 320.0, 16.0)
    }

    #[test]
    fn edytor_to_okno_minus_panel_i_marginesy() {
        let e = okno().editor();
        assert_eq!(e.size.x, 1280.0 - 320.0 - 32.0);
        assert_eq!(e.size.y, 720.0 - 32.0);
        assert_eq!(e.origin, Vec2::new(16.0, 16.0));
    }

    #[test]
    fn panel_siedzi_zprawej() {
        let p = okno().panel();
        assert_eq!(p.origin, Vec2::new(960.0, 0.0));
        assert_eq!(p.size, Vec2::new(320.0, 720.0));
    }

    #[test]
    fn panel_jest_przycinany_do_polowy_okna() {
        let szeroki = PanelLayout::new(Vec2::new(1000.0, 800.0), 900.0, 0.0);
        assert_eq!(szeroki.panel().size.x, 500.0);
        assert!(szeroki.editor().size.x >= 0.0);
    }

    #[test]
    fn ujemne_wartosci_sa_neutralizowane() {
        let layout = PanelLayout::new(Vec2::new(800.0, 600.0), -50.0, -10.0);
        assert_eq!(layout.panel().size.x, 0.0);
        assert_eq!(layout.editor().origin.y, 0.0);
    }

    #[test]
    fn srodek_prostokata_to_geometryczny_srodek() {
        let r = Rect {
            origin: Vec2::new(10.0, 20.0),
            size: Vec2::new(100.0, 40.0),
        };
        assert_eq!(r.center(), Vec2::new(60.0, 40.0));
        assert!(r.contains(Vec2::new(60.0, 40.0)));
        assert!(!r.contains(Vec2::new(9.0, 40.0)));
    }

    #[test]
    fn pozycja_srodka_jest_poprawna_lsp() {
        let p = okno().editor_center_position();
        assert!(p.line < 720 && p.character < 1280, "{p:?}");
    }
}
