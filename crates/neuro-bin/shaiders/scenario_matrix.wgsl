// neuro-bin/shaders/scenario_matrix.wgsl

// --- STRUKTURY DANYCH ---

// Stan pojedynczego neuronu (jego "forma" i wagi)
struct NeuronForm {
    // Wagi lokalne, które neuron modyfikuje podczas adaptacji (zmiany formy)
    weights: vec4<f32>, 
    // Próg pobudzenia (kiedy neuron uznaje, że musi zmienić formę)
    adaptation_threshold: f32,
    // Współczynnik synchronizacji z otoczeniem
    sync_rate: f32,
    // Aktualny poziom energii/stresu neuronu
    stress_level: f32,
    _pad: f32,
};

// --- SEKTORY HEX (Bufory wgpu) ---

// Sektor 1: Globalna Macierz Scenariuszy (2048 x 2048 floatów -> 2048 x 512 vec4)
// To tu neurony "rysują" swoje przewidywalne schematy liczbowe.
@group(0) @binding(0) var<storage, read_write> scenario_matrix: array<vec4<f32>>;

// Sektor 2: Tablica Form Neuronów (każdy neuron ma swoją wagę)
@group(0) @binding(1) var<storage, read_write> neuron_forms: array<NeuronForm>;

// Sektor 3: Wektor Impulsu (Stimulus) - punkt w matrycy, który wywołuje szok/adaptację
@group(0) @binding(2) var<storage, read> stimulus_input: array<vec4<f32>>;

// Konfiguracja
@group(0) @binding(3) var<uniform> config: vec4<u32>; // x: matrix_width(2048), y: matrix_height(2048), z: total_neurons, w: stimulus_active

// --- FUNKCJE POMOCNICZE (Indeksowanie 2D) ---

// Zamienia 2D koordynaty (x, y) na 1D indeks dla vec4 (dzielimy X przez 4)
fn get_1d_index(x: u32, y: u32, width: u32) -> u32 {
    return (y * (width / 4u)) + (x / 4u);
}

// Bezpieczny odczyt z matrycy (WGSL i tak to robi, ale dla jasności logiki)
fn read_matrix(x: u32, y: u32, width: u32, height: u32) -> vec4<f32> {
    if (x >= width || y >= height) { return vec4<f32>(0.0); }
    return scenario_matrix[get_1d_index(x, y, width)];
}

// --- GŁÓWNY KERNEL: SYMULACJA SCENARIUSZY I ADAPTACJA ---

@compute @workgroup_size(16, 16) // 256 wątków na grupę (idealne dla AMD)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x * 4u; // Każdy wątek przetwarza 4 piksele/komórki (vec4)
    let y = global_id.y;
    
    let width = config.x;
    let height = config.y;

    if (x >= width || y >= height) { return; }

    let idx = get_1d_index(x, y, width);
    
    // 1. ODCZYT AKTUALNEGO STANU Z MACIERZY SCENARIUSZY
    var current_state = scenario_matrix[idx];
    
    // 2. ODCZYT IMPULSU (STIMULUS)
    // Jeśli system zażądał "szoku", dodajemy energię w tym punkcie matrycy
    var stimulus_val = vec4<f32>(0.0);
    if (config.w == 1u) {
        stimulus_val = stimulus_input[idx];
        current_state = current_state + stimulus_val;
    }

    // 3. SYNCHRONIZACJA I PODEJMOWANIE MASY DECYZJI LICZBOWYCH
    // Neuron "patrzy" na swoich sąsiadów w matrycy 2048x2048, aby wytworzyć spójny wzorzec
    // Pobieramy stany z sąsiednich komórek (sąsiedztwo von Neumanna lub Moore'a)
    let up    = read_matrix(x, y - 1u, width, height);
    let down  = read_matrix(x, y + 1u, width, height);
    let left  = read_matrix(x - 4u, y, width, height); // -4u bo przesuwamy się o jeden vec4
    let right = read_matrix(x + 4u, y, width, height);

    // Neuron podejmuje "masę decyzji liczbowych" (średnia ważona otoczenia)
    // To tworzy fale, gradienty i przewidywalne formy geometryczne
    let neighbor_avg = (up + down + left + right) * 0.25;
    
    // Mieszanie stanu lokalnego z otoczeniem (dyfuzja/reakcja)
    let diffusion_rate = 0.15;
    current_state = current_state * (1.0 - diffusion_rate) + neighbor_avg * diffusion_rate;

    // 4. ZMIANA FORMY (ADAPTACJA NEURONU)
    // Jeśli lokalny stan (energia) przekracza próg, neuron musi zmienić swoją "formę" (wagi)
    // aby lepiej przetwarzać ten typ bodźca w przyszłości.
    let energy = dot(current_state, current_state); // Długość wektora stanu
    
    // Zakładamy, że neuron_forms jest mapowany 1:1 z blokami matrycy (np. jeden neuron na 8x8 komórek)
    // Dla uproszczenia w shaderze: używamy global_id.x jako ID neuronu
    let neuron_id = global_id.x + global_id.y * (width / 16u); // Przykładowe mapowanie
    
    if (neuron_id < config.z) {
        var form = neuron_forms[neuron_id];
        
        if (energy > form.adaptation_threshold) {
            // NEURON ZMIENIA FORMĘ: Aktualizuje swoje wagi na podstawie gradientu błędu/stanu
            // To jest deterministyczne uczenie się w locie (Hebbian-like)
            let gradient = current_state - neighbor_avg;
            form.weights = form.weights + gradient * form.sync_rate;
            
            // Normalizacja wag, aby zapobiec przepełnieniu (utrzymanie stabilności)
            let w_len = length(form.weights);
            if (w_len > 1.0) {
                form.weights = form.weights / w_len;
            }
            
            form.stress_level = 0.0; // Reset stresu po adaptacji
        } else {
            form.stress_level = form.stress_level + 0.01;
        }
        
        neuron_forms[neuron_id] = form;
        
        // 5. APLIKACJA NOWEJ FORMY NA DANE (Filtrowanie splotowe)
        // Nowe wagi neuronu modyfikują to, jak "widzi" on swoje otoczenie w następnym cyklu
        current_state = current_state * form.weights;
    }

    // 6. TŁUMIENIE I ZAPIS (Zachowanie stabilności schematów)
    // Lekkie tłumienie, aby stare scenariusze zanikały, a nowe były widoczne
    current_state = current_state * 0.98; 

    // Zapis z powrotem do Gigantycznej Macierzy Scenariuszy (Sektor Hex)
    scenario_matrix[idx] = current_state;
}