//! Typowany bufor GPU.
//!
//! [`Buffer<T>`] opakowuje `wgpu::Buffer` wraz z typem elementu, więc shader
//! czyta dokładnie ten typ, którego oczekuje (`array<f32>`, `array<vec3<f32>>`,
//! `array<u32>`…). Typ musi implementować `bytemuck::Pod` — tak robią wszystkie
//! typy z [`math`](crate::math) oraz typy prymitywne.
//!
//! ```
//! use aurum::gpu::{Buffer, Context};
//! use aurum::math::Vec3;
//! use aurum::Result;
//!
//! # fn main() -> Result<()> {
//! let ctx = Context::new()?;
//!
//! let mut buf = Buffer::zeros(&ctx, 4, "pozycje")?;
//! buf.write(&ctx, &[Vec3::splat(1.0), Vec3::splat(2.0)])?;
//!
//! let dane = buf.read(&ctx)?;
//! assert_eq!(dane.len(), 4);
//! assert_eq!(dane[0], Vec3::splat(1.0));
//! # Ok(())
//! # }
//! ```
//!
//! # Dlaczego jest bufor pośredni
//!
//! WebGPU zabrania łączyć `MAP_READ` z jakimkolwiek innym atrybutem poza
//! `COPY_DST` — bufora z danymi nie da się więc zmapować bezpośrednio.
//! [`Buffer::read`] kopiuje zawartość do małego bufora „stagingowego” i dopiero
//! jego mapuje. Bufor pośredni powstaje raz i jest używany wielokrotnie.
//!
//! # Rodzaje buforów
//!
//! * [`Buffer::from_slice`] / [`Buffer::zeros`] — bufor **danych** (`STORAGE`):
//!   do shaderów i z powrotem na CPU;
//! * [`Buffer::uniform`] — mały bufor **uniformowy** (`UNIFORM`), tylko do
//!   odczytu przez shader (wymiary, parametry splotu).
//!
//! Bufor o długości `0` jest odrzucany: pusty bufor nigdy nie ma sensu
//! w obliczeniach, a binding musi coś opisywać.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::sync::mpsc;

use bytemuck::Pod;

use crate::error::{Error, Result};
use crate::gpu::context::Context;

/// Rodzaj bufora — decyduje o atrybutach użycia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BufferKind {
    /// Bufor danych: binding `storage`, zapis i odczyt z hosta.
    Data,
    /// Bufor uniformowy: tylko odczyt przez shader.
    Uniform,
}
/// Bufor GPU przechowujący `len` elementów typu `T`.
pub struct Buffer<T: Pod> {
    inner: wgpu::Buffer,
    /// Bufor stagingowy do odczytu — tworzony leniwie przy pierwszym `read`.
    staging: RefCell<Option<wgpu::Buffer>>,
    kind: BufferKind,
    len: usize,
    label: String,
    _marker: PhantomData<T>,
}

impl<T: Pod> std::fmt::Debug for Buffer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Buffer")
            .field("label", &self.label)
            .field("len", &self.len)
            .field("bytes", &self.bytes())
            .finish()
    }
}

impl<T: Pod> Buffer<T> {
    /// Tworzy bufor danych z tablicy (kopia trafia na GPU).
    pub fn from_slice(ctx: &Context, data: &[T], label: &str) -> Result<Self> {
        if data.is_empty() {
            return Err(Error::buffer(label, "bufor nie może być pusty"));
        }

        let inner = ctx.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes_of::<T>(data.len())?,
            usage: usage_data(),
            mapped_at_creation: false,
        });
        ctx.queue()
            .write_buffer(&inner, 0, bytemuck::cast_slice(data));

        Ok(Self::assemble(inner, data.len(), label, BufferKind::Data))
    }

    /// Tworzy bufor danych wypełniony zerami.
    pub fn zeros(ctx: &Context, len: usize, label: &str) -> Result<Self> {
        Self::filled(ctx, len, bytemuck::Zeroable::zeroed(), label)
    }

    /// Tworzy bufor danych, którego każdy element to `value`.
    pub fn filled(ctx: &Context, len: usize, value: T, label: &str) -> Result<Self> {
        if len == 0 {
            return Err(Error::buffer(label, "bufor nie może być pusty"));
        }

        let inner = ctx.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes_of::<T>(len)?,
            usage: usage_data(),
            mapped_at_creation: false,
        });
        ctx.queue()
            .write_buffer(&inner, 0, bytemuck::cast_slice(&vec![value; len]));

        Ok(Self::assemble(inner, len, label, BufferKind::Data))
    }

    /// Tworzy mały bufor uniformowy (`var<uniform>` w WGSL).
    ///
    /// Uniformów nie da się odczytać na CPU — służą wyłącznie do przekazywania
    /// parametrów (wymiarów, jądra splotu) do shadera.
    pub fn uniform(ctx: &Context, data: &[T], label: &str) -> Result<Self> {
        if data.is_empty() {
            return Err(Error::buffer(label, "bufor nie może być pusty"));
        }

        let inner = ctx.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes_of::<T>(data.len())?,
            usage: usage_uniform(),
            mapped_at_creation: false,
        });
        ctx.queue()
            .write_buffer(&inner, 0, bytemuck::cast_slice(data));

        Ok(Self::assemble(inner, data.len(), label, BufferKind::Uniform))
    }

    /// Liczba elementów.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Czy bufor nie ma elementów.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Rozmiar bufora w bajtach.
    pub fn bytes(&self) -> u64 {
        std::mem::size_of::<T>() as u64 * self.len as u64
    }

    /// Etykieta bufora.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Surowy bufor `wgpu` — np. do `copy_buffer_to_buffer` w twoim encoderze.
    pub fn raw(&self) -> &wgpu::Buffer {
        &self.inner
    }

    /// Zapisuje dane na początek bufora.
    ///
    /// Dane nie mogą być dłuższe niż bufor. `wgpu` kopiuje je wewnętrznie, więc
    /// wywołanie nie wymaga oczekiwania na GPU.
    pub fn write(&self, ctx: &Context, data: &[T]) -> Result<()> {
        if data.len() > self.len {
            return Err(Error::buffer(
                &self.label,
                format!(
                    "zapis {} elementów do bufora o {} elementach",
                    data.len(),
                    self.len
                ),
            ));
        }
        if !data.is_empty() {
            ctx.queue()
                .write_buffer(&self.inner, 0, bytemuck::cast_slice(data));
        }
        Ok(())
    }

    /// Odczytuje zawartość bufora do hosta.
    ///
    /// Kopiuje dane do bufora stagingowego, mapuje go i czeka na GPU
    /// (`device.poll`) — po wywołaniu zawartość jest już pewna.
    ///
    /// ```
    /// use aurum::gpu::{Buffer, Context};
    /// use aurum::Result;
    ///
    /// # fn main() -> Result<()> {
    /// let ctx = Context::new()?;
    /// let buf = Buffer::from_slice(&ctx, &[1.0f32, 2.0, 3.0], "x")?;
    /// assert_eq!(buf.read(&ctx)?, vec![1.0, 2.0, 3.0]);
    /// # Ok(())
    /// # }
    /// ```
    pub fn read(&self, ctx: &Context) -> Result<Vec<T>> {
        let mut out = Vec::new();
        self.read_into(ctx, &mut out)?;
        Ok(out)
    }

    /// Odczytuje zawartość bufora do wcześniej zaalokowanego wektora.
    ///
    /// Oszczędza alokowanie w pętlach (np. przy animacji). Zawartość `out` jest
    /// wymieniana, nie dopisywana.
    pub fn read_into(&self, ctx: &Context, out: &mut Vec<T>) -> Result<()> {
        if self.kind == BufferKind::Uniform {
            return Err(Error::buffer(
                &self.label,
                "buforów uniformowych nie odczytuje się na CPU",
            ));
        }

        let bytes = self.bytes() as usize;
        if bytes == 0 {
            return Err(Error::empty("bufor"));
        }
        // `copy_buffer_to_buffer` wymaga wielokrotności 4 B (wyrównanie
        // `COPY_BYTES_PER_ROW_ALIGNMENT` = 256 B dotyczy tylko kopii
        // tekstura → bufor).
        const ALIGNMENT: usize = 4;
        if bytes % ALIGNMENT != 0 {
            return Err(Error::buffer(
                &self.label,
                format!("odczyt wymaga rozmiaru wielokrotności {ALIGNMENT} B, a bufor ma {bytes} B"),
            ));
        }

        // Bufor stagingowy tworzymy raz i trzymamy — `read` bywa wołane
        // w każdej klatce animacji.
        if self.staging.borrow().is_none() {
            let staging = ctx.device().create_buffer(&wgpu::BufferDescriptor {
                label: Some(&self.label),
                size: bytes as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            *self.staging.borrow_mut() = Some(staging);
        }

        let staging = self.staging.borrow();
        let staging = staging.as_ref().expect("bufor stagingowy utworzony wyżej");

        ctx.submit(|encoder| {
            encoder.copy_buffer_to_buffer(&self.inner, 0, staging, 0, bytes as u64);
            Ok(())
        })?;
        ctx.poll()?;

        let slice = staging.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });

        ctx.device()
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| Error::Poll(e.to_string()))?;
        receiver
            .recv()
            .map_err(|e| Error::Map(format!("callback mapowania nie przysiedł: {e}")))?
            .map_err(|e| Error::Map(e.to_string()))?;

        {
            let view = slice
                .get_mapped_range()
                .map_err(|e| Error::Map(e.to_string()))?;
            let typed = bytemuck::try_cast_slice::<u8, T>(&view)
                .map_err(|e| Error::Map(e.to_string()))?;
            out.clear();
            out.extend_from_slice(typed);
        }

        staging.unmap();
        Ok(())
    }

    /// Składa bufor po utworzeniu — wspólny ogon konstruktorów.
    fn assemble(inner: wgpu::Buffer, len: usize, label: &str, kind: BufferKind) -> Self {
        Self {
            inner,
            staging: RefCell::new(None),
            kind,
            len,
            label: label.to_owned(),
            _marker: PhantomData,
        }
    }
}

/// Flagi użycia bufora danych.
fn usage_data() -> wgpu::BufferUsages {
    wgpu::BufferUsages::STORAGE
        | wgpu::BufferUsages::COPY_DST
        | wgpu::BufferUsages::COPY_SRC
}

/// Flagi użycia bufora uniformowego.
fn usage_uniform() -> wgpu::BufferUsages {
    wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST
}

/// Rozmiar w bajtach z kontrolą przepełnienia.
fn bytes_of<T>(len: usize) -> Result<u64> {
    let size = std::mem::size_of::<T>() as u64;
    size.checked_mul(len as u64)
        .ok_or_else(|| Error::Buffer {
            label: "bufor".to_owned(),
            message: format!("{len} elementów × {size} B nie mieści się w u64"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rodzaje_buforow_maja_odmienne_flagi() {
        // Uniform nie może być `STORAGE` — inaczej nie da się go odczytać.
        assert!(usage_data().contains(wgpu::BufferUsages::STORAGE));
        assert!(usage_data().contains(wgpu::BufferUsages::COPY_SRC));
        assert!(usage_uniform().contains(wgpu::BufferUsages::UNIFORM));
        assert!(!usage_uniform().contains(wgpu::BufferUsages::STORAGE));
    }

    #[test]
    fn rozmiar_bufora_nie_przepelnia_sie() {
        assert_eq!(bytes_of::<f32>(4).unwrap(), 16);
        assert!(bytes_of::<f32>(usize::MAX).is_err());
    }
}