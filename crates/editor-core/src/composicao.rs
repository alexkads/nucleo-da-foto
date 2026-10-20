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
use crate::documento::{
    Camada, Documento, LeitorDaMascara, LeitorDasMascaras, Mascara, MascaraNoTile, Papel,
};
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
    compor_deslocado(base, doc, ret, saida, largura_da_saida, (0, 0), None);
}

/// Só o recorte `ret`, numa imagem do tamanho dele — a vista usa isto para não
/// alocar a foto inteira a cada gesto.
pub fn compor_recorte(base: &RgbImage, doc: &Documento, ret: &Retangulo) -> RgbImage {
    let ret = ret.limitado(base.width(), base.height());
    let mut saida = RgbImage::new(ret.largura, ret.altura);
    compor_deslocado(
        base,
        doc,
        &ret,
        &mut saida,
        ret.largura,
        (ret.x, ret.y),
        None,
    );
    saida
}

/// Um tile de uma camada na composição, ou `None` quando ela não muda nada
/// nele (sem pixels ali, ou a máscara esconde o tile inteiro).
fn tile_na_composicao<'a>(
    c: &'a Camada,
    leitor: Option<&'a LeitorDasMascaras<'a>>,
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
/// `(x − origem.0, y − origem.1)`. `sem` (índices do documento, inclusive)
/// fica de fora — o "original" do tratamento de pele, só na tela.
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
    sem: Option<(usize, usize)>,
) {
    let ret = ret.limitado(base.width(), base.height());
    if ret.vazio() {
        return;
    }
    Composicao::nova(base, doc, sem).compor_em(&ret, destino, largura_do_destino, origem);
}

/// A composição de um documento **preparada uma vez** para ser feita em
/// vários pedaços: as camadas que entram com o papel de cada uma, a conta de
/// cada ajuste e o leitor de cada máscara.
///
/// 🔑 **A preparação é cara e não depende do pedaço.** `papeis` varre o alfa
/// das camadas, cada ajuste monta tabelas de 256 (com `powf` nos Níveis) e
/// cada máscara pega o mapa da difusão e a cobertura da vetorial. O rascunho
/// do controle deslizante compõe a vista linha a linha — de mil a duas mil
/// vezes por tique, e a preparação era refeita em cada uma. Daqui sai a mesma
/// foto byte a byte: só a conta repetida deixou de ser feita.
pub(crate) struct Composicao<'a> {
    base: &'a RgbImage,
    camadas: Vec<(&'a Camada, Papel)>,
    preparados: Vec<Option<Preparado>>,
    leitores: Vec<Option<LeitorDasMascaras<'a>>>,
}

impl<'a> Composicao<'a> {
    /// Prepara `doc` sobre `base`. `sem` (índices do documento, inclusive)
    /// fica de fora — ver [`compor_deslocado`].
    pub(crate) fn nova(
        base: &'a RgbImage,
        doc: &'a Documento,
        sem: Option<(usize, usize)>,
    ) -> Self {
        let papeis = doc.papeis();
        let camadas: Vec<(&Camada, Papel)> = doc
            .camadas
            .iter()
            .zip(papeis)
            .enumerate()
            .filter(|(i, (_, p))| *p != Papel::Fora && sem.is_none_or(|(a, b)| *i < a || *i > b))
            .map(|(_, par)| par)
            .collect();
        // A conta de cada ajuste, montada uma vez (as tabelas de 256).
        let preparados: Vec<Option<Preparado>> = camadas
            .iter()
            .map(|(c, _)| c.ajuste.as_ref().map(Ajuste::preparar))
            .collect();
        // As máscaras de cada uma, prontas (o mapa da difusão, a densidade, a
        // cobertura da vetorial).
        let leitores: Vec<Option<LeitorDasMascaras>> = camadas
            .iter()
            .map(|(c, _)| c.leitor_das_mascaras())
            .collect();
        Composicao {
            base,
            camadas,
            preparados,
            leitores,
        }
    }

    /// Só o recorte `ret`, numa imagem do tamanho dele — o [`compor_recorte`].
    pub(crate) fn recorte(&self, ret: &Retangulo) -> RgbImage {
        let ret = ret.limitado(self.base.width(), self.base.height());
        let mut saida = RgbImage::new(ret.largura, ret.altura);
        if !ret.vazio() {
            self.compor_em(&ret, &mut saida, ret.largura, (ret.x, ret.y));
        }
        saida
    }

    /// Compõe `ret` (já limitado à foto e não vazio) em `destino` — ver
    /// [`compor_deslocado`].
    fn compor_em(
        &self,
        ret: &Retangulo,
        destino: &mut [u8],
        largura_do_destino: u32,
        origem: (u32, u32),
    ) {
        let base = self.base;
        let ret = *ret;
        let (largura, altura) = (base.width(), base.height());
        let destino_em_bytes = largura_do_destino as usize * 3;
        let referencia = CamadaDePixels::nova(largura, altura);
        let fonte = base.as_raw();
        let largura_em_bytes = largura as usize * 3;
        for posicao in referencia.tiles_do_retangulo(&ret) {
            let pedaco = retangulo_do_tile(posicao, largura, altura).limitado(largura, altura);
            let pedaco = interseccao(&pedaco, &ret);
            let tiles = self.tiles_em(posicao);
            for y in pedaco.y..pedaco.baixo() {
                let inicio = y as usize * largura_em_bytes + pedaco.x as usize * 3;
                let fim = inicio + pedaco.largura as usize * 3;
                let d_inicio =
                    (y - origem.1) as usize * destino_em_bytes + (pedaco.x - origem.0) as usize * 3;
                if tiles.is_empty() {
                    destino[d_inicio..d_inicio + (fim - inicio)]
                        .copy_from_slice(&fonte[inicio..fim]);
                    continue;
                }
                let ty = y % LADO_DO_TILE;
                for x in pedaco.x..pedaco.direita() {
                    let i = y as usize * largura_em_bytes + x as usize * 3;
                    let baixo = [fonte[i], fonte[i + 1], fonte[i + 2]];
                    let pixel = pixel_composto(&tiles, baixo, indice(x % LADO_DO_TILE, ty));
                    let d = d_inicio + (x - pedaco.x) as usize * 3;
                    destino[d..d + 3].copy_from_slice(&pixel);
                }
            }
        }
    }

    /// Só os pixels `(x, y)` de cada `x` em `xs` (em ordem crescente, dentro
    /// da foto), em RGB, um atrás do outro — o rascunho da vista.
    ///
    /// 🔑 **O rascunho usa um pixel da foto por pixel da vista**; compor a
    /// linha inteira para jogar fora f − 1 de cada f pixels custava f vezes o
    /// necessário. A conta de cada pixel é a de [`Self::recorte`], pela mesma
    /// [`pixel_composto`]: a amostra sai byte a byte igual.
    pub(crate) fn amostras(&self, y: u32, xs: &[u32]) -> Vec<u8> {
        let fonte = self.base.as_raw();
        let largura_em_bytes = self.base.width() as usize * 3;
        let ty = y % LADO_DO_TILE;
        let mut saida = Vec::with_capacity(xs.len() * 3);
        let mut k = 0;
        while k < xs.len() {
            // Os `x` do mesmo tile compartilham a preparação do tile.
            let posicao = crate::tiles::tile_de(xs[k], y);
            let tiles = self.tiles_em(posicao);
            while k < xs.len() && crate::tiles::tile_de(xs[k], y) == posicao {
                let x = xs[k];
                let i = y as usize * largura_em_bytes + x as usize * 3;
                let baixo = [fonte[i], fonte[i + 1], fonte[i + 2]];
                let pixel = if tiles.is_empty() {
                    baixo
                } else {
                    pixel_composto(&tiles, baixo, indice(x % LADO_DO_TILE, ty))
                };
                saida.extend_from_slice(&pixel);
                k += 1;
            }
        }
        saida
    }

    /// As camadas que mudam algo no tile `posicao`, prontas para a conta de
    /// cada pixel ([`pixel_composto`]). Vazio: o tile é a base.
    fn tiles_em(&self, posicao: crate::tiles::Posicao) -> Vec<TileNaComposicao<'_>> {
        // A máscara de cada camada neste tile: sem tile pintado nela, o tile
        // inteiro vale o fundo — e entra na opacidade de uma vez.
        let mut tiles: Vec<TileNaComposicao> = Vec::with_capacity(self.camadas.len());
        let mut base_presente = false;
        for (((c, papel), preparado), leitor) in self
            .camadas
            .iter()
            .zip(self.preparados.iter())
            .zip(self.leitores.iter())
        {
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
        tiles
    }
}

/// Um pixel da foto composto: `baixo` (o da base) com cada tile de camada por
/// cima, no pixel `j` do tile — a conta do topo do módulo, e a das máscaras de
/// corte (ver [`compor_deslocado`]).
#[inline]
fn pixel_composto(tiles: &[TileNaComposicao], baixo: [u8; 3], j: usize) -> [u8; 3] {
    let mut pixel = baixo;
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
    pixel
}

/// O que o palco mostra: a foto, ou a máscara de uma camada — sozinha, em
/// cinza (⌥ + clique na miniatura da máscara), ou como sobreposição rubi sobre
/// a foto (`\`). Só a tela: a imagem editada é sempre a foto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Exibicao {
    /// A foto composta, como a imagem editada sai (o padrão).
    #[default]
    Foto,
    /// A máscara da camada de índice dado, sozinha, em cinza: branco revela,
    /// preto esconde.
    SoAMascara(usize),
    /// A foto com o vermelho rubi por cima onde a máscara da camada de índice
    /// dado esconde (50% onde esconde tudo).
    Rubi(usize),
    /// Só os pixels da camada, sobre cinza 50% onde ela é transparente — a
    /// baixa ou a alta frequência isolada para inspeção.
    SoACamada(usize),
    /// A foto sem o conjunto de recorte que começa na camada (ela e as
    /// recortadas nela) — o "original" do tratamento de pele.
    SemOConjunto(usize),
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
    Exibidora::nova(base, doc, exibicao).recorte(ret)
}

/// [`compor_recorte_exibindo`] **preparado uma vez** para vários recortes do
/// mesmo documento — a vista pede um por linha no rascunho do controle
/// deslizante. Ver [`Composicao`].
pub(crate) struct Exibidora<'a> {
    base: &'a RgbImage,
    modo: Exibir<'a>,
}

/// O que a [`Exibidora`] faz em cada recorte.
enum Exibir<'a> {
    /// A foto composta (sem o conjunto, em [`Exibicao::SemOConjunto`]).
    Composta(Composicao<'a>),
    /// Os pixels de uma camada sozinha, sobre cinza.
    SoACamada(&'a CamadaDePixels),
    /// A máscara sozinha, em cinza.
    SoAMascara(Mascarada<'a>),
    /// A foto com o rubi por cima onde a máscara esconde.
    Rubi(Composicao<'a>, Mascarada<'a>),
}

/// Uma máscara pronta para ser lida: o leitor e o tamanho dos pixels dela.
struct Mascarada<'a> {
    leitor: LeitorDaMascara<'a>,
    largura: u32,
    altura: u32,
}

impl<'a> Mascarada<'a> {
    fn nova(mascara: &'a Mascara) -> Self {
        Mascarada {
            leitor: mascara.leitor(),
            largura: mascara.pixels.largura(),
            altura: mascara.pixels.altura(),
        }
    }
}

impl<'a> Exibidora<'a> {
    /// Prepara a [`Exibicao`] de `doc` sobre `base`.
    pub(crate) fn nova(base: &'a RgbImage, doc: &'a Documento, exibicao: Exibicao) -> Self {
        let foto = || Exibir::Composta(Composicao::nova(base, doc, None));
        let modo = match exibicao {
            Exibicao::Foto => foto(),
            Exibicao::SoAMascara(i) | Exibicao::Rubi(i) => {
                match doc.camadas.get(i).and_then(|c| c.mascara.as_ref()) {
                    None => foto(),
                    Some(m) if matches!(exibicao, Exibicao::SoAMascara(_)) => {
                        Exibir::SoAMascara(Mascarada::nova(m))
                    }
                    Some(m) => Exibir::Rubi(Composicao::nova(base, doc, None), Mascarada::nova(m)),
                }
            }
            Exibicao::SoACamada(i) => match doc.camadas.get(i) {
                Some(c) => Exibir::SoACamada(&c.pixels),
                None => foto(),
            },
            Exibicao::SemOConjunto(i) if i < doc.camadas.len() => Exibir::Composta(
                Composicao::nova(base, doc, Some((i, doc.fim_do_conjunto(i)))),
            ),
            Exibicao::SemOConjunto(_) => foto(),
        };
        Exibidora { base, modo }
    }

    /// O recorte `ret`, numa imagem do tamanho dele.
    pub(crate) fn recorte(&self, ret: &Retangulo) -> RgbImage {
        let ret = ret.limitado(self.base.width(), self.base.height());
        match &self.modo {
            Exibir::Composta(c) => c.recorte(&ret),
            Exibir::SoACamada(pixels) => so_a_camada(pixels, &ret),
            Exibir::SoAMascara(m) => {
                let valores = valores_da_mascara(m, &ret);
                RgbImage::from_fn(ret.largura, ret.altura, |x, y| {
                    let v = valores[(y * ret.largura + x) as usize];
                    image::Rgb([v, v, v])
                })
            }
            Exibir::Rubi(c, m) => {
                let valores = valores_da_mascara(m, &ret);
                let mut foto = c.recorte(&ret);
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
        }
    }

    /// Só os pixels `(x, y)` de cada `x` em `xs` (em ordem crescente, dentro
    /// da foto), em RGB — o rascunho da vista, sem compor a linha inteira. Cada
    /// pixel sai igual ao do mesmo lugar em [`Self::recorte`].
    pub(crate) fn amostras(&self, y: u32, xs: &[u32]) -> Vec<u8> {
        debug_assert!(xs.windows(2).all(|par| par[0] <= par[1]));
        debug_assert!(xs.iter().all(|&x| x < self.base.width()) && y < self.base.height());
        let cinza = |v: u8| [v, v, v];
        match &self.modo {
            Exibir::Composta(c) => c.amostras(y, xs),
            Exibir::SoACamada(pixels) => xs
                .iter()
                .flat_map(|&x| sobre([128; 3], pixels.pixel(x, y), 1.0))
                .collect(),
            Exibir::SoAMascara(m) => xs
                .iter()
                .flat_map(|&x| cinza(valor_da_mascara(m, x, y)))
                .collect(),
            Exibir::Rubi(c, m) => {
                let mut foto = c.amostras(y, xs);
                for (p, &x) in foto.chunks_exact_mut(3).zip(xs) {
                    let v = valor_da_mascara(m, x, y);
                    let a = OPACIDADE_DO_RUBI * (255 - v) as f32 / 255.0;
                    if a > 0.0 {
                        for (canal, rubi) in p.iter_mut().zip(RUBI) {
                            *canal = (*canal as f32 + (rubi - *canal as f32) * a).round() as u8;
                        }
                    }
                }
                foto
            }
        }
    }
}

/// O valor da máscara (com a densidade e a difusão) no pixel `(x, y)` — o
/// mesmo de [`valores_da_mascara`] ali.
fn valor_da_mascara(mascara: &Mascarada, x: u32, y: u32) -> u8 {
    mascara
        .leitor
        .no_tile(crate::tiles::tile_de(x, y))
        .valor(indice(x % LADO_DO_TILE, y % LADO_DO_TILE))
}

/// Os pixels de uma camada sozinha em `ret`, sobre cinza 50% — tile a tile.
fn so_a_camada(pixels: &CamadaDePixels, ret: &Retangulo) -> RgbImage {
    let mut saida = RgbImage::from_pixel(ret.largura, ret.altura, image::Rgb([128; 3]));
    for posicao in pixels.tiles_do_retangulo(ret) {
        let Some(tile) = pixels.tile(posicao) else {
            continue;
        };
        let pedaco = interseccao(
            &retangulo_do_tile(posicao, pixels.largura(), pixels.altura()),
            ret,
        );
        for y in pedaco.y..pedaco.baixo() {
            for x in pedaco.x..pedaco.direita() {
                let j = indice(x % LADO_DO_TILE, y % LADO_DO_TILE);
                let p = sobre(
                    [128; 3],
                    [tile[j], tile[j + 1], tile[j + 2], tile[j + 3]],
                    1.0,
                );
                saida.put_pixel(x - ret.x, y - ret.y, image::Rgb(p));
            }
        }
    }
    saida
}

/// O valor da máscara (com a densidade e a difusão) em cada pixel de `ret`,
/// linha a linha.
fn valores_da_mascara(mascara: &Mascarada, ret: &Retangulo) -> Vec<u8> {
    let leitor = &mascara.leitor;
    let mut valores = vec![0u8; (ret.largura * ret.altura) as usize];
    let referencia = CamadaDePixels::nova(mascara.largura, mascara.altura);
    for posicao in referencia.tiles_do_retangulo(ret) {
        let no_tile = leitor.no_tile(posicao);
        let pedaco = interseccao(
            &retangulo_do_tile(posicao, mascara.largura, mascara.altura),
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
