// neuro-bin/shaiders/visual_memory.wgsl

// Forma (wagi) pojedynczego neuronu — 32 bajty, zgodne z neuro_lib::NeuronForm.
struct NeuronForm {
    weights: vec4<f32>,
    adaptation_threshold: f32,
    sync_rate: f32,
    stress_level: f32,
    _pad: f32,
};

// --- SEKTORY HEX (Bufory wgpu) ---

// Sektor 0: Obraz referencyjny (PNG) w formacie RGBA, 256 × 256 komórek.
@group(0) @binding(0) var<storage, read> reference_image: array<vec4<f32>>;

// Sektor 1: To, co model "widzi" (wyobraźnia) — read_write, tu uczy się.
@group(0) @binding(1) var<storage, read_write> visual_output: array<vec4<f32>>;

// Sektor 2: Formy neuronów — tu loss modyfikuje wagi (gradient descent).
@group(0) @binding(2) var<storage, read_write> neuron_forms: array<NeuronForm>;

// Sektor 3: Konfiguracja: x = szerokość, y = wysokość, z = neurony, w = krok.
@group(0) @binding(3) var<uniform> config: vec4<u32>;

// === GŁÓWNY KERNEL: PĘTLA UCZENIA WIZUALNEGO ===

@compute @workgroup_size(16, 16)
fn visual_pass(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let width = config.x;
    let height = config.y;

    if (global_id.x >= width || global_id.y >= height) { return; }

    let idx = global_id.y * width + global_id.x;

    // 1. Pobierz piksel z "wyobraźni" modelu (rzutowany z neuronów)
    var my_pixel = visual_output[idx];

    // 2. Pobierz piksel z obrazu referencyjnego
    let target_pixel = reference_image[idx];

    // 3. Oblicz różnicę (Loss) — MSE między wyobraźnią a celem
    let diff = my_pixel - target_pixel;
    let loss = dot(diff, diff);

    // 4. KLUCZOWE: użyj Loss do modyfikacji wag neuronów!
    // Duży loss = neurony muszą mocniej zmienić wagi, by wygenerować lepszy wzorzec.
    // Siatka neuronów 16 × 16: każdy neuron odpowiada jednemu blokowi 16 × 16 pikseli.
    let block = vec2<u32>(global_id.x / 16u, global_id.y / 16u);
    let neuron_id = block.y * (width / 16u) + block.x;
    let neuron_count = config.z;

    if (neuron_id < neuron_count) {
        var form = neuron_forms[neuron_id];

        // Gradient descent: wagi = wagi - learning_rate * gradient(loss)
        // Sync_rate steruje siłą korekty — każdy neuron uczy się w swoim tempie.
        let correction = diff * form.sync_rate * 0.05;
        form.weights = form.weights - correction;

        // Adaptacja progu: duży loss podnosi próg (neuron czuje presję),
        // mały loss go obniża (neuron jest zadowolony).
        form.adaptation_threshold = form.adaptation_threshold + loss * 0.01 - 0.002;

        // Stres rośnie, gdy adaptacja nie pomaga.
        form.stress_level = form.stress_level + loss * 0.005 - 0.001;

        neuron_forms[neuron_id] = form;
    }

    // 5. Aktualizacja wyobraźni: zbieganie do celu
    let rate = 0.2;
    visual_output[idx] = my_pixel - diff * rate;

    // 6. Impuls strukturalny: piksele zielone (przyciski kalkulatora)
    //    wzmacniamy, żeby model trzymał kontury, a nie tylko średni kolor.
    if (target_pixel.g > target_pixel.r + 0.1 && target_pixel.g > target_pixel.b + 0.1) {
        visual_output[idx] = visual_output[idx] + vec4<f32>(0.0, 0.02, 0.0, 0.0);
    }
}
