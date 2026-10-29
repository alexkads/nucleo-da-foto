// O enquadramento na GPU: para cada pixel que sai, de onde ele vem na foto
// revelada — espelhos, giro de 90°, perspectiva, endireitamento e recorte
// numa matriz projetiva só, a mesma de `Corte::mapa` (transformacao.rs).
//
// É a conta de `aplicar_projetiva`, `endireitar_e_recortar` e `inclinar_inteira`
// feita pela GPU, com o filtro bilinear do amostrador no lugar de `amostrar`.
// Quem chama é `Motor::revelar_para_tela`; o arquivo que sai continua pelo
// caminho da CPU, byte a byte o de sempre.

struct Enquadramento {
    // As três linhas da matriz (a quarta coluna sobra: alinhamento de 16).
    linha_x: vec4<f32>,
    linha_y: vec4<f32>,
    linha_w: vec4<f32>,
    // O tamanho da saída e o da foto revelada, em pixels.
    saida: vec2<f32>,
    origem: vec2<f32>,
    // A cor de quem cai fora da foto, quando `modo_fora` é 1.
    fora: vec4<f32>,
    // 0: gruda na borda (o `clamp` de `amostrar`); 1: pinta `fora`.
    modo_fora: u32,
    // 1: escreve B, G, R, A — a ordem que a janela desenha, sem troca na CPU.
    bgra: u32,
    // A folga na borda, em meios pixels: 1 nos caminhos de `amostrar` e da
    // projetiva, 0 na foto inteira inclinada (`inclinar_inteira`).
    folga: f32,
    _reservado: u32,
};

@group(0) @binding(0) var revelada: texture_2d<f32>;
@group(0) @binding(1) var amostrador: sampler;
@group(0) @binding(2) var<uniform> e: Enquadramento;
@group(0) @binding(3) var saida: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (f32(id.x) >= e.saida.x || f32(id.y) >= e.saida.y) {
        return;
    }
    // O centro do pixel que sai, em fração da saída — como `linha_projetiva`.
    let st = (vec2<f32>(id.xy) + vec2<f32>(0.5, 0.5)) / e.saida;
    let z = e.linha_w.x * st.x + e.linha_w.y * st.y + e.linha_w.z;
    var cor = e.fora;
    if (z > 1e-12) {
        let u = (e.linha_x.x * st.x + e.linha_x.y * st.y + e.linha_x.z) / z;
        let w = (e.linha_y.x * st.x + e.linha_y.y * st.y + e.linha_y.z) / z;
        // Meio pixel de folga na borda gruda na borda, como na CPU: senão a
        // última coluna do recorte justo sairia com uma franja.
        let folga = vec2<f32>(0.5 * e.folga, 0.5 * e.folga) / e.origem;
        let dentro = u >= -folga.x && u <= 1.0 + folga.x && w >= -folga.y && w <= 1.0 + folga.y;
        if (dentro || e.modo_fora == 0u) {
            // Em fração da foto, o amostrador lê o pixel `u·largura − 0,5`:
            // exatamente o ponto que `amostrar_bruto` interpola.
            cor = textureSampleLevel(revelada, amostrador, vec2<f32>(u, w), 0.0);
        }
    }
    if (e.bgra == 1u) {
        cor = cor.bgra;
    }
    textureStore(saida, vec2<i32>(id.xy), cor);
}
