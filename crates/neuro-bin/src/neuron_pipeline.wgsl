// neuro-bin/shaders/neuron_pipeline.wgsl

// Struktura Neuronu (Musi idealnie pasować do Rusta - pamiętaj o paddingu do wielokrotności 16 bajtów!)
struct Neuron {
    input_signal: f32,
    input_weight: f32,
    core_state: f32,
    decision_projection: f32,
    
    connection_target_sector: u32,
    connection_target_index: u32,
    decision_prior: f32,
    decision_state: u32, // W WGSL używamy u32 zamiast u8 dla lepszego alignu
    
    action_intent: u32,
    output_signal: f32,
    current_hex_sector: u32,
    _padding: u32, // Wyrównanie struktury
};

// Konfiguracja uruchomienia (Uniformy)
struct Config {
    total_neurons: u32,
    time_step: f32,
    _pad1: u32,
    _pad2: u32,
};

// === SEKTORY HEX (Bufory Pamięci) ===
// W wgpu nie ma globalnej pamięci. Są tylko bindowane bufory.
// Grupa 0, Binding 0: Główna tablica neuronów (Sektor Roboczy)
@group(0) @binding(0) var<storage, read_write> neurons: array<Neuron>;

// Grupa 0, Binding 1: Tabela mapująca sektory hex na offsety (Sektor Systemowy)
@group(0) @binding(1) var<storage, read> sector_map: array<u32>;

// Grupa 0, Binding 2: Konfiguracja
@group(0) @binding(2) var<uniform> config: Config;

// === GŁÓWNY KERNEL (Compute Shader) ===
// Uruchamiany w grupach po 64 wątki (idealne dla AMD RDNA)
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let neuron_id = global_id.x;
    
    // Zabezpieczenie przed wyjściem poza przydzielony sektor
    if (neuron_id >= config.total_neurons) {
        return;
    }

    // Pobranie stanu neuronu z jego sektora hex
    var my_neuron = neurons[neuron_id];

    // --- ETAP 1: WEJŚCIE ---
    let current_input = my_neuron.input_signal * my_neuron.input_weight;

    // --- ETAP 2: RDZEŃ ---
    // Przetwarzanie w rdzeniu (funkcja aktywacji z pamięcią stanu)
    my_neuron.core_state = tanh(my_neuron.core_state * 0.9 + current_input);

    // --- ETAP 3: PROJEKT DECYZYJNY ---
    my_neuron.decision_projection = my_neuron.core_state * 1.5;

    // --- ETAP 4: ŁĄCZENIE ---
    // Obliczenie offsetu w docelowym sektorze hex
    let target_offset = sector_map[my_neuron.connection_target_sector] + my_neuron.connection_target_index;
    var feedback_signal = 0.0;
    
    // WGSL gwarantuje bezpieczeństwo: jeśli target_offset jest poza buforem, 
    // warstwa abstrakcji (wgpu) po prostu zwróci 0, bez crasha GPU!
    if (target_offset < config.total_neurons) {
        feedback_signal = neurons[target_offset].output_signal;
    }

    // --- ETAP 5: PRIORYTETY DECYZYJNE ---
    // Modyfikacja projektu o twarde priorytety (mechanizm anty-halucynacyjny)
    let adjusted_projection = my_neuron.decision_projection * (1.0 - my_neuron.decision_prior) 
                            + feedback_signal * my_neuron.decision_prior;

    // --- ETAP 6: DECYZJA (ZBALANSOWANA) ---
    // Energia neuronu: jak bardzo jest "podkręcony" po przetworzeniu sygnału.
    let energy = abs(adjusted_projection) + abs(feedback_signal);

    // Budżet energetyczny neuronu (0.6 .. 1.6) — neuron z wyższym decision_prior
    // jest bardziej odważny, ale migracja pozostaje ostatecznością.
    let budget = 0.6 + my_neuron.decision_prior;
    let migrate_threshold = budget * 1.5;
    let fire_threshold = budget * 0.5;

    if (energy > migrate_threshold) {
        // Tylko skrajne przeciążenie wymusza migrację do innego sektora
        my_neuron.decision_state = 2u; // MIGRATE
    } else if (energy > fire_threshold) {
        // Normalna praca — generowanie wzorca i adaptacja wag
        my_neuron.decision_state = 1u; // FIRE
    } else {
        // Spoczynek, oszczędzanie energii
        my_neuron.decision_state = 0u; // IDLE
    }

    // --- ETAP 7: OKREŚLENIE DZIAŁANIA ---
    my_neuron.action_intent = my_neuron.decision_state;

    // Jeśli neuron migruje, celowo dodajemy szum do jego rdzenia, 
    // aby w nowym sektorze musiał się na nowo "nauczyć" kontekstu.
    if (my_neuron.decision_state == 2u) {
        my_neuron.core_state = my_neuron.core_state + 0.2;
    }

    // --- ETAP 8: WYJŚCIE ---
    my_neuron.output_signal = my_neuron.core_state * f32(my_neuron.action_intent);
    
    // Reset wejścia na następny cykl
    my_neuron.input_signal = 0.0;

    // Zapis zaktualizowanego stanu z powrotem do sektora hex
    neurons[neuron_id] = my_neuron;
}