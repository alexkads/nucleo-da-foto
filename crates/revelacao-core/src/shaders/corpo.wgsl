// O corpo da revelação: os 53 ajustes, num pixel.
//
// 🔑 **Este arquivo não tem ponto de entrada.** Ele declara o `struct Params`,
// as texturas e a função `revelar_pixel(coord)`, e é concatenado a uma
// "entrada" em tempo de compilação do Rust (`motor.rs`):
//
//   - `entrada_compute.wgsl`   — `@compute`, escreve numa storage texture.
//     É o desktop (Metal, Vulkan, DX12).
//   - `entrada_fragmento.wgsl` — `@vertex` + `@fragment`, escreve no alvo
//     do render pass. É o navegador: o WebGL2 não tem compute nem storage
//     texture, e o WebGPU roda os dois.
//
// A matemática mora aqui, uma vez só, para que a tela do desktop, o arquivo
// exportado e a revelação no site atravessem o **mesmo** código.

// 🚨 A ordem aqui é a do `Ajustes` do Rust, campo a campo.
//
// O `uniform` viaja como bytes crus e casa por **posição**, não por nome. Este
// bloco declarava 28 campos para os 46 que a CPU manda: a partir da posição 23
// o shader lia o campo do vizinho (o matiz do vermelho virava redução de ruído)
// e da 28 em diante não lia nada — os 4 controles de Detalhe e os 3 de Lente
// não chegavam. Nada falhava: o buffer é maior que o mínimo do binding, então o
// wgpu aceita e ignora a sobra, e a duplicata de `nr_luminance` o naga aceitava.
//
// Quem confere os dois lados é `o_wgsl_declara_os_mesmos_46_campos_na_mesma_ordem`,
// que lê este arquivo e compara com a struct.
struct Params {
    exposure: f32,
    contrast: f32,
    temperature: f32,
    tint: f32,
    highlights: f32,
    shadows: f32,
    whites: f32,
    blacks: f32,
    clarity: f32,
    vibrance: f32,
    saturation: f32,
    // Tone curve parametric zones
    tone_curve_shadows: f32,
    tone_curve_darks: f32,
    tone_curve_lights: f32,
    tone_curve_highlights: f32,
    // HSL color channel saturations (-100 to +100)
    hsl_red_sat: f32,
    hsl_orange_sat: f32,
    hsl_yellow_sat: f32,
    hsl_green_sat: f32,
    hsl_aqua_sat: f32,
    hsl_blue_sat: f32,
    hsl_purple_sat: f32,
    hsl_magenta_sat: f32,
    // HSL hue rotation, in degrees (-180 to +180): hue is a circle
    hsl_red_hue: f32,
    hsl_orange_hue: f32,
    hsl_yellow_hue: f32,
    hsl_green_hue: f32,
    hsl_aqua_hue: f32,
    hsl_blue_hue: f32,
    hsl_purple_hue: f32,
    hsl_magenta_hue: f32,
    // HSL luminance (-100 to +100)
    hsl_red_lum: f32,
    hsl_orange_lum: f32,
    hsl_yellow_lum: f32,
    hsl_green_lum: f32,
    hsl_aqua_lum: f32,
    hsl_blue_lum: f32,
    hsl_purple_lum: f32,
    hsl_magenta_lum: f32,
    // Lens corrections: distortion resamples, vignetting shades by position
    lens_distortion: f32,
    lens_vignette_amount: f32,
    lens_vignette_midpoint: f32,
    // Detail: noise reduction and sharpening
    nr_luminance: f32,
    nr_color: f32,
    sharpen_amount: f32,
    sharpen_radius: f32,
    // Tonalização (split toning): a cor das sombras e a das altas luzes,
    // separadas, com um balanço que decide onde uma acaba e a outra começa. É o
    // virador do quarto escuro — e o único caminho para sépia, porque
    // temperatura e matiz agem ANTES da saturação e não recolorem um cinza.
    split_shadow_hue: f32,
    split_shadow_sat: f32,
    split_highlight_hue: f32,
    split_highlight_sat: f32,
    split_balance: f32,
    // Grão de filme: quanto, e de que tamanho.
    grain_amount: f32,
    grain_size: f32,
    // Calibração de câmera: move os PRIMÁRIOS, e não uma faixa de matiz como o
    // HSL. É a base da maioria dos presets de filme, e faltava inteira.
    calib_red_hue: f32,
    calib_red_sat: f32,
    calib_green_hue: f32,
    calib_green_sat: f32,
    calib_blue_hue: f32,
    calib_blue_sat: f32,
    calib_shadow_tint: f32,
    // Color Grading: os tons médios e o global que o split toning não tem, e a
    // mistura que decide a largura da transição entre as faixas.
    split_midtone_hue: f32,
    split_midtone_sat: f32,
    split_global_hue: f32,
    split_global_sat: f32,
    split_blending: f32,
    // Mixer de preto e branco: a luminância de cada faixa de matiz.
    bw_ativo: f32,
    bw_red: f32,
    bw_orange: f32,
    bw_yellow: f32,
    bw_green: f32,
    bw_aqua: f32,
    bw_blue: f32,
    bw_purple: f32,
    bw_magenta: f32,
    // Curva por ponto: nove alturas em x fixo, por canal. Ver `ajustes.rs`
    // para por que são nove alturas e não pontos livres.
    curva_m0: f32,
    curva_m1: f32,
    curva_m2: f32,
    curva_m3: f32,
    curva_m4: f32,
    curva_m5: f32,
    curva_m6: f32,
    curva_m7: f32,
    curva_m8: f32,
    curva_r0: f32,
    curva_r1: f32,
    curva_r2: f32,
    curva_r3: f32,
    curva_r4: f32,
    curva_r5: f32,
    curva_r6: f32,
    curva_r7: f32,
    curva_r8: f32,
    curva_g0: f32,
    curva_g1: f32,
    curva_g2: f32,
    curva_g3: f32,
    curva_g4: f32,
    curva_g5: f32,
    curva_g6: f32,
    curva_g7: f32,
    curva_g8: f32,
    curva_b0: f32,
    curva_b1: f32,
    curva_b2: f32,
    curva_b3: f32,
    curva_b4: f32,
    curva_b5: f32,
    curva_b6: f32,
    curva_b7: f32,
    curva_b8: f32,
    // Os controles do Lightroom que faltavam (2026-09-30). Ver `ajustes.rs`.
    texture: f32,
    dehaze: f32,
    tone_curve_split_shadows: f32,
    tone_curve_split_midtones: f32,
    tone_curve_split_highlights: f32,
    split_shadow_lum: f32,
    split_midtone_lum: f32,
    split_highlight_lum: f32,
    split_global_lum: f32,
    sharpen_detail: f32,
    sharpen_masking: f32,
    nr_luminance_detail: f32,
    nr_luminance_contrast: f32,
    nr_color_detail: f32,
    nr_color_smoothness: f32,
    pcv_style: f32,
    pcv_amount: f32,
    pcv_midpoint: f32,
    pcv_roundness: f32,
    pcv_feather: f32,
    pcv_highlights: f32,
    grain_roughness: f32,
    // 0: a conta de antes; 1: as tabelas medidas no Lightroom (`lightroom.rs`).
    processo: f32,
    // A vinheta do darktable (`vinheta_do_darktable`), os campos do
    // `dt_iop_vignette_params_t`: liga/desliga, início e raio do decaimento
    // em %, brilho e saturação −1..1, centro −1..1, proporção automática 0/1,
    // largura/altura 0..2, forma 0..5 e matização 0/1/2.
    darktable_vignette_ativo: f32,
    darktable_vignette_scale: f32,
    darktable_vignette_falloff_scale: f32,
    darktable_vignette_brightness: f32,
    darktable_vignette_saturation: f32,
    darktable_vignette_center_x: f32,
    darktable_vignette_center_y: f32,
    darktable_vignette_autoratio: f32,
    darktable_vignette_whratio: f32,
    darktable_vignette_shape: f32,
    darktable_vignette_dithering: f32,
    // 🔑 Sem enchimento desde 2/out/2026: o WebGL2 (`DownlevelFlags::BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED`
    // ausente) exige que o tipo do uniform tenha tamanho múltiplo de 16, e 171
    // `f32` davam 684, os 194 davam 776, os 133 (sem o estágio darktable)
    // davam 532 e pediam três campos `_enchimento`. Os 144 de hoje (com a
    // vinheta do darktable de volta) dão 576, múltiplo de 16. Se o número
    // mudar, o enchimento volta DEPOIS do último campo, com nome começando
    // por `_` — o teste que compara os nomes com o `Ajustes` o ignora.
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(2) var<uniform> params: Params;

// O quadro do arquivo que sai, visto do pixel revelado — `Quadro::para_gpu`.
//
// 🔑 **É o que deixa as vinhetas no recorte.** O shader revela a foto inteira e
// o enquadramento vem depois (`transformacao::aplicar`); as duas vinhetas — a
// de lente e a pós-corte — levam o pixel a este quadro antes de medir
// centro, proporção e escala. Sem enquadramento, é a identidade com as
// dimensões da foto, e as duas saem bit a bit as de antes.
struct QuadroDeSaida {
    // m0, m1, m2, largura do arquivo
    linha_x: vec4<f32>,
    // m3, m4, m5, altura do arquivo
    linha_y: vec4<f32>,
    // w0, w1, w2, 0 — a linha projetiva da perspectiva guiada; `(0, 0, 1)` sem
    // ela, e a divisão por 1 é exata.
    linha_w: vec4<f32>,
}

@group(0) @binding(6) var<uniform> quadro_de_saida: QuadroDeSaida;

// As máscaras locais (`mascaras.rs`): uma camada `R8Unorm` por máscara, do
// tamanho desta imagem, e os ajustes de cada uma.
//
// 🔑 `quantidade.x` zero pula o bloco inteiro — a foto sem máscara sai bit a bit
// a de antes, e a textura (1×1) nem é lida.
struct ParamsLocais {
    quantidade: vec4<f32>,
    // Por camada: exposição em EV, invertida (0/1), e dois lugares livres.
    camadas: array<vec4<f32>, 8>,
}

@group(0) @binding(7) var mascaras: texture_2d_array<f32>;
@group(0) @binding(8) var<uniform> locais: ParamsLocais;

// A guia (`guia.rs`): a foto reduzida, com a luminância desfocada larga (`r`,
// Claridade) e média (`g`, Textura) e o canal escuro (`b`, Remover névoa).
// Calculada na CPU uma vez por foto, e só quando um dos três está em uso.
struct DadosDaGuia {
    // x: a luz do céu do Remover névoa (0–1); y: 1 com guia, 0 sem.
    dados: vec4<f32>,
}

@group(0) @binding(9) var guia: texture_2d<f32>;
@group(0) @binding(10) var<uniform> dados_da_guia: DadosDaGuia;

// As tabelas medidas no Lightroom (`lightroom.rs`): `R32Float`, 256 × 269, uma
// curva por linha. Só o processo 1 as lê.
@group(0) @binding(11) var tabelas_lr: texture_2d<f32>;

// O desenho das linhas — a mesma conta de `lightroom.rs`, presa por
// `o_wgsl_usa_o_mesmo_desenho_das_tabelas`.
const LR_LINHA_EXPOSICAO: i32 = 0;
const LR_LINHA_CONTRASTE: i32 = 41;
const LR_LINHA_REALCES: i32 = 62;
const LR_LINHA_SOMBRAS: i32 = 83;
const LR_LINHA_BRANCOS: i32 = 104;
const LR_LINHA_PRETOS: i32 = 125;
const LR_LINHA_VINHETA: i32 = 146;
const LR_LINHA_SOBREPOSICAO: i32 = 167;
const LR_LINHA_MASCARA: i32 = 188;
const LR_DISTANCIA_MAXIMA: f32 = 1.45;
const LR_LINHA_TEMPERATURA: i32 = 269;
const LR_LINHA_MATIZ: i32 = 332;
const LR_LINHA_SATURACAO: i32 = 395;
const LR_LINHA_VIBRACAO: i32 = 416;
const LR_LINHA_SATURACAO_L: i32 = 437;
const LR_LINHA_VIBRACAO_L: i32 = 458;
const LR_LINHA_VIRAGEM: i32 = 479;
const LR_LINHA_VIRAGEM_SATURACAO: i32 = 527;
const LR_LINHA_VIRAGEM_EQUILIBRIO: i32 = 529;

/// Um ponto da tabela de croma: a linha, o matiz (0–23, dá a volta) e a
/// saturação (0–5).
fn lr_croma_em(linha: i32, matiz: i32, sat: i32) -> f32 {
    return textureLoad(tabelas_lr, vec2<i32>(((matiz % 24) + 24) % 24 * 6 + sat, linha), 0).r;
}

/// O fator de croma de uma cor (matiz em graus, saturação HSV 0–1) para um
/// valor de −100 a +100 de Saturação ou Vibração (`primeira` é a linha do −100):
/// interpolado entre os valores, entre os matizes de 15° e entre as seis
/// saturações medidas na carta (0,1 0,25 0,4 0,55 0,75 1).
fn lr_fator_de_croma(primeira: i32, valor: f32, matiz: f32, sat: f32) -> f32 {
    let p = clamp((valor + 100.0) / 10.0, 0.0, 20.0);
    let iv = min(i32(floor(p)), 19);
    let tv = p - f32(iv);
    let h = matiz / 15.0;
    let ih = i32(floor(h));
    let th = h - f32(ih);
    // A saturação, nos seis pontos da carta (abaixo de 0,1, o de 0,1).
    let s = clamp(sat, 0.1, 1.0);
    var is_ = 0;
    var ts = 0.0;
    if (s < 0.25) {
        is_ = 0; ts = (s - 0.1) / 0.15;
    } else if (s < 0.4) {
        is_ = 1; ts = (s - 0.25) / 0.15;
    } else if (s < 0.55) {
        is_ = 2; ts = (s - 0.4) / 0.15;
    } else if (s < 0.75) {
        is_ = 3; ts = (s - 0.55) / 0.2;
    } else {
        is_ = 4; ts = (s - 0.75) / 0.25;
    }
    let l0 = primeira + iv;
    let l1 = l0 + 1;
    let f0 = mix(
        mix(lr_croma_em(l0, ih, is_), lr_croma_em(l0, ih, is_ + 1), ts),
        mix(lr_croma_em(l0, ih + 1, is_), lr_croma_em(l0, ih + 1, is_ + 1), ts),
        th,
    );
    let f1 = mix(
        mix(lr_croma_em(l1, ih, is_), lr_croma_em(l1, ih, is_ + 1), ts),
        mix(lr_croma_em(l1, ih + 1, is_), lr_croma_em(l1, ih + 1, is_ + 1), ts),
        th,
    );
    return mix(f0, f1, tv);
}

/// O balanço de um valor de −100 a +100 (21 passos, três curvas cada: a
/// linha `primeira + passo·3 + canal`), canal a canal.
fn lr_balanco(primeira: i32, valor: f32, cor: vec3<f32>) -> vec3<f32> {
    let p = clamp((valor + 100.0) / 10.0, 0.0, 20.0);
    let i = min(i32(floor(p)), 19);
    let t = p - f32(i);
    let a = primeira + i * 3;
    let b = a + 3;
    return vec3<f32>(
        mix(lr_curva(a, cor.r), lr_curva(b, cor.r), t),
        mix(lr_curva(a + 1, cor.g), lr_curva(b + 1, cor.g), t),
        mix(lr_curva(a + 2, cor.b), lr_curva(b + 2, cor.b), t),
    );
}
// A fração da curva da rampa que Realces e Sombras aplicam numa foto — ver o
// bloco do processo 1 no tom.
const LR_FORCA_LOCAL: f32 = 0.5;

/// A linha `linha` da tabela em `x` (0–255), linear entre as colunas.
fn lr_curva(linha: i32, x: f32) -> f32 {
    let p = clamp(x, 0.0, 255.0);
    let i = min(i32(floor(p)), 254);
    let t = p - f32(i);
    let a = textureLoad(tabelas_lr, vec2<i32>(i, linha), 0).r;
    let b = textureLoad(tabelas_lr, vec2<i32>(i + 1, linha), 0).r;
    return mix(a, b, t);
}

/// Um slider medido de passo em passo: `posicao` em linhas a partir de
/// `primeira` (fracionária), linear entre as duas vizinhas.
fn lr_slider(primeira: i32, linhas: i32, posicao: f32, x: f32) -> f32 {
    let p = clamp(posicao, 0.0, f32(linhas - 1));
    let i = min(i32(floor(p)), linhas - 2);
    let t = p - f32(i);
    return mix(lr_curva(primeira + i, x), lr_curva(primeira + i + 1, x), t);
}

/// A forma da vinheta no arredondamento negativo (`r` de 0 a 100, o módulo):
/// o expoente da superelipse e a compressão da faixa contra a borda, ajustados
/// aos perfis da régua (erro abaixo de 1,2 nível em −50 … −100). Uma cadeia de
/// `if`, e não um array constante: o FXC do DX12 engasga com array constante.
fn lr_forma_negativa(r: f32) -> vec2<f32> {
    if (r <= 25.0) {
        return vec2<f32>(2.0, 1.0);
    } else if (r <= 50.0) {
        return mix(vec2<f32>(2.0, 1.0), vec2<f32>(2.5, 1.3), (r - 25.0) / 25.0);
    } else if (r <= 61.0) {
        return mix(vec2<f32>(2.5, 1.3), vec2<f32>(3.0, 1.6), (r - 50.0) / 11.0);
    } else if (r <= 80.0) {
        return mix(vec2<f32>(3.0, 1.6), vec2<f32>(5.5, 2.65), (r - 61.0) / 19.0);
    } else if (r <= 83.0) {
        return mix(vec2<f32>(5.5, 2.65), vec2<f32>(6.0, 2.95), (r - 80.0) / 3.0);
    }
    return mix(vec2<f32>(6.0, 2.95), vec2<f32>(16.5, 8.05), (min(r, 100.0) - 83.0) / 17.0);
}

/// Os cinco sliders de −100 a +100 (21 linhas, de 10 em 10), canal a canal.
fn lr_cem(primeira: i32, valor: f32, cor: vec3<f32>) -> vec3<f32> {
    let pos = (valor + 100.0) / 10.0;
    return vec3<f32>(
        lr_slider(primeira, 21, pos, cor.r),
        lr_slider(primeira, 21, pos, cor.g),
        lr_slider(primeira, 21, pos, cor.b),
    );
}

/// A guia na posição `pos` da foto revelada (em pixels dela), bilinear à mão —
/// a textura é `Rgba32Float`, que o WebGL2 não filtra sem extensão.
fn ler_guia(pos: vec2<f32>, dims: vec2<u32>) -> vec4<f32> {
    let gd = textureDimensions(guia);
    let escala = vec2<f32>(f32(gd.x), f32(gd.y)) / vec2<f32>(f32(dims.x), f32(dims.y));
    let ultimo = vec2<f32>(f32(gd.x) - 1.0, f32(gd.y) - 1.0);
    let p = clamp((pos + 0.5) * escala - 0.5, vec2<f32>(0.0, 0.0), ultimo);
    let base = floor(p);
    let f = p - base;
    let x0 = i32(base.x);
    let y0 = i32(base.y);
    let x1 = min(x0 + 1, i32(gd.x) - 1);
    let y1 = min(y0 + 1, i32(gd.y) - 1);
    let a = textureLoad(guia, vec2<i32>(x0, y0), 0);
    let b = textureLoad(guia, vec2<i32>(x1, y0), 0);
    let c = textureLoad(guia, vec2<i32>(x0, y1), 0);
    let d = textureLoad(guia, vec2<i32>(x1, y1), 0);
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

/// Leva a cor (0–255) da luminância `l` para `n` (0–1) sem mudar o matiz.
///
/// É a conta do tom por região: pela razão, que escala os três canais juntos,
/// e somada abaixo de ~5 % de luz, onde a razão explode.
fn com_luminancia(cor: vec3<f32>, l: f32, n: f32) -> vec3<f32> {
    let delta = (n - l) * 255.0;
    let escala = n / max(l, 0.0001);
    let mistura = smoothstep(0.0, 0.05, l);
    return mix(cor + vec3<f32>(delta), cor * escala, mistura);
}

/// A Divisão de tons e o Color Grading (as rodas e a luminância de cada faixa),
/// na ordem do processo 0: tudo antes das vinhetas, pesado pela luz de entrada.
fn viragem(entrada: vec3<f32>) -> vec3<f32> {
    let cor = clamp(entrada, vec3<f32>(0.0), vec3<f32>(255.0));
    let l = ((cor.r + cor.g + cor.b) / 3.0) / 255.0;
    return cor_da_viragem(luminancia_da_viragem(cor, l), l);
}

/// O peso de cada faixa da viragem num nível de luz `l` (0–1): sombras,
/// tons médios e realces.
fn faixas_da_viragem(l: f32) -> vec3<f32> {
    // O balanço desloca o ponto em que uma ponta cede para a outra:
    // positivo dá mais foto às altas luzes, negativo às sombras.
    let balanco = clamp(params.split_balance * 0.01, -1.0, 1.0);
    let centro = 0.5 - balanco * 0.4;

    // 🔑 **A mistura vira a LARGURA da transição**, que é o que o
    // `Blending` da Adobe faz: em 0 as faixas têm borda quase dura, em 100
    // elas se sobrepõem quase inteiras. No neutro (50) a meia-largura dá
    // **0,35** — exatamente o valor fixo que estava aqui antes de
    // 2026-09-12, e é por isso que toda revelação já gravada continua
    // saindo idêntica.
    let mistura = clamp(params.split_blending * 0.01, 0.0, 1.0);
    let meia = 0.10 + mistura * 0.50;
    let peso_alta = smoothstep(centro - meia, centro + meia, l);
    let peso_baixa = 1.0 - peso_alta;
    // O meio é o que as duas pontas não reivindicam — um sino em torno do
    // centro, que vale 1 onde as duas empatam e 0 nas extremidades.
    let peso_meio = 1.0 - abs(peso_alta - peso_baixa);
    return vec3<f32>(peso_baixa, peso_meio, peso_alta);
}

/// A Luminância de cada faixa (2026-09-30): clareia na direção do branco e
/// escurece na do preto, proporcionalmente, e nunca estoura.
///
/// 🔑 **No processo 1 ela fica antes das vinhetas; só a cor vai para depois**
/// (2/out/2026). A régua `vinheta-viragem` provou que a cor das sombras e dos
/// realces fica dentro da vinheta — e nada sobre a Luminância. Com ela junto,
/// depois da vinheta, a borda que a vinheta +100 clareia escurecia de novo
/// com a Luminância negativa dos realces, e o RF Bem Velhão (vinheta +100,
/// realces −38; fora das LRs desde 2/out) foi de ΔE 29 a 59 (rodada `tudo-5`);
/// na `tudo-4a`, com ela antes, eram 29.
fn luminancia_da_viragem(entrada: vec3<f32>, l: f32) -> vec3<f32> {
    let faixas = faixas_da_viragem(l);
    let dl = (params.split_shadow_lum * faixas.x * faixas.x
        + params.split_midtone_lum * faixas.y * faixas.y
        + params.split_highlight_lum * faixas.z * faixas.z
        + params.split_global_lum) * 0.01;
    if (dl == 0.0) {
        return entrada;
    }
    var n = l;
    if (dl > 0.0) {
        n = l + min(dl, 1.0) * (1.0 - l);
    } else {
        n = l * (1.0 + max(dl, -1.0));
    }
    return clamp(com_luminancia(entrada, l, n), vec3<f32>(0.0), vec3<f32>(255.0));
}

/// A cor das rodas, pesada pelas faixas do nível `l` (0–1).
///
/// 🔑 **No processo 1 ela vem depois das vinhetas** (régua `vinheta-viragem`,
/// 2/out/2026): numa foto P&B com viragem sépia, a borda que a vinheta branca
/// do Lightroom clareia continua sépia — a vinheta é uma camada dentro do
/// efeito, e não por cima dele, como no darktable. Com a viragem antes, a
/// vinheta levava a borda ao branco neutro (−11 a −16 de "quente" nos
/// realces). No processo 0 a ordem é a de antes, e foto revelada não muda.
fn cor_da_viragem(cor: vec3<f32>, l: f32) -> vec3<f32> {
    let faixas = faixas_da_viragem(l);
    let peso_baixa = faixas.x;
    let peso_meio = faixas.y;
    let peso_alta = faixas.z;

    // 🔑 **As rodas são as do Lightroom desde 2026-09-30**: cada uma soma
    // um deslocamento de cor em CIELab, de croma `0,4 × saturação` e
    // matiz `matiz + 28°`, pesado pela faixa — e a luminosidade L* fica.
    // Os dois números saíram de um DNG revelado no Lightroom (o
    // `_DSC0010-2` do Estúdio Canela): com eles, a cor das sombras, do
    // meio-tom e das altas luzes da prévia que o Lightroom gravou bate em
    // ~2 unidades de a*/b*. Até ali cada roda misturava a cor pura do
    // matiz HSV, e o mesmo número do Lightroom dava outra cor — o amarelo
    // 59 virava verde nas altas luzes e o vermelho 14 virava roxo.
    let lab = para_lab(cor);
    var desvio = roda_do_lightroom(params.split_midtone_hue, params.split_midtone_sat) * peso_meio
        + roda_do_lightroom(params.split_global_hue, params.split_global_sat);
    if (params.processo >= 0.5) {
        // 🔑 **No processo 1, sombras e realces são os do Lightroom medidos**
        // (régua `viragem`, 2/out/2026): as duas regiões se SOMAM, e o
        // Equilíbrio desloca e amplia cada uma — com as duas em 45°/50, o −100
        // dobra a cor dos meios-tons. A roda de antes repartia uma cor só entre
        // as duas, e pintava os realces quase 2× mais que o Lightroom.
        let nivel = l * 255.0;
        desvio += lr_viragem(0, params.split_shadow_hue, params.split_shadow_sat, nivel)
            + lr_viragem(1, params.split_highlight_hue, params.split_highlight_sat, nivel);
    } else {
        desvio += roda_do_lightroom(params.split_shadow_hue, params.split_shadow_sat) * peso_baixa
            + roda_do_lightroom(params.split_highlight_hue, params.split_highlight_sat) * peso_alta;
    }
    // O preto puro e o branco puro ficam como estão: não há cor que caiba
    // em L* 0 ou 100, e a conta voltaria com o canal estourado.
    desvio *= smoothstep(0.0, 8.0, lab.x) * (1.0 - smoothstep(92.0, 100.0, lab.x));
    return de_lab(vec3<f32>(lab.x, lab.y + desvio.x, lab.z + desvio.y));
}

/// O Δa*, Δb* que a Divisão de tons do Lightroom põe num nível de entrada
/// (0–255), numa região (0 sombras, 1 realces) — as tabelas da régua
/// `viragem` (`lightroom.rs`): o matiz entre os 12 medidos, a saturação pelo
/// fator sobre a de 50, e o Equilíbrio pelo ganho de cada nível.
fn lr_viragem(regiao: i32, matiz: f32, saturacao: f32, nivel: f32) -> vec2<f32> {
    if (saturacao <= 0.0) {
        return vec2<f32>(0.0);
    }
    let h = (((matiz % 360.0) + 360.0) % 360.0) / 30.0;
    let k0 = i32(floor(h)) % 12;
    let k1 = (k0 + 1) % 12;
    let t = h - floor(h);
    let base = LR_LINHA_VIRAGEM + regiao * 24;
    let d0 = vec2<f32>(lr_curva(base + k0 * 2, nivel), lr_curva(base + k0 * 2 + 1, nivel));
    let d1 = vec2<f32>(lr_curva(base + k1 * 2, nivel), lr_curva(base + k1 * 2 + 1, nivel));
    let fator = lr_curva(LR_LINHA_VIRAGEM_SATURACAO + regiao, clamp(saturacao, 0.0, 100.0));
    let p = clamp((params.split_balance + 100.0) / 25.0, 0.0, 8.0);
    let i = min(i32(floor(p)), 7);
    let linha_e = LR_LINHA_VIRAGEM_EQUILIBRIO + regiao * 9 + i;
    let ganho = mix(lr_curva(linha_e, nivel), lr_curva(linha_e + 1, nivel), p - f32(i));
    return mix(d0, d1, t) * fator * ganho;
}



/// O detalhe `d` (pixel − média) contido: a borda forte conta cada vez menos,
/// que é o que tira o halo de um contraste local feito com desfoque comum.
fn detalhe_contido(d: f32, dureza: f32) -> f32 {
    return d / (1.0 + abs(d) * dureza);
}

/// Uma zona da curva paramétrica: o triângulo centrado na zona `[a, b]`, com a
/// largura da própria zona para cada lado. Com as divisões no neutro (25/50/75)
/// são as quatro zonas fixas de antes, número por número.
fn zona_da_curva(cor: vec3<f32>, l: f32, a: f32, b: f32, valor: f32) -> vec3<f32> {
    let largura = b - a;
    let dist = abs(l - (a + b) * 0.5);
    if (valor == 0.0 || dist >= largura) {
        return cor;
    }
    let ajuste = valor * 0.01 * (1.0 - dist / largura);
    return cor + cor * ajuste;
}

/// sRGB (0–255) → linear, estendido: valores fora de 0–255 continuam a curva,
/// porque o pipeline deixa passar sobra até o fim (e a exposição global cria).
fn srgb_para_linear(v255: f32) -> f32 {
    let x = v255 / 255.0;
    if (x <= 0.04045) {
        return x / 12.92;
    }
    return pow((x + 0.055) / 1.055, 2.4);
}

/// Linear → sRGB (0–255), o inverso exato de [`srgb_para_linear`].
fn linear_para_srgb(v: f32) -> f32 {
    if (v <= 0.0031308) {
        return v * 12.92 * 255.0;
    }
    return (1.055 * pow(v, 1.0 / 2.4) - 0.055) * 255.0;
}

/// Quantos EV as máscaras dão a este pixel — a soma de cada camada pela
/// cobertura dela (invertida quando a camada pede).
fn exposicao_local(coord: vec2<u32>) -> f32 {
    let quantas = u32(locais.quantidade.x);
    var ev = 0.0;
    for (var i = 0u; i < quantas; i++) {
        var m = textureLoad(mascaras, vec2<i32>(coord), i32(i), 0).r;
        if (locais.camadas[i].y > 0.5) {
            m = 1.0 - m;
        }
        ev += locais.camadas[i].x * m;
    }
    return ev;
}

/// Onde o pixel `coord` da foto revelada cai no arquivo — `Quadro::no_quadro`,
/// na mesma ordem de operações.
fn no_quadro(coord: vec2<u32>) -> vec2<f32> {
    let x = f32(coord.x);
    let y = f32(coord.y);
    let z = quadro_de_saida.linha_w.x * x + quadro_de_saida.linha_w.y * y + quadro_de_saida.linha_w.z;
    return vec2<f32>(
        quadro_de_saida.linha_x.x * x + quadro_de_saida.linha_x.y * y + quadro_de_saida.linha_x.z,
        quadro_de_saida.linha_y.x * x + quadro_de_saida.linha_y.y * y + quadro_de_saida.linha_y.z,
    ) / z;
}

/// O tamanho do arquivo que sai, em pixels.
fn tamanho_do_quadro() -> vec2<f32> {
    return vec2<f32>(quadro_de_saida.linha_x.w, quadro_de_saida.linha_y.w);
}

/// O `encrypt_tea` do darktable (`src/common/tea.h`, 8 voltas): o sorteio da
/// matização. Lá o estado anda pixel a pixel numa linha; aqui cada pixel
/// começa do próprio `(x, y)` — a GPU não tem ordem —, e o gabarito
/// (`vinheta_darktable.rs`) faz igual.
fn dt_tea(x: u32, y: u32) -> u32 {
    var v0 = x;
    var v1 = y;
    var soma = 0u;
    for (var i = 0; i < 8; i++) {
        soma += 0x9e3779b9u;
        v0 += ((v1 << 4u) + 0xa341316cu) ^ (v1 + soma) ^ ((v1 >> 5u) + 0xc8013ea4u);
        v1 += ((v0 << 4u) + 0xad90777du) ^ (v0 + soma) ^ ((v0 >> 5u) + 0x7e95761eu);
    }
    return v0;
}

/// O `tpdf` do darktable: o sorteio em −1..1, triangular.
fn dt_tpdf(sorteio: u32) -> f32 {
    let f = f32(sorteio) / 4294967295.0;
    if (f < 0.5) {
        return sqrt(2.0 * f) - 1.0;
    }
    return 1.0 - sqrt(2.0 * (1.0 - f));
}

/// 🎞️ **A vinheta do darktable** — o `process` de `src/iop/vignette.c`, sempre
/// *unbound*. O gabarito é `vinheta_darktable.rs`, a conta que foi medida
/// contra o `darktable-cli` (máximo de 1 nível).
///
/// 🔑 **Brilho positivo SOMA luz linear, negativo multiplica**. É a borda
/// branca do `RecordarFotos P&B`, que as duas vinhetas do Lightroom não fazem.
/// A forma é a superelipse de expoente `2/forma` em torno do centro, com o
/// início do decaimento onde o efeito começa e o raio até onde ele chega
/// inteiro, tudo em fração do meio-quadro. Com a matização ligada, o peso
/// da faixa de transição vira o cosseno do darktable e ganha o ruído de
/// 1/256 (8 bits) ou 1/65536 (16 bits).
///
/// Medida no quadro do arquivo que sai (`no_quadro`): no darktable o `vignette`
/// vem depois do `crop`. A conta é no sRGB linear, de onde o pixel volta em
/// 0–255.
fn vinheta_do_darktable(cor255: vec3<f32>, coord: vec2<u32>) -> vec3<f32> {
    let tamanho = tamanho_do_quadro();
    let w = max(tamanho.x, 1.0);
    let h = max(tamanho.y, 1.0);
    let centro = vec2<f32>(
        w * 0.5 + params.darktable_vignette_center_x * w / 2.0,
        h * 0.5 + params.darktable_vignette_center_y * h / 2.0,
    );
    var xscale = 2.0 / w;
    var yscale = 2.0 / h;
    if (params.darktable_vignette_autoratio < 0.5) {
        let base = 2.0 / max(w, h);
        let proporcao = clamp(params.darktable_vignette_whratio, 0.001, 1.999);
        if (proporcao <= 1.0) {
            xscale = base / proporcao;
            yscale = base;
        } else {
            xscale = base;
            yscale = base / (2.0 - proporcao);
        }
    }
    let dscale = params.darktable_vignette_scale / 100.0;
    // O decaimento mínimo do darktable, contra o serrilhado.
    let min_falloff = 100.0 / min(w, h);
    let fscale = max(params.darktable_vignette_falloff_scale, min_falloff) / 100.0;
    let forma = max(params.darktable_vignette_shape, 0.001);
    let exp1 = 2.0 / forma;
    let exp2 = forma / 2.0;
    let aqui = no_quadro(coord);
    // ⚠️ O WGSL não define `pow(0, y)`: na linha e na coluna do centro `pv`
    // seria zero. O piso de 1e-12 não muda nível nenhum.
    let pv = max(
        vec2<f32>(
            abs(aqui.x * xscale - centro.x * xscale),
            abs(aqui.y * yscale - centro.y * yscale),
        ),
        vec2<f32>(1e-12),
    );
    let cplen = pow(pow(pv.x, exp1) + pow(pv.y, exp1), exp2);
    var peso = 0.0;
    var ruido = 0.0;
    if (cplen >= dscale) {
        peso = (cplen - dscale) / fscale;
        let matizacao = i32(round(params.darktable_vignette_dithering));
        if (peso >= 1.0) {
            peso = 1.0;
        } else if (peso <= 0.0) {
            peso = 0.0;
        } else if (matizacao == 1 || matizacao == 2) {
            peso = 0.5 - cos(3.14159265 * peso) / 2.0;
            let degrau = select(1.0 / 65536.0, 1.0 / 256.0, matizacao == 1);
            ruido = degrau * dt_tpdf(dt_tea(coord.x, coord.y));
        }
    }
    if (peso <= 0.0) {
        return cor255;
    }
    let entrada = clamp(cor255, vec3<f32>(0.0), vec3<f32>(255.0));
    var col = vec3<f32>(
        srgb_para_linear(entrada.r),
        srgb_para_linear(entrada.g),
        srgb_para_linear(entrada.b),
    );
    let brilho = params.darktable_vignette_brightness;
    if (brilho < 0.0) {
        col = col * (1.0 + peso * brilho) + vec3<f32>(ruido);
    } else {
        col = col + vec3<f32>(peso * brilho + ruido);
    }
    let mv = (col.r + col.g + col.b) / 3.0;
    col = col - (vec3<f32>(mv) - col) * (peso * params.darktable_vignette_saturation);
    col = max(col, vec3<f32>(0.0));
    return vec3<f32>(linear_para_srgb(col.r), linear_para_srgb(col.g), linear_para_srgb(col.b));
}

/// A cor pura de um matiz em graus, em 0.0–1.0 — saturação cheia, meio-tom.
///
/// É o mesmo `c`/`x`/`m` da conversão HSL do fim deste arquivo, com `sat = 1.0`
/// e `lightness = 0.5`: ali `c` vale 1 e `m` vale 0, e sobra só a roda de cor.
fn cor_do_matiz(graus: f32) -> vec3<f32> {
    let h = graus - floor(graus / 360.0) * 360.0;
    let x = 1.0 - abs((h / 60.0) % 2.0 - 1.0);
    if (h < 60.0) { return vec3<f32>(1.0, x, 0.0); }
    if (h < 120.0) { return vec3<f32>(x, 1.0, 0.0); }
    if (h < 180.0) { return vec3<f32>(0.0, 1.0, x); }
    if (h < 240.0) { return vec3<f32>(0.0, x, 1.0); }
    if (h < 300.0) { return vec3<f32>(x, 0.0, 1.0); }
    return vec3<f32>(1.0, 0.0, x);
}

/// A luminância percebida (Rec. 601), na mesma escala 0–255 do corpo.
/// O quanto este matiz "é" o primário de `centro`, com queda larga.
///
/// 🔑 **Larga de propósito, ao contrário das faixas do HSL.** O HSL existe para
/// mexer numa cor sem tocar na vizinha, e por isso suas faixas são estreitas e
/// somem em 15–45°. A calibração faz o oposto: ela move o primário e a foto
/// inteira acompanha. Com queda de 120° os três pesos somam perto de 1 em
/// qualquer matiz, e não sobra buraco entre um primário e o seguinte.
fn peso_do_primario(matiz: f32, centro: f32) -> f32 {
    var d = abs(matiz - centro);
    if (d > 180.0) { d = 360.0 - d; }
    let t = clamp(d / 120.0, 0.0, 1.0);
    let c = cos(t * 1.5707963);
    return c * c;
}

/// O ganho de luminância que o mixer de preto e branco dá a este matiz.
///
/// 🔑 **Os centros e as meias-larguras são os mesmos do HSL**, de propósito: um
/// preset que escurece o azul do céu no mixer e outro que o escurece no HSL têm
/// de pegar o mesmo pixel. Duas réguas para "azul" dariam dois resultados para
/// a mesma palavra.
fn mistura_pb(matiz: f32) -> f32 {
    var ganho = 0.0;
    if (matiz < 15.0 || matiz >= 345.0) {
        var d = matiz;
        if (d >= 345.0) { d = d - 360.0; }
        ganho += params.bw_red * (1.0 - min(abs(d) / 15.0, 1.0));
    }
    if (matiz >= 15.0 && matiz < 45.0) {
        ganho += params.bw_orange * (1.0 - min(abs(matiz - 30.0) / 15.0, 1.0));
    }
    if (matiz >= 45.0 && matiz < 75.0) {
        ganho += params.bw_yellow * (1.0 - min(abs(matiz - 60.0) / 15.0, 1.0));
    }
    if (matiz >= 75.0 && matiz < 165.0) {
        ganho += params.bw_green * (1.0 - min(abs(matiz - 120.0) / 45.0, 1.0));
    }
    if (matiz >= 165.0 && matiz < 210.0) {
        ganho += params.bw_aqua * (1.0 - min(abs(matiz - 187.5) / 22.5, 1.0));
    }
    if (matiz >= 210.0 && matiz < 270.0) {
        ganho += params.bw_blue * (1.0 - min(abs(matiz - 240.0) / 30.0, 1.0));
    }
    if (matiz >= 270.0 && matiz < 310.0) {
        ganho += params.bw_purple * (1.0 - min(abs(matiz - 290.0) / 20.0, 1.0));
    }
    if (matiz >= 310.0 && matiz < 345.0) {
        ganho += params.bw_magenta * (1.0 - min(abs(matiz - 327.5) / 17.5, 1.0));
    }
    return ganho;
}

/// O matiz (graus), a saturação e o valor de uma cor em 0–1. Croma zero devolve
/// matiz 0 — quem chama precisa tratar o cinza, que não tem matiz.
fn para_hsv(cor: vec3<f32>) -> vec3<f32> {
    let mx = max(cor.r, max(cor.g, cor.b));
    let mn = min(cor.r, min(cor.g, cor.b));
    let croma = mx - mn;
    var h = 0.0;
    if (croma > 0.0001) {
        if (mx == cor.r) {
            h = 60.0 * (((cor.g - cor.b) / croma) % 6.0);
        } else if (mx == cor.g) {
            h = 60.0 * (((cor.b - cor.r) / croma) + 2.0);
        } else {
            h = 60.0 * (((cor.r - cor.g) / croma) + 4.0);
        }
        if (h < 0.0) { h = h + 360.0; }
    }
    return vec3<f32>(h, croma / max(mx, 0.0001), mx);
}

/// Hermite **monótono** entre nove alturas igualmente espaçadas em 0–255.
///
/// # Por que não é spline nem reta
///
/// 🚨 **Spline cúbica comum oscila**, e numa curva de tom isso é visível: dois
/// pontos vizinhos quase no mesmo nível fazem a interpolação subir acima dos
/// dois e voltar, e o resultado é uma faixa clara atravessando um degradê liso.
/// É o defeito clássico de curva de tom implementada com Catmull-Rom.
///
/// 🚨 **E reta dá faceta.** Nove pontos ligados por segmentos deixam a derivada
/// saltar em cada nó; num céu, cada salto vira uma banda.
///
/// A saída é Fritsch–Carlson: a inclinação de cada nó é a média harmônica das
/// duas secantes vizinhas, e ela vai a zero quando as secantes trocam de sinal.
/// Isso garante que o trecho entre dois pontos **nunca saia do intervalo entre
/// eles** — a curva não inventa um tom que nenhum dos dois nós pediu.
fn curva_por_ponto(valor: f32, p: array<f32, 9>) -> f32 {
    let x = clamp(valor, 0.0, 255.0) / 255.0 * 8.0;
    let i = min(u32(floor(x)), 7u);
    let t = x - f32(i);

    // As secantes dos três trechos em volta (o passo em x é 1 nesta escala).
    var pontos = p;
    let y0 = pontos[i];
    let y1 = pontos[i + 1u];
    let d = y1 - y0;
    let d_ant = select(d, y0 - pontos[i - 1u], i > 0u);
    let d_prox = select(d, pontos[i + 2u] - y1, i < 7u);

    // Fritsch–Carlson: zero quando as secantes se opõem, média harmônica quando
    // concordam. É o que impede a curva de passar por fora dos dois pontos.
    var m0 = 0.0;
    if (d_ant * d > 0.0) { m0 = 2.0 * d_ant * d / (d_ant + d); }
    var m1 = 0.0;
    if (d * d_prox > 0.0) { m1 = 2.0 * d * d_prox / (d + d_prox); }

    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    return clamp(h00 * y0 + h10 * m0 + h01 * y1 + h11 * m1, 0.0, 255.0);
}

/// A curva está no neutro? Nove alturas na reta `y = x` não mexem em nada, e
/// pular o trecho inteiro poupa 12 avaliações por pixel na foto que não a usa —
/// que é a maioria.
fn curva_e_neutra(p: array<f32, 9>) -> bool {
    var pontos = p;
    for (var i = 0u; i < 9u; i = i + 1u) {
        if (abs(pontos[i] - f32(i) * 31.875) > 0.01) {
            return false;
        }
    }
    return true;
}

fn luminancia(cor: vec3<f32>) -> f32 {
    return dot(cor, vec3<f32>(0.299, 0.587, 0.114));
}

fn lab_f(t: f32) -> f32 {
    if (t > 0.008856) {
        return pow(t, 1.0 / 3.0);
    }
    return 7.787 * t + 16.0 / 116.0;
}

fn lab_f_inversa(t: f32) -> f32 {
    let t3 = t * t * t;
    if (t3 > 0.008856) {
        return t3;
    }
    return (t - 16.0 / 116.0) / 7.787;
}

/// sRGB (0–255, já recolhido à faixa) → CIELab, branco D65.
fn para_lab(cor: vec3<f32>) -> vec3<f32> {
    let lin = vec3<f32>(srgb_para_linear(cor.r), srgb_para_linear(cor.g), srgb_para_linear(cor.b));
    let fx = lab_f(dot(lin, vec3<f32>(0.4124, 0.3576, 0.1805)) / 0.95047);
    let fy = lab_f(dot(lin, vec3<f32>(0.2126, 0.7152, 0.0722)));
    let fz = lab_f(dot(lin, vec3<f32>(0.0193, 0.1192, 0.9505)) / 1.08883);
    return vec3<f32>(116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz));
}

/// CIELab → sRGB (0–255), recolhido: a cor fora do gamut encosta na borda.
fn de_lab(lab: vec3<f32>) -> vec3<f32> {
    let fy = (lab.x + 16.0) / 116.0;
    let x = lab_f_inversa(fy + lab.y / 500.0) * 0.95047;
    let y = lab_f_inversa(fy);
    let z = lab_f_inversa(fy - lab.z / 200.0) * 1.08883;
    let r = 3.2406 * x - 1.5372 * y - 0.4986 * z;
    let g = -0.9689 * x + 1.8758 * y + 0.0415 * z;
    let b = 0.0557 * x - 0.2040 * y + 1.0570 * z;
    return vec3<f32>(
        linear_para_srgb(clamp(r, 0.0, 1.0)),
        linear_para_srgb(clamp(g, 0.0, 1.0)),
        linear_para_srgb(clamp(b, 0.0, 1.0)),
    );
}

/// O deslocamento (a*, b*) de uma roda da Gradação de cores do Lightroom.
fn roda_do_lightroom(matiz: f32, saturacao: f32) -> vec2<f32> {
    let angulo = radians(matiz + 28.0);
    return vec2<f32>(cos(angulo), sin(angulo)) * 0.4 * clamp(saturacao, 0.0, 100.0);
}

/// Ruído determinístico de 32 bits — o mesmo pixel dá sempre o mesmo grão.
///
/// 🚨 **É aritmética inteira de propósito, e não `sin(dot(...))`.** O truque do
/// seno é o hash mais comum em shader e é o errado aqui: ele depende da precisão
/// do `sin` de cada backend, e este mesmo arquivo roda como compute no Metal e
/// como fragmento no WebGL2, com um teste que exige o **mesmo pixel** nos dois.
/// Multiplicação e deslocamento em `u32` são exatos em qualquer GPU.
fn embaralhar(x: u32) -> u32 {
    var v = x;
    v ^= v >> 17u;
    v *= 0xed5ad4bbu;
    v ^= v >> 11u;
    v *= 0xac4c1b51u;
    v ^= v >> 15u;
    v *= 0x31848babu;
    v ^= v >> 14u;
    return v;
}

/// Bilinear read at a fractional position, clamped to the image.
///
/// 🔑 It is EXACT at integer positions: `floor` of an integer float gives the
/// integer back, `frac` is 0.0, and `mix(a, b, 0.0)` is `a` — bit for bit. That
/// is what lets lens distortion share this path with every other adjustment
/// without moving a single pixel when it is neutral.
fn amostrar(pos: vec2<f32>, dims: vec2<u32>) -> vec4<f32> {
    let ultimo = vec2<f32>(f32(dims.x) - 1.0, f32(dims.y) - 1.0);
    let dentro = clamp(pos, vec2<f32>(0.0, 0.0), ultimo);

    let base = floor(dentro);
    let frac = dentro - base;

    let x0 = i32(base.x);
    let y0 = i32(base.y);
    let x1 = min(x0 + 1, i32(dims.x) - 1);
    let y1 = min(y0 + 1, i32(dims.y) - 1);

    let p00 = textureLoad(input_texture, vec2<i32>(x0, y0), 0);
    let p10 = textureLoad(input_texture, vec2<i32>(x1, y0), 0);
    let p01 = textureLoad(input_texture, vec2<i32>(x0, y1), 0);
    let p11 = textureLoad(input_texture, vec2<i32>(x1, y1), 0);

    return mix(mix(p00, p10, frac.x), mix(p01, p11, frac.x), frac.y);
}

/// Um pixel revelado: o que está em `coord` na entrada, com os 46 ajustes
/// aplicados, em 0.0–1.0.
fn revelar_pixel(coord: vec2<u32>) -> vec4<f32> {
    let dims = textureDimensions(input_texture);


    // Lens distortion: this pixel reads from somewhere ELSE in the source.
    //
    // The model is the usual radial one — r' = r * (1 + k*r²) — applied from the
    // center outward, with the radius normalized so that 1.0 is the corner.
    // Negative pulls the corners in (corrects barrel), positive pushes them out
    // (corrects pincushion).
    //
    // 🔑 It is the FIRST thing that happens, and it has to be: every other
    // adjustment reads the neighborhood, and reading a neighborhood of the
    // undistorted image and then moving the pixel would smear along the wrong
    // direction.
    var origem = vec2<f32>(f32(coord.x), f32(coord.y));
    if (params.lens_distortion != 0.0) {
        let centro = vec2<f32>(f32(dims.x), f32(dims.y)) * 0.5;
        let escala = max(length(centro), 1.0);
        let d = (origem - centro) / escala;
        let k = params.lens_distortion * 0.005; // -100..100 -> -0.5..0.5
        origem = centro + d * (1.0 + k * dot(d, d)) * escala;
    }

    // Load pixel (values in 0.0-1.0 range)
    let pixel = amostrar(origem, dims);
    var r = pixel.r * 255.0;
    var g = pixel.g * 255.0;
    var b = pixel.b * 255.0;
    let a = pixel.a;

    // ⚠️ The neighborhood loop below walks integers around the SOURCE pixel, and
    // not around this thread's own coordinate. With no distortion the two are
    // the same value, so nothing changes; with distortion, sampling around the
    // thread's coordinate would blur a neighborhood the output pixel never came
    // from.
    let base_x = i32(round(origem.x));
    let base_y = i32(round(origem.y));

    // 0. Noise Reduction & Sharpening
    // We combine NR and Sharpening (USM) in a single neighborhood loop for efficiency
    let do_nr_lum = params.nr_luminance > 0.0;
    let do_nr_col = params.nr_color > 0.0;
    let do_sharpen = params.sharpen_amount > 0.0;
    
    if (do_nr_lum || do_nr_col || do_sharpen) {
        var sum_r_lum = 0.0;
        var sum_g_lum = 0.0;
        var sum_b_lum = 0.0;
        var sum_weight_lum = 0.0;
        
        var sum_u_col = 0.0;
        var sum_v_col = 0.0;
        var sum_weight_col = 0.0;
        
        var sum_y_sharpen = 0.0;
        var sum_weight_sharpen = 0.0;
        
        // Calculate center luminance and YUV
        let center_y = 0.299 * r + 0.587 * g + 0.114 * b;
        let center_lum_norm = center_y / 255.0; 
        
        // Params
        let sigma_s = 2.0; // NR Spatial
        var sigma_r = max(params.nr_luminance * 0.002, 0.0001); // NR Range
        var sigma_color = max(params.nr_color * 0.05, 0.1); // Color NR
        let sigma_sharpen = max(params.sharpen_radius, 0.5); // Sharpen Radius

        // O Detalhe e a Suavidade do Lightroom (2026-09-30). Cada um só entra
        // fora do neutro (50), e por isso a revelação de antes sai bit a bit
        // igual: mais detalhe estreita o filtro (guarda textura), menos o abre.
        if (params.nr_luminance_detail != 50.0) {
            sigma_r *= mix(1.6, 0.4, clamp(params.nr_luminance_detail * 0.01, 0.0, 1.0));
        }
        if (params.nr_color_detail != 50.0) {
            sigma_color *= mix(1.5, 0.5, clamp(params.nr_color_detail * 0.01, 0.0, 1.0));
        }
        if (params.nr_color_smoothness != 50.0) {
            sigma_color *= mix(0.6, 1.4, clamp(params.nr_color_smoothness * 0.01, 0.0, 1.0));
        }
        // A Máscara da nitidez mede a borda na vizinhança 3×3.
        let medir_borda = do_sharpen && params.sharpen_masking > 0.0;
        var variacao = 0.0;
        
        // 5x5 Kernel
        for (var dy: i32 = -2; dy <= 2; dy++) {
            for (var dx: i32 = -2; dx <= 2; dx++) {
                let nx = base_x + dx;
                let ny = base_y + dy;
                
                if (nx >= 0 && ny >= 0 && nx < i32(dims.x) && ny < i32(dims.y)) {
                    let neighbor = textureLoad(input_texture, vec2<i32>(nx, ny), 0);
                    let nr = neighbor.r * 255.0;
                    let ng = neighbor.g * 255.0;
                    let nb = neighbor.b * 255.0;
                    
                    let dist_sq = f32(dx*dx + dy*dy);
                    let n_lum = (0.299 * nr + 0.587 * ng + 0.114 * nb) / 255.0;
                    if (medir_borda && abs(dx) <= 1 && abs(dy) <= 1) {
                        variacao += abs(n_lum - center_lum_norm);
                    }
                    
                    // --- Luminance NR (Bilateral) ---
                    if (do_nr_lum) {
                        let diff = center_lum_norm - n_lum;
                        let weight = exp(-dist_sq / (2.0 * sigma_s * sigma_s)) * 
                                     exp(-(diff * diff) / (2.0 * sigma_r * sigma_r));
                        sum_r_lum += nr * weight;
                        sum_g_lum += ng * weight;
                        sum_b_lum += nb * weight;
                        sum_weight_lum += weight;
                    }
                    
                    // --- Color NR (Gaussian on U/V) ---
                    if (do_nr_col) {
                         let nu = -0.147 * nr - 0.289 * ng + 0.436 * nb;
                         let nv = 0.615 * nr - 0.515 * ng - 0.100 * nb;
                         let weight = exp(-dist_sq / (2.0 * sigma_color * sigma_color));
                         sum_u_col += nu * weight;
                         sum_v_col += nv * weight;
                         sum_weight_col += weight;
                    }
                    
                    // --- Sharpening (Gaussian on Y) ---
                    if (do_sharpen) {
                        // Gaussian blur for USM
                        let weight = exp(-dist_sq / (2.0 * sigma_sharpen * sigma_sharpen));
                        let ny_val = 0.299 * nr + 0.587 * ng + 0.114 * nb;
                        sum_y_sharpen += ny_val * weight;
                        sum_weight_sharpen += weight;
                    }
                }
            }
        }
        
        // --- Recombine NR Results ---
        var final_y = center_y;
        
        if (do_nr_lum && sum_weight_lum > 0.0) {
            let fr = sum_r_lum / sum_weight_lum;
            let fg = sum_g_lum / sum_weight_lum;
            let fb = sum_b_lum / sum_weight_lum;
            final_y = 0.299 * fr + 0.587 * fg + 0.114 * fb;
            // O Contraste do ruído devolve parte do contraste fino que o
            // filtro levou.
            if (params.nr_luminance_contrast > 0.0) {
                final_y = mix(final_y, center_y, clamp(params.nr_luminance_contrast, 0.0, 100.0) * 0.005);
            }
        }
        
        var final_u = -0.147 * r - 0.289 * g + 0.436 * b;
        var final_v = 0.615 * r - 0.515 * g - 0.100 * b;
        
        if (do_nr_col && sum_weight_col > 0.0) {
            final_u = sum_u_col / sum_weight_col;
            final_v = sum_v_col / sum_weight_col;
        }
        
        // Apply NR changes to r,g,b
        if (do_nr_lum || do_nr_col) {
            r = final_y + 1.140 * final_v;
            g = final_y - 0.395 * final_u - 0.581 * final_v;
            b = final_y + 2.032 * final_u;
        }
        
        // --- Apply Sharpening (USM) ---
        if (do_sharpen && sum_weight_sharpen > 0.0) {
            let blurred_y = sum_y_sharpen / sum_weight_sharpen;
            var detail = final_y - blurred_y;
            var amount = params.sharpen_amount * 0.05; // Scale 0-100 to approx 0-5

            // O Detalhe da nitidez (neutro 25, o do Lightroom): abaixo dele o
            // detalhe grande é contido — é o que tira o halo da borda —, acima
            // dele a nitidez cresce no detalhe fino.
            if (params.sharpen_detail != 25.0) {
                let d = clamp(params.sharpen_detail, 0.0, 100.0);
                if (d < 25.0) {
                    detail = mix(detalhe_contido(detail / 255.0, 40.0) * 255.0, detail, d / 25.0);
                } else {
                    amount *= 1.0 + (d - 25.0) / 75.0 * 0.8;
                }
            }
            // A Máscara: a nitidez só fica onde há borda. Em 0, a foto inteira.
            if (medir_borda) {
                let borda = variacao / 8.0;
                let limiar = pow(clamp(params.sharpen_masking * 0.01, 0.0, 1.0), 2.0) * 0.12;
                amount *= smoothstep(limiar * 0.5, limiar, borda);
            }
            
            // Add detail back to RGB channels
            r += detail * amount;
            g += detail * amount;
            b += detail * amount;
        }
        
    }
    // 0. Calibração de câmera — **antes de tudo**, e é essa posição que a define.
    //
    // 🔑 No Lightroom ela age nos primários do perfil da câmera, antes de
    // qualquer revelação; rodá-la depois do contraste ou do HSL daria outra
    // coisa com o mesmo nome. Aqui é o primeiro ajuste de cor do pipeline, logo
    // após o ruído e a nitidez (que são espaciais e não mexem em matiz).
    //
    // ⚠️ **Aproximação declarada.** A da Adobe é uma matriz no espaço do perfil,
    // que este motor não tem — ele recebe RGB já revelado. Aqui cada primário
    // gira o matiz e escala a saturação com queda larga (`peso_do_primario`), o
    // que reproduz o comportamento visível na faixa que os presets usam. O
    // extremo diverge, e diverge de forma suave: nenhum valor inverte nada.
    if (params.calib_red_hue != 0.0 || params.calib_red_sat != 0.0
        || params.calib_green_hue != 0.0 || params.calib_green_sat != 0.0
        || params.calib_blue_hue != 0.0 || params.calib_blue_sat != 0.0) {
        let entrada = clamp(vec3<f32>(r, g, b) / 255.0, vec3<f32>(0.0), vec3<f32>(1.0));
        let hsv = para_hsv(entrada);
        if (hsv.y > 0.0005) {
            let pr = peso_do_primario(hsv.x, 0.0);
            let pg = peso_do_primario(hsv.x, 120.0);
            let pb = peso_do_primario(hsv.x, 240.0);

            // 🔑 O 0,3 é a mesma régua do matiz do HSL: o extremo do slider da
            // Adobe desloca cerca de 30°, não meia volta. Está escrito uma vez
            // aqui e uma vez no `lightroom.ts`, e os dois dizem o mesmo.
            let dh = (params.calib_red_hue * pr
                + params.calib_green_hue * pg
                + params.calib_blue_hue * pb) * 0.3;
            let ds = 1.0 + (params.calib_red_sat * pr
                + params.calib_green_sat * pg
                + params.calib_blue_sat * pb) * 0.01;

            var h = (hsv.x + dh) % 360.0;
            if (h < 0.0) { h = h + 360.0; }
            let sat = clamp(hsv.y * ds, 0.0, 1.0);
            // HSV → RGB pela cor pura do matiz: `v · mix(branco, pura, sat)`.
            let nova = hsv.z * mix(vec3<f32>(1.0), cor_do_matiz(h), sat) * 255.0;
            r = nova.r;
            g = nova.g;
            b = nova.b;
        }
    }

    // 0b. O matiz das sombras da calibração — verde/magenta, só embaixo.
    if (params.calib_shadow_tint != 0.0) {
        let l = clamp(((r + g + b) / 3.0) / 255.0, 0.0, 1.0);
        // Some no meio-tom: é o que o controle da Adobe faz, e é o que impede
        // um retrato de ganhar magenta na pele por causa de um preset de filme.
        let peso = 1.0 - smoothstep(0.0, 0.5, l);
        let t = params.calib_shadow_tint * 0.01 * peso * 12.0;
        r += t;
        b += t;
        g -= t;
    }

    // 0. O balanço de branco do processo 1 — **antes** da Exposição, como no
    // Lightroom. Uma curva por canal para cada valor, medida na rampa: o ganho
    // cai para o branco (Temperatura +30 leva o cinza 128 a 177/156/123; a
    // conta de antes, a 158/129/99, trocava vermelho por azul). Os campos
    // continuam na escala de antes (−10 a 10); a tabela é por valor do slider.
    if (params.processo >= 0.5 && (params.temperature != 0.0 || params.tint != 0.0)) {
        var cor = vec3<f32>(r, g, b);
        if (params.temperature != 0.0) {
            cor = lr_balanco(LR_LINHA_TEMPERATURA, params.temperature * 10.0, cor);
        }
        if (params.tint != 0.0) {
            cor = lr_balanco(LR_LINHA_MATIZ, params.tint * 10.0, cor);
        }
        r = cor.r;
        g = cor.g;
        b = cor.b;
    }

    if (params.exposure != 0.0) {
        if (params.processo >= 0.5) {
            // 🔑 A do Lightroom guarda o branco: −1 EV leva 128 a 83 e 255
            // continua 255; +1 leva 128 a 181 com ombro até o branco, em vez
            // de estourar tudo acima do meio. Medida de −5 a +5 EV, o alcance
            // inteiro do slider: até 1/out a tabela parava em ±2, e dali em
            // diante a foto não mudava mais (o "comportamento esquisito").
            let pos = (clamp(params.exposure, -5.0, 5.0) + 5.0) / 0.25;
            r = lr_slider(LR_LINHA_EXPOSICAO, 41, pos, r);
            g = lr_slider(LR_LINHA_EXPOSICAO, 41, pos, g);
            b = lr_slider(LR_LINHA_EXPOSICAO, 41, pos, b);
        } else {
            let factor = pow(2.0, params.exposure);
            r *= factor;
            g *= factor;
            b *= factor;
        }
    }

    // 1b. Exposição local — em RGB **linear**, pela máscara de cada camada.
    //
    // 🔑 Logo depois da global e antes do contraste: o resto do pipeline age
    // sobre a luz que a máscara deu, como no Lightroom. Em linear, +1 EV dobra
    // a luz de verdade; a global, herdada, multiplica o valor sRGB.
    if (locais.quantidade.x > 0.5) {
        let ev = exposicao_local(coord);
        if (ev != 0.0) {
            let fator = exp2(ev);
            r = linear_para_srgb(srgb_para_linear(r) * fator);
            g = linear_para_srgb(srgb_para_linear(g) * fator);
            b = linear_para_srgb(srgb_para_linear(b) * fator);
        }
    }
    
    // 1c. Remover névoa — antes do contraste, como no Lightroom.
    //
    // 🔑 **É o canal escuro de He et al.**: numa cena sem névoa, toda janela
    // tem algum canal quase preto; o que sobra nele é a névoa, que veio da luz
    // do céu `A`. A transmissão `t = 1 − ω·escuro/A` diz quanto da cena chegou,
    // e a cena limpa é `(I − A)/t + A`. O canal escuro e `A` vêm da guia.
    // Negativo, a névoa entra: a cena se mistura à luz do céu.
    if (params.dehaze != 0.0) {
        let gu = ler_guia(origem, dims);
        let ceu = dados_da_guia.dados.x;
        // No processo 1, a força que a varredura achou contra o Lightroom
        // (régua `nevoa`, 6 fotos, lidas em Adobe RGB, 1/out): 0,75 no
        // positivo (+100: ΔE 4,1; 0,9 já dá 4,2) e 0,6 no negativo (−100:
        // 8,3 → 7,9; −50: 4,5 → 4,2). A forma do negativo ainda difere: o
        // Lightroom mira uma névoa mais clara que a nossa luz do céu e poupa o
        // preto profundo (é por profundidade); com o escuro certo, o nosso
        // claro sai ~20 abaixo. O estúdio só usa de −12 a −17.
        let escala_lr = select(1.0, select(0.75, 0.6, params.dehaze < 0.0), params.processo >= 0.5);
        let forca = clamp(params.dehaze * 0.01 * escala_lr, -1.0, 1.0);
        var cor = vec3<f32>(r, g, b) / 255.0;
        if (forca > 0.0) {
            let t = max(1.0 - 0.9 * forca * gu.b / max(ceu, 0.05), 0.25);
            cor = (cor - vec3<f32>(ceu)) / t + vec3<f32>(ceu);
        } else {
            cor = mix(cor, vec3<f32>(ceu), -forca * 0.6);
        }
        r = cor.r * 255.0;
        g = cor.g * 255.0;
        b = cor.b * 255.0;
    }

    // 2. Contrast
    if (params.contrast != 1.0) {
        if (params.processo >= 0.5) {
            // 🔑 A curva do Lightroom guarda o preto e o branco: Contraste −100
            // ainda vai de 0 a 255 (a reta de antes virava cinza 128 em tudo),
            // e −57 leva 64 a 79 em vez de 100. O campo continua multiplicador
            // (1 + v/100); a tabela é por valor do slider.
            let c = lr_cem(LR_LINHA_CONTRASTE, (params.contrast - 1.0) * 100.0, vec3<f32>(r, g, b));
            r = c.r;
            g = c.g;
            b = c.b;
        } else {
            r = (r - 128.0) * params.contrast + 128.0;
            g = (g - 128.0) * params.contrast + 128.0;
            b = (b - 128.0) * params.contrast + 128.0;
        }
    }
    
    // 3. Temperature (warm/cool balance) — no processo 1 o balanço já entrou,
    // antes da Exposição (passo 0).
    if (params.temperature != 0.0 && params.processo < 0.5) {
        r += params.temperature * 10.0;
        b -= params.temperature * 10.0;
    }
    
    // 4. Tint (green/magenta balance)
    if (params.tint != 0.0 && params.processo < 0.5) {
        if (params.tint > 0.0) {
            r += params.tint * 5.0;
            b += params.tint * 5.0;
            g -= params.tint * 5.0;
        } else {
            g -= params.tint * 5.0;
            r += params.tint * 5.0;
            b += params.tint * 5.0;
        }
    }
    
    // 5-8. Altas luzes, sombras, brancos e pretos.
    //
    // 🚨 Até 2026-09-06 os quatro eram um `if` sobre a luminância — altas luzes
    // só tocavam `> 128`, brancos só `> 192`, sombras `< 128`, pretos `< 64` —
    // e multiplicavam a região inteira por um fator só. Isso manchava a foto de
    // duas maneiras, e o dono viu as duas na mesma imagem:
    //
    //   - **Degrau**: dois pixels vizinhos de 127 e 129 saíam com dezenas de
    //     níveis de diferença. Numa parede lisa isso vira contorno, e em área
    //     ruidosa vira o granulado que apareceu em volta das janelas.
    //   - **Inversão**: com "brancos" em -100 o fator virava 0,0 — o pixel de
    //     255 saía PRETO enquanto o vizinho de 190 ficava intacto. O branco da
    //     janela virava sujeira; "altas luzes" em -100 zerava tudo acima de 128.
    //
    // A forma nova é uma curva por região, aplicada sobre a luminância
    // normalizada. Cada uma é monotônica por construção — a derivada não fica
    // negativa em nenhum ponto de -100..+100 — e as quatro entram COMPOSTAS,
    // uma sobre a saída da outra, porque compor funções crescentes dá função
    // crescente: nem a combinação dos quatro consegue inverter dois níveis.
    //
    //   altas luzes  n + a·n²(1-n)     f' = 1 + a(2n - 3n²)  ≥ 0
    //   sombras      n + a·n(1-n)²     f' = 1 + a(1-n)(1-3n) ≥ 0
    //   brancos      n + a·n³/3        f' = 1 + a·n²         ≥ 0
    //   pretos       n + a·(1-n)³/3    f' = 1 - a(1-n)²      ≥ 0
    //
    // Os pesos também escolhem a faixa sem cortá-la: `n²(1-n)` é quase nada no
    // escuro e some de novo no branco puro (altas luzes recuperam, não movem o
    // ponto de branco), enquanto `n³` é o contrário — é o que faz "brancos" ser
    // o controle do topo da escala. Quem prende tudo isso é
    // `o_tom_por_regiao_nunca_inverte_nem_da_degrau`.
    if (params.processo >= 0.5 && (params.highlights != 0.0 || params.shadows != 0.0
        || params.whites != 0.0 || params.blacks != 0.0)) {
        // 🔑 **O processo 1: as curvas medidas no Lightroom.** Realces e Sombras
        // pela luminância, com a cor pela razão (no Lightroom os dois são
        // locais; numa rampa, a luminância erra metade do que erra o canal a
        // canal). Brancos e Pretos canal a canal, que é como os dois medem
        // melhor — e os dois guardam o preto e o branco: Pretos +50 não
        // levanta mais o 0 para 14.
        if (params.highlights != 0.0 || params.shadows != 0.0) {
            // 🔑 **Locais, como no Lightroom.** A curva medida na rampa vale
            // para uma área lisa; numa foto, aplicada pixel a pixel, ela achata
            // o detalhe e passa do ponto (Realces −100 escurecia o alto 39
            // níveis além do Lightroom). A curva age na **base** — a luminância
            // de agora menos o detalhe, que é medido na foto de entrada contra
            // a guia larga, como na Claridade — e o ganho da base vai ao
            // pixel. Numa área lisa a base é o próprio pixel, e o resultado é o
            // da régua.
            let l = clamp((r + g + b) / 3.0, 0.0, 255.0);
            var base = l;
            if (dados_da_guia.dados.y > 0.5) {
                let gu = ler_guia(origem, dims);
                let l0 = (pixel.r + pixel.g + pixel.b) / 3.0;
                base = clamp(l - (l0 - gu.r) * 255.0, 0.0, 255.0);
            }
            // ⚠️ **A força numa foto é ~1/3 a 1/2 da da rampa.** Na rampa (áreas
            // lisas enormes) o Lightroom aplica o efeito local inteiro; numa
            // foto de verdade ele age bem menos — um filtro que respeita borda,
            // que o desfoque gaussiano da guia não imita. Varrido com o
            // comparar_em_lote (1/out): força 0,35 / 0,5 / 1,0 dá 6,3 / 6,6 /
            // 9,0 nas fotos e 7,1 / 5,5 / 3,3 na rampa; nos 28 presets em 8
            // fotos, 15,1 / 15,2 / — (o processo 0 dá 24,4). O raio da base
            // (1,2 % a 15 % do lado) quase não muda nada.
            var n = base;
            if (params.highlights != 0.0) {
                n = lr_slider(LR_LINHA_REALCES, 21, (params.highlights * LR_FORCA_LOCAL + 100.0) / 10.0, n);
            }
            if (params.shadows != 0.0) {
                n = lr_slider(LR_LINHA_SOMBRAS, 21, (params.shadows * LR_FORCA_LOCAL + 100.0) / 10.0, n);
            }
            let delta = n - base;
            let escala = n / max(base, 0.01);
            let mistura = smoothstep(0.0, 12.0, base);
            r = mix(r + delta, r * escala, mistura);
            g = mix(g + delta, g * escala, mistura);
            b = mix(b + delta, b * escala, mistura);
        }
        if (params.whites != 0.0) {
            let c = lr_cem(LR_LINHA_BRANCOS, params.whites, vec3<f32>(r, g, b));
            r = c.r;
            g = c.g;
            b = c.b;
        }
        if (params.blacks != 0.0) {
            let c = lr_cem(LR_LINHA_PRETOS, params.blacks, vec3<f32>(r, g, b));
            r = c.r;
            g = c.g;
            b = c.b;
        }
    } else if (params.highlights != 0.0 || params.shadows != 0.0
        || params.whites != 0.0 || params.blacks != 0.0) {
        let l = clamp(((r + g + b) / 3.0) / 255.0, 0.0, 1.0);

        // 🔑 Uma de cada vez, cada uma sobre o resultado da anterior. A soma
        // dos quatro deltas seria mais curta e NÃO serviria: as garantias acima
        // valem para cada curva sozinha, e somadas elas se cancelam — "sombras"
        // em -100 com "pretos" em +100 devolveria derivada -1 no preto, que é a
        // inversão de volta. Compostas, a garantia de cada uma basta.
        var n = clamp(l + (params.whites * 0.01) * l * l * l / 3.0, 0.0, 1.0);
        // 🔑 Recuperar (negativo) usa outro peso desde 2026-09-30:
        // `1,2·n³(1−n)`, que poupa o meio-tom — a forma do Lightroom, medida
        // num DNG revelado lá (Realces −58 quase não mexia na mediana). O peso
        // de antes, `n²(1−n)`, escurecia o meio-tom com a mesma força do alto.
        // ⚠️ O Lightroom comprime o alto mais do que isto; mais força aqui
        // daria derivada acima de 2,2 junto do branco, e composta com
        // "brancos" vira degrau (`o_tom_por_regiao_nunca_inverte_nem_da_degrau`).
        if (params.highlights < 0.0) {
            n = clamp(n + params.highlights * 0.01 * 1.2 * n * n * n * (1.0 - n), 0.0, 1.0);
        } else {
            n = clamp(n + (params.highlights * 0.01) * n * n * (1.0 - n), 0.0, 1.0);
        }
        // 🔑 Clarear sombras (positivo) usa `1,4·n(1−n)³` desde 2026-09-30:
        // o peso de antes, `n(1−n)²`, se estendia até as altas luzes — no DNG
        // do Estúdio Canela, Sombras +80 sozinho subia o p90 em 15 níveis,
        // e o Lightroom quase não toca o alto. Derivada ≥ 1 − 1,4·0,25 > 0.
        if (params.shadows > 0.0) {
            let e3 = (1.0 - n) * (1.0 - n) * (1.0 - n);
            n = clamp(n + (params.shadows * 0.01) * 1.4 * n * e3, 0.0, 1.0);
        } else {
            n = clamp(n + (params.shadows * 0.01) * n * (1.0 - n) * (1.0 - n), 0.0, 1.0);
        }
        let e = 1.0 - n;
        n = clamp(n + (params.blacks * 0.01) * e * e * e / 9.0, 0.0, 1.0);

        // A cor se preserva pela razão, que escala os três canais juntos.
        // Abaixo de ~5% de luz não há razão que se sustente (o divisor tende a
        // zero e o matiz explode), e lá o ajuste entra somado — é o que deixa
        // "pretos" clarear um preto puro em vez de multiplicar zero por 1,3.
        let delta = (n - l) * 255.0;
        let escala = n / max(l, 0.0001);
        let mistura = smoothstep(0.0, 0.05, l);
        r = mix(r + delta, r * escala, mistura);
        g = mix(g + delta, g * escala, mistura);
        b = mix(b + delta, b * escala, mistura);
    }

    // Recalculate luminance after tonal adjustments
    let lum2 = (r + g + b) / 3.0;
    
    // 🎞️ **Com o P&B ligado, Saturação, Vibração e HSL não agem** — como no
    // Lightroom, que os apaga no painel e guarda o valor. 🚨 Até 2026-10-01 o
    // P&B vinha com `saturation = -1`, e a foto chegava ao mixer já cinza: os
    // oito sliders da Mistura de preto e branco não mudavam pixel nenhum, em
    // todo preset P&B importado do Lightroom.
    let em_pb = params.bw_ativo != 0.0;

    // 9-10, processo 1: Saturação e Vibração **medidas no Lightroom**, numa
    // carta de cores (24 matizes × 6 saturações). O croma em Lab é multiplicado
    // por um fator de cada matiz × saturação de entrada, e o L* fica: a
    // Vibração de antes (`1 + v·2`) zerava a cor em −50 e a **invertia** em
    // −100 (fator −1), e só agia abaixo de um corte seco. A do Lightroom tira
    // mais das cores apagadas no negativo, realça as apagadas no positivo e
    // protege os vermelhos (a pele).
    if (params.processo >= 0.5 && !em_pb && (params.saturation != 0.0 || params.vibrance != 0.0)) {
        let cor = clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(255.0));
        let maior = max(max(cor.r, cor.g), cor.b);
        let menor = min(min(cor.r, cor.g), cor.b);
        let sat = select(0.0, (maior - menor) / maior, maior > 0.5);
        if (sat > 0.001) {
            // O matiz HSV, em graus.
            var h = 0.0;
            let d = maior - menor;
            if (maior == cor.r) {
                h = 60.0 * (((cor.g - cor.b) / d) % 6.0);
            } else if (maior == cor.g) {
                h = 60.0 * ((cor.b - cor.r) / d + 2.0);
            } else {
                h = 60.0 * ((cor.r - cor.g) / d + 4.0);
            }
            if (h < 0.0) {
                h += 360.0;
            }
            // O ΔL* medido vem junto com o croma; abaixo da menor saturação da
            // carta (0,1) ele cai a zero com ela, para o cinza não mudar de claro.
            var fator = 1.0;
            var claro = 0.0;
            if (params.saturation != 0.0) {
                let v = params.saturation * 100.0;
                fator *= lr_fator_de_croma(LR_LINHA_SATURACAO, v, h, sat);
                claro += lr_fator_de_croma(LR_LINHA_SATURACAO_L, v, h, sat);
            }
            if (params.vibrance != 0.0) {
                let v = params.vibrance * 100.0;
                fator *= lr_fator_de_croma(LR_LINHA_VIBRACAO, v, h, sat);
                claro += lr_fator_de_croma(LR_LINHA_VIBRACAO_L, v, h, sat);
            }
            var lab = para_lab(cor);
            claro *= min(sat / 0.1, 1.0);
            lab = vec3<f32>(clamp(lab.x + claro, 0.0, 100.0), lab.y * fator, lab.z * fator);
            let nova = de_lab(lab);
            r = nova.r;
            g = nova.g;
            b = nova.b;
        }
    }

    // 9. Saturation (overall color intensity)
    if (params.saturation != 0.0 && !em_pb && params.processo < 0.5) {
        let factor = 1.0 + params.saturation;
        r = lum2 + (r - lum2) * factor;
        g = lum2 + (g - lum2) * factor;
        b = lum2 + (b - lum2) * factor;
    }
    
    // 10. Vibrance (intelligent saturation - affects muted colors more)
    if (params.vibrance != 0.0 && !em_pb && params.processo < 0.5) {
        let max_diff = max(max(abs(r - lum2), abs(g - lum2)), abs(b - lum2));
        if (max_diff < 64.0) {
            let factor = 1.0 + params.vibrance * 2.0;
            r = lum2 + (r - lum2) * factor;
            g = lum2 + (g - lum2) * factor;
            b = lum2 + (b - lum2) * factor;
        }
    }
    
    // 11. Claridade e Textura — contraste local, pela guia.
    //
    // 🚨 **Até 2026-09-30 a "Claridade" era uma saturação**: escalava a
    // distância de cada canal à média, o mesmo que o controle de saturação faz,
    // e a tela a chamava de "Textura". Numa foto P&B ela não fazia nada. Agora
    // as duas são o que o Lightroom chama por esses nomes: o pixel se afasta da
    // média da vizinhança — larga na Claridade (σ = 1,2 % do lado maior),
    // média na Textura (0,25 %).
    //
    // 🔑 O detalhe é medido **na foto de entrada**, a mesma de que a guia saiu,
    // e somado à luminância de agora: medir na foto já revelada misturaria a
    // exposição e as curvas no "detalhe". O meio-tom pesa mais (o Lightroom
    // poupa o preto e o branco), e a borda forte é contida — sem isso o
    // desfoque comum deixa halo em toda silhueta.
    if (params.clarity != 0.0 || params.texture != 0.0) {
        let gu = ler_guia(origem, dims);
        let l = clamp(((r + g + b) / 3.0) / 255.0, 0.0, 1.0);
        let l0 = (pixel.r + pixel.g + pixel.b) / 3.0;
        let meio = 1.0 - pow(abs(2.0 * l - 1.0), 3.0);
        var n = l;
        if (params.clarity != 0.0) {
            n += params.clarity * 0.9 * detalhe_contido(l0 - gu.r, 4.0) * meio;
        }
        if (params.texture != 0.0) {
            n += params.texture * 0.012 * detalhe_contido(l0 - gu.g, 8.0) * meio;
        }
        let cor = com_luminancia(vec3<f32>(r, g, b), l, clamp(n, 0.0, 1.0));
        r = cor.r;
        g = cor.g;
        b = cor.b;
    }
    
    // 12-15. Tone Curve Parametric Zones
    let lum_final = (r + g + b) / 3.0;
    let norm_lum = lum_final / 255.0;
    
    // As quatro zonas, entre as três divisões (o Lightroom abre em 25/50/75 —
    // e com elas no neutro estas são as zonas fixas de antes). As divisões
    // ficam em ordem mesmo quando o JSON traz outra coisa.
    {
        let d1 = clamp(params.tone_curve_split_shadows * 0.01, 0.0, 1.0);
        let d2 = clamp(params.tone_curve_split_midtones * 0.01, d1, 1.0);
        let d3 = clamp(params.tone_curve_split_highlights * 0.01, d2, 1.0);
        var cor = vec3<f32>(r, g, b);
        cor = zona_da_curva(cor, norm_lum, 0.0, d1, params.tone_curve_shadows);
        cor = zona_da_curva(cor, norm_lum, d1, d2, params.tone_curve_darks);
        cor = zona_da_curva(cor, norm_lum, d2, d3, params.tone_curve_lights);
        cor = zona_da_curva(cor, norm_lum, d3, 1.0, params.tone_curve_highlights);
        r = cor.r;
        g = cor.g;
        b = cor.b;
    }

    // 16. Curva por ponto — depois da paramétrica, como no Lightroom.
    //
    // 🚨 **Era o maior buraco da importação de presets**: 321 de 400 presets
    // comerciais usam curva por ponto, e até 2026-09-12 ela era descartada — o
    // preset chegava sem o pé de contraste que o define, e o operador concluía
    // que o importador estava quebrado.
    //
    // 🔑 **Os canais primeiro, o mestre depois**, que é a ordem da Adobe. Um
    // preset de filme costuma levantar o preto só no azul (a "sombra fria") e
    // depois aplicar um S no mestre; invertida, a ordem faz o S comer o
    // levantamento e a foto sai sem o virado de cor.
    //
    // ⚠️ **Os quatro são pulados quando estão na identidade.** São doze
    // avaliações de Hermite por pixel, e a maioria das fotos não usa curva
    // nenhuma — pagar isso em toda revelação seria desperdício puro.
    {
        let mestre = array<f32, 9>(params.curva_m0, params.curva_m1, params.curva_m2, params.curva_m3, params.curva_m4, params.curva_m5, params.curva_m6, params.curva_m7, params.curva_m8);
        let vermelho = array<f32, 9>(params.curva_r0, params.curva_r1, params.curva_r2, params.curva_r3, params.curva_r4, params.curva_r5, params.curva_r6, params.curva_r7, params.curva_r8);
        let verde = array<f32, 9>(params.curva_g0, params.curva_g1, params.curva_g2, params.curva_g3, params.curva_g4, params.curva_g5, params.curva_g6, params.curva_g7, params.curva_g8);
        let azul = array<f32, 9>(params.curva_b0, params.curva_b1, params.curva_b2, params.curva_b3, params.curva_b4, params.curva_b5, params.curva_b6, params.curva_b7, params.curva_b8);

        if (!curva_e_neutra(vermelho)) { r = curva_por_ponto(r, vermelho); }
        if (!curva_e_neutra(verde)) { g = curva_por_ponto(g, verde); }
        if (!curva_e_neutra(azul)) { b = curva_por_ponto(b, azul); }
        if (!curva_e_neutra(mestre)) {
            r = curva_por_ponto(r, mestre);
            g = curva_por_ponto(g, mestre);
            b = curva_por_ponto(b, mestre);
        }
    }

    // HSL: saturation, hue and luminance, per color band.
    //
    // Hue and luminance were dead until 2026-08-17: the fields existed in the
    // uniform and nothing in this file mentioned them. 18 of the 42 sliders in
    // the develop panel moved nothing.
    let has_hsl_sat = params.hsl_red_sat != 0.0 || params.hsl_orange_sat != 0.0
        || params.hsl_yellow_sat != 0.0 || params.hsl_green_sat != 0.0
        || params.hsl_aqua_sat != 0.0 || params.hsl_blue_sat != 0.0
        || params.hsl_purple_sat != 0.0 || params.hsl_magenta_sat != 0.0;
    let has_hsl_hue = params.hsl_red_hue != 0.0 || params.hsl_orange_hue != 0.0
        || params.hsl_yellow_hue != 0.0 || params.hsl_green_hue != 0.0
        || params.hsl_aqua_hue != 0.0 || params.hsl_blue_hue != 0.0
        || params.hsl_purple_hue != 0.0 || params.hsl_magenta_hue != 0.0;
    let has_hsl_lum = params.hsl_red_lum != 0.0 || params.hsl_orange_lum != 0.0
        || params.hsl_yellow_lum != 0.0 || params.hsl_green_lum != 0.0
        || params.hsl_aqua_lum != 0.0 || params.hsl_blue_lum != 0.0
        || params.hsl_purple_lum != 0.0 || params.hsl_magenta_lum != 0.0;
    // No P&B o painel HSL vira a Mistura de preto e branco (ver `em_pb`).
    let has_hsl = (has_hsl_sat || has_hsl_hue || has_hsl_lum) && !em_pb;

    if (has_hsl) {
        // Normalize RGB to 0-1 range
        let r_norm = r / 255.0;
        let g_norm = g / 255.0;
        let b_norm = b / 255.0;
        
        let max_c = max(max(r_norm, g_norm), b_norm);
        let min_c = min(min(r_norm, g_norm), b_norm);
        let delta = max_c - min_c;
        
        // Calculate hue (0-360 degrees)
        var hue: f32 = 0.0;
        if (delta != 0.0) {
            if (max_c == r_norm) {
                hue = 60.0 * (((g_norm - b_norm) / delta) % 6.0);
            } else if (max_c == g_norm) {
                hue = 60.0 * (((b_norm - r_norm) / delta) + 2.0);
            } else {
                hue = 60.0 * (((r_norm - g_norm) / delta) + 4.0);
            }
        }
        if (hue < 0.0) { hue += 360.0; }
        
        // Calculate lightness and saturation
        let lightness = (max_c + min_c) / 2.0;
        var sat: f32 = 0.0;
        if (delta != 0.0) {
            sat = delta / (1.0 - abs(2.0 * lightness - 1.0));
        }
        
        // The three adjustments share ONE band weight, computed from the
        // ORIGINAL hue. Two reasons, and both are bugs if ignored:
        //
        // 1. Sharing: a pixel that is 70% "red" must get 70% of red's hue, sat
        //    AND luminance. Weighing them apart lets the three disagree on how
        //    red the same pixel is.
        // 2. Original hue: rotating the hue moves the pixel into another band.
        //    Recomputing the weight after the shift would feed the adjustment
        //    back into itself — a small rotation would cascade.
        var sat_adjustment: f32 = 0.0;
        var hue_adjustment: f32 = 0.0;
        var lum_adjustment: f32 = 0.0;

        // Red (wraps around 0): 345-360, 0-15
        if (hue >= 345.0 || hue < 15.0) {
            var dist_hue: f32;
            if (hue >= 345.0) { dist_hue = hue - 360.0; } else { dist_hue = hue; }
            let weight = 1.0 - min(abs(dist_hue) / 15.0, 1.0);
            sat_adjustment += params.hsl_red_sat * weight;
            hue_adjustment += params.hsl_red_hue * weight;
            lum_adjustment += params.hsl_red_lum * weight;
        }
        // Orange: 15-45
        if (hue >= 15.0 && hue < 45.0) {
            let weight = 1.0 - min(abs(hue - 30.0) / 15.0, 1.0);
            sat_adjustment += params.hsl_orange_sat * weight;
            hue_adjustment += params.hsl_orange_hue * weight;
            lum_adjustment += params.hsl_orange_lum * weight;
        }
        // Yellow: 45-75
        if (hue >= 45.0 && hue < 75.0) {
            let weight = 1.0 - min(abs(hue - 60.0) / 15.0, 1.0);
            sat_adjustment += params.hsl_yellow_sat * weight;
            hue_adjustment += params.hsl_yellow_hue * weight;
            lum_adjustment += params.hsl_yellow_lum * weight;
        }
        // Green: 75-165
        if (hue >= 75.0 && hue < 165.0) {
            let weight = 1.0 - min(abs(hue - 120.0) / 45.0, 1.0);
            sat_adjustment += params.hsl_green_sat * weight;
            hue_adjustment += params.hsl_green_hue * weight;
            lum_adjustment += params.hsl_green_lum * weight;
        }
        // Aqua: 165-210
        if (hue >= 165.0 && hue < 210.0) {
            let weight = 1.0 - min(abs(hue - 187.5) / 22.5, 1.0);
            sat_adjustment += params.hsl_aqua_sat * weight;
            hue_adjustment += params.hsl_aqua_hue * weight;
            lum_adjustment += params.hsl_aqua_lum * weight;
        }
        // Blue: 210-270
        if (hue >= 210.0 && hue < 270.0) {
            let weight = 1.0 - min(abs(hue - 240.0) / 30.0, 1.0);
            sat_adjustment += params.hsl_blue_sat * weight;
            hue_adjustment += params.hsl_blue_hue * weight;
            lum_adjustment += params.hsl_blue_lum * weight;
        }
        // Purple: 270-310
        if (hue >= 270.0 && hue < 310.0) {
            let weight = 1.0 - min(abs(hue - 290.0) / 20.0, 1.0);
            sat_adjustment += params.hsl_purple_sat * weight;
            hue_adjustment += params.hsl_purple_hue * weight;
            lum_adjustment += params.hsl_purple_lum * weight;
        }
        // Magenta: 310-345
        if (hue >= 310.0 && hue < 345.0) {
            let weight = 1.0 - min(abs(hue - 327.5) / 17.5, 1.0);
            sat_adjustment += params.hsl_magenta_sat * weight;
            hue_adjustment += params.hsl_magenta_hue * weight;
            lum_adjustment += params.hsl_magenta_lum * weight;
        }

        sat_adjustment *= 0.01; // -100..100 -> -1..1
        lum_adjustment *= 0.01; // -100..100 -> -1..1
        // hue_adjustment is already in degrees: the slider goes -180..180,
        // because hue is a circle. It is NOT scaled.

        // 🚨 A gray pixel has NO hue: delta == 0 gives hue == 0, which lands in
        // the red band with weight 1.0. Harmless for saturation (sat is 0, so
        // `adj * sat` is 0), but luminance would brighten EVERY gray in the
        // photo — "HSL / luminance — Red" would silently become a global
        // brightness slider.
        //
        // The gate is the pixel's own saturation, and not `delta != 0.0`,
        // because a hard cutoff puts a visible step in any gradient that fades
        // to gray. Saturation goes to zero smoothly as the color does.
        let color_strength = clamp(sat, 0.0, 1.0);
        let effective_hue = hue_adjustment * color_strength;
        let effective_lum = lum_adjustment * color_strength;

        if (sat_adjustment != 0.0 || effective_hue != 0.0 || effective_lum != 0.0) {
            let new_sat = clamp(sat + sat_adjustment * sat, 0.0, 1.0);

            // Hue wraps: 350 + 20 is 10, not 370.
            var new_hue = hue + effective_hue;
            new_hue = new_hue - floor(new_hue / 360.0) * 360.0;

            // Luminance moves toward white or toward black, proportionally, so
            // it never clips: a pixel already at 1.0 cannot be brightened past
            // it, and the curve stays symmetric around the current value.
            var new_lightness = lightness;
            if (effective_lum > 0.0) {
                new_lightness = lightness + effective_lum * (1.0 - lightness);
            } else if (effective_lum < 0.0) {
                new_lightness = lightness * (1.0 + effective_lum);
            }
            new_lightness = clamp(new_lightness, 0.0, 1.0);

            // HSL to RGB conversion
            let c = (1.0 - abs(2.0 * new_lightness - 1.0)) * new_sat;
            let x = c * (1.0 - abs((new_hue / 60.0) % 2.0 - 1.0));
            let m = new_lightness - c / 2.0;

            var r1: f32 = 0.0;
            var g1: f32 = 0.0;
            var b1: f32 = 0.0;

            if (new_hue < 60.0) {
                r1 = c; g1 = x; b1 = 0.0;
            } else if (new_hue < 120.0) {
                r1 = x; g1 = c; b1 = 0.0;
            } else if (new_hue < 180.0) {
                r1 = 0.0; g1 = c; b1 = x;
            } else if (new_hue < 240.0) {
                r1 = 0.0; g1 = x; b1 = c;
            } else if (new_hue < 300.0) {
                r1 = x; g1 = 0.0; b1 = c;
            } else {
                r1 = c; g1 = 0.0; b1 = x;
            }

            r = (r1 + m) * 255.0;
            g = (g1 + m) * 255.0;
            b = (b1 + m) * 255.0;
        }
    }
    
    // Mixer de preto e branco — depois do HSL, antes da tonalização.
    //
    // 🔑 **É esta posição que faz a sépia funcionar.** A tonalização vem logo
    // abaixo justamente para recolorir uma foto já sem cor (ver o comentário
    // dela); converter para P&B aqui põe o mixer no mesmo lugar em que o
    // Lightroom o põe — depois da mistura de cor, antes do virador.
    //
    // 🚨 **A luminância não é um peso fixo.** É a `luminancia()` do motor
    // corrigida por faixa de matiz (`mistura_pb`), e é isso que separa um P&B
    // de retrato de um cinza chapado: escurecer o azul do céu sem levar a pele
    // junto é literalmente o que o operador pede a este controle.
    if (params.bw_ativo != 0.0) {
        let cor = clamp(
            vec3<f32>(r, g, b),
            vec3<f32>(0.0, 0.0, 0.0),
            vec3<f32>(255.0, 255.0, 255.0),
        );
        let hsv = para_hsv(cor / 255.0);
        // Cinza não tem matiz: sem saturação não há faixa a que pertencer, e o
        // ganho seria o do vermelho por acidente do `para_hsv`.
        let ganho = select(0.0, mistura_pb(hsv.x), hsv.y > 0.0005);
        // O slider da Adobe vai de -100 a +100 e clareia/escurece a faixa; o
        // peso pela saturação evita degrau entre um pixel quase cinza e o
        // vizinho colorido, que é o mesmo cuidado do tom por região.
        let y = clamp(luminancia(cor) * (1.0 + ganho * 0.01 * hsv.y), 0.0, 255.0);
        r = y;
        g = y;
        b = y;
    }

    // Tonalização — a cor das sombras e a das altas luzes, separadas.
    //
    // 🔑 **Vem depois da saturação, e é essa posição que a torna útil.**
    // Temperatura e matiz (3 e 4) agem sobre a foto ainda colorida, e o que vier
    // depois de `saturation = -1.0` já perdeu a cor: até 2026-09-06 não havia
    // como pintar de sépia uma foto em preto e branco — o preset "Sépia" do site
    // segurava a saturação em -0,82 justamente para sobrar cor que a temperatura
    // pudesse aquecer, e o resultado era uma foto meio colorida, não uma sépia.
    //
    // A conta é a do quarto escuro: cada ponta da escala de tons ganha um matiz
    // próprio, e cada pixel recebe uma mistura das duas conforme o quanto ele é
    // sombra ou luz. Sépia é uma ponta só — âmbar nas sombras, saturação alta —
    // sobre uma foto já dessaturada.
    //
    // 🚨 **A cor entra recolhida para 0–255, e isso não é zelo: é a correção da
    // mancha de 2026-09-06.** Contraste, nitidez e as curvas de tom entregam
    // valores fora da faixa — a nitidez por desenho, que sobressalto e
    // subsalto na borda é o que ela é — e até aqui isso nunca importou, porque
    // o `clamp` do fim recolhia tudo. `tonalizar` divide pela luminância da
    // mistura, e com luminância de ENTRADA negativa essa divisão troca o sinal
    // dos três canais: o pixel sai em dezenas de milhares e o `clamp` final o
    // deposita num canto puro da roda de cor. Daí o respingo magenta em área
    // escura e ao redor de borda forte, no meio de um degradê liso.
    //
    // Com a entrada em 0–255 o denominador é `(1-f)·luminância + f·luminância
    // do matiz`, soma de dois termos não-negativos com um deles positivo
    // sempre que `f > 0`: não cruza o zero, não inverte, e o fator fica
    // limitado. Quem prende é `a_tonalizacao_nao_mancha_o_que_veio_fora_da_faixa`.
    let tem_cor_na_viragem = params.split_shadow_sat != 0.0 || params.split_highlight_sat != 0.0 || params.split_midtone_sat != 0.0 || params.split_global_sat != 0.0;
    let tem_luz_na_viragem = params.split_shadow_lum != 0.0 || params.split_midtone_lum != 0.0 || params.split_highlight_lum != 0.0 || params.split_global_lum != 0.0;
    if ((tem_cor_na_viragem || tem_luz_na_viragem) && params.processo < 0.5) {
        let virada = viragem(vec3<f32>(r, g, b));
        r = virada.r;
        g = virada.g;
        b = virada.b;
    }
    // No processo 1 a Luminância fica aqui, antes das vinhetas, e a cor vai
    // para depois delas (ver `luminancia_da_viragem`).
    if (tem_luz_na_viragem && params.processo >= 0.5) {
        let cor = clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(255.0));
        let clareada = luminancia_da_viragem(cor, (cor.r + cor.g + cor.b) / 765.0);
        r = clareada.r;
        g = clareada.g;
        b = clareada.b;
    }

    // Lens vignetting — last, and on purpose.
    //
    // 🔑 It is the only adjustment that depends on WHERE the pixel is instead of
    // WHAT color it is. Running it before the tone and color work would feed the
    // darkened corners into highlights, shadows and HSL — the band a pixel falls
    // into would depend on its position in the frame, and two pixels of the same
    // color would be treated as different colors.
    //
    // 🔑 **Medida no quadro do arquivo que sai, e não no da foto revelada**
    // (`no_quadro`). É a vinheta *post-crop* do Lightroom — o `PostCropVignette`
    // que os presets trazem —, e pós-corte quer dizer relativa ao recorte: numa
    // foto 3:2 recortada em 3:4, medi-la na foto inteira deixava as laterais do
    // recorte limpas (dono, 2026-09-13). Sem enquadramento o quadro é a
    // identidade, e a conta é a de antes.
    if (params.lens_vignette_amount != 0.0) {
        let centro = tamanho_do_quadro() * 0.5;
        let aqui = no_quadro(coord);
        // 1.0 at the corner, 0.0 at the center.
        let distancia = length(aqui - centro) / max(length(centro), 1.0);

        // The midpoint is where the falloff STARTS: at 0 it starts at the very
        // center, at 100 there is nothing left to fall off over.
        let meio = clamp(params.lens_vignette_midpoint * 0.01, 0.0, 1.0);
        let t = clamp((distancia - meio) / max(1.0 - meio, 0.001), 0.0, 1.0);

        // Squared so the corner darkens smoothly instead of showing the ring
        // where the falloff begins.
        let forca = params.lens_vignette_amount * 0.01;
        let fator = 1.0 + forca * t * t;

        r *= fator;
        g *= fator;
        b *= fator;
    }

    // Vinheta pós-corte — a do painel Efeitos do Lightroom, no quadro que sai.
    //
    // 🔑 **A forma é uma superelipse.** No Arredondamento 0 é a elipse do
    // quadro; para +100 ela vira círculo; para −100 vira um retângulo de cantos
    // redondos que segue as bordas por igual (a faixa tem a mesma largura em
    // pixels nos quatro lados, como na do Lightroom). O Ponto médio leva a
    // linha de força total da borda (0) até o canto (100); a Difusão abre a
    // transição para dentro. Calibrada contra a prévia do Lightroom de um DNG
    // do Estúdio Canela (sobreposição branca, arredondamento −83, difusão 35).
    if (params.pcv_amount != 0.0) {
        let tam = tamanho_do_quadro();
        let c = tam * 0.5;
        let rel = abs(no_quadro(coord) - c);
        let curto = max(min(c.x, c.y), 1.0);
        let elipse = rel / max(c, vec2<f32>(1.0));
        let arredondamento = clamp(params.pcv_roundness * 0.01, -1.0, 1.0);
        var q = elipse;
        var expoente = 2.0;
        var canto_q = vec2<f32>(1.0, 1.0);
        if (arredondamento < 0.0) {
            let t = -arredondamento;
            let caixa = max(rel - (c - vec2<f32>(curto)), vec2<f32>(0.0)) / curto;
            q = mix(elipse, caixa, t);
            expoente = 2.0 + 8.0 * t * t;
        } else if (arredondamento > 0.0) {
            let circulo = rel / length(c) * 1.41421356;
            q = mix(elipse, circulo, arredondamento);
            canto_q = mix(vec2<f32>(1.0), c / length(c) * 1.41421356, arredondamento);
        }
        let d = pow(pow(q.x, expoente) + pow(q.y, expoente), 1.0 / expoente);
        let canto = pow(pow(canto_q.x, expoente) + pow(canto_q.y, expoente), 1.0 / expoente);

        if (params.processo >= 0.5) {
            // 🔑 **O processo 1: a vinheta medida no Lightroom.** Na foto cinza
            // da régua, a do Lightroom é a elipse inscrita no quadro (d = 1 no
            // meio da borda, √2 no canto) e começa a escurecer a ~40% do raio;
            // com −61 leva o cinza 128 a 29 no canto (a de antes, a 82). O
            // modelo que a régua confirmou: a máscara m(d) depende só do ponto
            // médio e da difusão, e o pixel sai como o efeito inteiro de uma
            // quantidade menor, `F(valor, quantidade × m)` — a mesma máscara em
            // todo nível de cinza. Estilos 1 e 2 são idênticos no Lightroom
            // (até em cor); o 3, sobreposição, tem tabela própria.
            // A distância elíptica, no referencial do Lightroom. Positivo e
            // zero: a superelipse acima, com o canto sempre em √2 (a régua:
            // +100 dá 39 na borda longa e 106 na curta, contra 38 e 106).
            // Negativo: a caixa de faixa igual nos quatro lados, com o
            // expoente e a compressão de `lr_forma_negativa`.
            var d_lr = d / max(canto, 0.001) * 1.41421356;
            if (arredondamento < 0.0) {
                let r = -params.pcv_roundness;
                let nk = lr_forma_negativa(r);
                let caixa = max(rel - (c - vec2<f32>(curto)), vec2<f32>(0.0)) / curto;
                let qn = mix(elipse, caixa, clamp(r / 50.0, 0.0, 1.0));
                let dn = pow(pow(qn.x, nk.x) + pow(qn.y, nk.x), 1.0 / nk.x);
                d_lr = max(1.0 - (1.0 - dn) * nk.y, 0.0);
            }
            let coluna = clamp(d_lr / LR_DISTANCIA_MAXIMA * 255.0, 0.0, 255.0);
            let pm = clamp(params.pcv_midpoint / 12.5, 0.0, 8.0);
            let pf = clamp(params.pcv_feather / 12.5, 0.0, 8.0);
            let im = min(i32(floor(pm)), 7);
            let jf = min(i32(floor(pf)), 7);
            let tm = pm - f32(im);
            let tf = pf - f32(jf);
            let base = LR_LINHA_MASCARA + im * 9 + jf;
            let m = mix(
                mix(lr_curva(base, coluna), lr_curva(base + 1, coluna), tf),
                mix(lr_curva(base + 9, coluna), lr_curva(base + 10, coluna), tf),
                tm,
            );
            let quantidade = clamp(params.pcv_amount, -100.0, 100.0) * m;
            let linha = select(LR_LINHA_VINHETA, LR_LINHA_SOBREPOSICAO, i32(round(params.pcv_style)) == 2);
            let pos = (quantidade + 100.0) / 10.0;
            r = lr_slider(linha, 21, pos, r);
            g = lr_slider(linha, 21, pos, g);
            b = lr_slider(linha, 21, pos, b);
        } else {
        let fim = mix(0.97, canto, clamp(params.pcv_midpoint * 0.01, 0.0, 1.0));
        let largura = max(pow(clamp(params.pcv_feather * 0.01, 0.0, 1.0), 1.5) * 0.5, 0.004);
        let peso = smoothstep(fim - largura * fim, fim, d);
        let forca = clamp(params.pcv_amount * 0.01, -1.0, 1.0) * peso;

        var cor = clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(255.0));
        let estilo = i32(round(params.pcv_style));
        if (estilo == 2) {
            // Sobreposição de tinta: mistura com branco ou preto.
            let tinta = select(vec3<f32>(0.0), vec3<f32>(255.0), forca > 0.0);
            cor = mix(cor, tinta, abs(forca));
        } else if (forca < 0.0) {
            // Escurecer. Prioridade de realces: em luz linear, como uma
            // exposição — e os Realces devolvem o claro que estava ali.
            // Prioridade de cores: no valor com gama, que guarda o matiz.
            let l = clamp(luminancia(cor) / 255.0, 0.0, 1.0);
            let poupar = clamp(params.pcv_highlights * 0.01, 0.0, 1.0) * smoothstep(0.5, 1.0, l);
            let f = 1.0 + forca * (1.0 - poupar);
            if (estilo == 0) {
                cor = vec3<f32>(
                    linear_para_srgb(srgb_para_linear(cor.r) * f),
                    linear_para_srgb(srgb_para_linear(cor.g) * f),
                    linear_para_srgb(srgb_para_linear(cor.b) * f),
                );
            } else {
                cor = cor * f;
            }
        } else {
            // Clarear: em direção ao branco, mais suave que a tinta.
            cor = cor + (vec3<f32>(255.0) - cor) * forca * select(0.8, 0.65, estilo == 1);
        }
        r = cor.r;
        g = cor.g;
        b = cor.b;
        }
    }

    // A vinheta do darktable — depois das duas do Lightroom e antes da cor da
    // viragem, como no darktable (`vignette` antes do `colorbalancergb`): a
    // borda que ela clareia recebe o creme por cima.
    if (params.darktable_vignette_ativo >= 0.5) {
        let vinhetada = vinheta_do_darktable(vec3<f32>(r, g, b), coord);
        r = vinhetada.r;
        g = vinhetada.g;
        b = vinhetada.b;
    }

    // A cor da viragem do processo 1 — depois das vinhetas (ver
    // `cor_da_viragem`).
    if (tem_cor_na_viragem && params.processo >= 0.5) {
        let cor = clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(255.0));
        let virada = cor_da_viragem(cor, (cor.r + cor.g + cor.b) / 765.0);
        r = virada.r;
        g = virada.g;
        b = virada.b;
    }

    // Grão de filme — por último, e depois até da vinheta.
    //
    // 🔑 O grão é da cópia, e não da cena: no filme ele é a prata do negativo,
    // e revelar mais ou menos não o move de lugar. Rodá-lo antes do contraste ou
    // da tonalização faria o ruído passar pelas mesmas curvas da imagem — o grão
    // mudaria de força ao mexer num slider que não é dele.
    //
    // ⚠️ **Ele é monocromático e some nas duas pontas.** O mesmo delta nos três
    // canais é o que dá grão de prata em vez de chuvisco colorido de sensor; e o
    // peso `4·l·(1-l)` tira o ruído do preto fechado e do branco estourado, onde
    // filme nenhum granula e onde o clamp o transformaria em mancha.
    if (params.grain_amount != 0.0) {
        // O tamanho é a aresta da célula em pixels: 0 dá grão fino de um pixel,
        // 100 dá grumo de cinco. Fora dessa faixa não há grão, há mosaico.
        let lado = 1.0 + clamp(params.grain_size, 0.0, 100.0) * 0.04;
        let celula = vec2<u32>(
            u32(f32(coord.x) / lado),
            u32(f32(coord.y) / lado),
        );
        // 🚨 **Os parênteses não são estilo: sem eles a foto fica PRETA no
        // Chrome.** O Tint (o compilador de WGSL do Dawn, que é quem recebe
        // este arquivo quando o navegador tem WebGPU) recusa misturar `*` e `^`
        // sem parênteses — "mixing '*' and '^' requires parenthesis". O naga,
        // que compila para o WebGL2 e roda em todos os testes nativos, aceita a
        // mesma linha. O shader inteiro era rejeitado, o pipeline nascia
        // inválido, e o render pass não escrevia nada: preto, sem erro na tela e
        // com todos os sliders no neutro.
        let semente = embaralhar((celula.x * 0x9e3779b9u) ^ embaralhar(celula.y));
        var ruido = f32(semente) / 4294967295.0 - 0.5;

        // A Aspereza (neutro 50, o de antes): acima, um grão de meia célula se
        // soma ao grão — irregular; abaixo, o grão se mistura ao das vizinhas
        // e alisa.
        if (params.grain_roughness != 50.0) {
            let k = (clamp(params.grain_roughness, 0.0, 100.0) - 50.0) / 50.0;
            if (k > 0.0) {
                let fina = vec2<u32>(u32(f32(coord.x) / max(lado * 0.5, 1.0)), u32(f32(coord.y) / max(lado * 0.5, 1.0)));
                let s2 = embaralhar((fina.x * 0x85ebca6bu) ^ embaralhar(fina.y + 0x27d4eb2fu));
                ruido += k * 0.6 * (f32(s2) / 4294967295.0 - 0.5);
            } else {
                let sx = embaralhar(((celula.x + 1u) * 0x9e3779b9u) ^ embaralhar(celula.y));
                let sy = embaralhar((celula.x * 0x9e3779b9u) ^ embaralhar(celula.y + 1u));
                let vizinhas = (f32(sx) + f32(sy)) / (2.0 * 4294967295.0) - 0.5;
                ruido = mix(ruido, (ruido + vizinhas) * 0.75, -k);
            }
        }

        let l = clamp(((r + g + b) / 3.0) / 255.0, 0.0, 1.0);
        let peso = 4.0 * l * (1.0 - l);
        let delta = ruido * clamp(params.grain_amount * 0.01, 0.0, 1.0) * 64.0 * peso;
        r += delta;
        g += delta;
        b += delta;
    }

    // A faixa 0–255 de volta para 0,0–1,0 — e nada de `NaN` chegando à tela.
    r = na_faixa(r) / 255.0;
    g = na_faixa(g) / 255.0;
    b = na_faixa(b) / 255.0;

    return vec4<f32>(r, g, b, a);
}

/// Prende o valor em 0–255 — **inclusive quando ele não é um número**.
///
/// # 🚨 `clamp(NaN, 0, 255)` não é preto em lugar nenhum, e é outra coisa em
/// cada GPU
///
/// O WGSL define `clamp(x, lo, hi)` como `min(max(x, lo), hi)` e deixa o caso
/// do `NaN` **indeterminado**: uma placa devolve `lo`, outra `hi`, outra o
/// próprio `NaN` — que vai para a textura e aparece como mancha colorida. É o
/// que o dono viu em 18/set/2026, *"o preto manchado de roxo, de forma
/// aleatória"*, numa máquina que não é a de desenvolvimento: o mesmo binário,
/// a mesma foto e a mesma receita, com um desfecho por hardware.
///
/// Aqui não há indeterminação: `NaN` falha em **toda** comparação, então ele
/// não entra em nenhum dos dois ramos e cai no `0.0` do fim. `+∞` vira 255,
/// `-∞` vira 0, e todo número normal atravessa igual.
///
/// ⚠️ **Isto é a rede, e não o conserto.** Um `NaN` aqui quer dizer que alguma
/// conta lá atrás dividiu por zero ou elevou um negativo — e o lugar de achar
/// isso é `nenhum_efeito_mancha_um_preto_chapado`, em `motor.rs`, que varre os
/// 171 ajustes. A rede existe porque o desfecho sem ela é o pior possível: a
/// foto do cliente com uma mancha roxa, na máquina do balcão, e nada no log.
fn na_faixa(x: f32) -> f32 {
    if (x >= 255.0) {
        return 255.0;
    }
    if (x > 0.0) {
        return x;
    }
    return 0.0;
}
