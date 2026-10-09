//! Atlas glifów: pojedyncza tekstura `Rgba8Unorm` z alokatorem „półek”.
//!
//! Glify są rasteryzowane przez `cosmic-text`/`swash` i wgrywane jako prostokąty
//! alpha do jednej tekstury. Alokator jest celowo prosty (shelf / półka):
//! przy typowych rozmiarach UI jego fragmentacja jest znikoma, a kod pozostaje
//! łatwy do audytu.

/// Pozycja prostokąta w teksturze atlasu (w pikselach).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasSlot {
    /// Lewy górny róg w texelach.
    pub x: u32,
    /// Górny róg w texelach.
    pub y: u32,
    /// Szerokość w texelach.
    pub width: u32,
    /// Wysokość w texelach.
    pub height: u32,
}

impl AtlasSlot {
    /// Pusty slot (brak glifu).
    pub const EMPTY: Self = Self {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };

    /// Czy slot ma niezerowy rozmiar.
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// Alokator przestrzeni w atlasie glifów.
#[derive(Debug)]
pub struct ShelfAllocator {
    size: u32,
    padding: u32,
    cursor_x: u32,
    cursor_y: u32,
    shelf_height: u32,
}

impl ShelfAllocator {
    /// Nowy alokator o zadanym rozmiarze tekstury.
    pub fn new(size: u32, padding: u32) -> Self {
        Self {
            size,
            padding,
            cursor_x: 0,
            cursor_y: 0,
            shelf_height: 0,
        }
    }

    /// Rozmiar tekstury.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// Resetuje alokację (używane przy przepełnieniu atlasu).
    pub fn reset(&mut self) {
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.shelf_height = 0;
    }

    /// Zajmuje prostokąt; `None` oznacza brak miejsca.
    pub fn allocate(&mut self, width: u32, height: u32) -> Option<AtlasSlot> {
        let width = width + self.padding;
        let height = height + self.padding;
        if width > self.size || height > self.size {
            return None;
        }

        if self.cursor_x + width > self.size {
            // Nowa półka pod obecną.
            self.cursor_y += self.shelf_height;
            self.cursor_x = 0;
            self.shelf_height = 0;
        }
        if self.cursor_y + height > self.size {
            return None;
        }

        let slot = AtlasSlot {
            x: self.cursor_x,
            y: self.cursor_y,
            width,
            height,
        };

        self.cursor_x += width;
        self.shelf_height = self.shelf_height.max(height);
        Some(slot)
    }

    /// Wykorzystana powierzchnia w procentach (do telemetrii w logach).
    pub fn usage(&self) -> f32 {
        let used = self.cursor_y + self.shelf_height;
        if self.size == 0 {
            0.0
        } else {
            used as f32 / self.size as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_fills_shelves_left_to_right() {
        let mut alloc = ShelfAllocator::new(64, 0);
        let a = alloc.allocate(10, 10).unwrap();
        let b = alloc.allocate(10, 10).unwrap();
        assert_eq!(a.x, 0);
        assert_eq!(b.x, 10);
        assert_eq!(a.y, b.y);
    }

    #[test]
    fn allocator_wraps_to_next_shelf() {
        let mut alloc = ShelfAllocator::new(64, 0);
        for _ in 0..6 {
            alloc.allocate(10, 10).unwrap();
        }
        // Szósty prostokąt nie mieści się w 64 px → zaczyna nową półkę.
        let last = alloc.allocate(10, 10).unwrap();
        assert_eq!(last.x, 0);
        assert_eq!(last.y, 10);
    }

    #[test]
    fn allocator_rejects_oversized() {
        let mut alloc = ShelfAllocator::new(32, 0);
        assert!(alloc.allocate(64, 8).is_none());
        assert!(alloc.allocate(8, 64).is_none());
    }

    #[test]
    fn reset_frees_everything() {
        let mut alloc = ShelfAllocator::new(32, 0);
        alloc.allocate(32, 32).unwrap();
        assert!(alloc.allocate(8, 8).is_none());
        alloc.reset();
        assert!(alloc.allocate(8, 8).is_some());
    }

    #[test]
    fn padding_shrinks_usable_area() {
        let mut alloc = ShelfAllocator::new(32, 2);
        let slot = alloc.allocate(28, 4).unwrap();
        assert_eq!(slot.width, 30);
        assert_eq!(slot.height, 6);
    }
}
