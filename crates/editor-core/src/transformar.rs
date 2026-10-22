//! A transformação livre (⌘T) e o mover da seleção: o conteúdo de uma camada
//! (inteira, ou só o que está selecionado) é tirado dela, transformado e posto
//! de volta por cima do que sobrou.
//!
//! ```text
//! fundo     = a camada sem o conteúdo (sem seleção: vazia)
//! conteúdo  = a camada × a máscara, num retângulo próprio (a "caixa")
//! camada    = fundo + transformado(conteúdo)          (Normal, com alfa)
//! ```
//!
//! A transformação é afim em volta do centro da caixa: escala, giro e
//! deslocamento. Cada pixel do destino busca o seu ponto no conteúdo pela
//! inversa e lê **bilinear com alfa pré-multiplicado** — sem isso, a borda de
//! um recorte ampliado ganharia um halo da cor dos pixels transparentes.
//!
//! 🔑 Deslocamento inteiro, sem escala nem giro, cai exatamente nos centros
//! dos pixels: o mover não borra nada.

use crate::mesclagem::{mesclar_em_camada, Modo};
use crate::retangulo::Retangulo;
use crate::selecao::Selecao;
use crate::tiles::{indice, origem_do_tile, CamadaDePixels, LADO_DO_TILE};

/// A caixa do conteúdo, em pixels da foto — pode começar antes da foto (o
/// conteúdo levado para fora, etapa 14), por isso com sinal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caixa {
    /// Coluna do canto esquerdo, em pixels da foto; negativa à esquerda dela.
    pub x: i32,
    /// Linha do canto de cima, em pixels da foto; negativa acima dela.
    pub y: i32,
    /// Largura em pixels.
    pub largura: u32,
    /// Altura em pixels.
    pub altura: u32,
}

impl Caixa {
    /// A caixa com canto em `(x, y)` e esse tamanho, sem limitar à foto.
    pub fn nova(x: i32, y: i32, largura: u32, altura: u32) -> Self {
        Self {
            x,
            y,
            largura,
            altura,
        }
    }

    /// A primeira coluna **fora** da caixa, à direita (`x + largura`).
    pub fn direita(&self) -> i32 {
        self.x + self.largura as i32
    }

    /// A primeira linha **fora** da caixa, embaixo (`y + altura`).
    pub fn baixo(&self) -> i32 {
        self.y + self.altura as i32
    }

    /// Não cobre pixel nenhum (largura ou altura zero).
    pub fn vazia(&self) -> bool {
        self.largura == 0 || self.altura == 0
    }

    /// A parte dentro da foto `largura × altura`.
    pub fn na_foto(&self, largura: u32, altura: u32) -> Retangulo {
        let x0 = self.x.clamp(0, largura as i32) as u32;
        let y0 = self.y.clamp(0, altura as i32) as u32;
        let x1 = self.direita().clamp(0, largura as i32) as u32;
        let y1 = self.baixo().clamp(0, altura as i32) as u32;
        Retangulo::novo(x0, y0, x1 - x0, y1 - y0)
    }
}

impl From<Retangulo> for Caixa {
    fn from(r: Retangulo) -> Self {
        Self::nova(r.x as i32, r.y as i32, r.largura, r.altura)
    }
}

/// `v / 255` para cada byte — a mesma divisão de sempre, feita uma vez: a
/// amostra bilinear do ⌘T fazia quatro por vizinho, quatro vizinhos por pixel.
const SOBRE_255: [f32; 256] = {
    let mut tabela = [0.0f32; 256];
    let mut v = 0;
    while v < 256 {
        tabela[v] = v as f32 / 255.0;
        v += 1;
    }
    tabela
};

/// A seleção sobre um tile da camada: o valor de cada pixel dele, com o tile
/// da seleção achado uma vez — o mesmo de [`Selecao::valor`], 0 fora da foto
/// (sem seleção, tudo conta).
struct SelecaoNoTile<'a> {
    origem: (i64, i64),
    foto: (i64, i64),
    mascara: Option<Result<&'a [u8], u8>>,
}

impl<'a> SelecaoNoTile<'a> {
    fn de(selecao: Option<&'a Selecao>, posicao: crate::tiles::Posicao) -> Self {
        Self {
            origem: origem_do_tile(posicao),
            foto: selecao.map_or((0, 0), |s| (s.largura() as i64, s.altura() as i64)),
            mascara: selecao.map(|s| s.do_tile(posicao)),
        }
    }

    #[inline]
    fn em(&self, lx: u32, ly: u32) -> u8 {
        let Some(mascara) = &self.mascara else {
            return 255;
        };
        let (x, y) = (self.origem.0 + lx as i64, self.origem.1 + ly as i64);
        if x < 0 || y < 0 || x >= self.foto.0 || y >= self.foto.1 {
            return 0;
        }
        match mascara {
            Ok(tile) => tile[(ly * LADO_DO_TILE + lx) as usize],
            Err(v) => *v,
        }
    }

    /// O valor é 255 em todo o trecho `lx0..lx1` da linha `ly`?
    fn inteira(&self, lx0: u32, lx1: u32, ly: u32) -> bool {
        let Some(mascara) = &self.mascara else {
            return true;
        };
        let y = self.origem.1 + ly as i64;
        let (xa, xb) = (self.origem.0 + lx0 as i64, self.origem.0 + lx1 as i64);
        if xa < 0 || y < 0 || xb > self.foto.0 || y >= self.foto.1 {
            return false;
        }
        match mascara {
            Ok(tile) => {
                let i = (ly * LADO_DO_TILE) as usize;
                tile[i + lx0 as usize..i + lx1 as usize]
                    .iter()
                    .all(|v| *v == 255)
            }
            Err(v) => *v == 255,
        }
    }
}

/// Quanto o ⌘T guarda além da borda da foto, em fração do maior lado: ampliar
/// um conteúdo 100× não pode alocar uma camada do tamanho de um prédio.
const MARGEM_FORA_DA_FOTO: f32 = 1.0;

/// O conteúdo tirado da camada: RGBA de alfa reto, do tamanho da caixa.
#[derive(Clone, Debug, PartialEq)]
pub struct Conteudo {
    /// Onde o conteúdo estava na foto (antes de transformar) e o tamanho dos
    /// pixels guardados.
    pub caixa: Caixa,
    rgba: Vec<u8>,
}

impl Conteudo {
    /// O que a camada tem (vezes a seleção, se houver). `None` quando não
    /// sobra pixel nenhum.
    pub fn da_camada(camada: &CamadaDePixels, selecao: Option<&Selecao>) -> Option<Self> {
        // 🔑 O tile da camada e o da seleção são achados uma vez por tile (e
        // por trecho de linha), não a cada pixel: é o começo do ⌘T e do Mover
        // com seleção, e numa camada de 12 MP eram duas buscas por pixel.
        //
        // A caixa justa: só os pixels com alfa — também os de fora da foto
        // (sem seleção; a seleção só existe dentro dela). Em cada linha de
        // tile bastam o primeiro e o último pixel que contam.
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for (posicao, tile) in camada.todos() {
            let (tx, ty) = origem_do_tile(*posicao);
            let valor = SelecaoNoTile::de(selecao, *posicao);
            for ly in 0..LADO_DO_TILE {
                let conta = |lx: &u32| tile[indice(*lx, ly) + 3] > 0 && valor.em(*lx, ly) > 0;
                let Some(primeiro) = (0..LADO_DO_TILE).find(conta) else {
                    continue;
                };
                let ultimo = (0..LADO_DO_TILE).rev().find(conta).unwrap_or(primeiro);
                let y = ty + ly as i64;
                x0 = x0.min(tx + primeiro as i64);
                y0 = y0.min(y);
                x1 = x1.max(tx + ultimo as i64 + 1);
                y1 = y1.max(y + 1);
            }
        }
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let caixa = Caixa::nova(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32);
        let lado = LADO_DO_TILE as i64;
        // Uma linha da caixa, trecho a trecho de tile.
        let linha = |linha: &mut [u8], y: i64| {
            let ly = y.rem_euclid(lado) as u32;
            let mut x = x0;
            while x < x1 {
                let fim = x1.min((x.div_euclid(lado) + 1) * lado);
                let posicao = (x.div_euclid(lado) as i32, y.div_euclid(lado) as i32);
                // Sem tile, o pixel é transparente: o zero que já está.
                if let Some(tile) = camada.tile(posicao) {
                    let valor = SelecaoNoTile::de(selecao, posicao);
                    let lx0 = x.rem_euclid(lado) as u32;
                    let lx1 = lx0 + (fim - x) as u32;
                    let (i, k) = (indice(lx0, ly), ((x - x0) * 4) as usize);
                    if valor.inteira(lx0, lx1, ly) {
                        // Com 255, `(α·255 + 127) / 255 = α`: o pixel como está.
                        let n = ((fim - x) * 4) as usize;
                        linha[k..k + n].copy_from_slice(&tile[i..i + n]);
                    } else {
                        for (lx, k) in (lx0..lx1).zip((k..).step_by(4)) {
                            let i = indice(lx, ly);
                            let m = valor.em(lx, ly) as u32;
                            linha[k..k + 3].copy_from_slice(&tile[i..i + 3]);
                            linha[k + 3] = ((tile[i + 3] as u32 * m + 127) / 255) as u8;
                        }
                    }
                }
                x = fim;
            }
        };
        let largura = caixa.largura as usize * 4;
        let mut rgba = vec![0; largura * caixa.altura as usize];
        // Faixas de linhas, uma por núcleo: cada linha só lê.
        let nucleos = crate::nucleos().unwrap_or(1).max(1);
        let por = (caixa.altura as usize).div_ceil(nucleos).max(64);
        if por >= caixa.altura as usize {
            for (l, y) in rgba.chunks_exact_mut(largura).zip(y0..) {
                linha(l, y);
            }
        } else {
            std::thread::scope(|escopo| {
                for (faixa, ya) in rgba.chunks_mut(por * largura).zip((y0..).step_by(por)) {
                    let linha = &linha;
                    escopo.spawn(move || {
                        for (l, y) in faixa.chunks_exact_mut(largura).zip(ya..) {
                            linha(l, y);
                        }
                    });
                }
            });
        }
        Some(Self { caixa, rgba })
    }

    /// O que a camada tem dentro de `caixa` (vezes a seleção, se houver),
    /// com a caixa exata pedida — o transparente de dentro conta. É o par do
    /// Deformar: a máscara vinculada se deforma pela malha da camada.
    pub fn da_caixa(camada: &CamadaDePixels, selecao: Option<&Selecao>, caixa: Caixa) -> Self {
        let valor = |x: i64, y: i64| match selecao {
            None => 255,
            Some(_) if x < 0 || y < 0 => 0,
            Some(s) => s.valor(x as u32, y as u32),
        };
        let mut rgba = Vec::with_capacity((caixa.largura * caixa.altura * 4) as usize);
        for y in caixa.y as i64..caixa.baixo() as i64 {
            for x in caixa.x as i64..caixa.direita() as i64 {
                let mut p = camada.pixel_em(x, y);
                let m = valor(x, y) as u32;
                p[3] = ((p[3] as u32 * m + 127) / 255) as u8;
                rgba.extend_from_slice(&p);
            }
        }
        Self { caixa, rgba }
    }

    /// Uma imagem RGBA (alfa reto) com o canto em `(x, y)` — o que veio de
    /// fora (área de transferência do sistema, arquivo importado).
    pub fn da_imagem(imagem: &image::RgbaImage, x: i32, y: i32) -> Self {
        Self {
            caixa: Caixa::nova(x, y, imagem.width(), imagem.height()),
            rgba: imagem.as_raw().clone(),
        }
    }

    /// Os pixels como imagem RGBA (alfa reto), do tamanho da caixa — para a
    /// área de transferência do sistema.
    pub fn imagem(&self) -> image::RgbaImage {
        image::RgbaImage::from_raw(self.caixa.largura, self.caixa.altura, self.rgba.clone())
            .unwrap_or_default()
    }

    /// O pixel `(x, y)` da caixa, **pré-multiplicado**, em `0..=1`; fora é
    /// transparente.
    fn premultiplicado(&self, x: i64, y: i64) -> [f32; 4] {
        if x < 0 || y < 0 || x >= self.caixa.largura as i64 || y >= self.caixa.altura as i64 {
            return [0.0; 4];
        }
        let k = ((y as u32 * self.caixa.largura + x as u32) * 4) as usize;
        let p = &self.rgba[k..k + 4];
        let a = SOBRE_255[p[3] as usize];
        [
            SOBRE_255[p[0] as usize] * a,
            SOBRE_255[p[1] as usize] * a,
            SOBRE_255[p[2] as usize] * a,
            a,
        ]
    }

    /// Lê bilinear no ponto `(u, v)` em pixels da caixa (o centro do pixel
    /// `(0, 0)` é `(0,5, 0,5)`). Devolve RGBA de alfa reto.
    pub(crate) fn amostra(&self, u: f32, v: f32) -> [u8; 4] {
        let (u, v) = (u - 0.5, v - 0.5);
        let (x0, y0) = (u.floor(), v.floor());
        let (fx, fy) = (u - x0, v - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let mut soma = [0.0f32; 4];
        for (dx, dy, peso) in [
            (0, 0, (1.0 - fx) * (1.0 - fy)),
            (1, 0, fx * (1.0 - fy)),
            (0, 1, (1.0 - fx) * fy),
            (1, 1, fx * fy),
        ] {
            if peso <= 0.0 {
                continue;
            }
            let p = self.premultiplicado(x0 + dx, y0 + dy);
            for i in 0..4 {
                soma[i] += p[i] * peso;
            }
        }
        let a = soma[3];
        if a <= 1.0 / 512.0 {
            return [0; 4];
        }
        let q = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
        [q(soma[0] / a), q(soma[1] / a), q(soma[2] / a), q(a)]
    }
}

/// Escala, giro e deslocamento em volta do centro da caixa.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transformacao {
    /// Deslocamento horizontal do centro, em pixels da foto (para a direita).
    pub dx: f32,
    /// Deslocamento vertical do centro, em pixels da foto (para baixo).
    pub dy: f32,
    /// Escala horizontal, antes do giro: 1 = o tamanho original (o padrão).
    pub escala_x: f32,
    /// Escala vertical, antes do giro: 1 = o tamanho original (o padrão).
    pub escala_y: f32,
    /// Em radianos, no sentido do relógio na tela (y para baixo).
    pub angulo: f32,
}

impl Default for Transformacao {
    fn default() -> Self {
        Self {
            dx: 0.0,
            dy: 0.0,
            escala_x: 1.0,
            escala_y: 1.0,
            angulo: 0.0,
        }
    }
}

impl Transformacao {
    /// Só andar `(dx, dy)` pixels da foto — o Mover. Com `dx` e `dy` inteiros
    /// o conteúdo não borra.
    pub fn deslocamento(dx: f32, dy: f32) -> Self {
        Self {
            dx,
            dy,
            ..Self::default()
        }
    }

    /// Só anda (sem escala nem giro)?
    pub fn so_desloca(&self) -> bool {
        self.escala_x == 1.0 && self.escala_y == 1.0 && self.angulo == 0.0
    }

    fn centro(caixa: &Caixa) -> (f32, f32) {
        (
            caixa.x as f32 + caixa.largura as f32 / 2.0,
            caixa.y as f32 + caixa.altura as f32 / 2.0,
        )
    }

    /// Um ponto da caixa (pixels da foto) para onde ele vai.
    pub fn aplicar(&self, caixa: &Caixa, x: f32, y: f32) -> (f32, f32) {
        let (cx, cy) = Self::centro(caixa);
        let (px, py) = ((x - cx) * self.escala_x, (y - cy) * self.escala_y);
        let (s, c) = self.angulo.sin_cos();
        (
            cx + self.dx + px * c - py * s,
            cy + self.dy + px * s + py * c,
        )
    }

    /// De onde, na caixa, vem o ponto `(x, y)` do destino.
    pub fn inversa(&self, caixa: &Caixa, x: f32, y: f32) -> (f32, f32) {
        let (cx, cy) = Self::centro(caixa);
        let (px, py) = (x - cx - self.dx, y - cy - self.dy);
        let (s, c) = self.angulo.sin_cos();
        let (rx, ry) = (px * c + py * s, -px * s + py * c);
        (
            cx + rx / self.escala_x.max(1e-4),
            cy + ry / self.escala_y.max(1e-4),
        )
    }

    /// As oito alças da caixa, **na caixa de origem**, no sentido do relógio a
    /// partir do canto de cima à esquerda: cantos nos índices pares, meios dos
    /// lados nos ímpares (0 ↖, 1 ↑, 2 ↗, 3 →, 4 ↘, 5 ↓, 6 ↙, 7 ←). A oposta de
    /// `i` é `(i + 4) % 8`.
    pub fn alcas(caixa: &Caixa) -> [(f32, f32); 8] {
        let (x0, y0) = (caixa.x as f32, caixa.y as f32);
        let (x1, y1) = (caixa.direita() as f32, caixa.baixo() as f32);
        let (xm, ym) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        [
            (x0, y0),
            (xm, y0),
            (x1, y0),
            (x1, ym),
            (x1, y1),
            (xm, y1),
            (x0, y1),
            (x0, ym),
        ]
    }

    /// A mesma escala e o mesmo giro, com o deslocamento acertado para o ponto
    /// `pivo` (da caixa de origem) ficar onde estava em `antes` — escalar ou
    /// girar em volta da alça oposta ou do ponto de referência.
    pub fn fixando(mut self, caixa: &Caixa, pivo: (f32, f32), antes: &Transformacao) -> Self {
        let alvo = antes.aplicar(caixa, pivo.0, pivo.1);
        let agora = self.aplicar(caixa, pivo.0, pivo.1);
        self.dx += alvo.0 - agora.0;
        self.dy += alvo.1 - agora.1;
        self
    }

    /// A alça `alca` arrastada até `ponteiro` (pixels da foto), com `ancora`
    /// (ponto da caixa de origem) parada: a alça oposta, ou o ponto de
    /// referência com ⌥. Num canto, `proporcional` mantém a razão (o padrão do
    /// Photoshop; ⇧ solta); num meio de lado, só aquele eixo muda. A escala não
    /// passa por zero nem vira espelho (mínimo de 1%).
    pub fn pela_alca(
        &self,
        caixa: &Caixa,
        alca: usize,
        ponteiro: (f32, f32),
        ancora: (f32, f32),
        proporcional: bool,
    ) -> Self {
        let alcas = Self::alcas(caixa);
        let h = alcas[alca % 8];
        let o = (h.0 - ancora.0, h.1 - ancora.1);
        let a = self.aplicar(caixa, ancora.0, ancora.1);
        // O ponteiro no referencial da caixa (sem o giro), a partir da âncora.
        let (s, c) = self.angulo.sin_cos();
        let (px, py) = (ponteiro.0 - a.0, ponteiro.1 - a.1);
        let d = (px * c + py * s, -px * s + py * c);
        let canto = alca.is_multiple_of(2);
        let mut nova = *self;
        const MINIMO: f32 = 0.01;
        if canto && proporcional {
            let v = (self.escala_x * o.0, self.escala_y * o.1);
            let v2 = v.0 * v.0 + v.1 * v.1;
            if v2 > 1e-9 {
                let k = (d.0 * v.0 + d.1 * v.1) / v2;
                let k = k.max(MINIMO / self.escala_x.min(self.escala_y).max(MINIMO));
                nova.escala_x = self.escala_x * k;
                nova.escala_y = self.escala_y * k;
            }
        } else {
            if o.0.abs() > 1e-6 {
                nova.escala_x = (d.0 / o.0).max(MINIMO);
            }
            if o.1.abs() > 1e-6 {
                nova.escala_y = (d.1 / o.1).max(MINIMO);
            }
        }
        nova.fixando(caixa, ancora, self)
    }

    /// Girada de `delta` radianos em volta de `pivo` (ponto da caixa de
    /// origem, o ponto de referência).
    pub fn girada_em_volta(&self, caixa: &Caixa, pivo: (f32, f32), delta: f32) -> Self {
        let mut nova = *self;
        nova.angulo += delta;
        nova.fixando(caixa, pivo, self)
    }

    /// Os quatro cantos da caixa transformada (sentido do relógio a partir do
    /// de cima à esquerda) — o que a tela desenha com as alças.
    pub fn cantos(&self, caixa: &Caixa) -> [(f32, f32); 4] {
        let (x0, y0) = (caixa.x as f32, caixa.y as f32);
        let (x1, y1) = (caixa.direita() as f32, caixa.baixo() as f32);
        [
            self.aplicar(caixa, x0, y0),
            self.aplicar(caixa, x1, y0),
            self.aplicar(caixa, x1, y1),
            self.aplicar(caixa, x0, y1),
        ]
    }
}

/// O conteúdo transformado numa camada transparente do tamanho da foto.
///
/// 🔑 **Rápido o bastante para o arrasto** (medido no app real: 125–165 ms por
/// evento numa peça de 3,5 MP na primeira versão). A inversa é afim, então
/// vira três coeficientes e uma soma por pixel; cada linha só percorre o
/// trecho que cai dentro da caixa; e cada faixa de tiles (256 linhas) roda numa
/// thread — elas escrevem em tiles diferentes.
pub fn desenhar(
    conteudo: &Conteudo,
    t: &Transformacao,
    largura: u32,
    altura: u32,
) -> CamadaDePixels {
    desenhar_na_referencia(conteudo, &conteudo.caixa, t, largura, altura)
}

/// O mesmo, com a transformação medida na caixa `referencia` (o centro e as
/// alças dela), e não na do conteúdo — a máscara vinculada anda pela conta
/// da camada, mesmo cobrindo outro pedaço da foto.
pub fn desenhar_na_referencia(
    conteudo: &Conteudo,
    referencia: &Caixa,
    t: &Transformacao,
    largura: u32,
    altura: u32,
) -> CamadaDePixels {
    let mut saida = CamadaDePixels::nova(largura, altura);
    let caixa = conteudo.caixa;
    let cantos = [
        (caixa.x as f32, caixa.y as f32),
        (caixa.direita() as f32, caixa.y as f32),
        (caixa.direita() as f32, caixa.baixo() as f32),
        (caixa.x as f32, caixa.baixo() as f32),
    ]
    .map(|(x, y)| t.aplicar(referencia, x, y));
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in cantos {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    // Sem cortar na borda da foto: o que sai fica como tile de fora (até a
    // margem), e volta quando a caixa volta.
    let margem = (largura.max(altura) as f32 * MARGEM_FORA_DA_FOTO).ceil();
    let x0 = (x0.floor() - 1.0).max(-margem) as i64;
    let y0 = (y0.floor() - 1.0).max(-margem) as i64;
    let x1 = (x1.ceil() + 1.0).min(largura as f32 + margem) as i64;
    let y1 = (y1.ceil() + 1.0).min(altura as f32 + margem) as i64;
    if x1 <= x0 || y1 <= y0 {
        return saida;
    }
    // A inversa em coordenadas da caixa: (u, v) = c + a·x + b·y.
    let em = |x: f32, y: f32| {
        let (u, v) = t.inversa(referencia, x, y);
        (u - caixa.x as f32, v - caixa.y as f32)
    };
    let c = em(0.0, 0.0);
    let a = (em(1.0, 0.0).0 - c.0, em(1.0, 0.0).1 - c.1);
    let b = (em(0.0, 1.0).0 - c.0, em(0.0, 1.0).1 - c.1);
    let (lc, ac) = (caixa.largura as f32, caixa.altura as f32);

    // 🔑 **Deslocamento inteiro** (o Mover com seleção, o colar): cada ponto cai
    // no centro de um pixel da caixa, com peso 1, e a amostra devolve o
    // próprio pixel — e o transparente como nada
    // (`no_centro_a_amostra_e_o_proprio_pixel` confere todo par de cor e
    // alfa). Então a linha é cópia de trechos, sem conta. Abaixo de 2²¹ as
    // somas de `u` e `v` são exatas: o pixel é o mesmo da conta.
    let exato = |v: f32| v.fract() == 0.0 && v.abs() < 2_097_152.0;
    let inteiro = (a == (1.0, 0.0)
        && b == (0.0, 1.0)
        && exato(c.0)
        && exato(c.1)
        && [x0, x1, y0, y1].iter().all(|v| v.abs() < 1 << 21))
    .then_some((c.0 as i64, c.1 as i64));

    let lado = LADO_DO_TILE as i64;
    // As colunas de tiles que a faixa pode tocar.
    let (tx0, tx1) = (x0.div_euclid(lado), (x1 - 1).div_euclid(lado));
    let faixa = |l0: i64| -> Vec<(crate::tiles::Posicao, Vec<u8>)> {
        // 🔑 Um tile por coluna, achado pelo índice — e não um `BTreeMap`
        // consultado a cada pixel (12 milhões de buscas por evento numa
        // camada de 12 MP). A faixa é uma linha de tiles: a coluna basta.
        let mut tiles: Vec<Option<Vec<u8>>> = (tx0..=tx1).map(|_| None).collect();
        let (ya, yb) = (y0.max(l0 * lado), y1.min((l0 + 1) * lado));
        for y in ya..yb {
            let yc = y as f32 + 0.5;
            let (pu, pv) = (c.0 + b.0 * yc, c.1 + b.1 * yc);
            // O trecho de x em que (u, v) cai na caixa, com um pixel de folga.
            let (mut xa, mut xb) = (x0 as f32, x1 as f32);
            for (p, d, limite) in [(pu, a.0, lc), (pv, a.1, ac)] {
                if d.abs() < 1e-9 {
                    if p < -1.0 || p > limite + 1.0 {
                        xb = xa - 1.0;
                    }
                } else {
                    let (e, f) = ((-1.0 - p) / d - 0.5, (limite + 1.0 - p) / d - 0.5);
                    xa = xa.max(e.min(f).floor());
                    xb = xb.min(e.max(f).ceil() + 1.0);
                }
            }
            if xb <= xa {
                continue;
            }
            let (xi, xf) = (xa.max(x0 as f32) as i64, xb.min(x1 as f32) as i64);
            if let Some((du, dv)) = inteiro {
                // `u − ½ = du + x` e `v − ½ = dv + y`: o pixel (du + x, dv + y).
                let sy = dv + y;
                if sy < 0 || sy >= caixa.altura as i64 {
                    continue;
                }
                let (mut x, xe) = (xi.max(-du), xf.min(caixa.largura as i64 - du));
                while x < xe {
                    let fim = xe.min((x.div_euclid(lado) + 1) * lado);
                    let k = ((sy * caixa.largura as i64 + du + x) * 4) as usize;
                    let origem = &conteudo.rgba[k..k + ((fim - x) * 4) as usize];
                    if origem.chunks_exact(4).any(|p| p[3] != 0) {
                        let tile = tiles[(x.div_euclid(lado) - tx0) as usize]
                            .get_or_insert_with(|| vec![0; crate::tiles::BYTES_DO_TILE]);
                        let i = indice(x.rem_euclid(lado) as u32, y.rem_euclid(lado) as u32);
                        let destino = &mut tile[i..i + origem.len()];
                        destino.copy_from_slice(origem);
                        // O transparente fica como nada, como a amostra o devolve.
                        for p in destino.chunks_exact_mut(4) {
                            if p[3] == 0 {
                                p.fill(0);
                            }
                        }
                    }
                    x = fim;
                }
                continue;
            }
            for x in xi..xf {
                let xc = x as f32 + 0.5;
                let p = conteudo.amostra(pu + a.0 * xc, pv + a.1 * xc);
                if p[3] == 0 {
                    continue;
                }
                let tile = tiles[(x.div_euclid(lado) - tx0) as usize]
                    .get_or_insert_with(|| vec![0; crate::tiles::BYTES_DO_TILE]);
                let i = indice(x.rem_euclid(lado) as u32, y.rem_euclid(lado) as u32);
                tile[i..i + 4].copy_from_slice(&p);
            }
        }
        tiles
            .into_iter()
            .zip(tx0..)
            .filter_map(|(tile, tx)| tile.map(|t| ((tx as i32, l0 as i32), t)))
            .collect()
    };
    let faixas: Vec<i64> = (y0.div_euclid(lado)..=(y1 - 1).div_euclid(lado)).collect();
    let resultados: Vec<Vec<_>> = if faixas.len() == 1 {
        vec![faixa(faixas[0])]
    } else {
        std::thread::scope(|escopo| {
            let tarefas: Vec<_> = faixas
                .iter()
                .map(|&l| {
                    let faixa = &faixa;
                    escopo.spawn(move || faixa(l))
                })
                .collect();
            tarefas
                .into_iter()
                .map(|t| t.join().unwrap_or_default())
                .collect()
        })
    };
    for (posicao, tile) in resultados.into_iter().flatten() {
        saida.definir(posicao, Some(std::sync::Arc::new(tile)));
    }
    saida
}

/// `camada` sem o que ela tem dentro de `caixa` (vezes a seleção) — o que
/// fica quando o pedaço da caixa sai para o Deformar (ver
/// [`Conteudo::da_caixa`]).
pub fn sem_a_caixa(
    camada: &CamadaDePixels,
    selecao: Option<&Selecao>,
    caixa: Caixa,
) -> CamadaDePixels {
    let mut saida = camada.clone();
    let lado = LADO_DO_TILE as i64;
    let mut tocados = std::collections::BTreeSet::new();
    for (posicao, tile) in camada.todos() {
        let (tx, ty) = origem_do_tile(*posicao);
        let x0 = (caixa.x as i64).max(tx);
        let x1 = (caixa.direita() as i64).min(tx + lado);
        let y0 = (caixa.y as i64).max(ty);
        let y1 = (caixa.baixo() as i64).min(ty + lado);
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let mut novo: Option<Vec<u8>> = None;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = indice((x - tx) as u32, (y - ty) as u32);
                if tile[i + 3] == 0 {
                    continue;
                }
                let s = match selecao {
                    None => 255,
                    Some(_) if x < 0 || y < 0 => 0,
                    Some(sel) => sel.valor(x as u32, y as u32),
                } as u32;
                if s == 0 {
                    continue;
                }
                let t = novo.get_or_insert_with(|| tile.as_ref().clone());
                t[i + 3] = ((t[i + 3] as u32 * (255 - s) + 127) / 255) as u8;
            }
        }
        if let Some(t) = novo {
            saida.definir(*posicao, Some(std::sync::Arc::new(t)));
            tocados.insert(*posicao);
        }
    }
    for posicao in tocados {
        saida.enxugar(posicao);
    }
    saida
}

/// `cima` sobre `fundo` (Normal), só onde `cima` tem tile. Onde o fundo não
/// tem nada, o tile de cima entra como está (é o caso de toda transformação
/// sem seleção).
pub fn sobre(fundo: &CamadaDePixels, cima: &CamadaDePixels) -> CamadaDePixels {
    let mut saida = fundo.clone();
    // Os tiles que precisam de conta (há fundo e cima no mesmo lugar); os
    // outros entram como estão.
    let mut juntar: Vec<(crate::tiles::Posicao, &[u8], &[u8])> = Vec::new();
    for (posicao, tile) in cima.todos() {
        match fundo.tile(*posicao) {
            None => saida.definir(*posicao, Some(tile.clone())),
            Some(baixo) => juntar.push((*posicao, baixo.as_slice(), tile.as_slice())),
        }
    }
    // 🔑 **Um tile por vez em cada núcleo**: cada tile só depende dos dois
    // tiles dele. A conta de cada pixel é a de sempre.
    let mesclar_tile = |baixo: &[u8], tile: &[u8]| -> Vec<u8> {
        let mut destino = baixo.to_vec();
        for k in (0..tile.len()).step_by(4) {
            if tile[k + 3] == 0 {
                continue;
            }
            let b = [destino[k], destino[k + 1], destino[k + 2], destino[k + 3]];
            let c = [tile[k], tile[k + 1], tile[k + 2], tile[k + 3]];
            destino[k..k + 4].copy_from_slice(&mesclar_em_camada(b, c, 1.0, Modo::Normal));
        }
        destino
    };
    let nucleos = crate::nucleos().unwrap_or(1).max(1);
    let feitos: Vec<(crate::tiles::Posicao, Vec<u8>)> = if juntar.len() < 2 || nucleos == 1 {
        juntar
            .iter()
            .map(|(p, baixo, tile)| (*p, mesclar_tile(baixo, tile)))
            .collect()
    } else {
        let por = juntar.len().div_ceil(nucleos);
        std::thread::scope(|escopo| {
            let tarefas: Vec<_> = juntar
                .chunks(por)
                .map(|parte| {
                    let mesclar_tile = &mesclar_tile;
                    escopo.spawn(move || {
                        parte
                            .iter()
                            .map(|(p, baixo, tile)| (*p, mesclar_tile(baixo, tile)))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            tarefas
                .into_iter()
                .flat_map(|t| t.join().unwrap_or_default())
                .collect()
        })
    };
    for (posicao, tile) in feitos {
        saida.definir(posicao, Some(std::sync::Arc::new(tile)));
    }
    saida
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::selecao::Forma;

    /// `da_camada` de antes de 2026-10-22: o tile da camada e o da seleção
    /// buscados a cada pixel.
    fn da_camada_antiga(camada: &CamadaDePixels, selecao: Option<&Selecao>) -> Option<Conteudo> {
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        let valor = |x: i64, y: i64| match selecao {
            None => 255,
            Some(_) if x < 0 || y < 0 => 0,
            Some(s) => s.valor(x as u32, y as u32),
        };
        let lado = LADO_DO_TILE as i64;
        for (posicao, tile) in camada.todos() {
            let (tx, ty) = origem_do_tile(*posicao);
            for ly in 0..lado {
                for lx in 0..lado {
                    let (x, y) = (tx + lx, ty + ly);
                    if tile[indice(lx as u32, ly as u32) + 3] > 0 && valor(x, y) > 0 {
                        x0 = x0.min(x);
                        y0 = y0.min(y);
                        x1 = x1.max(x + 1);
                        y1 = y1.max(y + 1);
                    }
                }
            }
        }
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let caixa = Caixa::nova(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32);
        let mut rgba = Vec::with_capacity((caixa.largura * caixa.altura * 4) as usize);
        for y in y0..y1 {
            for x in x0..x1 {
                let mut p = camada.pixel_em(x, y);
                let m = valor(x, y) as u32;
                p[3] = ((p[3] as u32 * m + 127) / 255) as u8;
                rgba.extend_from_slice(&p);
            }
        }
        Some(Conteudo { caixa, rgba })
    }

    /// 🔑 **O conteúdo tirado por tile é o de antes, byte a byte**: sem
    /// seleção, com uma elipse de borda suave e difusa, com ela invertida (o
    /// padrão 255, que fora da foto continua 0), com tudo e com nada — numa
    /// camada da foto, em duas com tiles fora dela (antes e depois) e numa
    /// vazia.
    #[test]
    fn o_conteudo_por_tile_e_o_de_antes() {
        let camada = camada_variada();
        let conteudo = Conteudo::da_camada(&camada, None).unwrap();
        let antes_da_foto = desenhar(
            &conteudo,
            &Transformacao::deslocamento(-300.0, -200.5),
            700,
            500,
        );
        let depois_da_foto = desenhar(
            &conteudo,
            &Transformacao::deslocamento(400.0, 300.0),
            700,
            500,
        );
        let elipse = Selecao::da_forma_com(
            700,
            500,
            &Forma::Elipse(Retangulo::novo(60, 40, 600, 420)),
            crate::selecao::Acabamento {
                suavizar: true,
                difusao: 6,
            },
        );
        let mut invertida = elipse.clone();
        invertida.inverter();
        let (tudo, nada) = (Selecao::tudo(700, 500), Selecao::vazia(700, 500));
        // Quase toda 255, com 254 espalhados: o trecho todo 255 copia, o
        // outro faz a conta.
        let densa: Vec<u8> = (0..700 * 500u32)
            .map(|i| {
                if (i * 7 + i / 700) % 997 == 0 {
                    254
                } else {
                    255
                }
            })
            .collect();
        let quase = Selecao::do_recorte(700, 500, &Retangulo::inteiro(700, 500), &densa);
        // Dois cantos: os tiles do meio da caixa não existem na seleção (o
        // padrão 0).
        let mut cantos =
            Selecao::da_forma(700, 500, &Forma::Retangulo(Retangulo::novo(45, 35, 50, 50)));
        cantos.combinar(
            &Selecao::da_forma(
                700,
                500,
                &Forma::Retangulo(Retangulo::novo(600, 420, 45, 40)),
            ),
            crate::selecao::Operacao::Somar,
        );
        let vazia = CamadaDePixels::nova(700, 500);
        for c in [&camada, &antes_da_foto, &depois_da_foto, &vazia] {
            for s in [
                None,
                Some(&quase),
                Some(&cantos),
                Some(&elipse),
                Some(&invertida),
                Some(&tudo),
                Some(&nada),
            ] {
                let novo = Conteudo::da_camada(c, s);
                let antes = da_camada_antiga(c, s);
                assert!(
                    novo.as_ref().map(|n| (n.caixa, &n.rgba))
                        == antes.as_ref().map(|a| (a.caixa, &a.rgba)),
                    "caixa {:?} × {:?}",
                    novo.map(|n| n.caixa),
                    antes.map(|a| a.caixa)
                );
            }
        }
    }

    /// A amostra de antes de 2026-10-22: a divisão por 255 a cada leitura.
    fn amostra_antiga(c: &Conteudo, u: f32, v: f32) -> [u8; 4] {
        let premultiplicado = |x: i64, y: i64| -> [f32; 4] {
            if x < 0 || y < 0 || x >= c.caixa.largura as i64 || y >= c.caixa.altura as i64 {
                return [0.0; 4];
            }
            let k = ((y as u32 * c.caixa.largura + x as u32) * 4) as usize;
            let a = c.rgba[k + 3] as f32 / 255.0;
            [
                c.rgba[k] as f32 / 255.0 * a,
                c.rgba[k + 1] as f32 / 255.0 * a,
                c.rgba[k + 2] as f32 / 255.0 * a,
                a,
            ]
        };
        let (u, v) = (u - 0.5, v - 0.5);
        let (x0, y0) = (u.floor(), v.floor());
        let (fx, fy) = (u - x0, v - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let mut soma = [0.0f32; 4];
        for (dx, dy, peso) in [
            (0, 0, (1.0 - fx) * (1.0 - fy)),
            (1, 0, fx * (1.0 - fy)),
            (0, 1, (1.0 - fx) * fy),
            (1, 1, fx * fy),
        ] {
            if peso <= 0.0 {
                continue;
            }
            let p = premultiplicado(x0 + dx, y0 + dy);
            for i in 0..4 {
                soma[i] += p[i] * peso;
            }
        }
        let a = soma[3];
        if a <= 1.0 / 512.0 {
            return [0; 4];
        }
        let q = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
        [q(soma[0] / a), q(soma[1] / a), q(soma[2] / a), q(a)]
    }

    fn desenhar_antigo(
        conteudo: &Conteudo,
        referencia: &Caixa,
        t: &Transformacao,
        largura: u32,
        altura: u32,
    ) -> CamadaDePixels {
        let mut saida = CamadaDePixels::nova(largura, altura);
        let caixa = conteudo.caixa;
        let cantos = [
            (caixa.x as f32, caixa.y as f32),
            (caixa.direita() as f32, caixa.y as f32),
            (caixa.direita() as f32, caixa.baixo() as f32),
            (caixa.x as f32, caixa.baixo() as f32),
        ]
        .map(|(x, y)| t.aplicar(referencia, x, y));
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (x, y) in cantos {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        // Sem cortar na borda da foto: o que sai fica como tile de fora (até a
        // margem), e volta quando a caixa volta.
        let margem = (largura.max(altura) as f32 * MARGEM_FORA_DA_FOTO).ceil();
        let x0 = (x0.floor() - 1.0).max(-margem) as i64;
        let y0 = (y0.floor() - 1.0).max(-margem) as i64;
        let x1 = (x1.ceil() + 1.0).min(largura as f32 + margem) as i64;
        let y1 = (y1.ceil() + 1.0).min(altura as f32 + margem) as i64;
        if x1 <= x0 || y1 <= y0 {
            return saida;
        }
        // A inversa em coordenadas da caixa: (u, v) = c + a·x + b·y.
        let em = |x: f32, y: f32| {
            let (u, v) = t.inversa(referencia, x, y);
            (u - caixa.x as f32, v - caixa.y as f32)
        };
        let c = em(0.0, 0.0);
        let a = (em(1.0, 0.0).0 - c.0, em(1.0, 0.0).1 - c.1);
        let b = (em(0.0, 1.0).0 - c.0, em(0.0, 1.0).1 - c.1);
        let (lc, ac) = (caixa.largura as f32, caixa.altura as f32);

        let lado = LADO_DO_TILE as i64;
        let faixa = |l0: i64| -> Vec<(crate::tiles::Posicao, Vec<u8>)> {
            let mut tiles: std::collections::BTreeMap<crate::tiles::Posicao, Vec<u8>> =
                std::collections::BTreeMap::new();
            let (ya, yb) = (y0.max(l0 * lado), y1.min((l0 + 1) * lado));
            for y in ya..yb {
                let yc = y as f32 + 0.5;
                let (pu, pv) = (c.0 + b.0 * yc, c.1 + b.1 * yc);
                // O trecho de x em que (u, v) cai na caixa, com um pixel de folga.
                let (mut xa, mut xb) = (x0 as f32, x1 as f32);
                for (p, d, limite) in [(pu, a.0, lc), (pv, a.1, ac)] {
                    if d.abs() < 1e-9 {
                        if p < -1.0 || p > limite + 1.0 {
                            xb = xa - 1.0;
                        }
                    } else {
                        let (e, f) = ((-1.0 - p) / d - 0.5, (limite + 1.0 - p) / d - 0.5);
                        xa = xa.max(e.min(f).floor());
                        xb = xb.min(e.max(f).ceil() + 1.0);
                    }
                }
                if xb <= xa {
                    continue;
                }
                for x in (xa.max(x0 as f32) as i64)..(xb.min(x1 as f32) as i64) {
                    let xc = x as f32 + 0.5;
                    let p = amostra_antiga(conteudo, pu + a.0 * xc, pv + a.1 * xc);
                    if p[3] == 0 {
                        continue;
                    }
                    let tile = tiles
                        .entry((x.div_euclid(lado) as i32, y.div_euclid(lado) as i32))
                        .or_insert_with(|| vec![0; crate::tiles::BYTES_DO_TILE]);
                    let i = indice(x.rem_euclid(lado) as u32, y.rem_euclid(lado) as u32);
                    tile[i..i + 4].copy_from_slice(&p);
                }
            }
            tiles.into_iter().collect()
        };
        let faixas: Vec<i64> = (y0.div_euclid(lado)..=(y1 - 1).div_euclid(lado)).collect();
        let resultados: Vec<Vec<_>> = if faixas.len() == 1 {
            vec![faixa(faixas[0])]
        } else {
            std::thread::scope(|escopo| {
                let tarefas: Vec<_> = faixas
                    .iter()
                    .map(|&l| {
                        let faixa = &faixa;
                        escopo.spawn(move || faixa(l))
                    })
                    .collect();
                tarefas
                    .into_iter()
                    .map(|t| t.join().unwrap_or_default())
                    .collect()
            })
        };
        for (posicao, tile) in resultados.into_iter().flatten() {
            saida.definir(posicao, Some(std::sync::Arc::new(tile)));
        }
        saida
    }

    fn sobre_antigo(fundo: &CamadaDePixels, cima: &CamadaDePixels) -> CamadaDePixels {
        let mut saida = fundo.clone();
        for (posicao, tile) in cima.todos() {
            if fundo.tile(*posicao).is_none() {
                saida.definir(*posicao, Some(tile.clone()));
                continue;
            }
            let destino = saida.tile_mut(*posicao);
            for k in (0..tile.len()).step_by(4) {
                if tile[k + 3] == 0 {
                    continue;
                }
                let b = [destino[k], destino[k + 1], destino[k + 2], destino[k + 3]];
                let c = [tile[k], tile[k + 1], tile[k + 2], tile[k + 3]];
                destino[k..k + 4].copy_from_slice(&mesclar_em_camada(b, c, 1.0, Modo::Normal));
            }
        }
        saida
    }

    /// Uma camada de 700 × 500 com cor e alfa variando (inclusive zero), que
    /// atravessa várias linhas e colunas de tiles.
    fn camada_variada() -> CamadaDePixels {
        let mut c = CamadaDePixels::nova(700, 500);
        for y in 30..470u32 {
            for x in 40..650u32 {
                let i = indice(x % 256, y % 256);
                let alfa = if (x / 37 + y / 23) % 5 == 0 {
                    0
                } else {
                    (x * 3 + y * 7) as u8 | 1
                };
                c.tile_mut(((x / 256) as i32, (y / 256) as i32))[i..i + 4].copy_from_slice(&[
                    (x * 5) as u8,
                    (y * 3) as u8,
                    (x ^ y) as u8,
                    alfa,
                ]);
            }
        }
        c
    }

    /// 🔑 **O ⌘T acelerado desenha a mesma camada, byte a byte** — achando o
    /// tile pela coluna e lendo `v / 255` da tabela — em deslocamentos
    /// inteiros e fracionários, escala, giro, saindo da foto e medido na caixa
    /// de outro (a máscara vinculada).
    #[test]
    fn desenhar_e_sobre_dao_o_que_davam() {
        let camada = camada_variada();
        let conteudo = Conteudo::da_camada(&camada, None).unwrap();
        let outra = Caixa::nova(100, 80, 300, 200);
        let casos = [
            Transformacao::deslocamento(13.0, 7.0),
            Transformacao::deslocamento(0.0, 0.0),
            Transformacao::deslocamento(-1.0, 1.0),
            Transformacao::deslocamento(256.0, -256.0),
            Transformacao::deslocamento(301.0, -263.0),
            Transformacao::deslocamento(-20.5, 3.25),
            Transformacao::deslocamento(13.0, 7.5),
            Transformacao::deslocamento(-500.0, 0.0),
            Transformacao::deslocamento(-2000.0, 900.0),
            Transformacao {
                dx: 5.0,
                dy: -3.0,
                escala_x: 1.3,
                escala_y: 0.8,
                angulo: 0.3,
            },
            Transformacao {
                dx: 0.0,
                dy: 0.0,
                escala_x: 0.45,
                escala_y: 0.45,
                angulo: -1.1,
            },
        ];
        // E um conteúdo que começa fora da foto (levado para fora antes).
        let mut rgba = Vec::new();
        for y in 0..300u32 {
            for x in 0..420u32 {
                let alfa = if (x + y) % 7 == 0 {
                    0
                } else {
                    (x ^ y) as u8 | 3
                };
                rgba.extend_from_slice(&[(x * 3) as u8, (y * 5) as u8, 77, alfa]);
            }
        }
        let fora = Conteudo::da_imagem(
            &image::RgbaImage::from_raw(420, 300, rgba).unwrap(),
            -90,
            -40,
        );
        // E dois cantos opacos com um vão transparente no meio: há tiles do
        // destino que só recebem transparência, e eles não podem aparecer.
        let cantos = Conteudo::da_imagem(
            &image::RgbaImage::from_fn(690, 480, |x, y| {
                let canto = (x < 40 && y < 40) || (x >= 650 && y >= 440);
                image::Rgba([x as u8, y as u8, 9, if canto { 255 } else { 0 }])
            }),
            5,
            10,
        );
        for t in casos {
            for (conteudo, referencia) in [
                (&conteudo, conteudo.caixa),
                (&conteudo, outra),
                (&fora, fora.caixa),
                (&cantos, cantos.caixa),
            ] {
                let novo = desenhar_na_referencia(conteudo, &referencia, &t, 700, 500);
                let antes = desenhar_antigo(conteudo, &referencia, &t, 700, 500);
                assert!(novo == antes, "{t:?} na caixa {referencia:?}");
                assert!(
                    sobre(&camada, &novo) == sobre_antigo(&camada, &novo),
                    "sobre: {t:?}"
                );
            }
        }
    }

    /// 🚨 **No centro de um pixel a amostra é o próprio pixel** — o que deixa
    /// o deslocamento inteiro copiar em vez de fazer a conta: para todo par de
    /// cor e alfa (cada canal tem todos os valores), o transparente como nada.
    #[test]
    fn no_centro_a_amostra_e_o_proprio_pixel() {
        let pixel = |c: u32, alfa: u32| [c as u8, 255 - c as u8, c as u8 ^ 0x5a, alfa as u8];
        let mut rgba = Vec::new();
        for alfa in 0..256 {
            for c in 0..256 {
                rgba.extend_from_slice(&pixel(c, alfa));
            }
        }
        let conteudo =
            Conteudo::da_imagem(&image::RgbaImage::from_raw(256, 256, rgba).unwrap(), 0, 0);
        for alfa in 0..256 {
            for c in 0..256 {
                let esperado = if alfa == 0 { [0; 4] } else { pixel(c, alfa) };
                assert_eq!(
                    conteudo.amostra(c as f32 + 0.5, alfa as f32 + 0.5),
                    esperado,
                    "cor {c}, alfa {alfa}"
                );
            }
        }
    }

    /// A amostra lida da tabela é a da divisão, em pontos que caem dentro,
    /// na borda e fora da caixa, inteiros e fracionários.
    #[test]
    fn a_amostra_da_tabela_e_a_da_divisao() {
        let conteudo = Conteudo::da_camada(&camada_variada(), None).unwrap();
        let (l, a) = (conteudo.caixa.largura as f32, conteudo.caixa.altura as f32);
        for k in 0..20_000u32 {
            let u = (k as f32 * 0.731) % (l + 4.0) - 2.0;
            let v = (k as f32 * 0.377) % (a + 4.0) - 2.0;
            assert_eq!(
                conteudo.amostra(u, v),
                amostra_antiga(&conteudo, u, v),
                "({u}, {v})"
            );
        }
    }

    fn camada_com_quadrado() -> CamadaDePixels {
        let mut c = CamadaDePixels::nova(600, 400);
        for y in 100..140 {
            for x in 200..260 {
                let i = indice(x % 256, y % 256);
                c.tile_mut(((x / 256) as i32, (y / 256) as i32))[i..i + 4].copy_from_slice(&[
                    (x - 200) as u8 * 4,
                    (y - 100) as u8 * 6,
                    77,
                    255,
                ]);
            }
        }
        c
    }

    #[test]
    fn o_conteudo_tem_a_caixa_justa_e_respeita_a_selecao() {
        let c = camada_com_quadrado();
        let tudo = Conteudo::da_camada(&c, None).unwrap();
        assert_eq!(tudo.caixa, Caixa::from(Retangulo::novo(200, 100, 60, 40)));
        let s = Selecao::da_forma(
            600,
            400,
            &Forma::Retangulo(Retangulo::novo(230, 0, 100, 400)),
        );
        let metade = Conteudo::da_camada(&c, Some(&s)).unwrap();
        assert_eq!(metade.caixa, Caixa::from(Retangulo::novo(230, 100, 30, 40)));
        assert!(Conteudo::da_camada(&CamadaDePixels::nova(10, 10), None).is_none());
    }

    #[test]
    fn deslocamento_inteiro_copia_sem_borrar() {
        let c = camada_com_quadrado();
        let conteudo = Conteudo::da_camada(&c, None).unwrap();
        let d = desenhar(
            &conteudo,
            &Transformacao::deslocamento(37.0, -20.0),
            600,
            400,
        );
        for (x, y) in [(200, 100), (259, 139), (230, 120)] {
            assert_eq!(d.pixel(x + 37, y - 20), c.pixel(x, y), "({x}, {y})");
        }
        assert_eq!(d.pixel(236, 79)[3], 0, "fora do quadrado andado");
        assert_eq!(d.pixel(297, 119)[3], 0);
    }

    #[test]
    fn escala_e_giro_vao_e_voltam() {
        let caixa = Caixa::from(Retangulo::novo(200, 100, 60, 40));
        let t = Transformacao {
            dx: 10.0,
            dy: -5.0,
            escala_x: 2.0,
            escala_y: 0.5,
            angulo: 0.7,
        };
        let (x, y) = t.aplicar(&caixa, 211.0, 133.0);
        let (u, v) = t.inversa(&caixa, x, y);
        assert!((u - 211.0).abs() < 1e-3 && (v - 133.0).abs() < 1e-3);
        // Dobrar a escala dobra a caixa em volta do centro.
        let c = camada_com_quadrado();
        let conteudo = Conteudo::da_camada(&c, None).unwrap();
        let t = Transformacao {
            escala_x: 2.0,
            escala_y: 2.0,
            ..Default::default()
        };
        let d = desenhar(&conteudo, &t, 600, 400);
        assert_eq!(d.pixel(171, 81)[3], 255, "o canto andou para fora");
        assert_eq!(d.pixel(168, 78)[3], 0);
        // 90°: a caixa deitada fica em pé.
        let t = Transformacao {
            angulo: std::f32::consts::FRAC_PI_2,
            ..Default::default()
        };
        let d = desenhar(&conteudo, &t, 600, 400);
        assert_eq!(d.pixel(230, 92)[3], 255, "30 px acima do centro (230, 120)");
        assert_eq!(d.pixel(205, 120)[3], 0, "onde a largura estava");
    }

    #[test]
    fn o_desenho_rapido_e_o_mesmo_da_inversa_pixel_a_pixel() {
        let c = camada_com_quadrado();
        let conteudo = Conteudo::da_camada(&c, None).unwrap();
        let t = Transformacao {
            dx: 33.0,
            dy: 140.0,
            escala_x: 3.3,
            escala_y: 2.1,
            angulo: 0.9,
        };
        let rapido = desenhar(&conteudo, &t, 600, 400);
        let caixa = conteudo.caixa;
        let mut diferentes = 0;
        for y in 0..400 {
            for x in 0..600 {
                let (u, v) = t.inversa(&caixa, x as f32 + 0.5, y as f32 + 0.5);
                let lento = conteudo.amostra(u - caixa.x as f32, v - caixa.y as f32);
                let r = rapido.pixel(x, y);
                // Com alfa quase zero a cor não aparece: só o alfa conta.
                let invisivel = r[3] <= 2 && lento[3] <= 2;
                let canais = if invisivel { 3..4 } else { 0..4 };
                if canais
                    .clone()
                    // Na borda inclinada e ampliada, a soma incremental e o
                    // seno/cosseno diferem no arredondamento: até 2 no alfa.
                    .any(|i| (r[i] as i32 - lento[i] as i32).abs() > 2)
                {
                    diferentes += 1;
                }
            }
        }
        assert_eq!(diferentes, 0);
        let linhas: std::collections::BTreeSet<i32> =
            rapido.existentes().map(|(p, _)| p.1).collect();
        assert!(linhas.len() > 1, "ocupou mais de uma faixa (threads)");
    }

    #[test]
    fn por_cima_do_fundo() {
        let mut fundo = CamadaDePixels::nova(300, 300);
        fundo.tile_mut((0, 0))[..4].copy_from_slice(&[10, 10, 10, 255]);
        let mut cima = CamadaDePixels::nova(300, 300);
        cima.tile_mut((0, 0))[4..8].copy_from_slice(&[200, 0, 0, 255]);
        let s = sobre(&fundo, &cima);
        assert_eq!(s.pixel(0, 0), [10, 10, 10, 255]);
        assert_eq!(s.pixel(1, 0), [200, 0, 0, 255]);
    }

    #[test]
    fn a_alca_escala_com_a_oposta_parada() {
        let caixa = Caixa::from(Retangulo::novo(100, 100, 200, 100));
        let t = Transformacao::default();
        let alcas = Transformacao::alcas(&caixa);
        // O canto ↘ (4) puxado até (500, 300) com o ↖ (0) parado, livre.
        let n = t.pela_alca(&caixa, 4, (500.0, 300.0), alcas[0], false);
        assert!((n.escala_x - 2.0).abs() < 1e-4 && (n.escala_y - 2.0).abs() < 1e-4);
        let p0 = n.aplicar(&caixa, 100.0, 100.0);
        assert!(
            (p0.0 - 100.0).abs() < 1e-3 && (p0.1 - 100.0).abs() < 1e-3,
            "{p0:?}"
        );
        // Proporcional: um arrasto torto mantém a razão.
        let n = t.pela_alca(&caixa, 4, (500.0, 250.0), alcas[0], true);
        assert!((n.escala_x - n.escala_y).abs() < 1e-4);
        // O meio do lado direito (3): só a largura.
        let n = t.pela_alca(&caixa, 3, (400.0, 170.0), alcas[7], true);
        assert!((n.escala_x - 1.5).abs() < 1e-4 && (n.escala_y - 1.0).abs() < 1e-6);
        let esquerda = n.aplicar(&caixa, 100.0, 150.0);
        assert!((esquerda.0 - 100.0).abs() < 1e-3);
        // Com ⌥ a âncora é o centro: o centro fica e os dois lados andam.
        let centro = (200.0, 150.0);
        let n = t.pela_alca(&caixa, 3, (400.0, 150.0), centro, false);
        assert!((n.escala_x - 2.0).abs() < 1e-4);
        let c = n.aplicar(&caixa, 200.0, 150.0);
        assert!((c.0 - 200.0).abs() < 1e-3);
        // Nunca vira espelho.
        let n = t.pela_alca(&caixa, 3, (0.0, 150.0), alcas[7], false);
        assert!(n.escala_x >= 0.01);
    }

    #[test]
    fn o_giro_e_a_escala_respeitam_o_ponto_de_referencia() {
        let caixa = Caixa::from(Retangulo::novo(100, 100, 200, 100));
        let t = Transformacao::default();
        let referencia = (100.0, 100.0);
        let n = t.girada_em_volta(&caixa, referencia, std::f32::consts::FRAC_PI_2);
        let r = n.aplicar(&caixa, 100.0, 100.0);
        assert!(
            (r.0 - 100.0).abs() < 1e-3 && (r.1 - 100.0).abs() < 1e-3,
            "{r:?}"
        );
        // O canto ↗ (300, 100) gira 90° em volta de (100, 100): vai a (100, 300).
        let q = n.aplicar(&caixa, 300.0, 100.0);
        assert!(
            (q.0 - 100.0).abs() < 1e-3 && (q.1 - 300.0).abs() < 1e-3,
            "{q:?}"
        );
        // Depois de girada, a alça segue no referencial da caixa.
        let e = n.pela_alca(
            &caixa,
            3,
            (100.0, 400.0),
            Transformacao::alcas(&caixa)[7],
            false,
        );
        assert!((e.escala_x - 1.5).abs() < 1e-3, "{}", e.escala_x);
    }
}
