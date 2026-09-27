// O laço: um polígono fechado, com a borda suave por distância à linha.
//
// `locais::distancia_ao_laco` e `locais::cobertura_do_laco`, na mesma ordem de
// operações. Dentro/fora pela regra par-ímpar; a transição do feather é
// centrada na linha do laço. O retângulo desenhado é a caixa do polígono mais
// meio feather — fora dele a cobertura é zero e o blend não muda nada.
//
// Os vértices vêm num `uniform` (dois por `vec4`), e não num storage buffer:
// o WebGL2 não tem storage buffer.

struct Laco {
    // largura, altura, feather em pixels, quantos vértices
    info: vec4<f32>,
    // x0, y0, x1, y1 da caixa, em pixels
    caixa: vec4<f32>,
    vertices: array<vec4<f32>, 128>,
}

@group(0) @binding(0) var<uniform> laco: Laco;

struct Vertice {
    @builtin(position) posicao: vec4<f32>,
    @location(0) pixel: vec2<f32>,
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Vertice {
    var cantos = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let pixel = mix(laco.caixa.xy, laco.caixa.zw, cantos[i]);
    var saida: Vertice;
    saida.posicao = vec4<f32>(pixel.x / laco.info.x * 2.0 - 1.0, 1.0 - pixel.y / laco.info.y * 2.0, 0.0, 1.0);
    saida.pixel = pixel;
    return saida;
}

fn vertice(i: u32) -> vec2<f32> {
    let v = laco.vertices[i / 2u];
    return select(v.xy, v.zw, (i & 1u) == 1u);
}

fn suave(borda0: f32, borda1: f32, x: f32) -> f32 {
    if (borda1 - borda0 <= 1e-6) {
        return select(1.0, 0.0, x < borda1);
    }
    let t = clamp((x - borda0) / (borda1 - borda0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

@fragment
fn fs(v: Vertice) -> @location(0) vec4<f32> {
    let p = v.pixel;
    let n = u32(laco.info.w);
    var dentro = false;
    var menor = 3.4e38;
    for (var i = 0u; i < n; i++) {
        let a = vertice(i);
        let b = vertice((i + 1u) % n);
        if ((a.y > p.y) != (b.y > p.y)) {
            let x = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
            if (p.x < x) {
                dentro = !dentro;
            }
        }
        let ab = b - a;
        let ap = p - a;
        let l2 = dot(ab, ab);
        var t = 0.0;
        if (l2 > 1e-12) {
            t = clamp(dot(ap, ab) / l2, 0.0, 1.0);
        }
        menor = min(menor, length(ap - ab * t));
    }
    let s = select(-menor, menor, dentro);
    let f = laco.info.z;
    let c = suave(-f * 0.5, f * 0.5, s);
    return vec4<f32>(c, 0.0, 0.0, c);
}
