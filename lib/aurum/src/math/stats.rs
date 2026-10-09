//! Statystyka na CPU — odpowiedniki operacji z modułu [`ops`](crate::ops).
//!
//! Te same funkcje liczą na GPU [`ops::reduce`](crate::ops::reduce) i
//! [`ops::histogram`](crate::ops::histogram), więc testy mogą porównać obie
//! implementacje na tych samych danych.
//!
//! ```
//! use aurum::math::stats::{mean, median, std_dev};
//!
//! let dane = [1.0f32, 2.0, 3.0, 4.0];
//! assert_eq!(mean(&dane), 2.5);
//! assert_eq!(median(&dane), 2.5);
//! assert!(std_dev(&dane) > 0.0);
//! ```

/// Suma elementów (pusta tablica daje `0.0`).
pub fn sum(values: &[f32]) -> f32 {
    values.iter().sum()
}

/// Średnia arytmetyczna.
pub fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    sum(values) / values.len() as f32
}

/// Wariancja populacji (dzielona przez `n`).
pub fn variance(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let m = mean(values);
    values.iter().map(|v| (v - m) * (v - m)).sum::<f32>() / values.len() as f32
}

/// Odchylenie standardowe populacji.
pub fn std_dev(values: &[f32]) -> f32 {
    variance(values).sqrt()
}

/// Najmniejszy element.
pub fn min(values: &[f32]) -> f32 {
    values.iter().copied().fold(f32::INFINITY, f32::min)
}

/// Największy element.
pub fn max(values: &[f32]) -> f32 {
    values.iter().copied().fold(f32::NEG_INFINITY, f32::max)
}

/// Indeks najmniejszego elementu (pierwszego w razie remisu).
pub fn argmin(values: &[f32]) -> Option<usize> {
    values
        .iter()
        .enumerate()
        .fold(None, |acc: Option<(usize, f32)>, (i, &v)| match acc {
            Some((_, best)) if best <= v => acc,
            _ => Some((i, v)),
        })
        .map(|(i, _)| i)
}

/// Indeks największego elementu (pierwszego w razie remisu).
pub fn argmax(values: &[f32]) -> Option<usize> {
    values
        .iter()
        .enumerate()
        .fold(None, |acc: Option<(usize, f32)>, (i, &v)| match acc {
            Some((_, best)) if best >= v => acc,
            _ => Some((i, v)),
        })
        .map(|(i, _)| i)
}

/// Mediana — średnia dwóch środkowych elementów po posortowaniu.
pub fn median(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) * 0.5
    } else {
        sorted[mid]
    }
}

/// Percentyl (`p` w przedziale `[0, 100]`), z interpolacją liniową.
pub fn percentile(values: &[f32], p: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let rank = (p.clamp(0.0, 100.0) / 100.0) * (sorted.len() - 1) as f32;
    let low = rank.floor() as usize;
    let high = rank.ceil() as usize;
    let frac = rank - low as f32;
    sorted[low] * (1.0 - frac) + sorted[high] * frac
}

/// Kowariancja populacji dwóch szeregów (albo `None`, gdy długości się różnią).
pub fn covariance(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let (ma, mb) = (mean(a), mean(b));
    Some(
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - ma) * (y - mb))
            .sum::<f32>()
            / a.len() as f32,
    )
}

/// Współczynnik korelacji Pearsona.
pub fn correlation(a: &[f32], b: &[f32]) -> Option<f32> {
    let cov = covariance(a, b)?;
    let denom = std_dev(a) as f64 * std_dev(b) as f64;
    if denom == 0.0 {
        return None;
    }
    Some((cov as f64 / denom) as f32)
}

/// Sumy prefiksowe — odpowiednik [`ops::scan::prefix_sum`](crate::ops::scan::prefix_sum).
pub fn cumulative_sum(values: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(values.len());
    let mut acc = 0.0;
    for &v in values {
        acc += v;
        out.push(acc);
    }
    out
}

/// Punkty równomiernie rozłożone na przedziale (wliczając oba końce).
pub fn linspace(from: f32, to: f32, count: usize) -> Vec<f32> {
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![from];
    }
    let step = (to - from) / (count - 1) as f32;
    (0..count).map(|i| from + step * i as f32).collect()
}

/// Histogram wartości z przedziału `[min, max)` na `bins` półkach.
///
/// Odpowiednik [`ops::histogram::histogram`](crate::ops::histogram::histogram);
/// wartości poza przedziałem są pomijane.
pub fn histogram(values: &[f32], bins: usize, min: f32, max: f32) -> Vec<u32> {
    let mut out = vec![0u32; bins];
    if bins == 0 || max <= min {
        return out;
    }
    let scale = bins as f32 / (max - min);
    for &v in values {
        if v < min || v >= max {
            continue;
        }
        let bin = ((v - min) * scale) as usize;
        out[bin.min(bins - 1)] += 1;
    }
    out
}

/// Normalizuje wektor: środek w `mean`, rozrzut w `std_dev`.
///
/// Zwraca `false` i nie rusza tablicy, gdy odchylenie jest zerowe.
pub fn standardize_in_place(values: &mut [f32]) -> bool {
    let (m, s) = (mean(values), std_dev(values));
    if s == 0.0 {
        return false;
    }
    for v in values.iter_mut() {
        *v = (*v - m) / s;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn podstawowe_statystyki() {
        let d = [1.0f32, 2.0, 3.0, 4.0];
        assert_eq!(sum(&d), 10.0);
        assert_eq!(mean(&d), 2.5);
        assert_eq!(min(&d), 1.0);
        assert_eq!(max(&d), 4.0);
        assert_eq!(argmin(&d), Some(0));
        assert_eq!(argmax(&d), Some(3));
        assert_eq!(min(&[]), f32::INFINITY);
    }

    #[test]
    fn wariancja_i_odchylenie() {
        let d = [2.0f32, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        assert!((variance(&d) - 4.0).abs() < 1e-5);
        assert!((std_dev(&d) - 2.0).abs() < 1e-5);
        assert_eq!(std_dev(&[]), 0.0);
    }

    #[test]
    fn mediana_bierze_srodek() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), 2.5);
        assert_eq!(median(&[]), 0.0);
    }

    #[test]
    fn percentyle_sa_odporne_na_kolejnosc() {
        let d = [10.0f32, 20.0, 30.0, 40.0];
        assert!((percentile(&d, 0.0) - 10.0).abs() < 1e-5);
        assert!((percentile(&d, 100.0) - 40.0).abs() < 1e-5);
        assert!((percentile(&[40.0, 10.0, 30.0, 20.0], 50.0) - 25.0).abs() < 1e-5);
    }

    #[test]
    fn korelacja_zwraca_jedynke_dla_tej_samej_serii() {
        let a = [1.0f32, 2.0, 3.0, 4.0];
        assert!((correlation(&a, &a).unwrap() - 1.0).abs() < 1e-5);
        assert!((correlation(&a, &[-1.0, -2.0, -3.0, -4.0]).unwrap() + 1.0).abs() < 1e-5);
        assert!(correlation(&a, &[2.0, 2.0, 2.0, 2.0]).is_none());
        assert!(correlation(&a, &[1.0]).is_none());
    }

    #[test]
    fn sumy_prefiksowe_i_linspace() {
        assert_eq!(cumulative_sum(&[1.0, 2.0, 3.0]), vec![1.0, 3.0, 6.0]);
        assert_eq!(linspace(0.0, 1.0, 3), vec![0.0, 0.5, 1.0]);
        assert_eq!(linspace(0.0, 1.0, 0).len(), 0);
        assert_eq!(linspace(2.0, 2.0, 4), vec![2.0; 4]);
    }

    #[test]
    fn histogram_pomija_wartosci_poza_zakresem() {
        let h = histogram(&[-1.0, 0.0, 0.25, 0.5, 0.75, 1.0, 2.0], 4, 0.0, 1.0);
        assert_eq!(h, vec![1, 1, 1, 1]);
        assert_eq!(histogram(&[0.5], 0, 0.0, 1.0).len(), 0);
    }

    #[test]
    fn standaryzacja_sprowadza_srednia_do_zera() {
        let mut d = [1.0f32, 2.0, 3.0, 4.0];
        assert!(standardize_in_place(&mut d));
        assert!(mean(&d).abs() < 1e-5);
        assert!((std_dev(&d) - 1.0).abs() < 1e-5);
        assert!(!standardize_in_place(&mut [2.0, 2.0]));
    }
}