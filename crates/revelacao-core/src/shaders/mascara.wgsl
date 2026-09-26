// O rasterizador das máscaras locais — a mesma conta de `locais.rs`, na GPU.
//
// 🔑 **É sempre render pass, nas duas entradas do motor.** O alvo é `R8Unorm`,
// que não é formato de storage no WebGPU; por render pass ele é alvo de cor e
// aceita blend no Metal, no Vulkan, no DX12, no WebGPU e no WebGL2 (GLES 3.0).
// Um rasterizador só é o que garante a mesma máscara no desktop e no navegador.
//
// 🔑 **A posição do pixel vem de um `varying`, e não de `@builtin(position)`**
// — o mesmo motivo de `entrada_fragmento.wgsl`: o espaço de clip é o que o wgpu
// garante igual no GL e no WebGPU. O vértice leva a posição em pixels junto, e a
// interpolação devolve o centro do pixel (`i + 0,5`) em cada fragmento.
//
// Três desenhos:
//
//   - `vs_capsula` / `fs_capsula`: um trecho de stroke, instanciado. O blend do
//     pipeline é **MAX**: dentro de um stroke, passar de novo não acumula.
//   - `vs_retangulo` / `fs_compor`: o stroke pronto (no rascunho) entra na
//     camada com a opacidade dele. Somar e Subtrair são dois blends.
//   - `vs_retangulo` / `fs_gradiente`: linear e radial, analíticos, direto na
//     camada, com os mesmos dois blends.

struct Desenho {
    // largura, altura, raio em pixels, feather
    dims: vec4<f32>,
    // x0, y0, x1, y1 do retângulo em pixels (compor e gradiente)
    rect: vec4<f32>,
    // linear: início.xy, fim.xy (pixels) — radial: centro.xy (pixels), raio_x, raio_y (pixels)
    a: vec4<f32>,
    // tipo (1 linear, 2 radial), opacidade, ângulo em radianos, fora (0/1)
    b: vec4<f32>,
}

@group(0) @binding(0) var<uniform> desenho: Desenho;
@group(0) @binding(1) var rascunho: texture_2d<f32>;

fn para_clip(pixel: vec2<f32>) -> vec4<f32> {
    let dims = desenho.dims.xy;
    return vec4<f32>(pixel.x / dims.x * 2.0 - 1.0, 1.0 - pixel.y / dims.y * 2.0, 0.0, 1.0);
}

// Os seis cantos de um retângulo (dois triângulos), de 0 a 1.
fn canto(i: u32) -> vec2<f32> {
    var cantos = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    return cantos[i];
}

/// `smoothstep` com a borda dura quando as pontas coincidem — `locais::suave`.
fn suave(borda0: f32, borda1: f32, x: f32) -> f32 {
    if (borda1 - borda0 <= 1e-6) {
        return select(1.0, 0.0, x < borda1);
    }
    let t = clamp((x - borda0) / (borda1 - borda0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// ------------------------------------------------------------------ cápsula

struct VerticeDaCapsula {
    @builtin(position) posicao: vec4<f32>,
    @location(0) pixel: vec2<f32>,
    @location(1) @interpolate(flat) a: vec2<f32>,
    @location(2) @interpolate(flat) b: vec2<f32>,
    @location(3) @interpolate(flat) pressao: vec2<f32>,
}

@vertex
fn vs_capsula(
    @builtin(vertex_index) i: u32,
    @location(0) trecho: vec4<f32>,
    @location(1) pressao: vec2<f32>,
) -> VerticeDaCapsula {
    let raio = desenho.dims.z;
    let minimo = min(trecho.xy, trecho.zw) - vec2<f32>(raio + 1.0);
    let maximo = max(trecho.xy, trecho.zw) + vec2<f32>(raio + 1.0);
    let pixel = mix(minimo, maximo, canto(i));
    var saida: VerticeDaCapsula;
    saida.posicao = para_clip(pixel);
    saida.pixel = pixel;
    saida.a = trecho.xy;
    saida.b = trecho.zw;
    saida.pressao = pressao;
    return saida;
}

/// `locais::cobertura_da_capsula`, na mesma ordem de operações.
@fragment
fn fs_capsula(v: VerticeDaCapsula) -> @location(0) vec4<f32> {
    let raio = desenho.dims.z;
    let feather = desenho.dims.w;
    let ab = v.b - v.a;
    let ap = v.pixel - v.a;
    let comprimento2 = dot(ab, ab);
    var t = 0.0;
    if (comprimento2 > 1e-12) {
        t = clamp(dot(ap, ab) / comprimento2, 0.0, 1.0);
    }
    let distancia = length(ap - ab * t);
    let pressao = v.pressao.x + (v.pressao.y - v.pressao.x) * t;
    let interno = raio * (1.0 - feather);
    let c = pressao * (1.0 - suave(interno, raio, distancia));
    return vec4<f32>(c, 0.0, 0.0, c);
}

// ---------------------------------------------------------------- retângulo

struct VerticeDoRetangulo {
    @builtin(position) posicao: vec4<f32>,
    @location(0) pixel: vec2<f32>,
}

@vertex
fn vs_retangulo(@builtin(vertex_index) i: u32) -> VerticeDoRetangulo {
    let pixel = mix(desenho.rect.xy, desenho.rect.zw, canto(i));
    var saida: VerticeDoRetangulo;
    saida.posicao = para_clip(pixel);
    saida.pixel = pixel;
    return saida;
}

/// O stroke do rascunho, com a opacidade dele — o blend decide o modo.
@fragment
fn fs_compor(v: VerticeDoRetangulo) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(desenho.dims.xy);
    let coord = clamp(vec2<i32>(floor(v.pixel)), vec2<i32>(0, 0), dims - vec2<i32>(1, 1));
    let s = textureLoad(rascunho, coord, 0).r * desenho.b.y;
    return vec4<f32>(s, 0.0, 0.0, s);
}

/// `locais::cobertura_do_gradiente`, na mesma ordem de operações.
@fragment
fn fs_gradiente(v: VerticeDoRetangulo) -> @location(0) vec4<f32> {
    let p = v.pixel;
    var c = 0.0;
    if (desenho.b.x < 1.5) {
        let ab = desenho.a.zw - desenho.a.xy;
        let comprimento2 = dot(ab, ab);
        if (comprimento2 > 1e-6) {
            let t = dot(p - desenho.a.xy, ab) / comprimento2;
            c = 1.0 - suave(0.0, 1.0, t);
        }
    } else {
        let d = p - desenho.a.xy;
        let seno = sin(-desenho.b.z);
        let cosseno = cos(-desenho.b.z);
        let local = vec2<f32>(d.x * cosseno - d.y * seno, d.x * seno + d.y * cosseno);
        let q = length(local / desenho.a.zw);
        let dentro = 1.0 - suave(1.0 - desenho.dims.w, 1.0, q);
        c = select(dentro, 1.0 - dentro, desenho.b.w > 0.5);
    }
    return vec4<f32>(c, 0.0, 0.0, c);
}
