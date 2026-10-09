//! Drzewo widgetów wraz ze stanem przechowywanym między klatkami.
//!
//! Identyfikatory (`Id`) nadawane są ze ścieżki w drzewie (indeks dziecka),
//! więc stan jest stabilny, dopóki struktura `view()` się nie zmienia. Widgety
//! generowane w pętli powinny dostać jawne `Id` przez `Element::with_id`.

use std::any::Any;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use crate::layout::Layout;

/// Unikalny identyfikator widgetu w ramach jednego okna.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Id(u64);

impl Id {
    /// Identyfikator z dowolnego haszowalnego źródła (np. `&str`).
    ///
    /// Przydatne dla stabilnych identyfikatorów w listach generowanych dynamicznie.
    pub fn new(source: impl Hash) -> Self {
        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        Self(hasher.finish())
    }

    /// Identyfikator korzenia.
    pub const ROOT: Self = Self(0);

    /// Numer bazowy (do diagnostyki).
    pub const fn to_u64(self) -> u64 {
        self.0
    }

    /// Tworzy identyfikator dziecka o zadanym indeksie.
    ///
    /// Używa mieszania zamiast konkatenacji, więc ścieżki o różnej długości
    /// nie kolidują ze sobą.
    pub fn child(self, index: usize) -> Self {
        let mut hasher = DefaultHasher::new();
        self.0.hash(&mut hasher);
        index.hash(&mut hasher);
        Self(hasher.finish())
    }

    /// Identyfikator potomka o zadanej ścieżce indeksów.
    pub fn path(self, path: &[usize]) -> Self {
        path.iter().fold(self, |acc, &i| acc.child(i))
    }
}

/// Stan pojedynczego widgetu (współdzielony między klatkami).
///
/// Stan jest typowany w dół, więc widget przechowuje np. `bool`, `f32` czy
/// własny stan, a nie `dyn Any` w każdym miejscu.
///
/// # Dlaczego `Mutex`
///
/// Węzeł drzewa jest współdzielony między fazami układania, rysowania
/// i obsługi zdarzeń, które wszystkie przyjmują `&Tree`. Stan typowany
/// w dół najprościej przechowywać za mutexem — koszt pomijalny wobec
/// kosztu renderowania.
#[derive(Clone, Default)]
pub struct State(Arc<Mutex<Option<Box<dyn Any + Send>>>>);

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("State")
    }
}

impl State {
    /// Pusty stan.
    pub fn new() -> Self {
        Self::default()
    }

    /// Czy stan zawiera wartość typu `T`.
    pub fn is<T: Send + 'static>(&self) -> bool {
        match self.0.lock() {
            Ok(guard) => guard.as_ref().is_some_and(|v| v.is::<T>()),
            Err(_) => false,
        }
    }

    /// Odczytuje kopię wartości typu `T`.
    pub fn get<T: Clone + Send + 'static>(&self) -> Option<T> {
        let guard = self.0.lock().ok()?;
        guard.as_ref().and_then(|v| v.downcast_ref::<T>()).cloned()
    }

    /// Ustawia wartość typu `T` (zastępuje poprzednią).
    pub fn insert<T: Send + 'static>(&mut self, value: T) {
        self.set(value);
    }

    /// Ustawia wartość typu `T` przez współdzielony stan.
    ///
    /// Widgety operują na `&Tree`, więc nie mogą użyć [`State::insert`].
    pub fn set<T: Send + 'static>(&self, value: T) {
        if let Ok(mut guard) = self.0.lock() {
            *guard = Some(Box::new(value));
        }
    }

    /// Wykonuje domknięcie na wartości typu `T`, wstawiając wartość domyślną.
    ///
    /// Wygodne dla typów takich jak `bool` (domyślnie `false`) lub liczb.
    ///
    /// ```ignore
    /// *tree.state.with_mut::<f32, _>(|v| *v += 1.0);
    /// ```
    pub fn with_mut<T: Default + Send + 'static, R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        if !self.is::<T>() {
            self.set(T::default());
        }
        let mut guard = self.0.lock().expect("stan widgetu jest zatruty");
        let slot = guard.as_mut().expect("wartość właśnie wstawiona");
        let value = slot
            .downcast_mut::<T>()
            .expect("typ stanu widgetu nie może się zmienić w trakcie życia");
        f(value)
    }

    /// Uruchamia domknięcie na wartości typu `T`.
    pub fn update<T: Send + 'static>(&self, f: impl FnOnce(&mut T)) {
        if let Ok(mut guard) = self.0.lock() {
            if let Some(value) = guard.as_mut().and_then(|v| v.downcast_mut::<T>()) {
                f(value);
            }
        }
    }
}

/// Węzeł drzewa widgetów.
#[derive(Debug, Default, Clone)]
pub struct Tree {
    /// Identyfikator węzła.
    pub id: Id,
    /// Stan węzła.
    pub state: State,
    /// Węzły potomków (w kolejności rysowania).
    pub children: Vec<Tree>,
    /// Wynik układania tego węzła.
    pub layout: Layout,
    /// Czy kursor znajduje się nad tym węzłem.
    pub is_over: bool,
}

impl Tree {
    /// Tworzy węzeł z danym identyfikatorem.
    pub fn new(id: Id) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }

    /// Synchronizuje węzeł z nowym elementem, przenosząc stan poprzedniej klatki.
    ///
    /// Stan jest dopasowywany po identyfikatorze węzła, więc zmiana kolejności
    /// lub dodanie/usunięcie elementów nie przenosi stanu na niewłaściwy widget.
    pub fn sync(&mut self, element: &crate::element::Element) {
        let id = self.id;
        *self = element.build_tree(id, Some(self));
    }

    /// Buduje węzły potomków, przenosząc stan ze wskazanego drzewa.
    pub fn children_from_iter<'a>(
        &mut self,
        elements: impl Iterator<Item = &'a crate::element::Element>,
    ) -> Vec<Tree> {
        let id = self.id;
        elements
            .enumerate()
            .map(|(index, element)| element.build_tree(id.child(index), self.children.get(index)))
            .collect()
    }

    /// Buduje węzły potomków z plastra elementów.
    pub fn children_from_slice(&mut self, elements: &[crate::element::Element]) -> Vec<Tree> {
        self.children_from_iter(elements.iter())
    }

    /// Zwraca węzeł po ścieżce indeksów.
    pub fn node_at(&self, path: &[usize]) -> Option<&Tree> {
        let mut node = self;
        for &index in path {
            node = node.children.get(index)?;
        }
        Some(node)
    }

    /// Głębokość drzewa (do diagnostyki i testów).
    pub fn depth(&self) -> usize {
        1 + self
            .children
            .iter()
            .map(Self::depth)
            .max()
            .unwrap_or_default()
    }

    /// Liczba węzłów w poddrzewie.
    pub fn node_count(&self) -> usize {
        1 + self.children.iter().map(Self::node_count).sum::<usize>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_ids_are_distinct_per_index() {
        let root = Id::ROOT;
        assert_ne!(root.child(0), root.child(1));
        assert_eq!(root.child(3), Id::ROOT.child(3));
    }

    #[test]
    fn paths_of_different_depth_do_not_collide() {
        let a = Id::ROOT.child(1).child(0);
        let b = Id::ROOT.child(0).child(1).child(0);
        assert_ne!(a, b);
    }

    #[test]
    fn named_ids_are_stable() {
        assert_eq!(Id::new("okno"), Id::new("okno"));
        assert_ne!(Id::new("okno"), Id::new("panel"));
    }

    #[test]
    fn state_roundtrip() {
        let mut state = State::new();
        assert!(!state.is::<f32>());
        state.insert(2.5_f32);
        assert!(state.is::<f32>());
        assert_eq!(state.get::<f32>(), Some(2.5));
    }

    #[test]
    fn with_mut_initializes_and_mutates() {
        let state = State::new();
        state.with_mut::<u32, _>(|v| *v += 7);
        state.with_mut::<u32, _>(|v| *v += 1);
        assert_eq!(state.get::<u32>(), Some(8));
    }

    #[test]
    fn state_update_runs_closure() {
        let mut state = State::new();
        state.insert(String::from("a"));
        state.update::<String>(|s| s.push('b'));
        assert_eq!(state.get::<String>().as_deref(), Some("ab"));
    }
}
