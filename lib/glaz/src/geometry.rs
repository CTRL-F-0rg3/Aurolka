//! Typy geometryczne używane przez całą bibliotekę.
//!
//! Wszystkie typy w tym module są „value types” — kopiowalne, tanie i bez
//! referencji. Nie re-eksportujemy tu typów `wgpu`, dzięki czemu API biblioteki
//! nie jest sprzężone z backendem graficznym.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// Dwuwymiarowy punkt w przestrzeni logicznej (piksele DPI-independent).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    /// Składowa pozioma.
    pub x: f32,
    /// Składowa pionowa.
    pub y: f32,
}

impl Point {
    /// Punkt `(0, 0)`.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    /// Nowy punkt.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

impl From<(f32, f32)> for Point {
    fn from((x, y): (f32, f32)) -> Self {
        Self::new(x, y)
    }
}

impl From<Point> for (f32, f32) {
    fn from(p: Point) -> Self {
        (p.x, p.y)
    }
}

impl Add for Point {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl AddAssign for Point {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for Point {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl SubAssign for Point {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl Mul<f32> for Point {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.x * rhs, self.y * rhs)
    }
}

impl Div<f32> for Point {
    type Output = Self;
    fn div(self, rhs: f32) -> Self {
        Self::new(self.x / rhs, self.y / rhs)
    }
}

impl Neg for Point {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y)
    }
}

/// Rozmiar prostokąta (szerokość i wysokość, nigdy ujemne).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Size {
    /// Szerokość.
    pub width: f32,
    /// Wysokość.
    pub height: f32,
}

impl Size {
    /// Rozmiar zerowy.
    pub const ZERO: Self = Self {
        width: 0.0,
        height: 0.0,
    };

    /// Nowy rozmiar. Wartości ujemne są przycinane do zera.
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            // `f32::max` zamienia `NaN` na drugi argument, co po cichu
            // naprawiałoby błędne dane. Wolimy `NaN` przepuścić — dzięki temu
            // `is_valid()` faktycznie je wychwyci.
            width: if width.is_nan() {
                width
            } else {
                width.max(0.0)
            },
            height: if height.is_nan() {
                height
            } else {
                height.max(0.0)
            },
        }
    }

    /// Rozmiar z linii wektorowej, przydatny np. do proporcji.
    pub fn from_ratio(ratio: f32) -> Self {
        Self::new(ratio, 1.0)
    }

    /// Oblicza rozmiar z zachowaniem proporcji `ratio` mieszcząc się w `bounds`.
    pub fn fit(self, bounds: Size, ratio: f32) -> Self {
        let w = bounds.width;
        let h = w / ratio;
        if h > bounds.height {
            Size::new(bounds.height * ratio, bounds.height)
        } else {
            Size::new(w, h)
        }
    }

    /// Zwraca `true`, jeśli oba wymiary są skończone i nieujemne.
    pub fn is_valid(&self) -> bool {
        self.width.is_finite() && self.height.is_finite() && self.width >= 0.0 && self.height >= 0.0
    }

    /// Zmniejsza oba wymiary (nigdy poniżej zera).
    pub fn shrink(self, amount: Size) -> Self {
        Self::new(self.width - amount.width, self.height - amount.height)
    }

    /// Zwiększa oba wymiary.
    pub fn expand(self, amount: Size) -> Self {
        Self::new(self.width + amount.width, self.height + amount.height)
    }
}

impl From<(f32, f32)> for Size {
    fn from((width, height): (f32, f32)) -> Self {
        Self::new(width, height)
    }
}

impl From<Size> for (f32, f32) {
    fn from(s: Size) -> Self {
        (s.width, s.height)
    }
}

impl Add for Size {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.width + rhs.width, self.height + rhs.height)
    }
}

impl AddAssign for Size {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Sub for Size {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.width - rhs.width, self.height - rhs.height)
    }
}

impl Mul<f32> for Size {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self::new(self.width * rhs, self.height * rhs)
    }
}

/// Prostokąt opisany przez pozycję lewego górnego rogu i rozmiar.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    /// Lewy górny róg.
    pub position: Point,
    /// Rozmiar.
    pub size: Size,
}

impl Rect {
    /// Prostokąt zerowy w punkcie `(0, 0)`.
    pub const ZERO: Self = Self {
        position: Point::ZERO,
        size: Size::ZERO,
    };

    /// Prostokąt w pełni pokrywający daną powierzchnię.
    pub fn new(position: Point, size: Size) -> Self {
        Self { position, size }
    }

    /// Prostokąt zdefiniowany przez dwie przeciwległe narożniki (normalizowane).
    pub fn from_corners(a: Point, b: Point) -> Self {
        let position = Point::new(a.x.min(b.x), a.y.min(b.y));
        let size = Size::new((a.x - b.x).abs(), (a.y - b.y).abs());
        Self { position, size }
    }

    /// Lewa krawędź.
    pub fn left(&self) -> f32 {
        self.position.x
    }

    /// Górna krawędź.
    pub fn top(&self) -> f32 {
        self.position.y
    }

    /// Prawa krawędź.
    pub fn right(&self) -> f32 {
        self.position.x + self.size.width
    }

    /// Dolna krawędź.
    pub fn bottom(&self) -> f32 {
        self.position.y + self.size.height
    }

    /// Środek prostokąta.
    pub fn center(&self) -> Point {
        Point::new(
            self.position.x + self.size.width / 2.0,
            self.position.y + self.size.height / 2.0,
        )
    }

    /// Zwraca `true`, gdy punkt leży wewnątrz prostokąta.
    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.left()
            && point.x <= self.right()
            && point.y >= self.top()
            && point.y <= self.bottom()
    }

    /// Przesuwa prostokąt o podany wektor.
    pub fn translate(&self, offset: Point) -> Self {
        Self {
            position: self.position + offset,
            size: self.size,
        }
    }

    /// Zwraca `true`, gdy prostokąty mają wspólny obszar.
    pub fn intersects(&self, other: &Rect) -> bool {
        self.left() < other.right()
            && other.left() < self.right()
            && self.top() < other.bottom()
            && other.top() < self.bottom()
    }

    /// Część wspólna dwóch prostokątów (może być pusta).
    pub fn intersection(&self, other: &Rect) -> Rect {
        Rect::from_corners(
            Point::new(self.left().max(other.left()), self.top().max(other.top())),
            Point::new(
                self.right().min(other.right()),
                self.bottom().min(other.bottom()),
            ),
        )
    }

    /// Zawęża prostokąt o zadane marginesy.
    pub fn shrink(&self, padding: Padding) -> Rect {
        Rect {
            position: Point::new(self.left() + padding.left, self.top() + padding.top),
            size: Size::new(
                (self.size.width - padding.left - padding.right).max(0.0),
                (self.size.height - padding.top - padding.bottom).max(0.0),
            ),
        }
    }

    /// Rozszerza prostokąt o zadane marginesy.
    pub fn expand(&self, padding: Padding) -> Rect {
        Rect {
            position: Point::new(self.left() - padding.left, self.top() - padding.top),
            size: Size::new(
                self.size.width + padding.left + padding.right,
                self.size.height + padding.top + padding.bottom,
            ),
        }
    }

    /// Najmniejszy prostokąt zawierający oba zbiory.
    pub fn union(&self, other: Rect) -> Rect {
        Rect::from_corners(
            Point::new(self.left().min(other.left()), self.top().min(other.top())),
            Point::new(
                self.right().max(other.right()),
                self.bottom().max(other.bottom()),
            ),
        )
    }

    /// Zaokrągla pozycję i rozmiar do całkowitych pikseli.
    pub fn round(&self) -> Rect {
        let left = self.left().round();
        let top = self.top().round();
        let right = self.right().round();
        let bottom = self.bottom().round();
        Rect {
            position: Point::new(left, top),
            size: Size::new(right - left, bottom - top),
        }
    }
}

impl From<(Point, Size)> for Rect {
    fn from((position, size): (Point, Size)) -> Self {
        Self { position, size }
    }
}

/// Odstępy wewnątrz prostokąta (padding / inset).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Padding {
    /// Odstęp od lewej krawędzi.
    pub left: f32,
    /// Odstęp od górnej krawędzi.
    pub top: f32,
    /// Odstęp od prawej krawędzi.
    pub right: f32,
    /// Odstęp od dolnej krawędzi.
    pub bottom: f32,
}

impl Padding {
    /// Brak odstępów.
    pub const ZERO: Self = Self {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    /// Symetryczne odstępy.
    pub const fn all(value: f32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    /// Odstępy poziome i pionowe.
    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            top: vertical,
            right: horizontal,
            bottom: vertical,
        }
    }

    /// Odstęp tylko z lewej.
    pub const fn left(value: f32) -> Self {
        Self {
            left: value,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        }
    }

    /// Odstęp tylko z góry.
    pub const fn top(value: f32) -> Self {
        Self {
            left: 0.0,
            top: value,
            right: 0.0,
            bottom: 0.0,
        }
    }

    /// Odstęp tylko z prawej.
    pub const fn right(value: f32) -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: value,
            bottom: 0.0,
        }
    }

    /// Odstęp tylko z dołu.
    pub const fn bottom(value: f32) -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: 0.0,
            bottom: value,
        }
    }

    /// Odstęp tylko poziomy.
    pub const fn horizontal(value: f32) -> Self {
        Self {
            left: value,
            top: 0.0,
            right: value,
            bottom: 0.0,
        }
    }

    /// Odstęp tylko pionowy.
    pub const fn vertical(value: f32) -> Self {
        Self {
            left: 0.0,
            top: value,
            right: 0.0,
            bottom: value,
        }
    }

    /// Suma odstępów w poziomie.
    pub fn horizontal_total(&self) -> f32 {
        self.left + self.right
    }

    /// Suma odstępów w pionie.
    pub fn vertical_total(&self) -> f32 {
        self.top + self.bottom
    }
}

impl From<f32> for Padding {
    fn from(value: f32) -> Self {
        Self::all(value)
    }
}

impl From<(f32, f32, f32, f32)> for Padding {
    /// Kolejność: lewa, górna, prawa, dolna.
    fn from((left, top, right, bottom): (f32, f32, f32, f32)) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
}

/// Promienie zaokrąglenia narożników.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Radius {
    /// Lewy górny narożnik.
    pub top_left: f32,
    /// Prawy górny narożnik.
    pub top_right: f32,
    /// Prawy dolny narożnik.
    pub bottom_right: f32,
    /// Lewy dolny narożnik.
    pub bottom_left: f32,
}

impl Radius {
    /// Wszystkie narożniki o tym samym promieniu.
    pub const fn uniform(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    /// Brak zaokrąglenia.
    pub const fn zero() -> Self {
        Self::uniform(0.0)
    }

    /// Ogranicza promienie do połowy rozmiaru, aby uniknąć artefaktów.
    pub fn clamp(self, size: Size) -> Self {
        let limit = (size.width.min(size.height) * 0.5).max(0.0);
        Self {
            top_left: self.top_left.clamp(0.0, limit),
            top_right: self.top_right.clamp(0.0, limit),
            bottom_right: self.bottom_right.clamp(0.0, limit),
            bottom_left: self.bottom_left.clamp(0.0, limit),
        }
    }
}

/// Wyrównanie w osi poziomej lub pionowej.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Początek osi (lewo / góra).
    #[default]
    Start,
    /// Środek osi.
    Center,
    /// Koniec osi (prawo / dół).
    End,
}

impl Align {
    /// Wyrównuje blok o rozmiarze `content` w dostępnej przestrzeni.
    pub fn position(self, offset: f32, available: f32, content: f32) -> f32 {
        match self {
            Self::Start => offset,
            Self::Center => offset + (available - content) / 2.0,
            Self::End => offset + available - content,
        }
    }
}

/// Kierunek osi wiodącej układu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Od góry do dołu.
    Vertical,
    /// Od lewej do prawej.
    Horizontal,
}

impl Direction {
    /// Oś odpowiadająca kierunkowi.
    pub fn axis(self) -> Axis {
        match self {
            Self::Vertical => Axis::Vertical,
            Self::Horizontal => Axis::Horizontal,
        }
    }

    /// Kierunek przeciwny.
    pub fn reverse(self) -> Self {
        match self {
            Self::Vertical => Self::Horizontal,
            Self::Horizontal => Self::Vertical,
        }
    }
}

/// Oś geometryczna.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Oś pozioma.
    Horizontal,
    /// Oś pionowa.
    Vertical,
}

impl Axis {
    /// Wymiar odpowiadający osi.
    pub fn size(self, size: Size) -> f32 {
        match self {
            Self::Horizontal => size.width,
            Self::Vertical => size.height,
        }
    }
}

/// Wymagany rozmiar elementu w układzie.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    /// Stały rozmiar w pikselach logicznych.
    Fixed(f32),
    /// Zajmij całą dostępną przestrzeń.
    Fill,
    /// Zajmij podany ułamek dostępnej przestrzeni.
    FillPortion(f32),
    /// Dopasuj się do zawartości.
    Shrink,
}

impl Length {
    /// Rozwiązuje długość: `fill` to dostępna przestrzeń, `shrink` to rozmiar zawartości.
    pub fn resolve(self, fill: f32, shrink: f32) -> f32 {
        match self {
            Self::Fixed(v) => v.max(0.0),
            Self::Fill => fill.max(0.0),
            Self::FillPortion(p) => (fill * p).max(0.0),
            Self::Shrink => shrink.max(0.0),
        }
    }
}

impl Default for Length {
    fn default() -> Self {
        Self::Shrink
    }
}

impl From<f32> for Length {
    fn from(value: f32) -> Self {
        Self::Fixed(value)
    }
}

/// Kolor w formacie straight alpha (kolejność RGBA, zakres `0.0..=1.0`).
///
/// Biblioteka używa własnego typu zamiast `wgpu::Color`, dzięki czemu API kolorów
/// pozostaje niezależne od backendu graficznego.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// Składowa czerwona.
    pub r: f32,
    /// Składowa zielona.
    pub g: f32,
    /// Składowa niebieska.
    pub b: f32,
    /// Kanał alpha.
    pub a: f32,
}

impl Default for Color {
    fn default() -> Self {
        Self::TRANSPARENT
    }
}

impl Color {
    /// Kolor w pełni przezroczysty.
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    /// Czarny nieprzezroczysty.
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    /// Biały nieprzezroczysty.
    pub const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };

    /// Kolor z kanałami `0.0..=1.0`.
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Nieprzezroczysty kolor z kanałami `0.0..=1.0`.
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// Kolor z kanałami `0..=255`.
    pub const fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    /// Kanały jako `0..=255`.
    pub fn to_rgba8(self) -> [u8; 4] {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        [c(self.r), c(self.g), c(self.b), c(self.a)]
    }

    /// Zmienia kanał alpha.
    pub fn with_alpha(self, a: f32) -> Self {
        Self {
            a: a.clamp(0.0, 1.0),
            ..self
        }
    }

    /// Mnoży kanał alpha.
    pub fn multiply_alpha(self, factor: f32) -> Self {
        self.with_alpha(self.a * factor)
    }

    /// Nakłada warstwę `top` z zadaną krotnością.
    pub fn overlay(self, top: Color, opacity: f32) -> Self {
        let t = opacity.clamp(0.0, 1.0);
        Self {
            r: self.r * (1.0 - t) + top.r * t,
            g: self.g * (1.0 - t) + top.g * t,
            b: self.b * (1.0 - t) + top.b * t,
            a: self.a * (1.0 - t) + top.a * t,
        }
    }

    /// Wartości jako tablica RGBA.
    pub const fn to_array(self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Kolor z tablicy RGBA (wartości przycinane do `0.0..=1.0`).
    pub fn from_array(v: [f32; 4]) -> Self {
        Self {
            r: v[0].clamp(0.0, 1.0),
            g: v[1].clamp(0.0, 1.0),
            b: v[2].clamp(0.0, 1.0),
            a: v[3].clamp(0.0, 1.0),
        }
    }
}

/// Kąt w radianach.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Angle(pub f32);

impl Angle {
    /// Tworzy kąt z wartości w stopniach.
    pub fn from_degrees(degrees: f32) -> Self {
        Self(degrees.to_radians())
    }

    /// Wartość w stopniach.
    pub fn to_degrees(self) -> f32 {
        self.0.to_degrees()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_intersection_and_union() {
        let a = Rect::new(Point::new(0.0, 0.0), Size::new(10.0, 10.0));
        let b = Rect::new(Point::new(5.0, 5.0), Size::new(10.0, 10.0));

        assert_eq!(
            a.intersection(&b),
            Rect::new(Point::new(5.0, 5.0), Size::new(5.0, 5.0))
        );
        assert_eq!(
            a.union(b),
            Rect::new(Point::new(0.0, 0.0), Size::new(15.0, 15.0))
        );
        assert!(a.intersects(&b));
    }

    #[test]
    fn rect_from_corners_normalizes() {
        let a = Rect::from_corners(Point::new(10.0, 10.0), Point::new(0.0, 0.0));
        assert_eq!(a, Rect::new(Point::ZERO, Size::new(10.0, 10.0)));
    }

    #[test]
    fn size_new_clamps_negatives() {
        assert_eq!(Size::new(-5.0, 3.0), Size::new(0.0, 3.0));
        assert!(!Size::new(f32::NAN, 1.0).is_valid());
    }

    #[test]
    fn color_conversion_roundtrip() {
        let c = Color::from_rgba8(12, 34, 56, 78);
        assert_eq!(c.to_rgba8(), [12, 34, 56, 78]);
    }

    #[test]
    fn padding_shrink_never_negative() {
        let r = Rect::new(Point::ZERO, Size::new(10.0, 10.0));
        assert_eq!(r.shrink(Padding::all(8.0)).size, Size::ZERO);
    }

    #[test]
    fn radius_clamped_to_half_size() {
        let r = Radius::uniform(100.0).clamp(Size::new(20.0, 20.0));
        assert_eq!(r.top_left, 10.0);
    }

    #[test]
    fn align_positions_block() {
        assert_eq!(Align::Start.position(0.0, 100.0, 20.0), 0.0);
        assert_eq!(Align::Center.position(0.0, 100.0, 20.0), 40.0);
        assert_eq!(Align::End.position(0.0, 100.0, 20.0), 80.0);
    }
}
