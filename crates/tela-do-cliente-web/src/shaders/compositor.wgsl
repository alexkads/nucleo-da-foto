// O compositor da tela do cliente: uma foto por camada, sobre preto.
//
// 🔑 **A revelação não acontece aqui.** Cada camada já chega revelada, numa
// textura que o `revelacao_core` desenhou com os mesmos 53 ajustes do editor.
// O que este shader faz é o que o CSS fazia antes: encaixar (contain),
// enquadrar (recorte, giro, espelho, endireitamento) e cruzar uma foto com a
// seguinte.
//
// ⚠️ **O enquadramento é feito nas UVs, e não recortando a textura.** Recortar
// exigiria uma passada a mais por gesto de enquadrar; um mapeamento de UV custa
// dois `mul` por fragmento e dá o mesmo resultado, porque a matriz é a mesma
// conta de `Corte::retangulo`.

struct Camada {
    // O quad no espaço de clip: meia-largura e meia-altura, e o centro.
    escala: vec2<f32>,
    centro: vec2<f32>,
    // A matriz 2x2 que leva a coordenada do quad (0..1) para a UV da textura,
    // com o deslocamento do recorte.
    uv_x: vec2<f32>,
    uv_y: vec2<f32>,
    uv_off: vec2<f32>,
    alfa: f32,
    _reservado: f32,
};

@group(0) @binding(0) var<uniform> camada: Camada;
@group(0) @binding(1) var textura: texture_2d<f32>;
@group(0) @binding(2) var amostrador: sampler;

struct Saida {
    @builtin(position) posicao: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs(@builtin(vertex_index) indice: u32) -> Saida {
    // Dois triângulos, em ordem de faixa: (0,0) (1,0) (0,1) (1,1).
    let canto = vec2<f32>(f32(indice & 1u), f32((indice >> 1u) & 1u));

    var saida: Saida;
    // De 0..1 para -1..1, escalado e centrado. O Y do clip cresce para cima e o
    // da imagem para baixo: o sinal troca aqui, uma vez.
    let p = (canto * 2.0 - 1.0) * camada.escala + camada.centro;
    saida.posicao = vec4<f32>(p.x, -p.y, 0.0, 1.0);
    saida.uv = camada.uv_off + camada.uv_x * canto.x + camada.uv_y * canto.y;
    return saida;
}

@fragment
fn fs(entrada: Saida) -> @location(0) vec4<f32> {
    // Fora da foto (o endireitamento puxa cantos de fora) fica preto, como o
    // fundo — e não a borda esticada, que na tela do cliente viraria um borrão
    // no canto da foto.
    let dentro = all(entrada.uv >= vec2<f32>(0.0)) && all(entrada.uv <= vec2<f32>(1.0));
    if (!dentro) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    let cor = textureSampleLevel(textura, amostrador, entrada.uv, 0.0);
    return vec4<f32>(cor.rgb, cor.a * camada.alfa);
}
