//! A composição: a base com as camadas por cima, de baixo para cima, cada uma
//! no seu modo de mesclagem (`mesclagem.rs`).
//!
//! ```text
//! a   = alfa_do_pixel / 255 · opacidade_da_camada
//! out = baixo + (B(baixo, cor) − baixo) · a        (Normal: B = cor)
//! ```
//!
//! 🔑 **Com `a = 0` a conta devolve a base exata** — não "quase": é o C30, uma
//! camada vazia compõe a base byte a byte, e a Revelação da imagem editada é a
//! mesma do bruto. Por isso o caminho de `a = 0` sai antes da conta em ponto
//! flutuante.

use image::RgbImage;

use crate::ajuste::{Ajuste, Preparado};
use crate::documento::{Camada, Documento, LeitorDaMascara, MascaraNoTile, Papel};
use crate::mesclagem::{mesclar, Modo};
use crate::retangulo::Retangulo;
use crate::tiles::{indice, retangulo_do_tile, CamadaDePixels, LADO_DO_TILE};

/// Um pixel da camada sobre um pixel de baixo, no modo Normal.
#[inline]
pub fn sobre(baixo: [u8; 3], cima: [u8; 4], opacidade: f32) -> [u8; 3] {
    mesclar(baixo, cima, opacidade, Modo::Normal)
}

/// Um tile de camada na composição: os pixels (`None` = camada de ajuste: a
/// cor de cima é a de baixo ajustada), a conta do ajuste, a opacidade (já com
/// a máscara quando ela é lisa no tile), o modo, a máscara do tile com o fundo
/// dela, e o papel no recorte.
struct TileNaComposicao<'a> {
    pixels: Option<&'a [u8]>,
    preparado: Option<&'a Preparado>,
    opacidade: f32,
    modo: Modo,
    mascara: Option<MascaraNoTile<'a>>,
    papel: Papel,
}

impl TileNaComposicao<'_> {
    /// A opacidade no pixel `j` do tile, com a máscara.
    #[inline]
    fn opacidade_em(&self, j: usize) -> f32 {
        match &self.mascara {
            None => self.opacidade,
            Some(m) => {
                let v = m.valor(j);
                if v == 0 {
                    return 0.0;
                }
                self.opacidade * v as f32 / 255.0
            }
        }
    }

    /// A cor de cima no pixel `j`, sobre `baixo`.
    #[inline]
    fn cima(&self, j: usize, baixo: [u8; 3]) -> Option<[u8; 4]> {
        match (self.pixels, self.preparado) {
            (Some(t), _) => Some([t[j], t[j + 1], t[j + 2], t[j + 3]]),
            (None, Some(p)) => {
                let [r, g, b] = p.aplicar(baixo);
                Some([r, g, b, 255])
            }
            (None, None) => None,
        }
    }
}

/// A imagem editada em resolução cheia.
pub fn compor(base: &RgbImage, doc: &Documento) -> RgbImage {
    let mut saida = base.clone();
    compor_regiao(
        base,
        doc,
        &Retangulo::inteiro(base.width(), base.height()),
        &mut saida,
    );
    saida
}

/// Recompõe só `ret` dentro de `saida` (do tamanho da base).
///
/// 🔑 **Anda por tile, e não por pixel**: a camada é um mapa de tiles, e
/// perguntar por cada um dos 24 milhões de pixels de uma foto grande seriam 24
/// milhões de buscas no mapa. Aqui é uma busca por tile e por camada.
pub fn compor_regiao(base: &RgbImage, doc: &Documento, ret: &Retangulo, saida: &mut RgbImage) {
    let largura_da_saida = saida.width();
    compor_deslocado(base, doc, ret, saida, largura_da_saida, (0, 0));
}

/// Só o recorte `ret`, numa imagem do tamanho dele — a vista usa isto para não
/// alocar a foto inteira a cada gesto.
pub fn compor_recorte(base: &RgbImage, doc: &Documento, ret: &Retangulo) -> RgbImage {
    let ret = ret.limitado(base.width(), base.height());
    let mut saida = RgbImage::new(ret.largura, ret.altura);
    compor_deslocado(base, doc, &ret, &mut saida, ret.largura, (ret.x, ret.y));
    saida
}

/// Um tile de uma camada na composição, ou `None` quando ela não muda nada
/// nele (sem pixels ali, ou a máscara esconde o tile inteiro).
fn tile_na_composicao<'a>(
    c: &'a Camada,
    leitor: Option<&'a LeitorDaMascara<'a>>,
    preparado: Option<&'a Preparado>,
    posicao: crate::tiles::Posicao,
    papel: Papel,
) -> Option<TileNaComposicao<'a>> {
    let pixels = match preparado {
        Some(_) => None,
        None => Some(c.pixels.tile(posicao)?.as_slice()),
    };
    let (opacidade, mascara) = match leitor.map(|l| l.no_tile(posicao)) {
        None => (c.opacidade, None),
        Some(MascaraNoTile::Constante(0)) => return None,
        Some(MascaraNoTile::Constante(v)) => (c.opacidade * v as f32 / 255.0, None),
        Some(m) => (c.opacidade, Some(m)),
    };
    Some(TileNaComposicao {
        pixels,
        preparado,
        opacidade,
        modo: c.modo,
        mascara,
        papel,
    })
}

/// Compõe `ret` e escreve em `destino`, onde o pixel `(x, y)` da foto mora em
/// `(x − origem.0, y − origem.1)`.
///
/// 🔑 **Máscara de corte** (`Papel::Base` seguida de `Papel::Recortada`): as
/// recortadas se mesclam **na cor da base**, com a transparência dela travada
/// — `g = mesclar(g, recortada, opacidade·máscara, modo)`, a partir de `g` =
/// a cor da base —, e o conjunto entra na foto como a base entraria, com o
/// alfa, a máscara, a opacidade e o modo **dela**: `mesclar(foto, [g, α_base],
/// opacidade_base·máscara_base, modo_base)`. Assim cada opacidade e cada
/// máscara entra uma vez, a borda semitransparente da base limita a recortada
/// na mesma proporção (α_recortada × α_base, a regra do Photoshop), e mesclar
/// a recortada na base ([`crate::operacoes::mesclar_recortada_na_base`]) dá a
/// mesma foto byte a byte.
fn compor_deslocado(
    base: &RgbImage,
    doc: &Documento,
    ret: &Retangulo,
    destino: &mut [u8],
    largura_do_destino: u32,
    origem: (u32, u32),
) {
    let (largura, altura) = (base.width(), base.height());
    let ret = ret.limitado(largura, altura);
    if ret.vazio() {
        return;
    }
    let destino_em_bytes = largura_do_destino as usize * 3;
    let papeis = doc.papeis();
    let camadas: Vec<(&Camada, Papel)> = doc
        .camadas
        .iter()
        .zip(papeis)
        .filter(|(_, p)| *p != Papel::Fora)
        .collect();
    // A conta de cada ajuste, montada uma vez (as tabelas de 256).
    let preparados: Vec<Option<Preparado>> = camadas
        .iter()
        .map(|(c, _)| c.ajuste.as_ref().map(Ajuste::preparar))
        .collect();
    // A máscara de cada uma, pronta (o mapa da difusão, a densidade).
    let leitores: Vec<Option<LeitorDaMascara>> = camadas
        .iter()
        .map(|(c, _)| c.mascara_ativa().map(|m| m.leitor()))
        .collect();
    let referencia = CamadaDePixels::nova(largura, altura);
    let fonte = base.as_raw();
    let largura_em_bytes = largura as usize * 3;
    for posicao in referencia.tiles_do_retangulo(&ret) {
        let pedaco = retangulo_do_tile(posicao, largura, altura).limitado(largura, altura);
        let pedaco = interseccao(&pedaco, &ret);
        // A máscara de cada camada neste tile: sem tile pintado nela, o tile
        // inteiro vale o fundo — e entra na opacidade de uma vez.
        let mut tiles: Vec<TileNaComposicao> = Vec::with_capacity(camadas.len());
        let mut base_presente = false;
        for (((c, papel), preparado), leitor) in camadas.iter().zip(&preparados).zip(&leitores) {
            let t = tile_na_composicao(c, leitor.as_ref(), preparado.as_ref(), posicao, *papel);
            match papel {
                // Sem a base neste tile, as recortadas nele também somem.
                Papel::Recortada if !base_presente => continue,
                Papel::Base | Papel::Solta => base_presente = t.is_some(),
                _ => {}
            }
            tiles.extend(t);
        }
        // A base que ficou sem recortada neste tile volta à conta de sempre.
        for k in 0..tiles.len() {
            if tiles[k].papel == Papel::Base
                && tiles.get(k + 1).is_none_or(|t| t.papel != Papel::Recortada)
            {
                tiles[k].papel = Papel::Solta;
            }
        }
        for y in pedaco.y..pedaco.baixo() {
            let inicio = y as usize * largura_em_bytes + pedaco.x as usize * 3;
            let fim = inicio + pedaco.largura as usize * 3;
            let d_inicio =
                (y - origem.1) as usize * destino_em_bytes + (pedaco.x - origem.0) as usize * 3;
            if tiles.is_empty() {
                destino[d_inicio..d_inicio + (fim - inicio)].copy_from_slice(&fonte[inicio..fim]);
                continue;
            }
            let ty = y % LADO_DO_TILE;
            for x in pedaco.x..pedaco.direita() {
                let i = y as usize * largura_em_bytes + x as usize * 3;
                let mut pixel = [fonte[i], fonte[i + 1], fonte[i + 2]];
                let j = indice(x % LADO_DO_TILE, ty);
                let mut k = 0;
                while k < tiles.len() {
                    let t = &tiles[k];
                    k += 1;
                    let opacidade = t.opacidade_em(j);
                    if t.papel == Papel::Base {
                        let inicio_do_conjunto = k;
                        while tiles.get(k).is_some_and(|r| r.papel == Papel::Recortada) {
                            k += 1;
                        }
                        let Some(b) = t.cima(j, pixel) else { continue };
                        if opacidade <= 0.0 || b[3] == 0 {
                            continue;
                        }
                        let mut g = [b[0], b[1], b[2]];
                        for r in &tiles[inicio_do_conjunto..k] {
                            let o = r.opacidade_em(j);
                            if o <= 0.0 {
                                continue;
                            }
                            if let Some(cima) = r.cima(j, g) {
                                g = mesclar(g, cima, o, r.modo);
                            }
                        }
                        pixel = mesclar(pixel, [g[0], g[1], g[2], b[3]], opacidade, t.modo);
                        continue;
                    }
                    if opacidade <= 0.0 {
                        continue;
                    }
                    if let Some(cima) = t.cima(j, pixel) {
                        pixel = mesclar(pixel, cima, opacidade, t.modo);
                    }
                }
                let d = d_inicio + (x - pedaco.x) as usize * 3;
                destino[d..d + 3].copy_from_slice(&pixel);
            }
        }
    }
}

/// O que o palco mostra: a foto, ou a máscara de uma camada — sozinha, em
/// cinza (⌥ + clique na miniatura da máscara), ou como sobreposição rubi sobre
/// a foto (`\`). Só a tela: a imagem editada é sempre a foto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Exibicao {
    #[default]
    Foto,
    SoAMascara(usize),
    Rubi(usize),
}

/// O vermelho da sobreposição e quanto ele cobre onde a máscara esconde tudo
/// (os 50% do Photoshop).
const RUBI: [f32; 3] = [255.0, 0.0, 0.0];
const OPACIDADE_DO_RUBI: f32 = 0.5;

/// [`compor_recorte`] com o que o palco mostra.
pub fn compor_recorte_exibindo(
    base: &RgbImage,
    doc: &Documento,
    ret: &Retangulo,
    exibicao: Exibicao,
) -> RgbImage {
    let mascara = match exibicao {
        Exibicao::Foto => None,
        Exibicao::SoAMascara(i) | Exibicao::Rubi(i) => {
            doc.camadas.get(i).and_then(|c| c.mascara.as_ref())
        }
    };
    let Some(mascara) = mascara else {
        return compor_recorte(base, doc, ret);
    };
    let ret = ret.limitado(base.width(), base.height());
    let valores = valores_da_mascara(mascara, &ret);
    if let Exibicao::SoAMascara(_) = exibicao {
        return RgbImage::from_fn(ret.largura, ret.altura, |x, y| {
            let v = valores[(y * ret.largura + x) as usize];
            image::Rgb([v, v, v])
        });
    }
    let mut foto = compor_recorte(base, doc, &ret);
    for (p, v) in foto.pixels_mut().zip(valores) {
        let a = OPACIDADE_DO_RUBI * (255 - v) as f32 / 255.0;
        if a > 0.0 {
            for (canal, rubi) in p.0.iter_mut().zip(RUBI) {
                *canal = (*canal as f32 + (rubi - *canal as f32) * a).round() as u8;
            }
        }
    }
    foto
}

/// O valor da máscara (com a densidade e a difusão) em cada pixel de `ret`,
/// linha a linha.
fn valores_da_mascara(mascara: &crate::documento::Mascara, ret: &Retangulo) -> Vec<u8> {
    let leitor = mascara.leitor();
    let mut valores = vec![0u8; (ret.largura * ret.altura) as usize];
    let referencia = CamadaDePixels::nova(mascara.pixels.largura(), mascara.pixels.altura());
    for posicao in referencia.tiles_do_retangulo(ret) {
        let no_tile = leitor.no_tile(posicao);
        let pedaco = interseccao(
            &retangulo_do_tile(posicao, mascara.pixels.largura(), mascara.pixels.altura()),
            ret,
        );
        for y in pedaco.y..pedaco.baixo() {
            for x in pedaco.x..pedaco.direita() {
                valores[((y - ret.y) * ret.largura + x - ret.x) as usize] =
                    no_tile.valor(indice(x % LADO_DO_TILE, y % LADO_DO_TILE));
            }
        }
    }
    valores
}

/// A parte comum de dois retângulos.
pub fn interseccao(a: &Retangulo, b: &Retangulo) -> Retangulo {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    Retangulo::novo(
        x,
        y,
        a.direita().min(b.direita()).saturating_sub(x),
        a.baixo().min(b.baixo()).saturating_sub(y),
    )
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::documento::BaseRef;

    fn base() -> RgbImage {
        RgbImage::from_fn(300, 280, |x, y| {
            image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x + y) % 7) as u8])
        })
    }

    #[test]
    fn camada_vazia_compoe_a_base_byte_a_byte() {
        let base = base();
        let doc = Documento::novo(BaseRef::da_imagem(&base));
        assert!(doc.neutro());
        assert_eq!(compor(&base, &doc).as_raw(), base.as_raw());
    }

    #[test]
    fn visibilidade_e_opacidade_entram_na_composicao() {
        let base = base();
        let mut doc = Documento::novo(BaseRef::da_imagem(&base));
        let tile = doc.camadas[0].pixels.tile_mut((0, 0));
        tile[..4].copy_from_slice(&[255, 0, 0, 255]);
        let cheia = compor(&base, &doc);
        assert_eq!(cheia.get_pixel(0, 0).0, [255, 0, 0]);
        assert_eq!(cheia.get_pixel(1, 0).0, base.get_pixel(1, 0).0);

        doc.camadas[0].opacidade = 0.5;
        let meia = compor(&base, &doc).get_pixel(0, 0).0;
        assert_eq!(meia, [128, 0, 0], "metade do vermelho sobre (0,0,0)");

        doc.camadas[0].visivel = false;
        assert!(doc.neutro());
        assert_eq!(compor(&base, &doc).as_raw(), base.as_raw());

        doc.camadas[0].visivel = true;
        doc.camadas[0].opacidade = 0.0;
        assert_eq!(compor(&base, &doc).as_raw(), base.as_raw());
    }

    #[test]
    fn as_camadas_compoem_de_baixo_para_cima_cada_uma_no_seu_modo() {
        let base = RgbImage::from_pixel(10, 10, image::Rgb([200, 100, 50]));
        let mut doc = Documento::novo(BaseRef::da_imagem(&base));
        doc.camadas[0].pixels.tile_mut((0, 0))[..4].copy_from_slice(&[0, 0, 255, 255]);
        let mut de_cima = Camada::nova("Camada 1", 10, 10);
        de_cima.pixels.tile_mut((0, 0))[..4].copy_from_slice(&[255, 255, 255, 255]);
        de_cima.modo = Modo::Multiplicacao;
        doc.camadas.push(de_cima);
        // Branco em Multiplicação não muda o azul de baixo.
        assert_eq!(compor(&base, &doc).get_pixel(0, 0).0, [0, 0, 255]);
        // Em Normal, a de cima vence.
        doc.camadas[1].modo = Modo::Normal;
        assert_eq!(compor(&base, &doc).get_pixel(0, 0).0, [255, 255, 255]);
        // Trocadas de lugar, o azul vence.
        doc.camadas.swap(0, 1);
        assert_eq!(compor(&base, &doc).get_pixel(0, 0).0, [0, 0, 255]);
        assert_eq!(compor(&base, &doc).get_pixel(1, 0).0, [200, 100, 50]);
    }

    #[test]
    fn a_composicao_de_uma_regiao_e_a_mesma_da_foto_inteira() {
        let base = base();
        let mut doc = Documento::novo(BaseRef::da_imagem(&base));
        let tile = doc.camadas[0].pixels.tile_mut((1, 1));
        for i in 0..tile.len() / 4 {
            tile[i * 4..i * 4 + 4].copy_from_slice(&[(i % 256) as u8, 40, 200, (i % 200) as u8]);
        }
        let inteira = compor(&base, &doc);
        let mut parcial = base.clone();
        compor_regiao(
            &base,
            &doc,
            &Retangulo::novo(250, 250, 50, 30),
            &mut parcial,
        );
        let recorte = compor_recorte(&base, &doc, &Retangulo::novo(240, 245, 60, 35));
        for y in 250..280 {
            for x in 250..300 {
                assert_eq!(parcial.get_pixel(x, y), inteira.get_pixel(x, y));
                assert_eq!(recorte.get_pixel(x - 240, y - 245), inteira.get_pixel(x, y));
            }
        }
    }
}
