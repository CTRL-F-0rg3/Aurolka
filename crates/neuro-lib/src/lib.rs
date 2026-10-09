//! **neuro-lib** — topologia neuronów i GPU-owy menedżer sektorów.
//!
//! Crate ma dwie części:
//!
//! * [`neuron_topology`] — warstwa CPU: [`Neuron`], [`RunConfig`] i [`Topology`]
//!   (mapa sektorów hex). [`Neuron`] jest `#[repr(C)]` i zgadza się bajt w bajt
//!   ze strukturą `Neuron` w shaderze WGSL.
//! * [`gpu_manager`] — [`GpuSectorManager`]: `Device`/`Queue` z `wgpu`,
//!   pipeline compute’owy, bufory sektorów oraz mechanizm uprawnień Płatków
//!   ([`SectorAccessRights`]).
//! * [`scenario_matrix`] — [`ScenarioMatrixManager`]: drugi kernel
//!   (`scenario_matrix.wgsl`) liczący globalną macierz scenariuszy i adaptację
//!   form neuronów ([`NeuronForm`]).
//!
//! Crate [`neuro-bin`](https://github.com/ctrl/aurola) pokazuje pełne
//! uruchomienie: shader `neuron_pipeline.wgsl`, kilka kroków symulacji
//! i odczyt wyników na CPU.
//!
//! # Przykład
//!
//! ```no_run
//! use neuro_lib::{GpuSectorManager, Neuron, SectorAccessRights, Topology};
//!
//! # const WGSL: &str = r#"
//! # struct Config { total_neurons: u32, time_step: f32, _pad1: u32, _pad2: u32 }
//! # @group(0) @binding(0) var<storage, read_write> neurons: array<Neuron>;
//! # @group(0) @binding(1) var<storage, read> sector_map: array<u32>;
//! # @group(0) @binding(2) var<uniform> config: Config;
//! # struct Neuron { a: f32, b: f32, c: f32, d: f32, e: u32, f: u32, g: f32, h: u32,
//! #                 i: u32, j: f32, k: u32, l: u32 }
//! # @compute @workgroup_size(64)
//! # fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
//! #     if (gid.x >= config.total_neurons) { return; }
//! # }
//! # "#;
//!
//! # fn main() -> neuro_lib::Result<()> {
//! let topology = Topology::new(&[64, 64])?;
//! let neurons = vec![Neuron::default(); topology.total_neurons() as usize];
//!
//! let manager = GpuSectorManager::new(WGSL, &neurons, &topology, 1.0)?;
//! let rights = [
//!     SectorAccessRights::NeuronWorkspace,
//!     SectorAccessRights::SystemMap,
//!     SectorAccessRights::Config,
//! ];
//! let group = manager.create_petal_bind_group(&rights);
//!
//! manager.execute_neuron_pipeline(&group, topology.total_neurons());
//! let po_kroku = manager.read_neurons()?;
//! assert_eq!(po_kroku.len(), neurons.len());
//! # Ok(())
//! # }
//! ```
//!
//! # Wątek
//!
//! [`GpuSectorManager`] nie jest `Sync` (device wgpu należy do jednego wątku),
//! więc jeden menedżer obsługuje jedną pętlę symulacji.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod error;
pub mod gpu_manager;
pub mod neuron_topology;
pub mod png;
pub mod scenario_engine;
pub mod scenario_matrix;
pub mod task_manager;
pub mod visual_memory;

pub use error::{Error, Result};
pub use gpu_manager::{GpuSectorManager, SectorAccessRights};
pub use neuron_topology::{Decision, Neuron, RunConfig, Topology};
pub use png::PngImage;
pub use scenario_engine::ScenarioEngine;
pub use scenario_matrix::{NeuronForm, ScenarioMatrixManager};
pub use task_manager::{
    Machine, MachineReport, MachineState, OpcodeConstraint, Operation, RegisterConstraint, TaskCell,
    TaskManager,
};
pub use visual_memory::VisualMemoryManager;
