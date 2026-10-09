// neuro-bin/shaders/assembly_logic_core.wgsl

// Struktura reprezentująca stan pojedynczego rejestru w sieci (np. RAX)
// Zamiast jednego floata, to wektor stanu (np. [wartość_liczbowa, typ_danych, pewnosc, stan_flagi])
struct RegisterState {
    value: vec4<f32>, 
};

// Struktura reprezentująca wygenerowaną instrukcję (Opcode + Operandy)
struct InstructionState {
    opcode_vec: vec4<f32>, // Wektor liczbowy, który dekoder zamieni na np. "ADD"
    operand1_vec: vec4<f32>, // Np. identyfikator rejestru źródłowego
    operand2_vec: vec4<f32>, // Np. identyfikator rejestru docelowego
    confidence: f32, // Jak bardzo sieć jest "pewna" tej instrukcji
};

// --- SEKTORY HEX (Bufory) ---
// Sektor Stanów Rejestrów (np. 16 rejestrów x 4 floaty)
@group(0) @binding(0) var<storage, read_write> gpr_states: array<RegisterState>;
// Sektor Stanu Flag (ALU)
@group(0) @binding(1) var<storage, read_write> alu_flags: array<f32>;
// Sektor Generowanych Instrukcji (Tutaj sieć "składa" kod)
@group(0) @binding(2) var<storage, read_write> generated_instructions: array<InstructionState>;

// --- KERNEL: SYMULACJA LOGIKI MASZYNOWEJ ---
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let reg_id = global_id.x;
    if (reg_id >= 16u) { return; } // Mamy 16 rejestrów GPR w x86_64

    var my_reg = gpr_states[reg_id];
    
    // --- LOGIKA ALU (Symulacja operacji na rejestrach) ---
    // Sieć "decyduje" o operacji na podstawie wag neuronów (z poprzednich warstw)
    // Tutaj dzieje się magia: liczby w my_reg.value zmieniają się w sposób deterministyczny,
    // symulując przepływ danych przez ALU.
    
    // Przykład: Symulacja operacji ADD (jeśli sieć wybrała taką ścieżkę decyzyjną)
    // W rzeczywistości to skomplikowana funkcja aktywacji oparta na wagach neuronu
    let alu_result = my_reg.value.x + gpr_states[(reg_id + 1u) % 16u].value.x; 
    
    // Aktualizacja stanu rejestru
    my_reg.value.x = alu_result;
    
    // --- AKTUALIZACJA FLAG (Kluczowe dla poprawnego kodu!) ---
    // Jeśli wynik to zero, sieć musi "zapalić" flagę Zero w macierzy alu_flags
    if (abs(alu_result) < 0.001) {
        alu_flags[0] = 1.0; // Zero Flag = True
    } else {
        alu_flags[0] = 0.0;
    }

    // --- GENEROWANIE KODU (Zapis do sektora instrukcji) ---
    // Sieć nie generuje tekstu "ADD RAX, RBX". Ona generuje wektor liczbowy (opcode_vec),
    // który jest matematycznym odwzorowaniem tej instrukcji.
    var instr = generated_instructions[reg_id];
    
    // Wektor opcode: np. [0.1, 0.9, 0.0, 0.0] może oznaczać "ADD" w słowniku dekodera
    instr.opcode_vec = vec4<f32>(0.1, 0.9, 0.0, 0.0); 
    instr.operand1_vec = vec4<f32>(f32(reg_id), 0.0, 0.0, 0.0); // Źródło
    instr.operand2_vec = vec4<f32>(f32((reg_id + 1u) % 16u), 0.0, 0.0, 0.0); // Cel
    instr.confidence = 0.95; // Pewność wygenerowana przez warstwę decyzyjną
    
    generated_instructions[reg_id] = instr;
    gpr_states[reg_id] = my_reg;
}