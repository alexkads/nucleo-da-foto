//! Do pixel revelado ao pixel exibido: corte, giro, espelho e endireitamento.
//!
//! O shader devolve a foto **inteira**, do tamanho de origem. O que a tela mostra
//! é essa foto depois de espelhada, girada em múltiplos de 90°, endireitada pelo
//! ângulo e recortada — nessa ordem, que é a do legado.
//!
//! ## 🔑 A ordem sai do `image_viewer.rs`, e não de uma escolha nossa
//!
//! O legado desenha o resultado como uma malha cujas UVs são calculadas do
//! **quadro** para a **textura**: descentraliza, corrige o aspecto, gira por
//! `-ângulo`, desfaz o giro de 90° e desfaz os espelhos. Lendo ao contrário, é o
//! caminho de ida:
//!
//! ```text
//! original → espelhos → giro de 90° → endireitamento → recorte
//! ```
//!
//! O retângulo de corte, portanto, mora no espaço **já girado e já endireitado** —
//! é isso que faz `CropSettings::to_visual_space` existir no `domain`, e é por
//! isso que endireitar uma foto não faz o corte sair do lugar.
//!
//! ⚠️ **`ImageProcessor::apply_crop`, do mesmo legado, faz outra coisa**: recorta
//! primeiro, espelha depois, gira por último — e **ignora o ângulo**. É a função
//! que gera miniatura, e é por isso que uma foto endireitada aparece torta na
//! grade e direita no viewer. Aqui vale a do viewer: é a Revelação que esta tela
//! porta.
//!
//! ## [`Corte`] em vez de `CropSettings`
//!
//! Este crate não conhece o `domain`. [`Corte`] tem os mesmos oito campos e o
//! mesmo `clamp` de `CropSettings::new`; quem tem um `CropSettings` na mão
//! converte em `infrastructure::transformacao`, e o resultado é o mesmo byte.

use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};

/// O enquadramento: retângulo normalizado (0–1) no espaço já girado e
/// endireitado, giro em múltiplos de 90°, ângulo fino e espelhos.
///
/// 🔑 Os valores são limitados na construção, exatamente como
/// `CropSettings::new` faz no `domain` — inclusive o mínimo de 1% de lado e os
/// 45° de ângulo. Dois clamps diferentes dariam dois enquadramentos para o mesmo
/// registro.
#[derive(Debug, Clone, PartialEq)]
pub struct Corte {
    x: f32,
    y: f32,
    largura: f32,
    altura: f32,
    giro_90: i32,
    angulo: f32,
    espelho_h: bool,
    espelho_v: bool,
}

impl Corte {
    const LADO_MINIMO: f32 = 0.01;
    const ANGULO_MAXIMO: f32 = 45.0;

    /// Os oito campos, na ordem de `CropSettings::new`, limitados às faixas válidas.
    #[allow(clippy::too_many_arguments)]
    pub fn novo(
        x: f32,
        y: f32,
        largura: f32,
        altura: f32,
        giro_90: i32,
        angulo: f32,
        espelho_h: bool,
        espelho_v: bool,
    ) -> Self {
        let x = x.clamp(0.0, 1.0);
        let y = y.clamp(0.0, 1.0);
        let largura = largura.clamp(Self::LADO_MINIMO, 1.0).min(1.0 - x);
        let altura = altura.clamp(Self::LADO_MINIMO, 1.0).min(1.0 - y);
        let giro_90 = giro_90.clamp(-1, 3);
        let angulo = angulo.clamp(-Self::ANGULO_MAXIMO, Self::ANGULO_MAXIMO);
        Self {
            x,
            y,
            largura,
            altura,
            giro_90,
            angulo,
            espelho_h,
            espelho_v,
        }
    }

    /// A foto inteira, sem giro, sem ângulo, sem espelho.
    pub fn inteiro() -> Self {
        Self::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, false, false)
    }

    pub fn x(&self) -> f32 {
        self.x
    }
    pub fn y(&self) -> f32 {
        self.y
    }
    pub fn largura(&self) -> f32 {
        self.largura
    }
    pub fn altura(&self) -> f32 {
        self.altura
    }
    pub fn giro_90(&self) -> i32 {
        self.giro_90
    }
    pub fn angulo(&self) -> f32 {
        self.angulo
    }
    pub fn espelho_h(&self) -> bool {
        self.espelho_h
    }
    pub fn espelho_v(&self) -> bool {
        self.espelho_v
    }

    /// O corte não muda nada? Ver [`e_identidade`], que é onde a folga mora.
    pub fn e_inteiro(&self) -> bool {
        e_identidade(self)
    }

    /// As dimensões depois dos espelhos e do giro de 90° — **o espaço em que o
    /// retângulo mora**.
    ///
    /// Espelhar não muda tamanho; girar por um múltiplo ímpar de 90° troca os
    /// lados. É a conversão que o preview precisa fazer para desenhar o
    /// retângulo no lugar certo.
    pub fn dimensoes_giradas(&self, largura: u32, altura: u32) -> (u32, u32) {
        if self.giro_90.rem_euclid(4) % 2 == 1 {
            (altura, largura)
        } else {
            (largura, altura)
        }
    }

    /// O retângulo em pixels, **dentro do espaço já girado** — exatamente o que
    /// [`recortar_reto`] copia.
    ///
    /// 🚨 **É a única conta.** Quem recorta e quem **desenha o preview** leem
    /// daqui: dois arredondamentos diferentes fariam o operador enquadrar uma
    /// coisa na tela e receber outra no arquivo, que é o pior desfecho possível
    /// num estúdio de retrato.
    pub fn retangulo(&self, largura_girada: u32, altura_girada: u32) -> (u32, u32, u32, u32) {
        let x =
            ((self.x * largura_girada as f32).round() as u32).min(largura_girada.saturating_sub(1));
        let y =
            ((self.y * altura_girada as f32).round() as u32).min(altura_girada.saturating_sub(1));
        let w = ((self.largura * largura_girada as f32).round() as u32)
            .max(1)
            .min(largura_girada - x);
        let h = ((self.altura * altura_girada as f32).round() as u32)
            .max(1)
            .min(altura_girada - y);
        (x, y, w, h)
    }

    /// O tamanho do arquivo que sai, a partir das dimensões **de origem**.
    ///
    /// ⚠️ Com ângulo é diferente do retângulo: o endireitamento reamostra para
    /// uma saída do tamanho pedido, sem o `min` da borda — o que cai fora da
    /// foto vira borda, não corta o resultado.
    pub fn dimensoes_de_saida(&self, largura: u32, altura: u32) -> (u32, u32) {
        let (l, a) = self.dimensoes_giradas(largura, altura);
        if self.angulo == 0.0 {
            let (_, _, w, h) = self.retangulo(l, a);
            (w, h)
        } else {
            (
                ((self.largura * l as f32).round() as u32).max(1),
                ((self.altura * a as f32).round() as u32).max(1),
            )
        }
    }
}

/// As UVs que levam do quad da tela ao pixel da textura — o enquadramento
/// feito pelo **amostrador da GPU**, sem copiar imagem nenhuma.
///
/// # Por que existe, e por que mora aqui
///
/// 🔑 A tela do cliente desenha a foto num quad e deixa a GPU ler o pedaço
/// certo; [`aplicar`] faz a mesma conta copiando pixels, e é dele que sai o
/// JPEG. As duas **precisam** concordar: é o que impede o operador de enquadrar
/// uma coisa e o cliente ver outra.
///
/// 🚨 **Ela nasceu dentro do `tela-do-cliente-web` e veio para cá em
/// 2026-09-12**, quando o dono relatou que *"o rotacionamento de fotos na tela
/// do cliente não está funcionando corretamente"* enquanto a revelação, as
/// tiras e a biblioteca giravam certo. Lá ela não tinha teste — o crate só
/// compila para `wasm32`, e o próprio arquivo diz que "o que se prova sem
/// navegador é o `revelacao-core`, onde a matemática mora". Aqui ela é
/// comparada com [`aplicar`] pixel a pixel, nos quatro giros.
///
/// Devolve `(uv_x, uv_y, uv_off)`, que o shader usa como
/// `uv = uv_off + uv_x·s + uv_y·t` para `(s, t)` no quad `0..1`.
pub fn uvs_do_enquadramento(
    largura: u32,
    altura: u32,
    corte: &Corte,
) -> ([f32; 2], [f32; 2], [f32; 2]) {
    let (lg, ag) = corte.dimensoes_giradas(largura, altura);

    // 1. Do quad (0..1) ao ponto normalizado no **espaço girado** — o recorte e,
    //    quando houver, o endireitamento.
    let (mut ux, mut uy, mut uoff) = if corte.angulo() == 0.0 {
        // Reto: o retângulo em pixels, como `recortar_reto` o recorta — é o
        // arredondamento dele que decide os limites, e dividir por lg/ag depois
        // mantém as duas contas no mesmo pixel.
        let (rx, ry, rw, rh) = corte.retangulo(lg, ag);
        (
            [rw as f32 / lg as f32, 0.0],
            [0.0, rh as f32 / ag as f32],
            [rx as f32 / lg as f32, ry as f32 / ag as f32],
        )
    } else {
        // 🚨 **Com ângulo, a conta é a de `endireitar_e_recortar`, e ela não é
        // "girar o retângulo".**
        //
        // Lá, cada pixel da saída vira um ponto normalizado dentro do recorte,
        // é centrado em 0,5, **corrigido pelo aspecto** (o espaço normalizado
        // não é quadrado: sem isso, endireitar uma foto deitada gira demais na
        // vertical), rotacionado por **−ângulo** e devolvido a pixel.
        //
        // A versão anterior girava o canto do retângulo em torno de 0,5 e
        // reaproveitava os vetores — o que dá o mesmo resultado só quando o
        // recorte é a foto inteira e o aspecto é 1. Era o defeito que o dono viu
        // em 2026-09-12: o editor mostrava a foto endireitada e a tela do
        // cliente, reta.
        let aspecto = lg as f32 / ag as f32;
        let (sen, cos) = (-corte.angulo().to_radians()).sin_cos();
        let (cx, cy) = (corte.x() - 0.5, corte.y() - 0.5);
        (
            // ∂/∂s: o lado do recorte, girado
            [corte.largura() * cos, corte.largura() * aspecto * sen],
            // ∂/∂t: o outro lado, girado — o aspecto entra invertido aqui
            [-corte.altura() * sen / aspecto, corte.altura() * cos],
            [
                0.5 + cx * cos - cy * sen / aspecto,
                0.5 + cx * aspecto * sen + cy * cos,
            ],
        )
    };

    // Giro de 90° e espelhos: uma troca de eixos e um sinal.
    let quartos = ((corte.giro_90() % 4) + 4) % 4;
    for _ in 0..quartos {
        // (x, y) → (y, 1 - x): um quarto de volta no espaço normalizado.
        let troca = |v: [f32; 2]| [v[1], -v[0]];
        ux = troca(ux);
        uy = troca(uy);
        uoff = [uoff[1], 1.0 - uoff[0]];
    }
    if corte.espelho_h() {
        ux[0] = -ux[0];
        uy[0] = -uy[0];
        uoff[0] = 1.0 - uoff[0];
    }
    if corte.espelho_v() {
        ux[1] = -ux[1];
        uy[1] = -uy[1];
        uoff[1] = 1.0 - uoff[1];
    }

    (ux, uy, uoff)
}

/// A foto pronta para a tela.
///
/// `recortar` é `false` no modo de corte: lá a foto aparece inteira (girada e
/// endireitada) com o retângulo desenhado por cima, senão não haveria o que
/// arrastar. É o `apply_crop_clip` do legado.
pub fn aplicar(imagem: &DynamicImage, corte: &Corte, recortar: bool) -> DynamicImage {
    // 🚨 **O caminho sem enquadramento nenhum sai daqui.**
    //
    // Ele é o caso comum — a maioria das fotos nunca é cortada — e era o mais
    // caro por engano: `espelhar_e_girar` clonava a imagem inteira antes de
    // descobrir que não havia o que espelhar, e `recortar_reto` copiava pixel a
    // pixel um retângulo que é a foto toda.
    //
    // ⚠️ **E isto roda a cada resultado da GPU**, não uma vez por foto: durante
    // um arrasto de slider são dezenas por segundo, na thread da interface.
    // Medido em 18/ago/2026 numa foto de 2560×2560: **8,6 ms** por resultado,
    // metade de um quadro de 60fps gasta para devolver a mesma imagem.
    if e_identidade(corte) {
        return imagem.clone();
    }

    let base = espelhar_e_girar(imagem, corte);

    if !recortar {
        return base;
    }

    if corte.angulo() == 0.0 {
        return recortar_reto(&base, corte);
    }

    endireitar_e_recortar(&base, corte)
}

/// O corte não muda nada na foto?
///
/// 🔑 **A comparação do retângulo tem folga.** `Corte::novo` limita os
/// valores, e uma largura gravada como `0.999999` depois de uma ida e volta pelo
/// banco descreve a foto inteira — mas não é `1.0`. Sem a folga, o caminho
/// rápido nunca valeria para foto que já passou por gravação.
fn e_identidade(corte: &Corte) -> bool {
    const FOLGA: f32 = 1e-4;
    corte.giro_90().rem_euclid(4) == 0
        && corte.angulo() == 0.0
        && !corte.espelho_h()
        && !corte.espelho_v()
        && corte.x().abs() < FOLGA
        && corte.y().abs() < FOLGA
        && (corte.largura() - 1.0).abs() < FOLGA
        && (corte.altura() - 1.0).abs() < FOLGA
}

/// Espelhos e giro de 90°, na ordem do legado (`apply_crop`: `fliph`, `flipv`,
/// depois `rotate90`).
///
/// 🚨 **A ordem entre espelho e giro muda o resultado.** Girar 90° e depois
/// espelhar na horizontal dá a mesma coisa que espelhar na vertical e depois
/// girar — trocar a ordem aqui produz a foto certa em metade dos casos e a
/// invertida na outra metade, o que na conferência parece "às vezes funciona".
fn espelhar_e_girar(imagem: &DynamicImage, corte: &Corte) -> DynamicImage {
    let mut saida = imagem.clone();

    if corte.espelho_h() {
        saida = saida.fliph();
    }
    if corte.espelho_v() {
        saida = saida.flipv();
    }

    match corte.giro_90().rem_euclid(4) {
        1 => saida.rotate90(),
        2 => saida.rotate180(),
        3 => saida.rotate270(),
        _ => saida,
    }
}

/// O recorte quando não há ângulo: cópia de bytes, sem reamostragem.
///
/// 🔑 Vale o caminho separado porque **reamostrar sem precisar borra**: uma
/// interpolação bilinear com deslocamento inteiro ainda mistura vizinhos nas
/// bordas, e o resultado é uma foto ligeiramente menos nítida do que a original —
/// sem ninguém ter pedido nada.
fn recortar_reto(base: &DynamicImage, corte: &Corte) -> DynamicImage {
    // `base` já passou por espelhos e giro: as dimensões aqui são as giradas.
    let (largura, altura) = base.dimensions();
    let (x, y, w, h) = corte.retangulo(largura, altura);
    base.crop_imm(x, y, w, h)
}

/// O recorte com ângulo: para cada pixel de saída, de onde ele vem na origem.
///
/// A conta é a do `image_viewer.rs` invertida — e é invertida de propósito. Girar
/// a imagem inteira e recortar depois precisaria de uma imagem intermediária
/// maior que as duas, e deixaria os cantos vazios visíveis. Perguntando "de onde
/// vem este pixel" não há intermediária, e o que cai fora da foto é resolvido
/// grudando na borda.
fn endireitar_e_recortar(base: &DynamicImage, corte: &Corte) -> DynamicImage {
    let origem = base.to_rgba8();
    let (largura, altura) = origem.dimensions();
    let (largura_f, altura_f) = (largura as f32, altura as f32);

    // `base` já está girada, e `dimensoes_de_saida` volta a girar — por isso a
    // chamada usa as dimensões **de origem** (as giradas, desgiradas de novo é
    // o mesmo par quando o giro é o mesmo). Aqui basta a conta direta, que é a
    // que aquele método faz no ramo com ângulo.
    let (saida_w, saida_h) = corte.dimensoes_de_saida(
        if corte.giro_90().rem_euclid(4) % 2 == 1 {
            altura
        } else {
            largura
        },
        if corte.giro_90().rem_euclid(4) % 2 == 1 {
            largura
        } else {
            altura
        },
    );

    // O aspecto entra na conta porque a rotação é geométrica, e o espaço
    // normalizado (0..1 nos dois eixos) não é. Sem esta correção, endireitar uma
    // foto deitada gira demais na vertical e de menos na horizontal — o horizonte
    // sai torto para o outro lado.
    let aspecto = largura_f / altura_f;
    let (sin, cos) = (-corte.angulo().to_radians()).sin_cos();

    let mut destino = RgbaImage::new(saida_w, saida_h);

    for j in 0..saida_h {
        for i in 0..saida_w {
            let fx = corte.x() + ((i as f32 + 0.5) / saida_w as f32) * corte.largura();
            let fy = corte.y() + ((j as f32 + 0.5) / saida_h as f32) * corte.altura();

            let px = (fx - 0.5) * aspecto;
            let py = fy - 0.5;

            let rx = px * cos - py * sin;
            let ry = px * sin + py * cos;

            let u = (0.5 + rx / aspecto) * largura_f - 0.5;
            let v = (0.5 + ry) * altura_f - 0.5;

            destino.put_pixel(i, j, amostrar(&origem, u, v));
        }
    }

    DynamicImage::ImageRgba8(destino)
}

/// Amostragem bilinear, grudando na borda quando cai fora.
///
/// ⚠️ **Grudar, e não deixar transparente**: o canto que o endireitamento puxa de
/// fora da foto vira uma faixa da cor da borda, que é o que o legado mostra (a
/// malha dele amostra com `clamp`). Transparente ali apareceria como buraco preto
/// na exportação.
fn amostrar(origem: &RgbaImage, u: f32, v: f32) -> Rgba<u8> {
    let (largura, altura) = origem.dimensions();
    let max_x = largura as i64 - 1;
    let max_y = altura as i64 - 1;

    let x0 = u.floor() as i64;
    let y0 = v.floor() as i64;
    let fx = u - x0 as f32;
    let fy = v - y0 as f32;

    let em = |x: i64, y: i64| -> [f32; 4] {
        let x = x.clamp(0, max_x) as u32;
        let y = y.clamp(0, max_y) as u32;
        let p = origem.get_pixel(x, y).0;
        [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32]
    };

    let (a, b, c, d) = (
        em(x0, y0),
        em(x0 + 1, y0),
        em(x0, y0 + 1),
        em(x0 + 1, y0 + 1),
    );

    let mut canais = [0u8; 4];
    for k in 0..4 {
        let cima = a[k] + (b[k] - a[k]) * fx;
        let baixo = c[k] + (d[k] - c[k]) * fx;
        canais[k] = (cima + (baixo - cima) * fy).round().clamp(0.0, 255.0) as u8;
    }

    Rgba(canais)
}

#[cfg(test)]
mod testes {
    /// 🚨 **O que o preview mostra é o que o arquivo recebe.**
    ///
    /// O editor do navegador não pode recalcular o enquadramento por conta
    /// própria: ele desenha o retângulo a partir de [`Corte::retangulo`] e
    /// dimensiona a área a partir de [`Corte::dimensoes_de_saida`]. Se um
    /// desses métodos discordasse de [`aplicar`] por um arredondamento, o
    /// operador enquadraria uma coisa na tela e o cliente receberia outra — e
    /// nada falharia em lugar nenhum.
    ///
    /// Este teste amarra os três sobre uma grade de casos, com e sem ângulo.
    #[test]
    fn as_dimensoes_de_saida_sao_as_do_arquivo() {
        let entrada = quadrantes(64);
        let casos = [
            ("inteiro", Corte::inteiro()),
            (
                "retangulo",
                Corte::novo(0.1, 0.2, 0.5, 0.3, 0, 0.0, false, false),
            ),
            (
                "girado 90",
                Corte::novo(0.1, 0.2, 0.5, 0.3, 1, 0.0, false, false),
            ),
            (
                "girado 180 com espelho",
                Corte::novo(0.0, 0.25, 0.8, 0.5, 2, 0.0, true, false),
            ),
            (
                "com angulo",
                Corte::novo(0.1, 0.1, 0.6, 0.6, 0, 12.0, false, false),
            ),
            (
                "girado com angulo",
                Corte::novo(0.05, 0.15, 0.7, 0.4, 3, -8.0, false, true),
            ),
            (
                "minusculo",
                Corte::novo(0.9, 0.9, 0.01, 0.01, 0, 0.0, false, false),
            ),
        ];
        for (rotulo, corte) in casos {
            let saida = aplicar(&entrada, &corte, true);
            assert_eq!(
                corte.dimensoes_de_saida(64, 64),
                saida.dimensions(),
                "`{rotulo}`: o preview anunciaria um tamanho e o arquivo sairia com outro"
            );
        }
    }

    /// O retângulo mora no espaço já girado, e cabe dentro dele.
    #[test]
    fn o_retangulo_cabe_no_espaco_girado() {
        // 90°: os lados trocam, e o retângulo é medido sobre os lados trocados.
        let corte = Corte::novo(0.5, 0.0, 0.5, 1.0, 1, 0.0, false, false);
        assert_eq!(corte.dimensoes_giradas(80, 40), (40, 80));
        let (x, y, w, h) = corte.retangulo(40, 80);
        assert_eq!((x, y, w, h), (20, 0, 20, 80));

        // Sem giro, os mesmos números valem sobre os lados originais.
        let reto = Corte::novo(0.5, 0.0, 0.5, 1.0, 0, 0.0, false, false);
        assert_eq!(reto.dimensoes_giradas(80, 40), (80, 40));
        assert_eq!(reto.retangulo(80, 40), (40, 0, 40, 40));

        // Nunca sai da imagem, nem com o retângulo colado na borda.
        let borda = Corte::novo(0.99, 0.99, 1.0, 1.0, 0, 0.0, false, false);
        let (x, y, w, h) = borda.retangulo(100, 100);
        assert!(x + w <= 100 && y + h <= 100, "{x},{y},{w},{h}");
        assert!(w >= 1 && h >= 1);
    }

    /// `e_inteiro` é o `e_identidade`, exposto para quem está fora do módulo —
    /// o editor pergunta "há enquadramento?" para saber se pode usar o caminho
    /// rápido da GPU.
    #[test]
    fn e_inteiro_responde_o_mesmo_que_a_identidade() {
        assert!(Corte::inteiro().e_inteiro());
        assert!(Corte::novo(0.00001, 0.0, 0.99999, 1.0, 0, 0.0, false, false).e_inteiro());
        assert!(!Corte::novo(0.1, 0.0, 0.8, 1.0, 0, 0.0, false, false).e_inteiro());
        assert!(!Corte::novo(0.0, 0.0, 1.0, 1.0, 1, 0.0, false, false).e_inteiro());
        assert!(!Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 3.0, false, false).e_inteiro());
        assert!(!Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, true, false).e_inteiro());
    }

    /// 🔑 **O caminho rápido devolve exatamente o que o lento devolveria.**
    ///
    /// Ele existe por desempenho — 8,6 ms por resultado da GPU numa foto de
    /// 2560×2560, medido em 18/ago/2026 — e otimização que muda o resultado não é
    /// otimização, é defeito. O teste compara os dois lados byte a byte.
    #[test]
    fn o_caminho_rapido_da_o_mesmo_que_o_lento() {
        let mut origem = RgbaImage::new(9, 7);
        for (x, y, p) in origem.enumerate_pixels_mut() {
            *p = Rgba([(x * 20) as u8, (y * 30) as u8, ((x + y) * 10) as u8, 255]);
        }
        let imagem = DynamicImage::ImageRgba8(origem);
        let identidade = Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, false, false);

        assert!(e_identidade(&identidade));

        let rapido = aplicar(&imagem, &identidade, true);
        // O lento, chamado direto.
        let lento = recortar_reto(&espelhar_e_girar(&imagem, &identidade), &identidade);

        assert_eq!(rapido.dimensions(), lento.dimensions());
        assert_eq!(rapido.to_rgba8().into_raw(), lento.to_rgba8().into_raw());
    }

    /// ⚠️ **Qualquer coisa diferente de identidade sai do caminho rápido.**
    ///
    /// Um `e_identidade` frouxo devolveria a foto sem cortar — o enquadramento
    /// simplesmente não seria aplicado, e ninguém veria erro.
    #[test]
    fn so_a_identidade_pega_o_caminho_rapido() {
        let casos = [
            (
                "corte",
                Corte::novo(0.1, 0.0, 0.8, 1.0, 0, 0.0, false, false),
            ),
            (
                "giro",
                Corte::novo(0.0, 0.0, 1.0, 1.0, 1, 0.0, false, false),
            ),
            (
                "ângulo",
                Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 5.0, false, false),
            ),
            (
                "espelho h",
                Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, true, false),
            ),
            (
                "espelho v",
                Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, false, true),
            ),
        ];
        for (nome, corte) in casos {
            assert!(!e_identidade(&corte), "`{nome}` passou por identidade");
        }
    }

    /// 🔑 **A folga existe para foto que já passou pelo banco.**
    ///
    /// `Corte::novo` limita os valores, e uma largura que volta como
    /// `0.99999` descreve a foto inteira sem ser `1.0`. Sem folga, o caminho
    /// rápido nunca valeria para foto gravada — que é justamente toda foto.
    #[test]
    fn a_folga_aceita_o_que_voltou_do_banco() {
        let quase = Corte::novo(0.00001, 0.0, 0.99999, 1.0, 0, 0.0, false, false);
        assert!(e_identidade(&quase));
    }

    use super::*;

    fn sem_corte() -> Corte {
        Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, false, false)
    }

    /// Quatro quadrantes de cores distintas: dá para dizer qual canto foi parar
    /// onde depois de girar ou espelhar.
    fn quadrantes(lado: u32) -> DynamicImage {
        let mut img = RgbaImage::new(lado, lado);
        for y in 0..lado {
            for x in 0..lado {
                let cor = match (x < lado / 2, y < lado / 2) {
                    (true, true) => [255, 0, 0, 255],     // superior esquerdo: vermelho
                    (false, true) => [0, 255, 0, 255],    // superior direito: verde
                    (true, false) => [0, 0, 255, 255],    // inferior esquerdo: azul
                    (false, false) => [255, 255, 0, 255], // inferior direito: amarelo
                };
                img.put_pixel(x, y, Rgba(cor));
            }
        }
        DynamicImage::ImageRgba8(img)
    }

    fn canto_superior_esquerdo(img: &DynamicImage) -> [u8; 4] {
        img.to_rgba8().get_pixel(1, 1).0
    }

    #[test]
    fn sem_nada_a_foto_atravessa_igual() {
        let entrada = quadrantes(8);
        let saida = aplicar(&entrada, &sem_corte(), true);

        assert_eq!(saida.dimensions(), (8, 8));
        assert_eq!(saida.to_rgba8().into_raw(), entrada.to_rgba8().into_raw());
    }

    /// 🚨 O recorte devolve **o pedaço certo**, e não só o tamanho certo.
    ///
    /// Conferir só as dimensões deixaria passar um recorte no canto errado — que
    /// é exatamente o que um `crop_x` trocado por `crop_y` produz.
    #[test]
    fn o_recorte_traz_o_quadrante_pedido() {
        let entrada = quadrantes(8);

        // Quadrante superior direito: verde.
        let corte = Corte::novo(0.5, 0.0, 0.5, 0.5, 0, 0.0, false, false);
        let saida = aplicar(&entrada, &corte, true);

        assert_eq!(saida.dimensions(), (4, 4));
        assert_eq!(canto_superior_esquerdo(&saida), [0, 255, 0, 255]);
    }

    /// No modo de corte a foto aparece inteira — senão não haveria o que arrastar.
    #[test]
    fn sem_recortar_a_foto_sai_inteira() {
        let entrada = quadrantes(8);
        let corte = Corte::novo(0.25, 0.25, 0.5, 0.5, 0, 0.0, false, false);

        let saida = aplicar(&entrada, &corte, false);

        assert_eq!(saida.dimensions(), (8, 8), "o recorte não foi aplicado");
    }

    /// Girar 90° leva o canto superior esquerdo para o superior direito.
    #[test]
    fn girar_noventa_leva_o_canto_esquerdo_para_a_direita() {
        let entrada = quadrantes(8);
        let corte = Corte::novo(0.0, 0.0, 1.0, 1.0, 1, 0.0, false, false);

        let saida = aplicar(&entrada, &corte, true);
        let pixels = saida.to_rgba8();

        assert_eq!(
            pixels.get_pixel(6, 1).0,
            [255, 0, 0, 255],
            "o vermelho (era superior esquerdo) tem de estar no superior direito"
        );
    }

    /// Espelhar na horizontal troca esquerda e direita.
    #[test]
    fn espelhar_troca_os_lados() {
        let entrada = quadrantes(8);
        let corte = Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, true, false);

        let saida = aplicar(&entrada, &corte, true);

        assert_eq!(
            canto_superior_esquerdo(&saida),
            [0, 255, 0, 255],
            "o verde (era superior direito) passa a ser o superior esquerdo"
        );
    }

    /// 🚨 Espelho **antes** do giro, como no legado.
    ///
    /// Trocar a ordem dá a foto certa em metade dos casos e a invertida na outra
    /// metade — na conferência isso aparece como "às vezes funciona", que é o
    /// tipo de sintoma que consome um dia.
    #[test]
    fn o_espelho_vem_antes_do_giro() {
        let entrada = quadrantes(8);
        let corte = Corte::novo(0.0, 0.0, 1.0, 1.0, 1, 0.0, true, false);

        let saida = aplicar(&entrada, &corte, true).to_rgba8();

        // espelho h: vermelho vai para a direita; giro 90° CW: a direita vira o
        // rodapé. O vermelho termina no canto inferior direito.
        assert_eq!(saida.get_pixel(6, 6).0, [255, 0, 0, 255]);
    }

    /// ⚠️ Ângulo zero **não** passa pela reamostragem.
    ///
    /// Bilinear com deslocamento inteiro ainda mistura vizinho na borda: a foto
    /// sairia um fio menos nítida do que entrou, em toda foto que ninguém
    /// endireitou. Este teste cobra igualdade byte a byte com o recorte reto.
    #[test]
    fn angulo_zero_nao_reamostra() {
        let entrada = quadrantes(64);
        let corte = Corte::novo(0.25, 0.25, 0.5, 0.5, 0, 0.0, false, false);

        let pelo_caminho_reto = recortar_reto(&entrada, &corte);
        let pela_funcao = aplicar(&entrada, &corte, true);

        assert_eq!(
            pela_funcao.to_rgba8().into_raw(),
            pelo_caminho_reto.to_rgba8().into_raw()
        );
    }

    /// Endireitar mantém o tamanho do recorte e mistura os quadrantes na diagonal.
    #[test]
    fn endireitar_gira_o_conteudo_e_mantem_o_tamanho() {
        let entrada = quadrantes(64);
        let reto = Corte::novo(0.25, 0.25, 0.5, 0.5, 0, 0.0, false, false);
        let torto = Corte::novo(0.25, 0.25, 0.5, 0.5, 0, 30.0, false, false);

        let a = aplicar(&entrada, &reto, true);
        let b = aplicar(&entrada, &torto, true);

        assert_eq!(
            a.dimensions(),
            b.dimensions(),
            "o recorte tem o mesmo tamanho"
        );
        assert_ne!(
            a.to_rgba8().into_raw(),
            b.to_rgba8().into_raw(),
            "30° tem de mudar alguma coisa"
        );
    }

    /// 🚨 **O endireitamento inclina a linha do horizonte, e para que lado.**
    ///
    /// `o_sinal_do_angulo_importa` prova que +θ e −θ diferem; este prova o que
    /// eles **fazem** — e é o que o preview do navegador precisa saber para
    /// girar do mesmo lado. Um sinal trocado aqui passaria por todos os outros
    /// testes e faria o operador endireitar o horizonte para o lado errado.
    #[test]
    fn o_endireitamento_inclina_a_linha_do_horizonte_no_sentido_horario() {
        // Metade de cima branca, metade de baixo preta: uma linha reta no meio.
        let lado = 200u32;
        let mut img = RgbaImage::new(lado, lado);
        for (_, y, p) in img.enumerate_pixels_mut() {
            *p = if y < lado / 2 {
                Rgba([255, 255, 255, 255])
            } else {
                Rgba([0, 0, 0, 255])
            };
        }
        let entrada = DynamicImage::ImageRgba8(img);

        // Recorte central, para os cantos não virem de fora da foto.
        let reto = Corte::novo(0.25, 0.25, 0.5, 0.5, 0, 0.0, false, false);
        let torto = Corte::novo(0.25, 0.25, 0.5, 0.5, 0, 20.0, false, false);

        let transicao = |img: &DynamicImage, x: u32| -> u32 {
            let rgba = img.to_rgba8();
            (0..rgba.height())
                .find(|&y| rgba.get_pixel(x, y).0[0] < 128)
                .unwrap_or(rgba.height())
        };

        let a = aplicar(&entrada, &reto, true);
        assert_eq!(
            transicao(&a, 5),
            transicao(&a, a.width() - 6),
            "sem ângulo a linha é horizontal"
        );

        let b = aplicar(&entrada, &torto, true);
        let (esquerda, direita) = (transicao(&b, 5), transicao(&b, b.width() - 6));
        assert!(
            direita > esquerda,
            "girar +20° baixa o lado direito da linha: esquerda {esquerda}, direita {direita}"
        );
    }

    /// 🚨 Endireitar por +θ e por −θ não pode dar a mesma imagem.
    ///
    /// É o teste que pega o sinal trocado na rotação — o defeito que faz o
    /// horizonte tombar para o lado errado, e que ninguém percebe olhando um
    /// gradiente.
    #[test]
    fn o_sinal_do_angulo_importa() {
        let entrada = quadrantes(64);
        let esquerda = Corte::novo(0.25, 0.25, 0.5, 0.5, 0, 12.0, false, false);
        let direita = Corte::novo(0.25, 0.25, 0.5, 0.5, 0, -12.0, false, false);

        assert_ne!(
            aplicar(&entrada, &esquerda, true).to_rgba8().into_raw(),
            aplicar(&entrada, &direita, true).to_rgba8().into_raw()
        );
    }

    /// ⚠️ O que o endireitamento puxa de fora da foto vira borda, e não buraco.
    ///
    /// Um recorte que ocupa a foto inteira com ângulo tem cantos que vêm de fora.
    /// Se a amostragem devolvesse transparente ali, a exportação mostraria quatro
    /// triângulos pretos.
    #[test]
    fn o_que_vem_de_fora_gruda_na_borda() {
        let entrada = quadrantes(32);
        let corte = Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 20.0, false, false);

        let saida = aplicar(&entrada, &corte, true).to_rgba8();

        for pixel in saida.pixels() {
            assert_eq!(pixel.0[3], 255, "nenhum pixel pode sair transparente");
        }
    }

    /// Corte mínimo não gera imagem de lado zero.
    #[test]
    fn corte_minusculo_ainda_tem_um_pixel() {
        let entrada = quadrantes(8);
        let corte = Corte::novo(0.0, 0.0, 0.01, 0.01, 0, 0.0, false, false);

        let saida = aplicar(&entrada, &corte, true);
        assert!(saida.width() >= 1 && saida.height() >= 1);
    }
}

#[cfg(test)]
mod testes_do_enquadramento {
    use super::{amostrar, aplicar, uvs_do_enquadramento, Corte};

    /// Uma foto **retangular** com cada pixel identificável pela cor.
    ///
    /// 🔑 **Retangular de propósito, e não quadrada.** Num quadrado, trocar os
    /// eixos do giro de 90° dá certo por acidente: os dois divisores são iguais,
    /// e um erro de normalização desaparece. É exatamente o caso que uma foto de
    /// câmera nunca é.
    fn foto() -> image::DynamicImage {
        let (w, h) = (8u32, 5u32);
        let mut img = image::RgbaImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            // Vermelho conta a coluna, verde conta a linha: qualquer troca de
            // eixo ou espelho aparece na cor.
            *p = image::Rgba([(x * 30) as u8, (y * 50) as u8, 7, 255]);
        }
        image::DynamicImage::ImageRgba8(img)
    }

    /// Amostra a textura pela UV que o shader calcularia para `(s, t)`.
    ///
    /// 🔑 **Interpola como a GPU, e não pega o pixel mais próximo.** O
    /// amostrador em modo linear mistura os quatro vizinhos a partir do centro
    /// do texel (`uv · tamanho − 0,5`), que é exatamente o que
    /// [`amostrar`] faz no caminho com ângulo. Comparar bilinear com "vizinho
    /// mais próximo" acusaria diferença em toda foto endireitada — e esconderia
    /// a diferença de verdade no meio do ruído de arredondamento.
    fn pela_uv(
        origem: &image::DynamicImage,
        uvs: ([f32; 2], [f32; 2], [f32; 2]),
        s: f32,
        t: f32,
    ) -> [u8; 4] {
        let (ux, uy, uoff) = uvs;
        let u = uoff[0] + ux[0] * s + uy[0] * t;
        let v = uoff[1] + ux[1] * s + uy[1] * t;
        let img = origem.to_rgba8();
        let (w, h) = (img.width() as f32, img.height() as f32);
        amostrar(&img, u * w - 0.5, v * h - 0.5).0
    }

    /// O que a tela do cliente mostra tem de ser o que o arquivo tem.
    ///
    /// 🚨 É a prova do que o dono relatou em 2026-09-12: a revelação e a
    /// biblioteca giravam certo e a tela do cliente, não. O juiz é o
    /// `transformacao::aplicar` do core — a mesma função que recorta o JPEG.
    fn confere(corte: &Corte, caso: &str) {
        confere_com(corte, caso, 0);
    }

    /// `folga` é quanto cada canal pode diferir.
    ///
    /// ⚠️ **Zero no caminho reto, e alguma folga no endireitado.** Sem ângulo as
    /// duas contas caem no mesmo pixel e a igualdade é exata. Com ângulo, as
    /// duas interpolam a partir de coordenadas calculadas em ordens diferentes
    /// (a GPU em UV, o core em pixel), e o último bit não tem por que bater —
    /// o que precisa bater é a **posição**, e um erro de posição move o pixel
    /// inteiro, muito além da folga.
    fn confere_com(corte: &Corte, caso: &str, folga: i32) {
        let origem = foto();
        let esperada = aplicar(&origem, corte, true).to_rgba8();
        let uvs = uvs_do_enquadramento(origem.width(), origem.height(), corte);

        for j in 0..esperada.height() {
            for i in 0..esperada.width() {
                let s = (i as f32 + 0.5) / esperada.width() as f32;
                let t = (j as f32 + 0.5) / esperada.height() as f32;
                let vista = pela_uv(&origem, uvs, s, t);
                let arquivo = esperada.get_pixel(i, j).0;
                let longe = (0..3).any(|c| (vista[c] as i32 - arquivo[c] as i32).abs() > folga);
                assert!(
                    !longe,
                    "{caso}: em ({i},{j}) a tela do cliente mostra {vista:?} e o arquivo tem {arquivo:?}"
                );
            }
        }
    }

    fn corte(giro: i32, espelho_h: bool, espelho_v: bool) -> Corte {
        Corte::novo(0.0, 0.0, 1.0, 1.0, giro, 0.0, espelho_h, espelho_v)
    }

    #[test]
    fn sem_enquadramento_a_foto_e_ela_mesma() {
        confere(&corte(0, false, false), "neutro");
    }

    #[test]
    fn os_quatro_giros_de_90_batem_com_o_arquivo() {
        for giro in [1, 2, 3] {
            confere(&corte(giro, false, false), &format!("giro {giro}"));
        }
    }

    #[test]
    fn os_espelhos_batem_com_o_arquivo() {
        confere(&corte(0, true, false), "espelho horizontal");
        confere(&corte(0, false, true), "espelho vertical");
    }

    #[test]
    fn giro_com_espelho_bate_com_o_arquivo() {
        // 🚨 O caso em que a **ordem** aparece: espelhar e depois girar não dá o
        // mesmo que girar e depois espelhar.
        for giro in [1, 2, 3] {
            confere(
                &corte(giro, true, false),
                &format!("giro {giro} + espelho h"),
            );
            confere(
                &corte(giro, false, true),
                &format!("giro {giro} + espelho v"),
            );
        }
    }

    #[test]
    fn o_endireitamento_bate_com_o_arquivo() {
        // 🚨 O caso que o dono viu em 2026-09-12: no editor a foto aparece
        // endireitada e na tela do cliente, reta. Os testes de giro de 90°
        // passavam porque nenhum deles tinha ângulo.
        for grau in [-8.0, -3.0, 3.0, 8.0] {
            let c = Corte::novo(0.1, 0.1, 0.8, 0.8, 0, grau, false, false);
            // Folga de 1: as duas interpolam, em ordens diferentes.
            confere_com(&c, &format!("endireitamento {grau}°"), 1);
        }
    }

    #[test]
    fn endireitamento_com_giro_e_espelho_bate_com_o_arquivo() {
        // 🚨 O caso completo, e o que o balcão faz de verdade: girar um quarto
        // de volta e endireitar o horizonte na mesma foto.
        for giro in [0, 1, 2, 3] {
            for espelho in [false, true] {
                let c = Corte::novo(0.15, 0.1, 0.7, 0.75, giro, -6.0, espelho, false);
                confere_com(&c, &format!("giro {giro} + 6° + espelho {espelho}"), 1);
            }
        }
    }

    #[test]
    fn recorte_com_giro_bate_com_o_arquivo() {
        // Meio quadro, girado: recorte e giro se compõem, e é onde um erro de
        // normalização desloca a imagem em vez de distorcê-la.
        for giro in [0, 1, 2, 3] {
            let c = Corte::novo(0.25, 0.25, 0.5, 0.5, giro, 0.0, false, false);
            confere(&c, &format!("recorte + giro {giro}"));
        }
    }
}
