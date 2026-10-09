//! Elementy drzewa UI i trait [`Widget`].
//!
//! Aplikacja buduje interfejs w `view()` zwracając [`Element`]. Element jest
//! lekkim uchwytem do widgetu — można go kopiować referencyjnie, przenosić
//! do kontenerów i case'ować bez utraty stanu.

use std::fmt;

use crate::context::Context;
use crate::event::Event;
use crate::geometry::Size;
use crate::layout::{Cursor, EventStatus, Interaction, Layout, Limits};
use crate::tree::{Id, Tree};

/// Widget — element interfejsu użytkownika.
///
/// Implementacja dzieli się na cztery fazy wykonywane w tej kolejności:
///
/// 1. [`Widget::children`] — budowa drzewa potomków (raz na klatkę),
/// 2. [`Widget::layout`] — obliczenie rozmiaru i pozycji (raz na klatkę),
/// 3. [`Widget::draw`] — rysowanie,
/// 4. [`Widget::event`] / [`Widget::cursor`] — reakcja na wejście.
///
/// Stan między klatkami trzymany jest w [`Tree::state`] (patrz [`State`]).
/// Domyślne implementacje `event` i `cursor` są puste, więc widgety statyczne
/// muszą nadpisać tylko `layout` i `draw`.
pub trait Widget: fmt::Debug + Send + Sync {
    /// Buduje węzły potomków na podstawie elementów z `view()`.
    ///
    /// Wywoływane raz na klatkę przed układaniem.
    fn children(&self, tree: &mut Tree) -> Vec<Tree> {
        let _ = tree;
        Vec::new()
    }

    /// Oblicza rozmiar i pozycję widgetu.
    ///
    /// Zwraca `(rozmiar zajmowany przez widget, szczegóły layoutu)`.
    fn layout(&self, tree: &mut Tree, limits: &Limits, ctx: &mut Context<'_>) -> (Size, Layout);

    /// Rysuje widget. `layout.bounds` podaje pozycję absolutną.
    fn draw(&self, tree: &Tree, ctx: &mut Context<'_>, layout: &Layout);

    /// Obsługuje zdarzenie wejścia.
    fn event(
        &self,
        tree: &Tree,
        event: &Event,
        ctx: &mut Context<'_>,
        layout: &Layout,
    ) -> EventStatus {
        let _ = (tree, event, ctx, layout);
        EventStatus::Ignored
    }

    /// Zgłasza interakcję pod kursorem.
    ///
    /// Wywoływane tylko, gdy kursor znajduje się nad `layout.bounds`.
    fn cursor(&self, tree: &Tree, layout: &Layout, cursor: Cursor) -> Interaction {
        let _ = (tree, layout, cursor);
        Interaction::None
    }

    /// Czy widget rozciąga się na całą dostępną przestrzeń w swojej osi
    /// głównej.
    ///
    /// `Flex` używa tego, aby podzielić pozostałą przestrzeń między dzieci
    /// oznaczone `Length::Fill`. Widgety, które nie rozciągają się, zwracają
    /// `false` (domyślnie).
    fn fills_main_axis(&self) -> bool {
        false
    }

    /// Czy widget powinien otrzymywać zdarzenia, gdy okno nie ma fokusu.
    ///
    /// Domyślnie `false` — tylko pola tekstowe i inne kontrolki edytowalne
    /// nadpisują to na `true`.
    fn wants_focus(&self) -> bool {
        false
    }
}

/// Uchwyt do widgetu w drzewie UI.
///
/// Element jest typem „interfejsowym” — nie przechowuje stanu aplikacji,
/// a jedynie wskaźnik do widgetu i opcjonalny jawny [`Id`].
pub struct Element {
    widget: Box<dyn Widget>,
    id: Option<Id>,
}

impl fmt::Debug for Element {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Element").finish_non_exhaustive()
    }
}

impl Element {
    /// Tworzy element z dowolnego widgetu.
    pub fn new(widget: impl Widget + 'static) -> Self {
        Self {
            widget: Box::new(widget),
            id: None,
        }
    }

    /// Przypisuje jawny identyfikator.
    ///
    /// **Ważne:** widgety generowane w pętli (np. lista elementów) powinny mieć
    /// stabilne `Id`, inaczej ich stan (np. `is_focused`) resetuje się przy
    /// zmianie kolejności.
    pub fn with_id(mut self, id: Id) -> Self {
        self.id = Some(id);
        self
    }

    /// Referencja do widgetu (np. do diagnostyki w testach).
    pub fn as_widget(&self) -> &dyn Widget {
        self.widget.as_ref()
    }

    /// Buduje węzeł drzewa dla tego elementu (wewnętrzne).
    ///
    /// `previous` to węzeł z poprzedniej klatki — jeśli identyfikatory się
    /// zgadzają, stan (i stany potomków) są przenoszone.
    pub(crate) fn build_tree(&self, fallback_id: Id, previous: Option<&Tree>) -> Tree {
        let id = self.id.unwrap_or(fallback_id);
        let mut node = Tree::new(id);

        if let Some(previous) = previous {
            if previous.id == id {
                node.state = previous.state.clone();
                node.children = previous.children.clone();
            }
        }

        node.children = self.widget.children(&mut node);
        node
    }
}

/// Reaktywne API stylu *fluent* dla widgetów wymagających budowania krok po kroku.
pub trait Build: Sized {
    /// Buduje element z skumulowanego buildera.
    fn build(self) -> Element;
}
