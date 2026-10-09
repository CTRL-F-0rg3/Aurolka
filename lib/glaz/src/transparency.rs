//! Przezroczystość okna i efekty „backdrop” (rozmycie, akryl, mica).
//!
//! To jest serce biblioteki: pozwala zbudować okno, które rysuje własne
//! dekoracje i ma przezroczyste tło, opcjonalnie z rozmyciem zawartości
//! narysowanej wcześniej w klatce.
//!
//! # Jak to działa
//!
//! 1. Okno jest tworzone z `wgpu::CompositeAlphaMode::PreMultiplied`, a scena
//!    jest renderowana do tekstury `rgba16float`, zamiast prosto na surface.
//! 2. Gdy w klatce pojawi się element z [`Backdrop`], renderer kopiuje aktualną
//!    zawartość sceny, wykonuje separable gaussian blur (poziomy + pionowy),
//!    a dalej rysowane elementy backdropowe próbkują rozmyty bufor.
//! 3. Na końcu scena jest komponowana na surface z maską kształtu okna,
//!    co daje przezroczyste narożniki.
//!
//! Dzięki temu ten sam kod działa na X11, Waylandzie, Windows i macOS, bez
//! zależności od natywnych API composytora. Efekty *systemowe* (np. natywny
//! acrylic Windows 11) są dostępne przez [`crate::window::PlatformEffects`].

use crate::geometry::{Color, Radius, Rect};

/// Tryb przezroczystości okna.
///
/// Wariant jest `#[non_exhaustive]`, więc przyszłe tryby nie złamią kompatybilności.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Transparency {
    /// Okno w pełni nieprzezroczyste. Najwydajniejsze — pomijamy cały
    /// pipeline pośredni i komponujemy wprost na surface.
    Opaque,
    /// Przezroczyste tło okna, bez efektów. Piksele o `alpha == 0` są
    /// faktycznie przezroczyste dla composytora.
    Alpha,
    /// Rozmycie zawartości sceny (acrylic / mica / frosted glass).
    Blur(Backdrop),
}

impl Default for Transparency {
    fn default() -> Self {
        Self::Opaque
    }
}

impl Transparency {
    /// Czy wymaga renderowania pośredniego (tekstury sceny).
    pub fn requires_offscreen(&self) -> bool {
        match self {
            Self::Opaque => false,
            Self::Alpha | Self::Blur(_) => true,
        }
    }

    /// Czy okno powinno być utworzone jako przezroczyste na poziomie systemu okien.
    ///
    /// Dla [`Transparency::Opaque`] zwracamy `false`, co pozwala systemowi
    /// okien zastosować natywny kompozyt i ominąć tryb alpha.
    pub fn is_transparent(&self) -> bool {
        !matches!(self, Self::Opaque)
    }

    /// Parametry rozmycia, jeśli tryb ich wymaga.
    pub fn backdrop(&self) -> Option<&Backdrop> {
        match self {
            Self::Blur(b) => Some(b),
            _ => None,
        }
    }
}

/// Efekt rozmycia tła.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Backdrop {
    /// Promień rozmycia w pikselach logicznych (`0.0` = bez rozmycia).
    pub radius: f32,
    /// Liczba dodatkowych przebiegów rozmycia (0 = tylko H+V).
    ///
    /// Więcej przebiegów daje gładszy wynik przy większym koszcie.
    pub passes: u8,
    /// Barwa nasycona na rozmycie (np. delikatny akcent kolorystyczny).
    pub tint: Color,
    /// Mnożnik nasycenia barwy rozmycia (`1.0` = bez zmian).
    pub saturation: f32,
    /// Kształt okna — używany do maskowania rozmycia i kompozycji.
    pub shape: WindowShape,
}

impl Default for Backdrop {
    fn default() -> Self {
        Self {
            radius: 24.0,
            passes: 0,
            tint: Color::WHITE,
            saturation: 1.0,
            shape: WindowShape::Rectangle,
        }
    }
}

impl Backdrop {
    /// Rozmycie o zadanym promieniu, bez dodatkowej barwy.
    pub fn blur(radius: f32) -> Self {
        Self {
            radius: radius.max(0.0),
            ..Self::default()
        }
    }

    /// Styl „acrylic” — mocne rozmycie z subtelną barwą.
    pub fn acrylic(tint: Color) -> Self {
        Self {
            radius: 40.0,
            passes: 1,
            tint,
            saturation: 1.15,
            shape: WindowShape::rounded(12.0),
        }
    }

    /// Styl „mica” — subtelniejsze rozmycie niż acrylic.
    pub fn mica(tint: Color) -> Self {
        Self {
            radius: 24.0,
            passes: 0,
            tint,
            saturation: 1.0,
            shape: WindowShape::Rectangle,
        }
    }

    /// Ustawia kształt okna.
    pub fn shape(mut self, shape: WindowShape) -> Self {
        self.shape = shape;
        self
    }

    /// Ustawia dodatkowe przebiegi rozmycia (jakość).
    pub fn quality(mut self, passes: u8) -> Self {
        self.passes = passes.min(3);
        self
    }
}

/// Kształt powierzchni okna.
///
/// Kształt jest maskowany w shaderze kompozycji, więc działa niezależnie od
/// systemu okien — okno może mieć zaokrąglone narożniki także na X11/Wayland.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum WindowShape {
    /// Prostokąt o ostrych krawędziach.
    #[default]
    Rectangle,
    /// Prostokąt z zaokrąglonymi narożnikami.
    Rounded(Radius),
}

impl WindowShape {
    /// Promienie narożników (zerowe dla prostokąta).
    pub fn radius(&self) -> Radius {
        match self {
            Self::Rectangle => Radius::zero(),
            Self::Rounded(r) => *r,
        }
    }

    /// Zaokrąglone narożniki o zadanym promieniu.
    pub fn rounded(radius: f32) -> Self {
        Self::Rounded(Radius::uniform(radius))
    }

    /// Przycina maskę do prostokąta (używane przy kompozycji).
    pub fn mask(&self, bounds: Rect) -> MaskRegion {
        match self {
            Self::Rectangle => MaskRegion::Rect(bounds),
            Self::Rounded(radius) => MaskRegion::Rounded {
                bounds,
                radius: radius.clamp(bounds.size),
            },
        }
    }
}

/// Opis maski używanej w shaderze kompozycji.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MaskRegion {
    /// Zwykły prostokąt.
    Rect(Rect),
    /// Prostokąt z zaokrąglonymi narożnikami.
    Rounded {
        /// Obszar maski.
        bounds: Rect,
        /// Promienie narożników.
        radius: Radius,
    },
}

impl MaskRegion {
    /// Promienie narożników maski.
    pub fn radius(&self) -> Radius {
        match self {
            Self::Rect(_) => Radius::zero(),
            Self::Rounded { radius, .. } => *radius,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Point, Size};

    #[test]
    fn opaque_needs_no_offscreen() {
        assert!(!Transparency::Opaque.requires_offscreen());
        assert!(!Transparency::Opaque.is_transparent());
    }

    #[test]
    fn alpha_mode_is_transparent() {
        assert!(Transparency::Alpha.is_transparent());
        assert!(Transparency::Alpha.requires_offscreen());
        assert!(Transparency::Alpha.backdrop().is_none());
    }

    #[test]
    fn blur_exposes_backdrop() {
        let t = Transparency::Blur(Backdrop::blur(12.0));
        assert_eq!(t.backdrop().map(|b| b.radius), Some(12.0));
    }

    #[test]
    fn quality_is_capped() {
        assert_eq!(Backdrop::default().quality(99).passes, 3);
    }

    #[test]
    fn mask_clamps_radius() {
        let shape = WindowShape::rounded(500.0);
        let MaskRegion::Rounded { radius, .. } =
            shape.mask(Rect::new(Point::ZERO, Size::new(40.0, 40.0)))
        else {
            panic!("oczekiwano zaokrąglonej maski");
        };
        assert_eq!(radius.top_left, 20.0);
    }

    #[test]
    fn acrylic_has_rounded_shape() {
        assert!(matches!(
            Backdrop::acrylic(Color::WHITE).shape,
            WindowShape::Rounded(_)
        ));
    }
}
