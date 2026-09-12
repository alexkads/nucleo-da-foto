// O estágio darktable da revelação: os módulos do darktable 5.6.1, por pixel.
//
// 🔑 **O gabarito é `src/darktable.rs`**, medido contra o darktable-cli 5.6.1
// (máximo de 1 nível na carta de teste; média de 0,15 numa foto de 24 MP). Cada
// função aqui é a tradução dele, na mesma ordem de operações, e os testes
// `o_estagio_darktable_*` de `motor.rs` comparam os dois pixel a pixel.
//
// O espaço de trabalho é o do darktable: RGB linear Rec.2020, branco D50. As
// matrizes e a tabela de gamut vêm de `darktable_constantes.wgsl`, gerado a
// partir do Rust.
//
// ⚠️ **O WGSL não define `pow(0, 0)`**, e o C devolve 1. Toda potência cujo
// expoente pode ser zero passa por `dt_pow`; as outras têm base ou expoente
// positivos por construção, e o comentário de cada uma diz por quê.

const DT_PI: f32 = 3.14159265358979;
const DT_FLT_MIN: f32 = 1.17549435e-38;
const DT_UCS_L_STAR_RANGE: f32 = 2.098883786377;
const DT_UCS_L_STAR_UPPER_LIMIT: f32 = 2.09885;
const DT_D65_X: f32 = 0.31271;
const DT_D65_Y: f32 = 0.32902;
const DT_ANGLE_SHIFT: f32 = -30.0;

fn dt_mul3(m: array<vec3<f32>, 3>, v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(m[0], v), dot(m[1], v), dot(m[2], v));
}

/// `powf` do C para base não-negativa: expoente zero dá 1.
fn dt_pow(x: f32, y: f32) -> f32 {
    if (y == 0.0) {
        return 1.0;
    }
    return pow(x, y);
}

// ------------------------------------------------------------------ tubulação

/// A curva do sRGB (a paramétrica tipo 4 do lcms, `colorspaces.c:405`).
fn dt_srgb_para_linear(v255: f32) -> f32 {
    let x = v255 / 255.0;
    if (x <= 0.04045) {
        return x / 12.92;
    }
    return pow((x + 0.055) / 1.055, 2.4);
}

fn dt_linear_para_srgb(v: f32) -> f32 {
    let x = max(v, 0.0);
    if (x <= 0.0031308) {
        return 12.92 * x * 255.0;
    }
    return (1.055 * pow(x, 1.0 / 2.4) - 0.055) * 255.0;
}

// ------------------------------------------------------------------- exposure

/// `exposure.c:44, 508, 562`.
fn dt_exposure(t: vec3<f32>) -> vec3<f32> {
    let white = pow(2.0, -params.dt_exposure_exposure);
    let scale = 1.0 / (white - params.dt_exposure_black);
    return (t - vec3<f32>(params.dt_exposure_black)) * scale;
}

// ------------------------------------------------------------------- vignette

/// `vignette.c:694–841`, sem pontilhamento. Brilho positivo SOMA.
fn dt_vignette(t: vec3<f32>, coord: vec2<u32>, dims: vec2<u32>) -> vec3<f32> {
    let w = f32(dims.x);
    let h = f32(dims.y);
    let centro = vec2<f32>(
        w * 0.5 + params.dt_vignette_center_x * w / 2.0,
        h * 0.5 + params.dt_vignette_center_y * h / 2.0,
    );
    var xscale = 2.0 / w;
    var yscale = 2.0 / h;
    if (params.dt_vignette_autoratio == 0.0) {
        let base = 2.0 / max(w, h);
        if (params.dt_vignette_whratio <= 1.0) {
            xscale = base / params.dt_vignette_whratio;
            yscale = base;
        } else {
            xscale = base;
            yscale = base / (2.0 - params.dt_vignette_whratio);
        }
    }
    let dscale = params.dt_vignette_scale / 100.0;
    let min_falloff = 100.0 / min(w, h);
    let fscale = max(params.dt_vignette_falloff_scale, min_falloff) / 100.0;
    let forma = max(params.dt_vignette_shape, 0.001);
    let exp1 = 2.0 / forma;
    let exp2 = forma / 2.0;
    let pv = vec2<f32>(
        abs(f32(coord.x) * xscale - centro.x * xscale),
        abs(f32(coord.y) * yscale - centro.y * yscale),
    );
    // Bases e expoentes positivos: `pv` é valor absoluto e `exp1`, `exp2` > 0.
    let cplen = pow(pow(pv.x, exp1) + pow(pv.y, exp1), exp2);
    var peso = 0.0;
    if (cplen >= dscale) {
        peso = clamp((cplen - dscale) / fscale, 0.0, 1.0);
    }
    if (peso <= 0.0) {
        return t;
    }
    var col = t;
    if (params.dt_vignette_brightness < 0.0) {
        col = col * (1.0 + peso * params.dt_vignette_brightness);
    } else {
        col = col + vec3<f32>(peso * params.dt_vignette_brightness);
    }
    if (params.dt_vignette_unbound == 0.0) {
        col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    let mv = (col.r + col.g + col.b) / 3.0;
    col = col - (vec3<f32>(mv) - col) * (peso * params.dt_vignette_saturation);
    if (params.dt_vignette_unbound == 0.0) {
        col = clamp(col, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return col;
}

// ------------------------------------------------------------ color balance rgb

fn dt_lms_para_yrg(lms: vec3<f32>) -> vec3<f32> {
    let y = 0.68990272 * lms.x + 0.34832189 * lms.y;
    let a = lms.x + lms.y + lms.z;
    var n = vec3<f32>(0.0);
    if (a != 0.0) {
        n = lms / a;
    }
    let rgb = dt_mul3(DT_LMS_PARA_FILMLIGHT, n);
    return vec3<f32>(y, rgb.x, rgb.y);
}

fn dt_yrg_para_lms(yrg: vec3<f32>) -> vec3<f32> {
    let lms = dt_mul3(DT_FILMLIGHT_PARA_LMS, vec3<f32>(yrg.y, yrg.z, 1.0 - yrg.y - yrg.z));
    let den = 0.68990272 * lms.x + 0.34832189 * lms.y;
    var a = 0.0;
    if (den != 0.0) {
        a = yrg.x / den;
    }
    return lms * a;
}

/// `(Y, c, cos h, sin h)`.
fn dt_yrg_para_ych(yrg: vec3<f32>) -> vec4<f32> {
    let r = yrg.y - 0.21902143;
    let g = yrg.z - 0.54371398;
    let c = sqrt(g * g + r * r);
    if (c != 0.0) {
        return vec4<f32>(yrg.x, c, r / c, g / c);
    }
    return vec4<f32>(yrg.x, c, 1.0, 0.0);
}

fn dt_ych_para_yrg(ych: vec4<f32>) -> vec3<f32> {
    return vec3<f32>(ych.x, ych.y * ych.z + 0.21902143, ych.y * ych.w + 0.54371398);
}

fn dt_ych_para_grading(ych: vec4<f32>) -> vec3<f32> {
    return dt_mul3(DT_LMS_PARA_FILMLIGHT, dt_yrg_para_lms(dt_ych_para_yrg(ych)));
}

fn dt_gamut_check_yrg(entrada: vec4<f32>) -> vec4<f32> {
    var ych = entrada;
    let yrg = dt_ych_para_yrg(ych);
    let d65_r = 0.21902143;
    let d65_g = 0.54371398;
    var max_c = ych.y;
    if (yrg.y < 0.0) {
        max_c = min(-d65_r / ych.z, max_c);
    }
    if (yrg.z < 0.0) {
        max_c = min(-d65_g / ych.w, max_c);
    }
    if (yrg.y + yrg.z > 1.0) {
        max_c = min((1.0 - d65_r - d65_g) / (ych.z + ych.w), max_c);
    }
    ych.y = max_c;
    return ych;
}

fn dt_soft_clip(x: f32, suave: f32, duro: f32) -> f32 {
    let norma = duro - suave;
    if (x > suave) {
        return suave + (1.0 - exp(-(x - suave) / norma)) * norma;
    }
    return x;
}

/// Base `y >= 0` (vem de `max(..., 0)`), expoente positivo.
fn dt_y_para_l_star(y: f32) -> f32 {
    let y_hat = pow(y, 0.631651345306265);
    return DT_UCS_L_STAR_RANGE * y_hat / (y_hat + 1.12426773749357);
}

/// `l` está em `[0, DT_UCS_L_STAR_UPPER_LIMIT]`: a base é positiva.
fn dt_l_star_para_y(l: f32) -> f32 {
    return pow(1.12426773749357 * l / (DT_UCS_L_STAR_RANGE - l), 1.5831518565279648);
}

fn dt_divisor_seguro(v: f32) -> f32 {
    if (v >= 0.0) {
        return max(v, DT_FLT_MIN);
    }
    return min(v, -DT_FLT_MIN);
}

fn dt_xyy_para_uv(xyy: vec3<f32>) -> vec2<f32> {
    let uvd = vec3<f32>(
        -0.783941002840055 * xyy.x + 0.277512987809202 * xyy.y + 0.153836578598858,
        0.745273540913283 * xyy.x - 0.205375866083878 * xyy.y - 0.165478376301988,
        0.318707282433486 * xyy.x + 2.16743692732158 * xyy.y + 0.291320554395942,
    );
    let div = dt_divisor_seguro(uvd.z);
    let u = uvd.x / div;
    let v = uvd.y / div;
    let u_estrela = 1.39656225667 * u / (abs(u) + 1.49217352929);
    let v_estrela = 1.4513954287 * v / (abs(v) + 1.52488637914);
    return vec2<f32>(
        -1.124983854323892 * u_estrela - 0.980483721769325 * v_estrela,
        1.86323315098672 * u_estrela + 1.971853092390862 * v_estrela,
    );
}

fn dt_xyy_para_jch(xyy: vec3<f32>, l_white: f32) -> vec3<f32> {
    let uv = dt_xyy_para_uv(xyy);
    let l_star = dt_y_para_l_star(xyy.z);
    let m2 = uv.x * uv.x + uv.y * uv.y;
    // `l_star` e `m2` são não-negativos, e os expoentes positivos.
    return vec3<f32>(
        l_star / l_white,
        15.932993652962535 * pow(l_star, 0.6523997524738018) * pow(m2, 0.6007557017508491) / l_white,
        atan2(uv.y, uv.x),
    );
}

fn dt_jch_para_xyy(jch: vec3<f32>, l_white: f32) -> vec3<f32> {
    let l_star = clamp(jch.x * l_white, 0.0, DT_UCS_L_STAR_UPPER_LIMIT);
    var m = 0.0;
    if (l_star != 0.0) {
        m = pow(jch.y * l_white / (15.932993652962535 * pow(l_star, 0.6523997524738018)), 0.8322850678616855);
    }
    let u = m * cos(jch.z);
    let v = m * sin(jch.z);
    let u_estrela = -5.037522385190711 * u - 2.504856328185843 * v;
    let v_estrela = 4.760029407436461 * u + 2.874012963239247 * v;
    let ux = -1.49217352929 * u_estrela / (abs(u_estrela) - 1.39656225667);
    let vx = -1.52488637914 * v_estrela / (abs(v_estrela) - 1.4513954287);
    let xyd = vec3<f32>(
        0.167171472114775 * ux + 0.141299802443708 * vx - 0.00801531300850582,
        -0.150959086409163 * ux - 0.155185060382272 * vx - 0.00843312433578007,
        0.940254742367256 * ux + 1.0 * vx - 0.0256325967652889,
    );
    let div = dt_divisor_seguro(xyd.z);
    return vec3<f32>(xyd.x / div, xyd.y / div, dt_l_star_para_y(l_star));
}

fn dt_xyz_para_xyy(entrada: vec3<f32>) -> vec3<f32> {
    let xyz = max(entrada, vec3<f32>(0.0));
    let soma = xyz.x + xyz.y + xyz.z;
    if (soma > 0.0) {
        return vec3<f32>(xyz.x / soma, xyz.y / soma, xyz.y);
    }
    return vec3<f32>(DT_D65_X, DT_D65_Y, xyz.y);
}

fn dt_xyy_para_xyz(xyy: vec3<f32>) -> vec3<f32> {
    if (xyy.y == 0.0) {
        return vec3<f32>(0.0);
    }
    return vec3<f32>(xyy.z * xyy.x / xyy.y, xyy.z, xyy.z * (1.0 - xyy.x - xyy.y) / xyy.y);
}

/// `lookup_gamut` (`darktable_ucs_22_helpers.h:132–154`).
fn dt_consultar_gamut(matiz: f32) -> f32 {
    let x = 512.0 * (matiz + DT_PI) / (2.0 * DT_PI);
    let xp = floor(x);
    let xn = ceil(x);
    let xi = u32(i32(xp) & 511);
    let xii = u32(i32(xn) & 511);
    let yp = DT_GAMUT[xi];
    if (xi != xii) {
        return yp + (x - xp) * (DT_GAMUT[xii] - yp);
    }
    return yp;
}

/// `opacity_masks` (`colorbalancergb.c:551–577`): `[opacidades, complementos]`.
fn dt_mascaras(x: f32, sw: f32, hw: f32, mw: f32, fulcro: f32) -> array<vec3<f32>, 2> {
    let off = x - fulcro;
    let norma = off / fulcro;
    let alpha = 1.0 / (1.0 + exp(norma * sw));
    let beta = 1.0 / (1.0 + exp(-norma * hw));
    let ac = 1.0 - alpha;
    let bc = 1.0 - beta;
    let gamma = exp(-(off * off) * mw / 4.0) * ac * ac * bc * bc * 8.0;
    return array<vec3<f32>, 2>(vec3<f32>(alpha, gamma, beta), vec3<f32>(ac, 1.0 - gamma, bc));
}

fn dt_cb_faixa(c: f32, h_graus: f32) -> vec3<f32> {
    let h = radians(h_graus + DT_ANGLE_SHIFT);
    return dt_ych_para_grading(vec4<f32>(1.0, c, cos(h), sin(h)));
}

/// `colorbalancergb.c` — `commit_params` (1087–1168) e `process` (579–901),
/// no ramo de saturação dt UCS.
fn dt_color_balance_rgb(px: vec3<f32>) -> vec3<f32> {
    // commit_params
    let rgb_norm = dt_ych_para_grading(vec4<f32>(1.0, 0.0, 1.0, 0.0));
    let g_faixa = dt_cb_faixa(params.dt_cb_global_c, params.dt_cb_global_h);
    let global = (g_faixa - rgb_norm) + rgb_norm * params.dt_cb_global_y;
    let s_faixa = dt_cb_faixa(params.dt_cb_shadows_c, params.dt_cb_shadows_h);
    let shadows = vec3<f32>(1.0) + (s_faixa - rgb_norm) + vec3<f32>(params.dt_cb_shadows_y);
    let h_faixa = dt_cb_faixa(params.dt_cb_highlights_c, params.dt_cb_highlights_h);
    let highlights = vec3<f32>(1.0) + (h_faixa - rgb_norm) + vec3<f32>(params.dt_cb_highlights_y);
    let m_faixa = dt_cb_faixa(params.dt_cb_midtones_c, params.dt_cb_midtones_h);
    let midtones = vec3<f32>(1.0) / (vec3<f32>(1.0) + (m_faixa - rgb_norm));
    let midtones_y = 1.0 / (1.0 + params.dt_cb_midtones_y);
    let sw = 2.0 + params.dt_cb_shadows_weight * 2.0;
    let hw = 2.0 + params.dt_cb_highlights_weight * 2.0;
    let mw = sw * sw * hw * hw / (sw * sw + hw * hw);
    let white_fulcrum = pow(2.0, params.dt_cb_white_fulcrum);
    let mask_fulcro = pow(params.dt_cb_mask_grey_fulcrum, 0.4101205819200422);
    let contraste = 1.0 + params.dt_cb_contrast;
    let rotacao = radians(params.dt_cb_hue_angle);
    let cos_rot = cos(rotacao);
    let sin_rot = sin(rotacao);
    let l_white = dt_y_para_l_star(white_fulcrum);

    // process
    let rgb = max(px, vec3<f32>(0.0));
    var ych = dt_yrg_para_ych(dt_lms_para_yrg(dt_mul3(DT_CB_ENTRADA, rgb)));
    ych.x = max(ych.x, 0.0);
    let mascara = dt_mascaras(pow(ych.x, 0.4101205819200422), sw, hw, mw, mask_fulcro);
    let op = mascara[0];
    let opc = mascara[1];

    let cos_h = ych.z;
    let sin_h = ych.w;
    ych.z = cos_rot * cos_h - sin_rot * sin_h;
    ych.w = sin_rot * cos_h + cos_rot * sin_h;

    let reforco = params.dt_cb_chroma_global
        + op.x * params.dt_cb_chroma_shadows
        + op.y * params.dt_cb_chroma_midtones
        + op.z * params.dt_cb_chroma_highlights;
    // Com vibração zero o C dá `0 · (1 − c⁰) = 0`; aqui `pow(0, 0)` não existe.
    var vibracao = 0.0;
    if (params.dt_cb_vibrance != 0.0) {
        vibracao = params.dt_cb_vibrance * (1.0 - pow(ych.y, abs(params.dt_cb_vibrance)));
    }
    ych.y = ych.y * max(1.0 + reforco + vibracao, 0.0);
    ych = dt_gamut_check_yrg(ych);

    var cor = dt_mul3(DT_LMS_PARA_FILMLIGHT, dt_yrg_para_lms(dt_ych_para_yrg(ych)));
    cor = cor + global;
    cor = cor * (opc.z * (vec3<f32>(opc.x) + op.x * shadows) + op.z * highlights);
    for (var c = 0; c < 3; c = c + 1) {
        var sinal = 1.0;
        if (cor[c] < 0.0) {
            sinal = -1.0;
        }
        cor[c] = dt_pow(abs(cor[c]) / white_fulcrum, midtones[c]) * sinal * white_fulcrum;
    }

    var yrg = dt_lms_para_yrg(dt_mul3(DT_FILMLIGHT_PARA_LMS, cor));
    yrg.x = dt_pow(max(yrg.x / white_fulcrum, 0.0), midtones_y) * white_fulcrum;
    yrg.x = params.dt_cb_grey_fulcrum * dt_pow(yrg.x / params.dt_cb_grey_fulcrum, contraste);

    let xyz_d65 = dt_mul3(DT_LMS_2006_PARA_XYZ_D65, dt_yrg_para_lms(yrg));

    // Ramo dt UCS (colorbalancergb.c:852–900).
    let jch = dt_xyy_para_jch(dt_xyz_para_xyy(xyz_d65), l_white);
    // `jch.y >= 0`: sai de `pow` de base não-negativa.
    var hcb = vec3<f32>(jch.z, jch.y, jch.x * (pow(jch.y, 1.33654221029386) + 1.0));
    let raio = sqrt(hcb.y * hcb.y + hcb.z * hcb.z);
    var sin_t = 0.0;
    var cos_t = 0.0;
    if (raio > 0.0) {
        sin_t = hcb.y / raio;
        cos_t = hcb.z / raio;
    }
    let pp = max(hcb.y, DT_FLT_MIN);
    let ww = sin_t * hcb.y + cos_t * hcb.z;
    var sat = max(1.0 + params.dt_cb_saturation_global
        + op.x * params.dt_cb_saturation_shadows
        + op.y * params.dt_cb_saturation_midtones
        + op.z * params.dt_cb_saturation_highlights, 0.0);
    let brilho = max(1.0 + params.dt_cb_brilliance_global
        + op.x * params.dt_cb_brilliance_shadows
        + op.y * params.dt_cb_brilliance_midtones
        + op.z * params.dt_cb_brilliance_highlights, 0.0);
    let max_a = sqrt(pp * pp + ww * ww) / pp;
    sat = dt_soft_clip(sat, 0.5 * max_a, max_a);
    let p_linha = (sat - 1.0) * pp;
    let dentro = pp * pp * (1.0 - sat * sat) + ww * ww;
    if (dentro < 0.0) {
        // No C a raiz dá NaN e o `MAX(NaN, 0)` devolve 0 nas duas componentes.
        hcb.y = 0.0;
        hcb.z = 0.0;
    } else {
        let w_linha = sqrt(dentro) * brilho;
        hcb.y = max(cos_t * p_linha + sin_t * w_linha, 0.0);
        hcb.z = max(-sin_t * p_linha + cos_t * w_linha, 0.0);
    }

    var jch2 = vec3<f32>(hcb.z / (pow(hcb.y, 1.33654221029386) + 1.0), hcb.y, hcb.x);
    let max_m2 = dt_consultar_gamut(jch2.z);
    // `jch2.x`, `max_m2` e `max_chroma` são não-negativos.
    let max_chroma = 15.932993652962535 * pow(jch2.x * l_white, 0.6523997524738018)
        * pow(max_m2, 0.6007557017508491) / l_white;
    let borda_b = jch2.x * (pow(max_chroma, 1.33654221029386) + 1.0);
    var borda_s = 0.0;
    if (borda_b > 0.0) {
        borda_s = max_chroma / borda_b;
    }
    var hsb = vec3<f32>(hcb.x, 0.0, hcb.z);
    if (hcb.z > 0.0) {
        hsb.y = hcb.y / hcb.z;
    }
    hsb.y = dt_soft_clip(hsb.y, 0.8 * borda_s, borda_s);
    var jch3 = vec3<f32>(0.0, hsb.y * hsb.z, hsb.x);
    jch3.x = hsb.z / (pow(jch3.y, 1.33654221029386) + 1.0);
    let xyz2 = dt_xyy_para_xyz(dt_jch_para_xyy(jch3, l_white));
    return max(dt_mul3(DT_CB_SAIDA, xyz2), vec3<f32>(0.0));
}

// ------------------------------------------------------------- grades e Lab

/// O tamanho `(size_x, size_y, size_z, 0)` e os sigmas `(σs, σr, 0, 0)` de cada
/// grade bilateral, em múltiplos de 16 bytes como o uniform exige.
struct DadosDasGrades {
    sh_tamanho: vec4<f32>,
    sh_sigma: vec4<f32>,
    mo_tamanho: vec4<f32>,
    mo_sigma: vec4<f32>,
}

@group(0) @binding(3) var dt_grade_shadhi: texture_2d<f32>;
@group(0) @binding(4) var dt_grade_monochrome: texture_2d<f32>;
@group(0) @binding(5) var<uniform> dt_grades: DadosDasGrades;

/// `dt_XYZ_to_Lab` (`colorspaces_inline_conversions.h:148`), branco D50.
fn dt_lab_f(x: f32) -> f32 {
    if (x > 216.0 / 24389.0) {
        return pow(x, 1.0 / 3.0);
    }
    return (24389.0 / 27.0 * x + 16.0) / 116.0;
}

fn dt_lab_f_inv(x: f32) -> f32 {
    if (x > 0.20689655172413796) {
        return x * x * x;
    }
    return (116.0 * x - 16.0) / (24389.0 / 27.0);
}

fn dt_para_lab(t: vec3<f32>) -> vec3<f32> {
    let xyz = dt_mul3(DT_TRABALHO_PARA_XYZ_D50, t);
    let fx = dt_lab_f(xyz.x / 0.9642);
    let fy = dt_lab_f(xyz.y);
    let fz = dt_lab_f(xyz.z / 0.8249);
    return vec3<f32>(116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz));
}

fn dt_de_lab(lab: vec3<f32>) -> vec3<f32> {
    let fy = (lab.x + 16.0) / 116.0;
    let fx = fy + lab.y / 500.0;
    let fz = fy - lab.z / 200.0;
    let xyz = vec3<f32>(0.9642 * dt_lab_f_inv(fx), dt_lab_f_inv(fy), 0.8249 * dt_lab_f_inv(fz));
    return dt_mul3(DT_XYZ_D50_PARA_TRABALHO, xyz);
}

/// A célula da grade e os pesos trilineares de um pixel (`bilateral.c:398–439`).
struct CelulaDaGrade {
    xi: i32,
    yi: i32,
    zi: i32,
    sx: i32,
    xf: f32,
    yf: f32,
    zf: f32,
}

fn dt_celula(tamanho: vec4<f32>, sigma: vec4<f32>, coord: vec2<u32>, l: f32) -> CelulaDaGrade {
    let s_inv = 1.0 / sigma.x;
    let r_inv = 1.0 / sigma.y;
    let x = clamp(f32(coord.x) * s_inv, 0.0, tamanho.x - 1.0);
    let y = clamp(f32(coord.y) * s_inv, 0.0, tamanho.y - 1.0);
    let z = clamp(l * r_inv, 0.0, tamanho.z - 1.0);
    let xi = min(i32(x), i32(tamanho.x) - 2);
    let yi = min(i32(y), i32(tamanho.y) - 2);
    let zi = min(i32(z), i32(tamanho.z) - 2);
    return CelulaDaGrade(xi, yi, zi, i32(tamanho.x), x - f32(xi), y - f32(yi), z - f32(zi));
}

/// O texel `(x + z·size_x, y)` do atlas é a célula `(x, y, z)`.
fn dt_texel_sh(c: CelulaDaGrade, dx: i32, dy: i32, dz: i32) -> f32 {
    return textureLoad(dt_grade_shadhi, vec2<i32>(c.xi + dx + (c.zi + dz) * c.sx, c.yi + dy), 0).r;
}

fn dt_texel_mo(c: CelulaDaGrade, dx: i32, dy: i32, dz: i32) -> f32 {
    return textureLoad(dt_grade_monochrome, vec2<i32>(c.xi + dx + (c.zi + dz) * c.sx, c.yi + dy), 0).r;
}

/// Os oito pesos na ordem do `dt_bilateral_slice`.
fn dt_trilinear(c: CelulaDaGrade, t000: f32, t100: f32, t010: f32, t110: f32, t001: f32, t101: f32, t011: f32, t111: f32) -> f32 {
    return t000 * (1.0 - c.xf) * (1.0 - c.yf) * (1.0 - c.zf)
        + t100 * c.xf * (1.0 - c.yf) * (1.0 - c.zf)
        + t010 * (1.0 - c.xf) * c.yf * (1.0 - c.zf)
        + t110 * c.xf * c.yf * (1.0 - c.zf)
        + t001 * (1.0 - c.xf) * (1.0 - c.yf) * c.zf
        + t101 * c.xf * (1.0 - c.yf) * c.zf
        + t011 * (1.0 - c.xf) * c.yf * c.zf
        + t111 * c.xf * c.yf * c.zf;
}

/// O L da base (detalhe −1): `max(0, L + σr · 0,04 · soma)`.
fn dt_fatiar_shadhi(coord: vec2<u32>, l: f32) -> f32 {
    let c = dt_celula(dt_grades.sh_tamanho, dt_grades.sh_sigma, coord, l);
    let soma = dt_trilinear(c,
        dt_texel_sh(c, 0, 0, 0), dt_texel_sh(c, 1, 0, 0), dt_texel_sh(c, 0, 1, 0), dt_texel_sh(c, 1, 1, 0),
        dt_texel_sh(c, 0, 0, 1), dt_texel_sh(c, 1, 0, 1), dt_texel_sh(c, 0, 1, 1), dt_texel_sh(c, 1, 1, 1));
    return max(0.0, l + dt_grades.sh_sigma.y * 0.04 * soma);
}

fn dt_fatiar_monochrome(coord: vec2<u32>, l: f32) -> f32 {
    let c = dt_celula(dt_grades.mo_tamanho, dt_grades.mo_sigma, coord, l);
    let soma = dt_trilinear(c,
        dt_texel_mo(c, 0, 0, 0), dt_texel_mo(c, 1, 0, 0), dt_texel_mo(c, 0, 1, 0), dt_texel_mo(c, 1, 1, 0),
        dt_texel_mo(c, 0, 0, 1), dt_texel_mo(c, 1, 0, 1), dt_texel_mo(c, 0, 1, 1), dt_texel_mo(c, 1, 1, 1));
    return max(0.0, l + dt_grades.mo_sigma.y * 0.04 * soma);
}

// ----------------------------------------------------- shadows and highlights

fn dt_sinal(x: f32) -> f32 {
    if (x < 0.0) {
        return -1.0;
    }
    return 1.0;
}

fn dt_copysign(v: f32, s: f32) -> f32 {
    if (s < 0.0) {
        return -abs(v);
    }
    return abs(v);
}

/// `shadhi.c:336–491`, algoritmo bilateral. Ver `darktable::shadhi`.
fn dt_shadhi(t: vec3<f32>, coord: vec2<u32>) -> vec3<f32> {
    let flags = u32(params.dt_shadhi_flags);
    let lab = dt_para_lab(t);
    let base = dt_fatiar_shadhi(coord, lab.x);
    let shadows = 2.0 * clamp(params.dt_shadhi_shadows / 100.0, -1.0, 1.0);
    let highlights = 2.0 * clamp(params.dt_shadhi_highlights / 100.0, -1.0, 1.0);
    let whitepoint = max(1.0 - params.dt_shadhi_whitepoint / 100.0, 0.01);
    let compress = clamp(params.dt_shadhi_compress / 100.0, 0.0, 0.99);
    let sh_cc = (clamp(params.dt_shadhi_shadows_ccorrect / 100.0, 0.0, 1.0) - 0.5) * dt_sinal(shadows) + 0.5;
    let hl_cc = (clamp(params.dt_shadhi_highlights_ccorrect / 100.0, 0.0, 1.0) - 0.5) * dt_sinal(-highlights) + 0.5;
    let unbound_mask = (flags & 128u) != 0u;
    let low = 1e-6;

    var ta = vec3<f32>(lab.x / 100.0, lab.y / 128.0, lab.z / 128.0);
    var tb0 = (100.0 - base) / 100.0;
    if (ta.x > 0.0) {
        ta.x = ta.x / whitepoint;
    }
    if (tb0 > 0.0) {
        tb0 = tb0 / whitepoint;
    }

    // Altas luzes (shadhi.c:424–454). `tb` a e b são zero.
    var h2 = highlights * highlights;
    let hx = clamp(1.0 - tb0 / (1.0 - compress), 0.0, 1.0);
    loop {
        if (!(h2 > 0.0)) {
            break;
        }
        var la = ta.x;
        if ((flags & 8u) == 0u) {
            la = clamp(ta.x, 0.0, 1.0);
        }
        var lb = (tb0 - 0.5) * dt_sinal(-highlights) * dt_sinal(1.0 - la) + 0.5;
        if (!unbound_mask) {
            lb = clamp(lb, 0.0, 1.0);
        }
        let lref = dt_copysign(select(1.0 / low, 1.0 / abs(la), abs(la) > low), la);
        let href = dt_copysign(select(1.0 / low, 1.0 / abs(1.0 - la), abs(1.0 - la) > low), 1.0 - la);
        let op = min(h2, 1.0) * hx;
        h2 = h2 - 1.0;
        var sobre = 2.0 * la * lb;
        if (la > 0.5) {
            sobre = 1.0 - (1.0 - 2.0 * (la - 0.5)) * (1.0 - lb);
        }
        ta.x = la * (1.0 - op) + sobre * op;
        if ((flags & 8u) == 0u) {
            ta.x = clamp(ta.x, 0.0, 1.0);
        }
        let cf = ta.x * lref * (1.0 - hl_cc) + (1.0 - ta.x) * href * hl_cc;
        ta.y = ta.y * (1.0 - op) + ta.y * cf * op;
        if ((flags & 16u) == 0u) {
            ta.y = clamp(ta.y, -1.0, 1.0);
        }
        ta.z = ta.z * (1.0 - op) + ta.z * cf * op;
        if ((flags & 32u) == 0u) {
            ta.z = clamp(ta.z, -1.0, 1.0);
        }
    }

    // Sombras (shadhi.c:456–487).
    var s2 = shadows * shadows;
    let sx = clamp(tb0 / (1.0 - compress) - compress / (1.0 - compress), 0.0, 1.0);
    loop {
        if (!(s2 > 0.0)) {
            break;
        }
        // Sic: o darktable lê `UNBOUND_HIGHLIGHTS_L` aqui (shadhi.c:462).
        var la = ta.x;
        if ((flags & 8u) == 0u) {
            la = clamp(ta.x, 0.0, 1.0);
        }
        var lb = (tb0 - 0.5) * dt_sinal(shadows) * dt_sinal(1.0 - la) + 0.5;
        if (!unbound_mask) {
            lb = clamp(lb, 0.0, 1.0);
        }
        let lref = dt_copysign(select(1.0 / low, 1.0 / abs(la), abs(la) > low), la);
        let href = dt_copysign(select(1.0 / low, 1.0 / abs(1.0 - la), abs(1.0 - la) > low), 1.0 - la);
        let op = min(s2, 1.0) * sx;
        s2 = s2 - 1.0;
        var sobre = 2.0 * la * lb;
        if (la > 0.5) {
            sobre = 1.0 - (1.0 - 2.0 * (la - 0.5)) * (1.0 - lb);
        }
        ta.x = la * (1.0 - op) + sobre * op;
        if ((flags & 1u) == 0u) {
            ta.x = clamp(ta.x, 0.0, 1.0);
        }
        let cf = ta.x * lref * sh_cc + (1.0 - ta.x) * href * (1.0 - sh_cc);
        ta.y = ta.y * (1.0 - op) + ta.y * cf * op;
        if ((flags & 2u) == 0u) {
            ta.y = clamp(ta.y, -1.0, 1.0);
        }
        ta.z = ta.z * (1.0 - op) + ta.z * cf * op;
        if ((flags & 4u) == 0u) {
            ta.z = clamp(ta.z, -1.0, 1.0);
        }
    }

    return dt_de_lab(vec3<f32>(ta.x * 100.0, ta.y * 128.0, ta.z * 128.0));
}

// ----------------------------------------------------------------- monochrome

/// `dt_fast_expf` (`math.h:418–431`): a interpolação na representação binária,
/// e não o `exp`.
fn dt_fast_expf(x: f32) -> f32 {
    let k0 = i32(1065353216.0 + x * 11401300.0);
    return bitcast<f32>(u32(max(k0, 0)));
}

fn dt_envelope(l: f32) -> f32 {
    let x = clamp(l / 100.0, 0.0, 1.0);
    if (x < 0.6) {
        let t = x / 0.6 - 1.0;
        return 1.0 - t * t;
    }
    let t1 = (1.0 - x) / 0.4;
    let t2 = t1 * t1;
    return 3.0 * t2 - 2.0 * t2 * t1;
}

/// `monochrome.c:196–249`. Ver `darktable::monochrome`.
fn dt_monochrome(t: vec3<f32>, coord: vec2<u32>) -> vec3<f32> {
    let lab = dt_para_lab(t);
    let tamanho = params.dt_monochrome_size * 128.0;
    let sigma2 = 2.0 * tamanho * tamanho;
    let da = lab.y - params.dt_monochrome_a;
    let db = lab.z - params.dt_monochrome_b;
    let filtro = 100.0 * dt_fast_expf(-clamp((da * da + db * db) / sigma2, 0.0, 1.0));
    let borrado = dt_fatiar_monochrome(coord, filtro);
    let tt = dt_envelope(lab.x);
    let mistura = tt + (1.0 - tt) * (1.0 - params.dt_monochrome_highlights);
    let saida = (1.0 - mistura) * lab.x + mistura * borrado * (1.0 / 100.0) * lab.x;
    return dt_de_lab(vec3<f32>(saida, 0.0, 0.0));
}

// --------------------------------------------------------------------- estágio

/// Do sRGB 0–255 do corpo ao espaço do darktable, os módulos na ordem do
/// pipeline dele, e de volta.
///
/// 🔑 **A ordem é a do `iop_order` do darktable 5.6**: exposure, shadows and
/// highlights, monochrome, vignetting, color balance rgb.
fn dt_estagio(rgb255: vec3<f32>, coord: vec2<u32>, dims: vec2<u32>) -> vec3<f32> {
    let linear = vec3<f32>(
        dt_srgb_para_linear(rgb255.r),
        dt_srgb_para_linear(rgb255.g),
        dt_srgb_para_linear(rgb255.b),
    );
    var t = dt_mul3(DT_SRGB_PARA_TRABALHO, linear);
    if (params.dt_exposure_ativo != 0.0) {
        t = dt_exposure(t);
    }
    if (params.dt_shadhi_ativo != 0.0) {
        t = dt_shadhi(t, coord);
    }
    if (params.dt_monochrome_ativo != 0.0) {
        t = dt_monochrome(t, coord);
    }
    if (params.dt_vignette_ativo != 0.0) {
        t = dt_vignette(t, coord, dims);
    }
    if (params.dt_cb_ativo != 0.0) {
        t = dt_color_balance_rgb(t);
    }
    let s = dt_mul3(DT_TRABALHO_PARA_SRGB, t);
    return vec3<f32>(dt_linear_para_srgb(s.r), dt_linear_para_srgb(s.g), dt_linear_para_srgb(s.b));
}
