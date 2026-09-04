
// A entrada por fragmento: um triângulo que cobre o alvo, um fragmento por
// pixel. É o navegador — WebGL2 não tem compute — e serve ao WebGPU também.
//
// 🔑 **A coordenada do pixel vem de um `uv` interpolado, e não de
// `@builtin(position)`.** O `gl_FragCoord` do GL conta de baixo para cima e o
// do WebGPU de cima para baixo; o wgpu compensa o espaço de clip no vértice,
// mas não promete o mesmo para o `position` do fragmento. O `uv` nasce do
// espaço de clip, que o wgpu **garante** igual nos dois backends, então
// `floor(uv * dims)` é o mesmo pixel em qualquer um.
//
// No centro do pixel `i`, `uv * dims` é `i + 0.5` — o `floor` devolve `i`
// com meio pixel de folga para o erro de interpolação.
struct Vertice {
    @builtin(position) posicao: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> Vertice {
    // Três vértices que cobrem o quadrado de clip inteiro: (-1,-1), (3,-1), (-1,3).
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    var saida: Vertice;
    saida.posicao = vec4<f32>(x, y, 0.0, 1.0);
    // uv (0,0) é o canto superior esquerdo: y de clip +1 é o topo.
    saida.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return saida;
}

@fragment
fn fs(entrada: Vertice) -> @location(0) vec4<f32> {
    let dims = textureDimensions(input_texture);
    let coord = vec2<u32>(floor(entrada.uv * vec2<f32>(dims)));
    return revelar_pixel(min(coord, dims - vec2<u32>(1u, 1u)));
}
