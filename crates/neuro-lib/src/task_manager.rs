//! Zadania dla Aurole: plik XML → [`TaskCell`] i maszyna stanów [`Machine`].
//!
//! Plik XML (np. `tasks/calc_x86_64.xml`) opisuje:
//!
//! * docelową architekturę (`TargetArch`),
//! * mapowanie rejestrów na wektory GPU (`RegisterMapping`),
//! * wymagane mnemoniki (`RequiredOpcodes`),
//! * **operacje kalkulatora** (`<Operations>`),
//! * konfigurację macierzy: punkt impulsu, tempo dyfuzji, limit iteracji.
//!
//! [`Machine`] pilnuje stanu maszynowego: każde przejście jest od razu
//! zapisywane do śladu i logowane (`log::info!`), więc nic nie umyka.

use std::fmt;
use std::fs;
use std::path::Path;

use crate::error::{Error, Result};

/// Operacja kalkulatora — nazwa z XML, mnemonik i etykieta po polsku.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Dodawanie (`ADD`).
    Add,
    /// Odejmowanie (`SUB`).
    Sub,
    /// Mnożenie (`MUL`).
    Mul,
    /// Dzielenie (`DIV`).
    Div,
    /// Logarytm naturalny (`LOG`).
    Log,
    /// Pierwiastek kwadratowy (`SQRT`).
    Sqrt,
}

impl Operation {
    /// Nazwa używana w XML (`add`, `sub`, …).
    pub fn name(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Sub => "sub",
            Self::Mul => "mul",
            Self::Div => "div",
            Self::Log => "log",
            Self::Sqrt => "sqrt",
        }
    }

    /// Mnemonik zgodny z `RequiredOpcodes` w XML (`ADD`, `SUB`, …).
    pub fn mnemonic(self) -> &'static str {
        match self {
            Self::Add => "ADD",
            Self::Sub => "SUB",
            Self::Mul => "MUL",
            Self::Div => "DIV",
            Self::Log => "LOG",
            Self::Sqrt => "SQRT",
        }
    }

    /// Opis po polsku (do raportu).
    pub fn label(self) -> &'static str {
        match self {
            Self::Add => "dodawanie",
            Self::Sub => "odejmowanie",
            Self::Mul => "mnożenie",
            Self::Div => "dzielenie",
            Self::Log => "logarytm naturalny",
            Self::Sqrt => "pierwiastek kwadratowy",
        }
    }

    /// Rozpoznaje nazwę z XML; nieznana nazwa to błąd zadania.
    pub fn from_name(name: &str) -> Result<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "add" | "+" => Ok(Self::Add),
            "sub" | "-" => Ok(Self::Sub),
            "mul" | "*" => Ok(Self::Mul),
            "div" | "/" => Ok(Self::Div),
            "log" | "ln" => Ok(Self::Log),
            "sqrt" | "root" => Ok(Self::Sqrt),
            other => Err(Error::Task(format!("nieznana operacja `{other}`"))),
        }
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.label(), self.mnemonic())
    }
}

/// Ograniczenie na pojedynczy rejestr — odpowiednik wektora na GPU.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterConstraint {
    /// Nazwa rejestru (np. `RAX`).
    pub name: String,
    /// Który wektor w buforze GPU to ten rejestr.
    pub vector_id: u32,
    /// Rola w operacji (np. `accumulator`).
    pub role: String,
}

/// Ograniczenie na mnemonik — ma się pojawić w wygenerowanym programie.
#[derive(Debug, Clone, PartialEq)]
pub struct OpcodeConstraint {
    /// Mnemonik (np. `ADD`).
    pub mnemonic: String,
    /// Jak silny ma być impuls dla tej instrukcji.
    pub target_weight: f32,
}

/// Wczytywanie zadań z plików XML.
pub struct TaskManager;

type Xml<'a> = roxmltree::Node<'a, 'a>;

impl TaskManager {
    /// Wczytuje zadanie z pliku XML.
    pub fn load_task_from_xml(path: impl AsRef<Path>) -> Result<TaskCell> {
        let path = path.as_ref();
        let xml = fs::read_to_string(path).map_err(|error| {
            Error::Task(format!("nie udało się wczytać {}: {error}", path.display()))
        })?;
        Self::parse_task(&xml)
            .map_err(|error| Error::Task(format!("{}: {error}", path.display())))
    }

    /// Parsuje treść XML z zadaniem dla Aurole.
    pub fn parse_task(xml: &str) -> Result<TaskCell> {
        let doc =
            roxmltree::Document::parse(xml).map_err(|error| Error::Task(error.to_string()))?;
        let root = doc.root_element();
        if !root.has_tag_name("Task") {
            return Err(Error::Task(format!(
                "oczekiwano korzenia <Task>, jest <{}>",
                root.tag_name().name()
            )));
        }

        let id = root.attribute("id").unwrap_or("unknown").to_string();
        let arch = child_text(root, "TargetArch").unwrap_or_else(|| "x86_64".to_string());

        let register_map = parse_registers(root)?;
        let required_opcodes = parse_opcodes(root)?;
        let operations = parse_operations(root)?;
        let (impulse_origin, max_iterations, diffusion_rate) = parse_matrix_config(root)?;

        Ok(TaskCell {
            id,
            arch,
            register_map,
            required_opcodes,
            impulse_origin,
            max_iterations,
            diffusion_rate,
            operations,
        })
    }
}

/// Pierwszy element-dziecko o podanej nazwie.
fn child<'a>(node: Xml<'a>, name: &str) -> Option<Xml<'a>> {
    node.children()
        .find(|n| n.is_element() && n.has_tag_name(name))
}

/// Pierwszy potomek o podanej nazwie (dowolna głębokość).
fn descendant<'a>(node: Xml<'a>, name: &str) -> Option<Xml<'a>> {
    node.descendants().find(|n| n.is_element() && n.has_tag_name(name))
}

/// Tekst wewnątrz elementu.
fn child_text(node: Xml<'_>, name: &str) -> Option<String> {
    child(node, name)
        .and_then(|n| n.text())
        .map(|text| text.trim().to_string())
}

/// Atrybut wymagany — z czytelnym komunikatem o braku.
fn required_attr(node: Xml<'_>, name: &str, ctx: &str) -> Result<String> {
    node.attribute(name)
        .map(|value| value.to_string())
        .ok_or_else(|| Error::Task(format!("{ctx}: brak atrybutu `{name}`")))
}

fn parse_registers(root: Xml<'_>) -> Result<Vec<RegisterConstraint>> {
    let mut registers = Vec::new();
    if let Some(mapping) = descendant(root, "RegisterMapping") {
        for reg in mapping.children().filter(|n| n.is_element() && n.has_tag_name("Register")) {
            let name = required_attr(reg, "name", "Register")?;
            let vector_id = required_attr(reg, "vector_id", "Register")?
                .parse()
                .map_err(|error| Error::Task(format!("Register `{name}`: vector_id: {error}")))?;
            let role = required_attr(reg, "role", "Register")?;
            registers.push(RegisterConstraint {
                name,
                vector_id,
                role,
            });
        }
    }
    Ok(registers)
}

fn parse_opcodes(root: Xml<'_>) -> Result<Vec<OpcodeConstraint>> {
    let mut opcodes = Vec::new();
    if let Some(required) = descendant(root, "RequiredOpcodes") {
        for op in required.children().filter(|n| n.is_element() && n.has_tag_name("Opcode")) {
            let mnemonic = required_attr(op, "mnemonic", "Opcode")?;
            let target_weight = match op.attribute("target_weight") {
                Some(value) => value
                    .parse()
                    .map_err(|error| Error::Task(format!("Opcode `{mnemonic}`: {error}")))?,
                None => 1.0,
            };
            opcodes.push(OpcodeConstraint {
                mnemonic,
                target_weight,
            });
        }
    }
    Ok(opcodes)
}

fn parse_operations(root: Xml<'_>) -> Result<Vec<Operation>> {
    let Some(section) = descendant(root, "Operations") else {
        // Zadanie sprzed listy operacji opisywało samo dodawanie.
        return Ok(vec![Operation::Add]);
    };

    let mut operations = Vec::new();
    for op in section.children().filter(|n| n.is_element()) {
        let name = op
            .attribute("name")
            .or_else(|| op.attribute("kind"))
            .or_else(|| op.text())
            .ok_or_else(|| Error::Task("Operations: element bez nazwy operacji".into()))?;
        operations.push(Operation::from_name(name)?);
    }

    if operations.is_empty() {
        return Err(Error::Task("sekcja <Operations> jest pusta".into()));
    }
    Ok(operations)
}

fn parse_matrix_config(root: Xml<'_>) -> Result<((u32, u32), u32, f32)> {
    let mut origin = (32, 32);
    let mut max_iterations = 50;
    let mut diffusion_rate = 0.15;

    if let Some(cfg) = child(root, "MatrixConfig") {
        if let Some(imp) = child(cfg, "ImpulseOrigin") {
            let x = required_attr(imp, "x", "ImpulseOrigin")?
                .parse()
                .map_err(|error| Error::Task(format!("ImpulseOrigin.x: {error}")))?;
            let y = required_attr(imp, "y", "ImpulseOrigin")?
                .parse()
                .map_err(|error| Error::Task(format!("ImpulseOrigin.y: {error}")))?;
            origin = (x, y);
        }
        if let Some(iter) = child_text(cfg, "MaxIterations") {
            max_iterations = iter
                .parse()
                .map_err(|error| Error::Task(format!("MaxIterations: {error}")))?;
        }
        if let Some(rate) = child_text(cfg, "DiffusionRate") {
            diffusion_rate = rate
                .parse()
                .map_err(|error| Error::Task(format!("DiffusionRate: {error}")))?;
        }
    }

    Ok((origin, max_iterations, diffusion_rate))
}

/// Jedno zadanie wczytane z pliku XML.
#[derive(Debug, Clone)]
pub struct TaskCell {
    /// Identyfikator zadania (atrybut `id`).
    pub id: String,
    /// Architektura docelowa (np. `x86_64`).
    pub arch: String,
    /// Mapowanie rejestrów.
    pub register_map: Vec<RegisterConstraint>,
    /// Mnemoniki, które muszą znaleźć się w programie.
    pub required_opcodes: Vec<OpcodeConstraint>,
    /// Współrzędne impulsu w macierzy.
    pub impulse_origin: (u32, u32),
    /// Limit iteracji maszyny.
    pub max_iterations: u32,
    /// Tempo dyfuzji z macierzy (`DiffusionRate`).
    pub diffusion_rate: f32,
    /// Operacje kalkulatora do wykonania.
    pub operations: Vec<Operation>,
}

/// Stan maszyny wykonującej zadanie.
///
/// Oznaczany **natychmiast** po każdym kroku — patrz [`Machine::transition`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MachineState {
    /// Maszyna stoi, zadanie jeszcze nie ruszyło.
    Idle,
    /// Wczytywanie zadania i inicjalizacja zasobów.
    Loading,
    /// Wykonanie na GPU: impuls w macierzy + cykle symulacji.
    Executing,
    /// Odczyt stanu macierzy z GPU do CPU.
    Reading,
    /// Dekoder zamienia wzorzec na linię asemblera.
    Decoding,
    /// Dekoder odrzucił wzorzec — powtórka tej samej instrukcji.
    Retrying,
    /// Instrukcja przyjęta, trafia do programu.
    Emitting,
    /// Sprawdzenie ograniczeń XML i składni.
    Verifying,
    /// Zadanie wykonane w całości.
    Completed,
    /// Zadanie przerwane (błąd lub wyczerpany limit iteracji).
    Failed,
}

impl MachineState {
    /// Krótki kod do linii raportu (`WYKONANIE`, `DEKODOWANIE`, …).
    pub fn code(self) -> &'static str {
        match self {
            Self::Idle => "BEZCZYNNY",
            Self::Loading => "ŁADOWANIE",
            Self::Executing => "WYKONANIE",
            Self::Reading => "ODCZYT",
            Self::Decoding => "DEKODOWANIE",
            Self::Retrying => "POWTÓRKA",
            Self::Emitting => "ZAPIS",
            Self::Verifying => "WERYFIKACJA",
            Self::Completed => "WYKONANE",
            Self::Failed => "BŁĄD",
        }
    }

    /// Czy maszyna zakończyła pracę (dobrze albo źle)?
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

impl fmt::Display for MachineState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Pojedynczy zapis stanu maszynowego.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineReport {
    /// Numer iteracji (0 = jeszcze przed pętlą).
    pub iteration: u32,
    /// Stan po przejściu.
    pub state: MachineState,
    /// Co się wydarzyło.
    pub detail: String,
}

impl fmt::Display for MachineReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[maszyna] iter={:>3} stan={:<12} {}",
            self.iteration,
            self.state.code(),
            self.detail
        )
    }
}

/// Maszyna stanów zadania — pilnuje iteracji i śladu przejść.
#[derive(Debug)]
pub struct Machine {
    state: MachineState,
    iteration: u32,
    trace: Vec<MachineReport>,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    /// Nowa maszyna w stanie [`MachineState::Idle`].
    pub fn new() -> Self {
        Self {
            state: MachineState::Idle,
            iteration: 0,
            trace: Vec::new(),
        }
    }

    /// Bieżący stan.
    pub fn state(&self) -> MachineState {
        self.state
    }

    /// Bieżąca iteracja pętli wykonania.
    pub fn iteration(&self) -> u32 {
        self.iteration
    }

    /// Ślad wszystkich przejść (od najstarszego).
    pub fn trace(&self) -> &[MachineReport] {
        &self.trace
    }

    /// Zwiększa licznik iteracji.
    pub fn tick(&mut self) {
        self.iteration += 1;
    }

    /// Przejście do nowego stanu — **od razu** zapisuje ślad i loguje.
    pub fn transition(&mut self, state: MachineState, detail: impl Into<String>) -> MachineReport {
        self.state = state;
        let report = MachineReport {
            iteration: self.iteration,
            state,
            detail: detail.into(),
        };
        log::info!("{report}");
        self.trace.push(report);
        self.trace
            .last()
            .cloned()
            .expect("raport dopiero co zapisany w śladzie")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        <Task id="t1" type="assembly_generation">
            <TargetArch>x86_64</TargetArch>
            <Constraints>
                <RegisterMapping>
                    <Register name="RAX" vector_id="0" role="accumulator" />
                    <Register name="RBX" vector_id="1" role="operand_src_1" />
                </RegisterMapping>
                <RequiredOpcodes>
                    <Opcode mnemonic="ADD" target_weight="1.0" />
                </RequiredOpcodes>
            </Constraints>
            <Operations>
                <Op name="add" />
                <Op name="sqrt" />
                <Op name="log" />
            </Operations>
            <MatrixConfig>
                <ImpulseOrigin x="16" y="32" />
                <DiffusionRate>0.25</DiffusionRate>
                <MaxIterations>7</MaxIterations>
            </MatrixConfig>
        </Task>
    "#;

    #[test]
    fn zadanie_parsuje_sie_z_xml() {
        let task = TaskManager::parse_task(SAMPLE).unwrap();
        assert_eq!(task.id, "t1");
        assert_eq!(task.arch, "x86_64");
        assert_eq!(task.impulse_origin, (16, 32));
        assert_eq!(task.max_iterations, 7);
        assert_eq!(task.diffusion_rate, 0.25);
        assert_eq!(
            task.operations,
            vec![Operation::Add, Operation::Sqrt, Operation::Log]
        );
        assert_eq!(task.register_map.len(), 2);
        assert_eq!(task.register_map[0].name, "RAX");
        assert_eq!(task.required_opcodes[0].mnemonic, "ADD");
    }

    #[test]
    fn pusta_sekcja_operations_to_blad_a_brak_to_dodawanie() {
        assert!(TaskManager::parse_task(r#"<Task id="x"><Operations /></Task>"#).is_err());
        assert_eq!(
            TaskManager::parse_task(r#"<Task id="x"></Task>"#)
                .unwrap()
                .operations,
            vec![Operation::Add]
        );
    }

    #[test]
    fn nieznana_operacja_to_blad() {
        assert!(Operation::from_name("sinus").is_err());
        assert!(Operation::from_name("DIV").is_ok());
    }

    #[test]
    fn maszyna_natychmiast_oznacza_stan() {
        let mut machine = Machine::new();
        assert_eq!(machine.state(), MachineState::Idle);

        let report = machine.transition(MachineState::Loading, "wczytuję XML");
        assert_eq!(report.state, MachineState::Loading);
        assert_eq!(machine.state(), MachineState::Loading);

        machine.tick();
        machine.transition(MachineState::Executing, "impuls");
        machine.transition(MachineState::Completed, "koniec");

        assert_eq!(machine.trace().len(), 3);
        assert_eq!(machine.iteration(), 1);
        assert!(machine.state().is_terminal());
        assert_eq!(machine.trace()[0].iteration, 0);
        assert_eq!(machine.trace()[2].iteration, 1);
        assert_eq!(machine.trace()[2].state, MachineState::Completed);
    }
}
