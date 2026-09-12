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
    // 🔑 Enchimento, e não campo: o WebGL2 (`DownlevelFlags::BUFFER_BINDINGS_NOT_16_BYTE_ALIGNED`
    // ausente) exige que o tipo do uniform tenha tamanho múltiplo de 16, e 110
    // `f32` dão 440. O Rust continua mandando 440 bytes num buffer de 448
    // (`TAMANHO_DO_UNIFORM`); estes dois nunca são lidos. Ficam DEPOIS dos 110
    // para não deslocar nenhuma posição — e o teste que compara os nomes com o
    // `Ajustes` ignora o que começa com `_`.
    _enchimento_a: f32,
    _enchimento_b: f32,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(2) var<uniform> params: Params;

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

/// Puxa a cor na direção de um matiz **sem mudar o brilho do pixel**.
///
/// 🔑 A recuperação da luminância no fim é o que separa tonalizar de manchar:
/// misturar com âmbar puro escureceria o azul e clarearia o amarelo, e o
/// resultado seria uma foto com o contraste redesenhado pela escolha da cor. Do
/// jeito que está, `forca = 1.0` num cinza dá o matiz puro naquele mesmo nível
/// de cinza — que é exatamente o que uma sépia é.
///
/// ⚠️ O preto puro fica preto: não há brilho que uma cor possa ter e continuar
/// preto, e a divisão protegida devolve zero em vez de explodir.
fn tonalizar(cor: vec3<f32>, matiz: f32, forca: f32) -> vec3<f32> {
    let alvo = cor_do_matiz(matiz) * 255.0;
    let antes = luminancia(cor);
    let misturado = mix(cor, alvo, forca);
    return misturado * (antes / max(luminancia(misturado), 0.0001));
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
        let sigma_r = max(params.nr_luminance * 0.002, 0.0001); // NR Range
        let sigma_color = max(params.nr_color * 0.05, 0.1); // Color NR
        let sigma_sharpen = max(params.sharpen_radius, 0.5); // Sharpen Radius
        
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
            let detail = final_y - blurred_y;
            let amount = params.sharpen_amount * 0.05; // Scale 0-100 to approx 0-5
            
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

    if (params.exposure != 0.0) {
        let factor = pow(2.0, params.exposure);
        r *= factor;
        g *= factor;
        b *= factor;
    }
    
    // 2. Contrast
    if (params.contrast != 1.0) {
        r = (r - 128.0) * params.contrast + 128.0;
        g = (g - 128.0) * params.contrast + 128.0;
        b = (b - 128.0) * params.contrast + 128.0;
    }
    
    // 3. Temperature (warm/cool balance)
    if (params.temperature != 0.0) {
        r += params.temperature * 10.0;
        b -= params.temperature * 10.0;
    }
    
    // 4. Tint (green/magenta balance)
    if (params.tint != 0.0) {
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
    if (params.highlights != 0.0 || params.shadows != 0.0
        || params.whites != 0.0 || params.blacks != 0.0) {
        let l = clamp(((r + g + b) / 3.0) / 255.0, 0.0, 1.0);

        // 🔑 Uma de cada vez, cada uma sobre o resultado da anterior. A soma
        // dos quatro deltas seria mais curta e NÃO serviria: as garantias acima
        // valem para cada curva sozinha, e somadas elas se cancelam — "sombras"
        // em -100 com "pretos" em +100 devolveria derivada -1 no preto, que é a
        // inversão de volta. Compostas, a garantia de cada uma basta.
        var n = clamp(l + (params.whites * 0.01) * l * l * l / 3.0, 0.0, 1.0);
        n = clamp(n + (params.highlights * 0.01) * n * n * (1.0 - n), 0.0, 1.0);
        n = clamp(n + (params.shadows * 0.01) * n * (1.0 - n) * (1.0 - n), 0.0, 1.0);
        let e = 1.0 - n;
        n = clamp(n + (params.blacks * 0.01) * e * e * e / 3.0, 0.0, 1.0);

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
    
    // 9. Saturation (overall color intensity)
    if (params.saturation != 0.0) {
        let factor = 1.0 + params.saturation;
        r = lum2 + (r - lum2) * factor;
        g = lum2 + (g - lum2) * factor;
        b = lum2 + (b - lum2) * factor;
    }
    
    // 10. Vibrance (intelligent saturation - affects muted colors more)
    if (params.vibrance != 0.0) {
        let max_diff = max(max(abs(r - lum2), abs(g - lum2)), abs(b - lum2));
        if (max_diff < 64.0) {
            let factor = 1.0 + params.vibrance * 2.0;
            r = lum2 + (r - lum2) * factor;
            g = lum2 + (g - lum2) * factor;
            b = lum2 + (b - lum2) * factor;
        }
    }
    
    // 11. Clarity (local contrast - simplified)
    if (params.clarity != 0.0) {
        let factor = 1.0 + (params.clarity * 0.5);
        let lum3 = (r + g + b) / 3.0;
        r = lum3 + (r - lum3) * factor;
        g = lum3 + (g - lum3) * factor;
        b = lum3 + (b - lum3) * factor;
    }
    
    // 12-15. Tone Curve Parametric Zones
    let lum_final = (r + g + b) / 3.0;
    let norm_lum = lum_final / 255.0;
    
    // Zone 1: Shadows (0.0 - 0.25 range)
    if (params.tone_curve_shadows != 0.0) {
        let zone_center = 0.125;
        let zone_width = 0.25;
        let dist = abs(norm_lum - zone_center);
        if (dist < zone_width) {
            let weight = 1.0 - (dist / zone_width);
            let adjustment = params.tone_curve_shadows * 0.01 * weight;
            r += r * adjustment;
            g += g * adjustment;
            b += b * adjustment;
        }
    }
    
    // Zone 2: Darks (0.25 - 0.5 range)
    if (params.tone_curve_darks != 0.0) {
        let zone_center = 0.375;
        let zone_width = 0.25;
        let dist = abs(norm_lum - zone_center);
        if (dist < zone_width) {
            let weight = 1.0 - (dist / zone_width);
            let adjustment = params.tone_curve_darks * 0.01 * weight;
            r += r * adjustment;
            g += g * adjustment;
            b += b * adjustment;
        }
    }
    
    // Zone 3: Lights (0.5 - 0.75 range)
    if (params.tone_curve_lights != 0.0) {
        let zone_center = 0.625;
        let zone_width = 0.25;
        let dist = abs(norm_lum - zone_center);
        if (dist < zone_width) {
            let weight = 1.0 - (dist / zone_width);
            let adjustment = params.tone_curve_lights * 0.01 * weight;
            r += r * adjustment;
            g += g * adjustment;
            b += b * adjustment;
        }
    }
    
    // Zone 4: Highlights (0.75 - 1.0 range)
    if (params.tone_curve_highlights != 0.0) {
        let zone_center = 0.875;
        let zone_width = 0.25;
        let dist = abs(norm_lum - zone_center);
        if (dist < zone_width) {
            let weight = 1.0 - (dist / zone_width);
            let adjustment = params.tone_curve_highlights * 0.01 * weight;
            r += r * adjustment;
            g += g * adjustment;
            b += b * adjustment;
        }
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
    let has_hsl = has_hsl_sat || has_hsl_hue || has_hsl_lum;

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
    if (params.split_shadow_sat != 0.0 || params.split_highlight_sat != 0.0
        || params.split_midtone_sat != 0.0 || params.split_global_sat != 0.0) {
        var cor = clamp(
            vec3<f32>(r, g, b),
            vec3<f32>(0.0, 0.0, 0.0),
            vec3<f32>(255.0, 255.0, 255.0),
        );
        let l = ((cor.r + cor.g + cor.b) / 3.0) / 255.0;

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

        cor = tonalizar(
            cor,
            params.split_shadow_hue,
            clamp(params.split_shadow_sat * 0.01, 0.0, 1.0) * peso_baixa,
        );
        cor = tonalizar(
            cor,
            params.split_midtone_hue,
            clamp(params.split_midtone_sat * 0.01, 0.0, 1.0) * peso_meio,
        );
        cor = tonalizar(
            cor,
            params.split_highlight_hue,
            clamp(params.split_highlight_sat * 0.01, 0.0, 1.0) * peso_alta,
        );
        // 🔑 O global não tem peso de faixa — é a tinta que cai na foto
        // inteira, e entra por último para tingir também o que as três faixas
        // acabaram de fazer.
        cor = tonalizar(
            cor,
            params.split_global_hue,
            clamp(params.split_global_sat * 0.01, 0.0, 1.0),
        );
        r = cor.r;
        g = cor.g;
        b = cor.b;
    }

    // Lens vignetting — last, and on purpose.
    //
    // 🔑 It is the only adjustment that depends on WHERE the pixel is instead of
    // WHAT color it is. Running it before the tone and color work would feed the
    // darkened corners into highlights, shadows and HSL — the band a pixel falls
    // into would depend on its position in the frame, and two pixels of the same
    // color would be treated as different colors.
    if (params.lens_vignette_amount != 0.0) {
        let centro = vec2<f32>(f32(dims.x), f32(dims.y)) * 0.5;
        let aqui = vec2<f32>(f32(coord.x), f32(coord.y));
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
        let ruido = f32(semente) / 4294967295.0 - 0.5;

        let l = clamp(((r + g + b) / 3.0) / 255.0, 0.0, 1.0);
        let peso = 4.0 * l * (1.0 - l);
        let delta = ruido * clamp(params.grain_amount * 0.01, 0.0, 1.0) * 64.0 * peso;
        r += delta;
        g += delta;
        b += delta;
    }

    // Clamp values to 0-255 and convert back to 0.0-1.0
    r = clamp(r, 0.0, 255.0) / 255.0;
    g = clamp(g, 0.0, 255.0) / 255.0;
    b = clamp(b, 0.0, 255.0) / 255.0;
    
    return vec4<f32>(r, g, b, a);
}
