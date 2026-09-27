// O retoque — Clone (carimbo), Heal (band-aid) e Content-Aware — antes da revelação.
//
// 🔑 **Nunca lê e escreve a mesma textura.** Cada retoque é um passe que lê a
// `fonte` (a entrada original, no primeiro; a saída do anterior, nos outros) e
// escreve o alvo inteiro, que é outra textura. O bruto e os pixels de origem
// não mudam: o que muda é qual textura o shader de revelação lê.
//
// A máscara do destino (`mascara`, R8) foi desenhada antes pelo mesmo
// rasterizador de cápsulas do pincel, com o raio e o feather do carimbo.
//
// ## Clone
//
// O pixel `p` recebe a fonte em `p + deslocamento` (bilinear), misturado pela
// máscara. **Fonte fora da foto não pinta**: a cobertura cai a zero e o
// destino fica como estava — nunca se estica a borda para dentro da foto.
//
// ## Heal
//
// O mesmo transplante, mais uma **membrana** de correção em RGB linear:
//
//     saída(p) = fonte(p + o) + M(p)
//     M(p)     = Σ w_j · (destino(q_j) − fonte(q_j + o)) / Σ w_j,   w_j = 1 / |p − q_j|²
//
// com `q_j` o anel de amostras logo fora da máscara (`locais::anel_do_carimbo`).
// Na borda, `M` vale a diferença que está ali, e o remendo encosta na
// vizinhança sem degrau; no meio, é uma média suave dela. A textura (alta
// frequência) é a da fonte; a cor e a luz (baixa frequência) passam a ser as
// do destino.
//
// ⚠️ É uma aproximação local e verificável do seamless cloning (Pérez et al.,
// 2003): interpolação de Shepard no lugar da solução harmônica de Poisson —
// o mesmo espírito das coordenadas de valor médio (Farbman et al., 2009). Não
// sintetiza textura, e um anel que atravessa bordas fortes leva essas bordas,
// suavizadas, para dentro do remendo.

// ## Content-Aware (tipo 3)
//
// O remendo foi sintetizado na CPU (`preenchimento.rs`) e chega como textura
// do tamanho da caixa do buraco; aqui ele só é misturado pela máscara, na
// mesma cadeia e na mesma ordem dos outros retoques.

struct Retoque {
    // largura, altura, tipo (1 clone, 2 heal, 3 preencher), opacidade
    dims: vec4<f32>,
    // clone/heal: deslocamento em pixels (x, y), amostras do anel, —
    // preencher: canto da caixa do remendo (x0, y0), largura, altura
    desloc: vec4<f32>,
    // xy: um ponto do anel, em pixels (centro do pixel = i + 0,5)
    anel: array<vec4<f32>, 96>,
}

@group(0) @binding(0) var<uniform> r: Retoque;
@group(0) @binding(1) var fonte: texture_2d<f32>;
@group(0) @binding(2) var mascara: texture_2d<f32>;
@group(0) @binding(3) var remendo: texture_2d<f32>;

struct Vertice {
    @builtin(position) posicao: vec4<f32>,
    @location(0) pixel: vec2<f32>,
}

// Um triângulo que cobre o alvo; o `pixel` interpolado dá o centro de cada um
// (ver `entrada_fragmento.wgsl` para por que não `@builtin(position)`).
@vertex
fn vs(@builtin(vertex_index) i: u32) -> Vertice {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    var saida: Vertice;
    saida.posicao = vec4<f32>(x, y, 0.0, 1.0);
    saida.pixel = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5) * r.dims.xy;
    return saida;
}

fn ler(c: vec2<i32>) -> vec4<f32> {
    let dims = vec2<i32>(r.dims.xy);
    return textureLoad(fonte, clamp(c, vec2<i32>(0, 0), dims - vec2<i32>(1, 1)), 0);
}

/// Bilinear em coordenadas de pixel (o centro do pixel `i` é `i + 0,5`).
fn amostrar(p: vec2<f32>) -> vec4<f32> {
    let q = p - vec2<f32>(0.5);
    let base = floor(q);
    let f = q - base;
    let c = vec2<i32>(base);
    let a = mix(ler(c), ler(c + vec2<i32>(1, 0)), f.x);
    let b = mix(ler(c + vec2<i32>(0, 1)), ler(c + vec2<i32>(1, 1)), f.x);
    return mix(a, b, f.y);
}

fn para_linear(c: vec3<f32>) -> vec3<f32> {
    let baixo = c / 12.92;
    let alto = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(alto, baixo, c <= vec3<f32>(0.04045));
}

fn para_srgb(c: vec3<f32>) -> vec3<f32> {
    let v = max(c, vec3<f32>(0.0));
    let baixo = v * 12.92;
    let alto = 1.055 * pow(v, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(alto, baixo, v <= vec3<f32>(0.0031308));
}

fn dentro(p: vec2<f32>) -> bool {
    return p.x >= 0.5 && p.y >= 0.5 && p.x <= r.dims.x - 0.5 && p.y <= r.dims.y - 0.5;
}

@fragment
fn fs(v: Vertice) -> @location(0) vec4<f32> {
    let coord = vec2<i32>(floor(v.pixel));
    let base = ler(coord);
    let m = textureLoad(mascara, coord, 0).r * r.dims.w;
    if (r.dims.z > 2.5) {
        let local = coord - vec2<i32>(r.desloc.xy);
        if (m <= 0.0 || any(local < vec2<i32>(0, 0)) || any(local >= vec2<i32>(r.desloc.zw))) {
            return base;
        }
        return vec4<f32>(mix(base.rgb, textureLoad(remendo, local, 0).rgb, m), base.a);
    }
    let s = v.pixel + r.desloc.xy;
    if (m <= 0.0 || !dentro(s)) {
        return base;
    }
    let origem = amostrar(s);
    if (r.dims.z < 1.5) {
        return vec4<f32>(mix(base.rgb, origem.rgb, m), base.a);
    }

    // Heal: a membrana, em linear.
    let n = u32(r.desloc.z);
    var soma = vec3<f32>(0.0);
    var pesos = 0.0;
    for (var j = 0u; j < n; j++) {
        let q = r.anel[j].xy;
        let d = v.pixel - q;
        let w = 1.0 / max(dot(d, d), 1.0);
        let diferenca = para_linear(amostrar(q).rgb) - para_linear(amostrar(q + r.desloc.xy).rgb);
        soma += w * diferenca;
        pesos += w;
    }
    var corrigida = para_linear(origem.rgb);
    if (pesos > 0.0) {
        corrigida += soma / pesos;
    }
    let curado = clamp(para_srgb(corrigida), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(mix(base.rgb, curado, m), base.a);
}
