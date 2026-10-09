//! Liczby zespolone nad `f32` i `f64`.
//!
//! [`Complex`] jest generyczna nad [`ComplexScalar`], więc ten sam kod działa
//! dla obu typów — w WGSL liczby zespolone to po prostu para `vec2<f32>`.
//!
//! ```
//! use aurum::math::Complex;
//!
//! let z = Complex::new(0.0f32, 1.0);
//! let wynik = z * z + 1.0;
//! assert!(wynik.abs() < 1e-6);
//! assert!((z.exp().im - 1.0f32.sin()).abs() < 1e-6);
//! ```

use std::fmt::Debug;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// Typ skalarny, na którym da się liczyć liczby zespolone.
///
/// Zaimplementowane dla `f32` i `f64`; własne typy liczb trzeba „opakować”
/// własną implementacją tego traitu.
pub trait ComplexScalar:
    Copy + Debug + PartialEq + PartialOrd + Add<Output = Self> + Sub<Output = Self>
    + Mul<Output = Self> + Div<Output = Self> + Neg<Output = Self>
{
    /// Liczba rzeczywista zapisana w scalarnym typie.
    fn from_f64(value: f64) -> Self;
    /// Liczba rzeczywista jako `f64`.
    fn to_f64(self) -> f64;
    /// Pierwiastek kwadratowy.
    fn csqrt(self) -> Self;
    /// Wykładnik.
    fn cexp(self) -> Self;
    /// Logarytm naturalny.
    fn cln(self) -> Self;
    /// Sinus i cosinus naraz.
    fn csincos(self) -> (Self, Self);
    /// Arcus tangens.
    fn catan2(self, other: Self) -> Self;
    /// Pierwiastek z sumy kwadratów.
    fn chypot(self, other: Self) -> Self;
    /// Sinus hiperboliczny — `sinh`.
    fn csinh(self) -> Self;
    /// Cosinus hiperboliczny — `cosh`.
    fn ccosh(self) -> Self;
    /// Maksimum.
    fn cmax(self, other: Self) -> Self;
    /// Minimum.
    fn cmin(self, other: Self) -> Self;
}

impl ComplexScalar for f32 {
    fn from_f64(value: f64) -> Self {
        value as f32
    }
    fn to_f64(self) -> f64 {
        self as f64
    }
    fn csqrt(self) -> Self {
        f32::sqrt(self)
    }
    fn cexp(self) -> Self {
        f32::exp(self)
    }
    fn cln(self) -> Self {
        f32::ln(self)
    }
    fn csincos(self) -> (Self, Self) {
        self.sin_cos()
    }
    fn catan2(self, other: Self) -> Self {
        f32::atan2(self, other)
    }
    fn chypot(self, other: Self) -> Self {
        f32::hypot(self, other)
    }
    fn csinh(self) -> Self {
        (self.exp() - (-self).exp()) * 0.5
    }
    fn ccosh(self) -> Self {
        (self.exp() + (-self).exp()) * 0.5
    }
    fn cmax(self, other: Self) -> Self {
        f32::max(self, other)
    }
    fn cmin(self, other: Self) -> Self {
        f32::min(self, other)
    }
}

impl ComplexScalar for f64 {
    fn from_f64(value: f64) -> Self {
        value
    }
    fn to_f64(self) -> f64 {
        self
    }
    fn csqrt(self) -> Self {
        f64::sqrt(self)
    }
    fn cexp(self) -> Self {
        f64::exp(self)
    }
    fn cln(self) -> Self {
        f64::ln(self)
    }
    fn csincos(self) -> (Self, Self) {
        self.sin_cos()
    }
    fn catan2(self, other: Self) -> Self {
        f64::atan2(self, other)
    }
    fn chypot(self, other: Self) -> Self {
        f64::hypot(self, other)
    }
    fn csinh(self) -> Self {
        (self.exp() - (-self).exp()) * 0.5
    }
    fn ccosh(self) -> Self {
        (self.exp() + (-self).exp()) * 0.5
    }
    fn cmax(self, other: Self) -> Self {
        f64::max(self, other)
    }
    fn cmin(self, other: Self) -> Self {
        f64::min(self, other)
    }
}

/// Liczba zespolona `re + i·im`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Complex<T> {
    /// Część rzeczywista.
    pub re: T,
    /// Część urojona.
    pub im: T,
}

impl<T: ComplexScalar> Complex<T> {
    /// Liczba zespolona z części rzeczywistej i urojonej.
    pub const fn new(re: T, im: T) -> Self {
        Self { re, im }
    }

    /// Liczba rzeczywista (bez części urojonej).
    pub fn real(re: T) -> Self {
        Self::new(re, T::from_f64(0.0))
    }

    /// Liczba urojona.
    pub fn imaginary(im: T) -> Self {
        Self::new(T::from_f64(0.0), im)
    }

    /// Jedynka.
    pub fn one() -> Self {
        Self::real(T::from_f64(1.0))
    }

    /// Liczba z układu biegunowego: `r·e^{iθ}`.
    pub fn from_polar(r: T, theta: T) -> Self {
        let (sin, cos) = theta.csincos();
        Self::new(r * cos, r * sin)
    }

    /// Sprzężenie: `re - i·im`.
    pub fn conj(self) -> Self {
        Self::new(self.re, -self.im)
    }

    /// Moduł (długość wektora `(re, im)`).
    pub fn abs(self) -> T {
        self.re.chypot(self.im)
    }

    /// Kwadrat modułu — bez pierwiastka.
    pub fn norm_squared(self) -> T {
        self.re * self.re + self.im * self.im
    }

    /// Liczba zespolona w układzie biegunowym: `(moduł, argument)`.
    pub fn to_polar(self) -> (T, T) {
        (self.abs(), self.im.catan2(self.re))
    }

    /// Argument (kąt) w radianach.
    pub fn arg(self) -> T {
        self.im.catan2(self.re)
    }

    /// Pierwiastek kwadratowy (gałąź główna).
    pub fn sqrt(self) -> Self {
        if self.re == T::from_f64(0.0) && self.im == T::from_f64(0.0) {
            return Self::new(T::from_f64(0.0), T::from_f64(0.0));
        }
        let r = self.abs();
        let re_part = ((r + self.re) / T::from_f64(2.0)).csqrt();
        let im_part = ((r - self.re) / T::from_f64(2.0)).csqrt();
        if self.im < T::from_f64(0.0) {
            Self::new(re_part, -im_part)
        } else {
            Self::new(re_part, im_part)
        }
    }

    /// Potęgowanie `z^w` przez logarytm i wykładnik.
    pub fn pow(self, exponent: T) -> Self {
        if self.re == T::from_f64(0.0) && self.im == T::from_f64(0.0) {
            return if exponent == T::from_f64(0.0) {
                Self::one()
            } else {
                Self::new(T::from_f64(0.0), T::from_f64(0.0))
            };
        }
        (self.ln() * exponent).exp()
    }

    /// Potęgowanie całkowite.
    pub fn powi(self, exponent: i32) -> Self {
        if exponent < 0 {
            return Self::one() / self.powi(-exponent);
        }
        let mut result = Self::one();
        for _ in 0..exponent {
            result = result * self;
        }
        result
    }

    /// Wykładnik.
    pub fn exp(self) -> Self {
        let e = self.re.cexp();
        let (sin, cos) = self.im.csincos();
        Self::new(e * cos, e * sin)
    }

    /// Logarytm naturalny.
    pub fn ln(self) -> Self {
        Self::new(self.abs().cln(), self.arg())
    }

    /// Sinus: `sin(x + iy) = sin(x)·cosh(y) + i·cos(x)·sinh(y)`.
    pub fn sin(self) -> Self {
        let (sin_re, cos_re) = self.re.csincos();
        Self::new(sin_re * self.im.ccosh(), cos_re * self.im.csinh())
    }

    /// Cosinus: `cos(x + iy) = cos(x)·cosh(y) − i·sin(x)·sinh(y)`.
    pub fn cos(self) -> Self {
        let (sin_re, cos_re) = self.re.csincos();
        Self::new(cos_re * self.im.ccosh(), -sin_re * self.im.csinh())
    }

    /// Tangens.
    pub fn tan(self) -> Self {
        self.sin() / self.cos()
    }

    /// Odwrotność (albo `None`, gdy liczba jest zerowa).
    pub fn inverse(self) -> Option<Self> {
        let n = self.norm_squared();
        if n == T::from_f64(0.0) {
            return None;
        }
        Some(self.conj() / n)
    }
}

impl<T: ComplexScalar> Add for Complex<T> {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self::new(self.re + rhs.re, self.im + rhs.im)
    }
}

impl<T: ComplexScalar> Sub for Complex<T> {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self::new(self.re - rhs.re, self.im - rhs.im)
    }
}

impl<T: ComplexScalar> Mul for Complex<T> {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self {
        Self::new(
            self.re * rhs.re - self.im * rhs.im,
            self.re * rhs.im + self.im * rhs.re,
        )
    }
}

impl<T: ComplexScalar> Div for Complex<T> {
    type Output = Self;

    fn div(self, rhs: Self) -> Self {
        let n = rhs.norm_squared();
        Self::new(
            (self.re * rhs.re + self.im * rhs.im) / n,
            (self.im * rhs.re - self.re * rhs.im) / n,
        )
    }
}

impl<T: ComplexScalar> Neg for Complex<T> {
    type Output = Self;

    fn neg(self) -> Self {
        Self::new(-self.re, -self.im)
    }
}

impl<T: ComplexScalar> Add<T> for Complex<T> {
    type Output = Self;

    fn add(self, rhs: T) -> Self {
        Self::new(self.re + rhs, self.im)
    }
}

impl<T: ComplexScalar> Mul<T> for Complex<T> {
    type Output = Self;

    fn mul(self, rhs: T) -> Self {
        Self::new(self.re * rhs, self.im * rhs)
    }
}

impl<T: ComplexScalar> Div<T> for Complex<T> {
    type Output = Self;

    fn div(self, rhs: T) -> Self {
        Self::new(self.re / rhs, self.im / rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnozenie_i_dzielenie_sa_spojne() {
        let a = Complex::new(3.0f64, 4.0);
        let b = Complex::new(1.0f64, -2.0);

        // (3 + 4i)(1 − 2i) = 3 − 6i + 4i + 8 = 11 − 2i
        assert!(((a * b).re - 11.0).abs() < 1e-12 && ((a * b).im + 2.0).abs() < 1e-12);

        let iloczyn = a * b / b;
        assert!((iloczyn.re - 3.0).abs() < 1e-12 && (iloczyn.im - 4.0).abs() < 1e-12);
    }

    #[test]
    fn pierwiastki_z_kwadratem_daja_liczbe() {
        let z = Complex::new(-4.0f32, 3.0);
        let w = z.sqrt();
        assert!((w * w - z).abs() < 1e-4);

        // Liczba czysto urojona: pierwiastek też jest urojony, a nie „-2”.
        let urojona = Complex::new(0.0f32, -4.0);
        let pierwiastek = urojona.sqrt();
        assert!(pierwiastek.im < 0.0, "gałąź główna ma ujemną część urojoną");
        assert!((pierwiastek * pierwiastek - urojona).abs() < 1e-5);
        assert!((pierwiastek.abs() - 2.0).abs() < 1e-5);
    }

    #[test]
    fn tozsamosci_eulera() {
        let z = Complex::new(0.7f32, 1.3);

        // cos² + sin² = 1
        let (c, s) = (z.cos(), z.sin());
        assert!((c * c + s * s - Complex::one()).abs() < 1e-4);

        // exp(ln z) = z dla z poza ujemną osią rzeczywistą.
        let w = z.ln().exp();
        assert!((w - z).abs() < 1e-4);
    }

    #[test]
    fn potegowanie_zgadza_sie_z_wielomianem() {
        let z = Complex::new(1.0f32, 1.0);
        assert!((z.powi(3) - z.powi(2) * z).abs() < 1e-5);
        assert!((z.pow(3.0f32) - z.powi(3)).abs() < 1e-3);
        // Ujemny wykładnik to odwrotność: z⁻² · z² = 1.
        assert!((z.powi(-2) * z.powi(2) - Complex::one()).abs() < 1e-5);
    }

    #[test]
    fn odwrotnosc_zera_to_none() {
        assert!(Complex::new(0.0f32, 0.0).inverse().is_none());
        let z = Complex::new(2.0f32, -3.0);
        assert!((z * z.inverse().unwrap() - Complex::one()).abs() < 1e-5);
    }

    #[test]
    fn uklad_biegunowy_powraca_do_liczby() {
        let z = Complex::new(-2.0f32, 1.0);
        let (r, theta) = z.to_polar();
        assert!((Complex::from_polar(r, theta) - z).abs() < 1e-5);
        assert!((z.ln().exp() - z).abs() < 1e-4);
    }
}