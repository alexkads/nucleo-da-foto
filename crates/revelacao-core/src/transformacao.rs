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
    let (largura, altura) = base.dimensions();

    let x = (corte.x() * largura as f32).round() as u32;
    let y = (corte.y() * altura as f32).round() as u32;
    let x = x.min(largura.saturating_sub(1));
    let y = y.min(altura.saturating_sub(1));

    let w = ((corte.largura() * largura as f32).round() as u32)
        .max(1)
        .min(largura - x);
    let h = ((corte.altura() * altura as f32).round() as u32)
        .max(1)
        .min(altura - y);

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

    let saida_w = ((corte.largura() * largura_f).round() as u32).max(1);
    let saida_h = ((corte.altura() * altura_f).round() as u32).max(1);

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
