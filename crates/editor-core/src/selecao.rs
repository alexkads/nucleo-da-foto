//! A seleção: uma máscara de 0 a 255 por pixel da foto, que limita o pincel, a
//! borracha, o apagar e o preencher — o "letreiro" do Photoshop.
//!
//! 🔑 **Esparsa como as camadas.** Tiles de 256×256 bytes, e um valor
//! **padrão** para o tile que não existe: 0 numa seleção comum, 255 em
//! "Selecionar tudo" e na maior parte de uma seleção invertida. Selecionar
//! tudo numa foto de 24 MP não aloca nada; um retângulo pequeno aloca os tiles
//! que ele toca.
//!
//! As formas viram máscara na hora (rasterizadas): retângulo de borda exata,
//! elipse e laço com ou sem antisserrilhado ([`Acabamento`]) — sem ele, pelo
//! centro de cada pixel (par e ímpar no laço); com ele, um pixel de rampa na
//! elipse e a cobertura de verdade no laço (quatro linhas por pixel, a área
//! exata na horizontal). A difusão do acabamento é a da forma **antes** de
//! entrar na seleção, como o "Difusão" da barra de opções do Photoshop.
//! Somar (⇧) é o máximo das duas, subtrair (⌥) é `a · (1 − b)`, cruzar (⇧⌥) é
//! o mínimo.
//!
//! A seleção **entra no desfazer** como no Photoshop (`Comando::Selecao`, desde
//! a etapa 13), mas **não vai para o projeto**: não muda pixel, e a seleção não
//! é salva.

use std::collections::BTreeMap;
use std::sync::Arc;

use image::RgbImage;

use crate::documento::Mascara;
use crate::retangulo::Retangulo;
use crate::tiles::{retangulo_do_tile, CamadaDePixels, Posicao, LADO_DO_TILE};

/// A média numa janela de `2r + 1` ao longo das linhas (`horizontal`) ou das
/// colunas, com a borda repetida. Valores 0..=255 em `u16` (a soma não estoura
/// no acumulador `u32`).
fn caixa(d: &mut [u16], l: usize, a: usize, r: usize, horizontal: bool) {
    let (n, linhas) = if horizontal { (l, a) } else { (a, l) };
    let indice = |linha: usize, k: usize| {
        if horizontal {
            linha * l + k
        } else {
            k * l + linha
        }
    };
    let mut copia = vec![0u16; n];
    let janela = (2 * r + 1) as u32;
    for linha in 0..linhas {
        for (k, c) in copia.iter_mut().enumerate() {
            *c = d[indice(linha, k)];
        }
        let em = |k: isize| copia[k.clamp(0, n as isize - 1) as usize] as u32;
        let mut soma: u32 = (-(r as isize)..=r as isize).map(em).sum();
        for k in 0..n {
            d[indice(linha, k)] = ((soma + janela / 2) / janela) as u16;
            soma += em(k as isize + r as isize + 1);
            soma -= em(k as isize - r as isize);
        }
    }
}

/// O máximo numa janela de `2r + 1` ao longo das linhas ou das colunas — a
/// dilatação, separável num quadrado.
fn maximo(d: &mut [u8], l: usize, a: usize, r: usize, horizontal: bool) {
    let (n, linhas) = if horizontal { (l, a) } else { (a, l) };
    let indice = |linha: usize, k: usize| {
        if horizontal {
            linha * l + k
        } else {
            k * l + linha
        }
    };
    let mut copia = vec![0u8; n];
    // A fila de índices com valores decrescentes (máximo deslizante).
    let mut fila: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for linha in 0..linhas {
        for (k, c) in copia.iter_mut().enumerate() {
            *c = d[indice(linha, k)];
        }
        fila.clear();
        let mut proximo = 0usize;
        for k in 0..n {
            let fim = (k + r).min(n - 1);
            while proximo <= fim {
                while fila.back().is_some_and(|&b| copia[b] <= copia[proximo]) {
                    fila.pop_back();
                }
                fila.push_back(proximo);
                proximo += 1;
            }
            while fila.front().is_some_and(|&f| f + r < k) {
                fila.pop_front();
            }
            d[indice(linha, k)] = copia[*fila.front().unwrap_or(&k)];
        }
    }
}

/// Os cruzamentos das arestas do polígono com a linha horizontal `yc`,
/// ordenados — dentro é entre o 1º e o 2º, o 3º e o 4º… (par e ímpar).
fn cortes(pontos: &[(f32, f32)], yc: f32) -> Vec<f32> {
    let mut cortes = Vec::new();
    for i in 0..pontos.len() {
        let (ax, ay) = pontos[i];
        let (bx, by) = pontos[(i + 1) % pontos.len()];
        if (ay <= yc) != (by <= yc) {
            cortes.push(ax + (yc - ay) / (by - ay) * (bx - ax));
        }
    }
    cortes.sort_by(f32::total_cmp);
    cortes
}

/// Quantas linhas por pixel o laço antisserrilhado amostra.
const LINHAS_POR_PIXEL: usize = 4;

/// O polígono rasterizado em `caixa`, um byte por pixel. Sem `suavizar`, pelo
/// centro de cada pixel; com ele, a cobertura: quatro linhas por pixel, e em
/// cada uma a fração exata de cada pixel que cai dentro.
fn cobertura_do_poligono(pontos: &[(f32, f32)], caixa: &Retangulo, suavizar: bool) -> Vec<u8> {
    let (l, a) = (caixa.largura as usize, caixa.altura as usize);
    let mut saida = vec![0u8; l * a];
    let mut soma = vec![0f32; l];
    let (xa, xb) = (caixa.x as f32, caixa.direita() as f32);
    for linha in 0..a {
        let y = (caixa.y as usize + linha) as f32;
        let fora = &mut saida[linha * l..(linha + 1) * l];
        if !suavizar {
            let c = cortes(pontos, y + 0.5);
            for (i, v) in fora.iter_mut().enumerate() {
                let xc = xa + i as f32 + 0.5;
                if c.iter().take_while(|k| **k <= xc).count() % 2 == 1 {
                    *v = 255;
                }
            }
            continue;
        }
        soma.iter_mut().for_each(|s| *s = 0.0);
        for k in 0..LINHAS_POR_PIXEL {
            let c = cortes(pontos, y + (k as f32 + 0.5) / LINHAS_POR_PIXEL as f32);
            for par in c.as_chunks::<2>().0 {
                let (de, ate) = (par[0].max(xa), par[1].min(xb));
                if ate <= de {
                    continue;
                }
                let (i0, i1) = ((de - xa) as usize, ((ate - xa).ceil() as usize).min(l));
                for (i, s) in soma.iter_mut().enumerate().take(i1).skip(i0) {
                    let x = xa + i as f32;
                    *s += (ate.min(x + 1.0) - de.max(x)).max(0.0);
                }
            }
        }
        for (v, s) in fora.iter_mut().zip(&soma) {
            *v = (s / LINHAS_POR_PIXEL as f32 * 255.0).round().min(255.0) as u8;
        }
    }
    saida
}

const BYTES: usize = (LADO_DO_TILE * LADO_DO_TILE) as usize;

/// Uma forma desenhada, em pixels da foto.
#[derive(Clone, Debug, PartialEq)]
pub enum Forma {
    Retangulo(Retangulo),
    /// A elipse inscrita no retângulo.
    Elipse(Retangulo),
    /// A elipse inscrita na caixa `(x0, y0, x1, y1)`, que pode passar da foto:
    /// o pedaço de fora se perde, sem achatar a elipse (o `Retangulo` não tem
    /// canto negativo, e cortá-lo antes mudaria a forma).
    ElipseNaCaixa(f32, f32, f32, f32),
    /// Os vértices do laço, fechado do último ao primeiro (o laço livre e o
    /// poligonal).
    Laco(Vec<(f32, f32)>),
}

/// Como a forma vira máscara: a borda antisserrilhada e a difusão, em pixels
/// do documento — as opções da ferramenta valem para a **próxima** seleção, e
/// nunca mudam a que já existe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Acabamento {
    /// Antisserrilhado na elipse e no laço (o retângulo é sempre exato).
    pub suavizar: bool,
    /// Difusão da forma antes de entrar na seleção (0 = borda como desenhada).
    pub difusao: u32,
}

impl Default for Acabamento {
    /// O padrão do Photoshop: antisserrilhado ligado, difusão 0.
    fn default() -> Self {
        Self {
            suavizar: true,
            difusao: 0,
        }
    }
}

/// O estilo da seleção retangular e da elíptica ("Estilo" na barra de opções do
/// Photoshop). Tudo em pixels **do documento**: zoom e giro da vista não mudam
/// nada.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Estilo {
    /// O arrasto decide a caixa (⇧ quadrado, ⌥ do centro).
    #[default]
    Normal,
    /// A razão largura:altura fica presa; o arrasto decide o tamanho.
    Proporcao { largura: f32, altura: f32 },
    /// Largura e altura exatas; o arrasto só leva a caixa.
    Tamanho { largura: u32, altura: u32 },
}

impl Estilo {
    /// As predefinições de proporção da barra.
    pub const PROPORCOES: [(u32, u32); 4] = [(1, 1), (3, 2), (4, 3), (16, 9)];

    /// Largura e altura trocadas (o ⇄ da barra).
    pub fn trocado(self) -> Self {
        match self {
            Estilo::Normal => Estilo::Normal,
            Estilo::Proporcao { largura, altura } => Estilo::Proporcao {
                largura: altura,
                altura: largura,
            },
            Estilo::Tamanho { largura, altura } => Estilo::Tamanho {
                largura: altura,
                altura: largura,
            },
        }
    }
}

/// O sinal de um deslocamento, com o zero para a frente (`f32::signum(0.0)` é
/// 1, mas `-0.0` daria −1).
fn sentido(d: f32) -> f32 {
    if d < 0.0 {
        -1.0
    } else {
        1.0
    }
}

/// Os dois cantos opostos da caixa de um arrasto de seleção, em pixels da foto
/// (sem ordenar e sem cortar na foto).
///
/// Como os modificadores conversam com o estilo — a regra desta casa, a mesma
/// do Photoshop onde ele é claro:
///
/// | estilo | ⇧ no arrasto | ⌥ no arrasto | Espaço |
/// |---|---|---|---|
/// | Normal | quadrado/círculo | do centro | reposiciona (quem chama anda `inicio`) |
/// | Proporção fixa | nada (a razão já prende) | do centro, com a razão | reposiciona |
/// | Tamanho fixo | nada | a caixa centrada no ponteiro | a caixa já segue o ponteiro |
///
/// Na proporção fixa vale a maior das duas medidas que o arrasto pede (a caixa
/// cobre sempre o ponteiro). No tamanho fixo o canto de cima à esquerda fica
/// no ponteiro, e a caixa anda com ele enquanto o botão está apertado.
pub fn caixa_do_arrasto(
    inicio: (f32, f32),
    fim: (f32, f32),
    estilo: Estilo,
    quadrado: bool,
    do_centro: bool,
) -> ((f32, f32), (f32, f32)) {
    let (dx, dy) = (fim.0 - inicio.0, fim.1 - inicio.1);
    let b = match estilo {
        Estilo::Tamanho { largura, altura } => {
            let (l, a) = (largura as f32, altura as f32);
            if do_centro {
                let x0 = (fim.0 - l / 2.0).round();
                let y0 = (fim.1 - a / 2.0).round();
                return ((x0, y0), (x0 + l, y0 + a));
            }
            let (x0, y0) = (fim.0.round(), fim.1.round());
            return ((x0, y0), (x0 + l, y0 + a));
        }
        Estilo::Proporcao { largura, altura } if largura > 0.0 && altura > 0.0 => {
            let razao = altura / largura;
            let l = dx.abs().max(dy.abs() / razao);
            (
                inicio.0 + l * sentido(dx),
                inicio.1 + l * razao * sentido(dy),
            )
        }
        _ if quadrado => {
            let lado = dx.abs().max(dy.abs());
            // Sem andar num eixo, fica no lugar nele (a lição do `signum` da
            // etapa 3).
            (
                if dx == 0.0 {
                    inicio.0
                } else {
                    inicio.0 + lado * sentido(dx)
                },
                if dy == 0.0 {
                    inicio.1
                } else {
                    inicio.1 + lado * sentido(dy)
                },
            )
        }
        _ => fim,
    };
    if do_centro {
        ((2.0 * inicio.0 - b.0, 2.0 * inicio.1 - b.1), b)
    } else {
        (inicio, b)
    }
}

/// Largura e altura, em pixels do documento, da caixa de dois cantos — o que a
/// tela mostra durante o arrasto.
pub fn medida_da_caixa(a: (f32, f32), b: (f32, f32)) -> (u32, u32) {
    (
        (a.0.round() - b.0.round()).abs() as u32,
        (a.1.round() - b.1.round()).abs() as u32,
    )
}

/// O que a varinha mágica lê: um pixel do documento por posição, `[r, g, b,
/// a]` com a cor **pré-multiplicada** pelo alfa — o transparente é tudo zero,
/// qualquer que seja a cor guardada no pixel apagado, e um vermelho meio
/// apagado não passa por um vermelho cheio.
///
/// 🔑 A fonte de amostragem fica fora da operação de seleção
/// ([`Selecao::por_cor_em`]): a sessão escolhe entre a camada atual e a foto
/// como aparece.
pub struct Amostra {
    largura: u32,
    altura: u32,
    pixels: Vec<[u8; 4]>,
}

impl Amostra {
    /// A foto composta (todas as camadas visíveis sobre a base): opaca.
    pub fn da_imagem(foto: &RgbImage) -> Self {
        Self {
            largura: foto.width(),
            altura: foto.height(),
            pixels: foto.pixels().map(|p| [p[0], p[1], p[2], 255]).collect(),
        }
    }

    /// Só os pixels de uma camada, no espaço do documento `largura × altura` —
    /// sem a base nem as outras camadas. O que a camada não cobre (tile que não
    /// existe, ou fora dela) é transparente.
    pub fn da_camada(camada: &CamadaDePixels, largura: u32, altura: u32) -> Self {
        let (l, a) = (largura as usize, altura as usize);
        let mut pixels = vec![[0u8; 4]; l * a];
        for (posicao, tile) in camada.existentes() {
            let r = retangulo_do_tile(*posicao, camada.largura(), camada.altura())
                .limitado(largura, altura);
            for y in 0..r.altura {
                for x in 0..r.largura {
                    let k = (y * LADO_DO_TILE + x) as usize * 4;
                    let p = &tile[k..k + 4];
                    let alfa = p[3] as u32;
                    let pre = |c: u8| ((c as u32 * alfa + 127) / 255) as u8;
                    pixels[(r.y + y) as usize * l + (r.x + x) as usize] =
                        [pre(p[0]), pre(p[1]), pre(p[2]), p[3]];
                }
            }
        }
        Self {
            largura,
            altura,
            pixels,
        }
    }

    /// Uma máscara como cinza opaco (a varinha com a máscara escolhida).
    pub fn da_mascara(mascara: &Mascara) -> Self {
        let (largura, altura) = (mascara.pixels.largura(), mascara.pixels.altura());
        let f = mascara.fundo;
        let mut pixels = vec![[f, f, f, 255]; largura as usize * altura as usize];
        for (posicao, tile) in mascara.pixels.existentes() {
            let r = retangulo_do_tile(*posicao, largura, altura);
            for y in 0..r.altura {
                for x in 0..r.largura {
                    let k = (y * LADO_DO_TILE + x) as usize * 4;
                    let p = [tile[k], tile[k + 1], tile[k + 2], tile[k + 3]];
                    let v = Mascara::valor_do_pixel(f, p);
                    pixels[(r.y + y) as usize * largura as usize + (r.x + x) as usize] =
                        [v, v, v, 255];
                }
            }
        }
        Self {
            largura,
            altura,
            pixels,
        }
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        self.pixels[y as usize * self.largura as usize + x as usize]
    }
}

/// Como uma forma nova entra na seleção que já existe.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Operacao {
    #[default]
    Nova,
    /// ⇧ — soma à seleção.
    Somar,
    /// ⌥ — tira da seleção.
    Subtrair,
    /// ⇧⌥ — fica só o que está nas duas.
    Intersecao,
}

impl Operacao {
    /// A operação dos modificadores, a mesma em toda ferramenta de seleção e na
    /// miniatura da camada: ⇧ soma, ⌥ tira, ⇧⌥ cruza (a tabela "Select and move
    /// objects" da Adobe).
    pub fn dos_modificadores(shift: bool, alt: bool) -> Self {
        match (shift, alt) {
            (true, true) => Operacao::Intersecao,
            (true, false) => Operacao::Somar,
            (false, true) => Operacao::Subtrair,
            (false, false) => Operacao::Nova,
        }
    }
}

/// A seleção guardada para ser transformada muitas vezes — o arrasto do
/// "Transformar seleção": a máscara densa do pedaço selecionado sai uma vez,
/// e cada passo do arrasto só a amostra.
#[derive(Clone, Debug)]
pub struct Molde {
    largura: u32,
    altura: u32,
    regiao: Retangulo,
    densa: Arc<Vec<u8>>,
}

impl Molde {
    pub fn de(selecao: &Selecao) -> Self {
        let regiao = selecao.caixa_justa();
        Self {
            largura: selecao.largura,
            altura: selecao.altura,
            regiao,
            densa: Arc::new(selecao.densa_em(&regiao)),
        }
    }

    /// A caixa do que está selecionado — a caixa da transformação.
    pub fn caixa(&self) -> Retangulo {
        self.regiao
    }

    /// A seleção transformada pela afim `(u, v) = c + a·x + b·y`, que leva o
    /// centro de um pixel do destino ao ponto da seleção de antes de onde ele
    /// vem — bilinear, fora do molde é 0. Só calcula dentro de `destino`; uma
    /// faixa de tiles por thread, como o ⌘T dos pixels.
    pub fn transformado(
        &self,
        destino: &Retangulo,
        c: (f32, f32),
        a: (f32, f32),
        b: (f32, f32),
    ) -> Selecao {
        let mut s = Selecao::vazia(self.largura, self.altura);
        let destino = destino.limitado(self.largura, self.altura);
        if destino.vazio() || self.regiao.vazio() {
            return s;
        }
        let (rx, ry) = (self.regiao.x as i64, self.regiao.y as i64);
        let (rl, ra) = (self.regiao.largura as i64, self.regiao.altura as i64);
        let ler = |x: i64, y: i64| -> f32 {
            let (i, j) = (x - rx, y - ry);
            if i < 0 || j < 0 || i >= rl || j >= ra {
                0.0
            } else {
                self.densa[(j * rl + i) as usize] as f32
            }
        };
        let faixa = |linha: u32| -> Vec<(Posicao, Vec<u8>)> {
            let mut tiles: BTreeMap<Posicao, Vec<u8>> = BTreeMap::new();
            let ya = destino.y.max(linha * LADO_DO_TILE);
            let yb = destino.baixo().min((linha + 1) * LADO_DO_TILE);
            for y in ya..yb {
                let yc = y as f32 + 0.5;
                for x in destino.x..destino.direita() {
                    let xc = x as f32 + 0.5;
                    let u = c.0 + a.0 * xc + b.0 * yc - 0.5;
                    let v = c.1 + a.1 * xc + b.1 * yc - 0.5;
                    let (x0, y0) = (u.floor(), v.floor());
                    let (fx, fy) = (u - x0, v - y0);
                    let (x0, y0) = (x0 as i64, y0 as i64);
                    let topo = ler(x0, y0) * (1.0 - fx) + ler(x0 + 1, y0) * fx;
                    let base = ler(x0, y0 + 1) * (1.0 - fx) + ler(x0 + 1, y0 + 1) * fx;
                    let valor = (topo * (1.0 - fy) + base * fy).round().clamp(0.0, 255.0) as u8;
                    if valor == 0 {
                        continue;
                    }
                    let tile = tiles
                        .entry((x / LADO_DO_TILE, y / LADO_DO_TILE))
                        .or_insert_with(|| vec![0; BYTES]);
                    tile[((y % LADO_DO_TILE) * LADO_DO_TILE + x % LADO_DO_TILE) as usize] = valor;
                }
            }
            tiles.into_iter().collect()
        };
        let linhas: Vec<u32> =
            (destino.y / LADO_DO_TILE..=(destino.baixo() - 1) / LADO_DO_TILE).collect();
        let resultados: Vec<Vec<(Posicao, Vec<u8>)>> = std::thread::scope(|escopo| {
            let tarefas: Vec<_> = linhas
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
        });
        for (posicao, tile) in resultados.into_iter().flatten() {
            s.tiles.insert(posicao, Arc::new(tile));
        }
        s
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Selecao {
    largura: u32,
    altura: u32,
    padrao: u8,
    tiles: BTreeMap<Posicao, Arc<Vec<u8>>>,
}

impl Selecao {
    /// Nada selecionado.
    pub fn vazia(largura: u32, altura: u32) -> Self {
        Self {
            largura,
            altura,
            padrao: 0,
            tiles: BTreeMap::new(),
        }
    }

    /// A foto inteira (⌘A).
    pub fn tudo(largura: u32, altura: u32) -> Self {
        Self {
            padrao: 255,
            ..Self::vazia(largura, altura)
        }
    }

    /// A máscara de uma forma, com o acabamento padrão (antisserrilhado, sem
    /// difusão).
    pub fn da_forma(largura: u32, altura: u32, forma: &Forma) -> Self {
        Self::da_forma_com(largura, altura, forma, Acabamento::default())
    }

    /// A máscara de uma forma com o acabamento da ferramenta: antisserrilhado
    /// ou não, e a difusão aplicada à forma (antes de ela entrar na seleção).
    pub fn da_forma_com(largura: u32, altura: u32, forma: &Forma, acabamento: Acabamento) -> Self {
        let mut s = Self::vazia(largura, altura);
        let suavizar = acabamento.suavizar;
        match forma {
            Forma::Retangulo(r) => {
                let r = r.limitado(largura, altura);
                s.pintar(&r, |_, _| 255);
            }
            Forma::Elipse(r) => {
                let (x0, y0) = (r.x as f32, r.y as f32);
                return Self::da_forma_com(
                    largura,
                    altura,
                    &Forma::ElipseNaCaixa(x0, y0, x0 + r.largura as f32, y0 + r.altura as f32),
                    acabamento,
                );
            }
            Forma::ElipseNaCaixa(a0, b0, a1, b1) => {
                let (x0, x1) = (a0.min(*a1), a0.max(*a1));
                let (y0, y1) = (b0.min(*b1), b0.max(*b1));
                let (rx, ry) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
                if rx < 0.5 || ry < 0.5 {
                    return s;
                }
                let (cx, cy) = (x0 + rx, y0 + ry);
                let menor = rx.min(ry);
                let caixa = Retangulo::novo(
                    x0.floor().max(0.0) as u32,
                    y0.floor().max(0.0) as u32,
                    (x1.ceil() - x0.floor().max(0.0)).max(0.0) as u32,
                    (y1.ceil() - y0.floor().max(0.0)).max(0.0) as u32,
                )
                .limitado(largura, altura);
                s.pintar(&caixa, |x, y| {
                    let (dx, dy) = ((x as f32 + 0.5 - cx) / rx, (y as f32 + 0.5 - cy) / ry);
                    let raio = (dx * dx + dy * dy).sqrt();
                    if !suavizar {
                        return if raio <= 1.0 { 255 } else { 0 };
                    }
                    // A distância à borda, em pixels, aproximada pelo raio menor:
                    // meio pixel de cada lado da borda é a rampa.
                    let dentro = (0.5 - (raio - 1.0) * menor).clamp(0.0, 1.0);
                    (dentro * 255.0).round() as u8
                });
            }
            Forma::Laco(pontos) => {
                if pontos.len() < 3 {
                    return s;
                }
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for &(x, y) in pontos {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                }
                let caixa = Retangulo::novo(
                    x0.floor().max(0.0) as u32,
                    y0.floor().max(0.0) as u32,
                    (x1.ceil() - x0.floor()).max(0.0) as u32 + 1,
                    (y1.ceil() - y0.floor()).max(0.0) as u32 + 1,
                )
                .limitado(largura, altura);
                if caixa.vazio() {
                    return s;
                }
                let cobertura = cobertura_do_poligono(pontos, &caixa, suavizar);
                let l = caixa.largura as usize;
                s.pintar(&caixa, |x, y| {
                    cobertura[(y - caixa.y) as usize * l + (x - caixa.x) as usize]
                });
            }
        }
        s.enxugar();
        if acabamento.difusao > 0 {
            s = s.difusa(acabamento.difusao);
        }
        s
    }

    /// O pincel da região de amostragem do preenchimento: um disco de `raio`
    /// em volta de `(x, y)` vira selecionado (`incluir`) ou não, com um pixel
    /// de rampa na borda. Devolve o retângulo mexido.
    pub fn pintar_disco(&mut self, x: f32, y: f32, raio: f32, incluir: bool) -> Retangulo {
        let r = raio.max(0.5);
        let caixa = Retangulo::novo(
            (x - r - 1.0).floor().max(0.0) as u32,
            (y - r - 1.0).floor().max(0.0) as u32,
            (2.0 * r + 3.0) as u32,
            (2.0 * r + 3.0) as u32,
        )
        .limitado(self.largura, self.altura);
        let antes = self.clone();
        self.pintar(&caixa, |px, py| {
            let d = (px as f32 + 0.5 - x).hypot(py as f32 + 0.5 - y);
            let cobertura = (r + 0.5 - d).clamp(0.0, 1.0);
            let velho = antes.valor(px, py) as f32;
            let alvo = if incluir { 255.0 } else { 0.0 };
            (velho + (alvo - velho) * cobertura).round() as u8
        });
        self.enxugar();
        caixa
    }

    /// A varinha mágica (W) numa foto opaca — ver [`Self::por_cor_em`].
    pub fn por_cor(foto: &RgbImage, ponto: (u32, u32), tolerancia: u8, contigua: bool) -> Self {
        Self::por_cor_em(
            &Amostra::da_imagem(foto),
            ponto,
            tolerancia,
            contigua,
            false,
        )
    }

    /// A varinha mágica (W): os pixels da `amostra` cuja cor (pré-multiplicada)
    /// e cujo alfa diferem dos de `(x, y)` no máximo `tolerancia` em cada canal
    /// (32 no Photoshop). Contígua, só a área ligada ao ponto; senão, todos os
    /// parecidos. Num pixel transparente, pega o transparente em volta.
    /// `suavizar`: a borda ganha um pixel de rampa (média 3×3 da máscara), com
    /// a metade do caminho na borda de antes.
    pub fn por_cor_em(
        amostra: &Amostra,
        (x, y): (u32, u32),
        tolerancia: u8,
        contigua: bool,
        suavizar: bool,
    ) -> Self {
        let (largura, altura) = (amostra.largura, amostra.altura);
        let mut s = Self::vazia(largura, altura);
        if x >= largura || y >= altura {
            return s;
        }
        let alvo = amostra.pixel(x, y);
        let tol = tolerancia as i16;
        let (l, a) = (largura as usize, altura as usize);
        let bruto = &amostra.pixels;
        let parecido =
            |i: usize| (0..4).all(|c| (bruto[i][c] as i16 - alvo[c] as i16).abs() <= tol);
        let mut marcado = vec![false; l * a];
        if contigua {
            // Por linhas, como a lata de tinta.
            let mut pilha = vec![(x as usize, y as usize)];
            let pode = |m: &[bool], x: usize, y: usize| !m[y * l + x] && parecido(y * l + x);
            while let Some((px, py)) = pilha.pop() {
                if !pode(&marcado, px, py) {
                    continue;
                }
                let mut x0 = px;
                while x0 > 0 && pode(&marcado, x0 - 1, py) {
                    x0 -= 1;
                }
                let mut x1 = px;
                while x1 + 1 < l && pode(&marcado, x1 + 1, py) {
                    x1 += 1;
                }
                marcado[py * l + x0..=py * l + x1].fill(true);
                for ny in [py.wrapping_sub(1), py + 1] {
                    if ny >= a {
                        continue;
                    }
                    let mut xx = x0;
                    while xx <= x1 {
                        if pode(&marcado, xx, ny) {
                            pilha.push((xx, ny));
                            while xx <= x1 && pode(&marcado, xx, ny) {
                                xx += 1;
                            }
                        } else {
                            xx += 1;
                        }
                    }
                }
            }
        } else {
            for (i, m) in marcado.iter_mut().enumerate() {
                *m = parecido(i);
            }
        }
        let densa: Vec<u8> = if suavizar {
            let mut d: Vec<u16> = marcado.iter().map(|m| if *m { 255 } else { 0 }).collect();
            caixa(&mut d, l, a, 1, true);
            caixa(&mut d, l, a, 1, false);
            d.iter().map(|v| *v as u8).collect()
        } else {
            marcado.iter().map(|m| if *m { 255 } else { 0 }).collect()
        };
        s.carregar_densa(&densa);
        s
    }

    /// A seleção do alfa de uma camada (⌘ + clique na miniatura): o que está
    /// pintado fica selecionado, na proporção da opacidade.
    pub fn do_alfa(camada: &CamadaDePixels) -> Self {
        let mut s = Self::vazia(camada.largura(), camada.altura());
        for (posicao, tile) in camada.existentes() {
            let valores: Vec<u8> = tile.iter().skip(3).step_by(4).copied().collect();
            s.tiles.insert(*posicao, Arc::new(valores));
        }
        s.enxugar();
        s
    }

    /// A seleção de uma máscara (⌘ + clique na miniatura dela): o que ela
    /// revela.
    pub fn da_mascara(mascara: &Mascara) -> Self {
        let (largura, altura) = (mascara.pixels.largura(), mascara.pixels.altura());
        let mut s = Self {
            padrao: mascara.fundo,
            ..Self::vazia(largura, altura)
        };
        for (posicao, tile) in mascara.pixels.existentes() {
            let valores: Vec<u8> = tile
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| Mascara::valor_do_pixel(mascara.fundo, *p))
                .collect();
            s.tiles.insert(*posicao, Arc::new(valores));
        }
        s.enxugar();
        s
    }

    /// A máscara inteira, um byte por pixel da foto.
    fn densa(&self) -> Vec<u8> {
        self.densa_em(&Retangulo::inteiro(self.largura, self.altura))
    }

    /// A máscara do recorte `ret`, um byte por pixel, linha a linha.
    fn densa_em(&self, ret: &Retangulo) -> Vec<u8> {
        let l = ret.largura as usize;
        let mut d = vec![self.padrao; l * ret.altura as usize];
        for (posicao, tile) in &self.tiles {
            let t = retangulo_do_tile(*posicao, self.largura, self.altura);
            let (x0, x1) = (t.x.max(ret.x), t.direita().min(ret.direita()));
            let (y0, y1) = (t.y.max(ret.y), t.baixo().min(ret.baixo()));
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            for y in y0..y1 {
                let de = ((y - t.y) * LADO_DO_TILE + (x0 - t.x)) as usize;
                let para = (y - ret.y) as usize * l + (x0 - ret.x) as usize;
                let n = (x1 - x0) as usize;
                d[para..para + n].copy_from_slice(&tile[de..de + n]);
            }
        }
        d
    }

    /// Troca o conteúdo pelo de uma máscara densa (do tamanho da foto).
    fn carregar_densa(&mut self, d: &[u8]) {
        self.tiles.clear();
        self.padrao = 0;
        let inteiro = Retangulo::inteiro(self.largura, self.altura);
        self.carregar_recorte(&inteiro, d);
    }

    /// Escreve a máscara densa do recorte `ret` (o resto fica como está — 0
    /// numa seleção recém-criada).
    fn carregar_recorte(&mut self, ret: &Retangulo, d: &[u8]) {
        let l = ret.largura as usize;
        self.pintar(ret, |x, y| {
            d[(y - ret.y) as usize * l + (x - ret.x) as usize]
        });
        self.enxugar();
    }

    /// Difusão (⇧F6): a borda vira uma rampa de `raio` pixels — três passadas
    /// de caixa em cada direção, perto de um desfoque gaussiano. Só o pedaço
    /// em volta do selecionado é calculado: fora dele tudo é zero e continua
    /// zero (a margem cobre o alcance das três passadas).
    pub fn difusa(&self, raio: u32) -> Selecao {
        if raio == 0 || self.nada() {
            return self.clone();
        }
        let r = (raio as usize).div_ceil(2).max(1);
        let regiao = if self.padrao == 0 {
            let margem = (3 * r + 1) as u32;
            let c = self.caixa_justa();
            let (x0, y0) = (c.x.saturating_sub(margem), c.y.saturating_sub(margem));
            Retangulo::novo(x0, y0, c.direita() + margem - x0, c.baixo() + margem - y0)
                .limitado(self.largura, self.altura)
        } else {
            Retangulo::inteiro(self.largura, self.altura)
        };
        let (l, a) = (regiao.largura as usize, regiao.altura as usize);
        let mut d: Vec<u16> = self.densa_em(&regiao).iter().map(|v| *v as u16).collect();
        for _ in 0..3 {
            caixa(&mut d, l, a, r, true);
            caixa(&mut d, l, a, r, false);
        }
        let mut s = Selecao::vazia(self.largura, self.altura);
        s.carregar_recorte(&regiao, &d.iter().map(|v| *v as u8).collect::<Vec<_>>());
        s
    }

    /// Expandir (`px` > 0) ou contrair (`px` < 0) a seleção, em pixels — o
    /// máximo (ou o mínimo) numa janela quadrada em volta de cada pixel.
    pub fn expandida(&self, px: i32) -> Selecao {
        if px == 0 || self.nada() {
            return self.clone();
        }
        let (l, a) = (self.largura as usize, self.altura as usize);
        let contrair = px < 0;
        let mut d = self.densa();
        if contrair {
            d.iter_mut().for_each(|v| *v = 255 - *v);
        }
        let r = px.unsigned_abs() as usize;
        maximo(&mut d, l, a, r, true);
        maximo(&mut d, l, a, r, false);
        if contrair {
            // A borda da foto não conta: o que encosta nela não recua dela —
            // o padrão do Photoshop ("aplicar na borda da tela" desligado).
            d.iter_mut().for_each(|v| *v = 255 - *v);
        }
        let mut s = Selecao::vazia(self.largura, self.altura);
        s.carregar_densa(&d);
        s
    }

    /// Escreve `valor(x, y)` em cada pixel de `ret` (os de fora ficam no padrão).
    fn pintar(&mut self, ret: &Retangulo, valor: impl Fn(u32, u32) -> u8) {
        if ret.vazio() {
            return;
        }
        let (c0, c1) = (ret.x / LADO_DO_TILE, (ret.direita() - 1) / LADO_DO_TILE);
        let (l0, l1) = (ret.y / LADO_DO_TILE, (ret.baixo() - 1) / LADO_DO_TILE);
        for l in l0..=l1 {
            for c in c0..=c1 {
                let padrao = self.padrao;
                let tile = Arc::make_mut(
                    self.tiles
                        .entry((c, l))
                        .or_insert_with(|| Arc::new(vec![padrao; BYTES])),
                );
                let pedaco = retangulo_do_tile((c, l), self.largura, self.altura);
                let x0 = pedaco.x.max(ret.x);
                let x1 = pedaco.direita().min(ret.direita());
                let y0 = pedaco.y.max(ret.y);
                let y1 = pedaco.baixo().min(ret.baixo());
                for y in y0..y1 {
                    for x in x0..x1 {
                        let k = ((y - pedaco.y) * LADO_DO_TILE + (x - pedaco.x)) as usize;
                        tile[k] = valor(x, y);
                    }
                }
            }
        }
    }

    /// Tira os tiles iguais ao padrão — a seleção volta a ser esparsa.
    fn enxugar(&mut self) {
        let padrao = self.padrao;
        self.tiles.retain(|_, t| t.iter().any(|v| *v != padrao));
    }

    pub fn largura(&self) -> u32 {
        self.largura
    }

    pub fn altura(&self) -> u32 {
        self.altura
    }

    /// O valor da máscara em `(x, y)`; fora da foto é 0.
    pub fn valor(&self, x: u32, y: u32) -> u8 {
        if x >= self.largura || y >= self.altura {
            return 0;
        }
        match self.tiles.get(&(x / LADO_DO_TILE, y / LADO_DO_TILE)) {
            Some(t) => t[((y % LADO_DO_TILE) * LADO_DO_TILE + x % LADO_DO_TILE) as usize],
            None => self.padrao,
        }
    }

    /// A máscara de um tile (256×256), ou o valor de todos os pixels dele.
    pub fn do_tile(&self, posicao: Posicao) -> Result<&[u8], u8> {
        match self.tiles.get(&posicao) {
            Some(t) => Ok(t.as_slice()),
            None => Err(self.padrao),
        }
    }

    /// Quanto a seleção ocupa de memória (os tiles alocados).
    pub fn bytes(&self) -> usize {
        self.tiles.len() * BYTES
    }

    /// Nenhum pixel selecionado.
    pub fn nada(&self) -> bool {
        self.padrao == 0 && self.tiles.is_empty()
    }

    /// O menor retângulo com tudo o que está selecionado.
    pub fn limites(&self) -> Retangulo {
        if self.padrao > 0 {
            return Retangulo::inteiro(self.largura, self.altura);
        }
        self.tiles.keys().fold(Retangulo::default(), |a, p| {
            a.uniao(&retangulo_do_tile(*p, self.largura, self.altura))
        })
    }

    /// A caixa exata do que está selecionado (valor > 0) — a de
    /// [`Self::limites`] é a dos tiles, de 256 em 256.
    pub fn caixa_justa(&self) -> Retangulo {
        if self.padrao > 0 {
            return Retangulo::inteiro(self.largura, self.altura);
        }
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for (posicao, tile) in &self.tiles {
            let r = retangulo_do_tile(*posicao, self.largura, self.altura);
            for y in 0..r.altura {
                for x in 0..r.largura {
                    if tile[(y * LADO_DO_TILE + x) as usize] > 0 {
                        x0 = x0.min(r.x + x);
                        y0 = y0.min(r.y + y);
                        x1 = x1.max(r.x + x + 1);
                        y1 = y1.max(r.y + y + 1);
                    }
                }
            }
        }
        if x1 <= x0 {
            return Retangulo::default();
        }
        Retangulo::novo(x0, y0, x1 - x0, y1 - y0)
    }

    /// A seleção andada `(dx, dy)` pixels (o Mover leva a seleção junto). O
    /// que sai da foto se perde.
    pub fn deslocada(&self, dx: i64, dy: i64) -> Selecao {
        if self.padrao != 0 {
            // Tudo (ou invertida): o que entra pela borda continua selecionado
            // — andar não muda quase nada, e a seleção fica.
            return self.clone();
        }
        let mut nova = Selecao::vazia(self.largura, self.altura);
        let l = self.limites();
        let destino = Retangulo::novo(
            (l.x as i64 + dx).max(0) as u32,
            (l.y as i64 + dy).max(0) as u32,
            l.largura,
            l.altura,
        )
        .limitado(self.largura, self.altura);
        nova.pintar(&destino, |x, y| {
            let (ox, oy) = (x as i64 - dx, y as i64 - dy);
            if ox < 0 || oy < 0 {
                0
            } else {
                self.valor(ox as u32, oy as u32)
            }
        });
        nova.enxugar();
        nova
    }

    /// ⇧⌘I.
    pub fn inverter(&mut self) {
        self.padrao = 255 - self.padrao;
        for tile in self.tiles.values_mut() {
            for v in Arc::make_mut(tile).iter_mut() {
                *v = 255 - *v;
            }
        }
    }

    /// Junta `outra` a esta, conforme a operação.
    pub fn combinar(&mut self, outra: &Selecao, operacao: Operacao) {
        let juntar = |a: u8, b: u8| -> u8 {
            match operacao {
                Operacao::Nova => b,
                Operacao::Somar => a.max(b),
                Operacao::Subtrair => ((a as u32 * (255 - b as u32) + 127) / 255) as u8,
                Operacao::Intersecao => a.min(b),
            }
        };
        let posicoes: std::collections::BTreeSet<Posicao> = self
            .tiles
            .keys()
            .chain(outra.tiles.keys())
            .copied()
            .collect();
        let (padrao_a, padrao_b) = (self.padrao, outra.padrao);
        let mut tiles = BTreeMap::new();
        for p in posicoes {
            let a = self.tiles.get(&p);
            let b = outra.tiles.get(&p);
            let novo: Vec<u8> = (0..BYTES)
                .map(|k| juntar(a.map_or(padrao_a, |t| t[k]), b.map_or(padrao_b, |t| t[k])))
                .collect();
            tiles.insert(p, Arc::new(novo));
        }
        self.padrao = juntar(padrao_a, padrao_b);
        self.tiles = tiles;
        self.enxugar();
    }

    /// A borda da seleção dentro de `regiao`, amostrada a cada `passo`
    /// pixels: segmentos horizontais e verticais `(x0, y0, x1, y1)` em pixels
    /// da foto, já emendados. É o que a tela desenha como letreiro.
    ///
    /// Um bloco `passo × passo` conta como selecionado quando o pixel do meio
    /// dele passa da metade (≥ 128).
    pub fn bordas(&self, regiao: &Retangulo, passo: u32) -> Vec<(u32, u32, u32, u32)> {
        let passo = passo.max(1);
        let r = regiao.limitado(self.largura, self.altura);
        if r.vazio() {
            return Vec::new();
        }
        let (bx0, by0) = (r.x / passo, r.y / passo);
        let (bx1, by1) = (r.direita().div_ceil(passo), r.baixo().div_ceil(passo));
        let (colunas, linhas) = ((bx1 - bx0) as usize, (by1 - by0) as usize);
        let (mx, my) = (self.largura.div_ceil(passo), self.altura.div_ceil(passo));
        // Uma moldura de um bloco em volta: a borda da foto também é borda.
        let dentro = |bx: i64, by: i64| -> bool {
            if bx < 0 || by < 0 || bx >= mx as i64 || by >= my as i64 {
                return false;
            }
            let x = (bx as u32 * passo + passo / 2).min(self.largura - 1);
            let y = (by as u32 * passo + passo / 2).min(self.altura - 1);
            self.valor(x, y) >= 128
        };
        let mut grade = vec![false; (colunas + 2) * (linhas + 2)];
        for j in 0..linhas + 2 {
            for i in 0..colunas + 2 {
                grade[j * (colunas + 2) + i] =
                    dentro(bx0 as i64 + i as i64 - 1, by0 as i64 + j as i64 - 1);
            }
        }
        let em = |i: usize, j: usize| grade[j * (colunas + 2) + i];
        let mut segmentos = Vec::new();
        // Arestas horizontais: entre a linha j e a j+1 da grade.
        for j in 0..=linhas {
            let mut inicio: Option<usize> = None;
            for i in 1..=colunas + 1 {
                let borda = i <= colunas && em(i, j) != em(i, j + 1);
                match (borda, inicio) {
                    (true, None) => inicio = Some(i),
                    (false, Some(a)) => {
                        let y = (by0 as usize + j) as u32 * passo;
                        let xa = (bx0 as usize + a - 1) as u32 * passo;
                        let xb = (bx0 as usize + i - 1) as u32 * passo;
                        segmentos.push((
                            xa.min(self.largura),
                            y.min(self.altura),
                            xb.min(self.largura),
                            y.min(self.altura),
                        ));
                        inicio = None;
                    }
                    _ => {}
                }
            }
        }
        // Arestas verticais: entre a coluna i e a i+1.
        for i in 0..=colunas {
            let mut inicio: Option<usize> = None;
            for j in 1..=linhas + 1 {
                let borda = j <= linhas && em(i, j) != em(i + 1, j);
                match (borda, inicio) {
                    (true, None) => inicio = Some(j),
                    (false, Some(a)) => {
                        let x = (bx0 as usize + i) as u32 * passo;
                        let ya = (by0 as usize + a - 1) as u32 * passo;
                        let yb = (by0 as usize + j - 1) as u32 * passo;
                        segmentos.push((
                            x.min(self.largura),
                            ya.min(self.altura),
                            x.min(self.largura),
                            yb.min(self.altura),
                        ));
                        inicio = None;
                    }
                    _ => {}
                }
            }
        }
        segmentos
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn o_retangulo_seleciona_so_dentro() {
        let s = Selecao::da_forma(
            600,
            400,
            &Forma::Retangulo(Retangulo::novo(100, 50, 200, 100)),
        );
        assert_eq!(s.valor(100, 50), 255);
        assert_eq!(s.valor(299, 149), 255);
        assert_eq!(s.valor(300, 149), 0);
        assert_eq!(s.valor(99, 60), 0);
        assert_eq!(s.limites(), Retangulo::novo(0, 0, 512, 256));
        assert!(!s.nada());
    }

    #[test]
    fn a_elipse_tem_borda_macia_e_centro_cheio() {
        let s = Selecao::da_forma(
            400,
            400,
            &Forma::Elipse(Retangulo::novo(100, 100, 200, 100)),
        );
        assert_eq!(s.valor(200, 150), 255);
        assert_eq!(s.valor(105, 105), 0, "o canto da caixa fica de fora");
        let borda = s.valor(100, 150);
        assert!(borda > 0 && borda < 255, "um pixel de rampa: {borda}");
    }

    #[test]
    fn o_laco_segue_o_poligono() {
        // Um triângulo.
        let s = Selecao::da_forma(
            300,
            300,
            &Forma::Laco(vec![(10.0, 10.0), (210.0, 10.0), (10.0, 210.0)]),
        );
        assert_eq!(s.valor(20, 20), 255);
        assert_eq!(s.valor(150, 150), 0, "do outro lado da hipotenusa");
        assert_eq!(s.valor(100, 50), 255);
        assert!(Selecao::da_forma(300, 300, &Forma::Laco(vec![(1.0, 1.0), (5.0, 5.0)])).nada());
    }

    #[test]
    fn tudo_inverter_somar_e_subtrair() {
        let (l, a) = (600, 400);
        let mut s = Selecao::tudo(l, a);
        assert_eq!(s.valor(599, 399), 255);
        assert_eq!(s.limites(), Retangulo::inteiro(l, a));
        s.inverter();
        assert!(s.nada());

        let mut s = Selecao::da_forma(l, a, &Forma::Retangulo(Retangulo::novo(0, 0, 100, 100)));
        s.combinar(
            &Selecao::da_forma(l, a, &Forma::Retangulo(Retangulo::novo(400, 300, 50, 50))),
            Operacao::Somar,
        );
        assert_eq!(
            (s.valor(50, 50), s.valor(420, 320), s.valor(200, 200)),
            (255, 255, 0)
        );
        s.combinar(
            &Selecao::da_forma(l, a, &Forma::Retangulo(Retangulo::novo(0, 0, 50, 100))),
            Operacao::Subtrair,
        );
        assert_eq!((s.valor(20, 20), s.valor(70, 20)), (0, 255));
        s.inverter();
        assert_eq!(
            (s.valor(20, 20), s.valor(70, 20), s.valor(599, 0)),
            (255, 0, 255)
        );
    }

    #[test]
    fn a_selecao_anda() {
        let s = Selecao::da_forma(
            600,
            400,
            &Forma::Retangulo(Retangulo::novo(100, 50, 20, 20)),
        );
        let d = s.deslocada(300, 10);
        assert_eq!((d.valor(400, 60), d.valor(100, 50)), (255, 0));
        let fora = s.deslocada(-110, 0);
        assert_eq!(fora.valor(0, 60), 255);
        assert_eq!(fora.valor(10, 60), 0);
    }

    #[test]
    fn a_borda_de_um_retangulo_sao_quatro_segmentos() {
        let s = Selecao::da_forma(
            600,
            400,
            &Forma::Retangulo(Retangulo::novo(100, 50, 200, 100)),
        );
        let mut b = s.bordas(&Retangulo::inteiro(600, 400), 1);
        b.sort();
        assert_eq!(
            b,
            vec![
                (100, 50, 100, 150),
                (100, 50, 300, 50),
                (100, 150, 300, 150),
                (300, 50, 300, 150),
            ]
        );
        // Selecionar tudo: a borda é a da foto.
        assert_eq!(
            Selecao::tudo(600, 400)
                .bordas(&Retangulo::inteiro(600, 400), 4)
                .len(),
            4
        );
        // Amostrada de 10 em 10, cai na grade de 10.
        let grossa = s.bordas(&Retangulo::inteiro(600, 400), 10);
        assert!(grossa
            .iter()
            .all(|(x0, y0, x1, y1)| [x0, y0, x1, y1].iter().all(|v| *v % 10 == 0)));
    }

    /// Uma foto com dois quadrados vermelhos separados num fundo azul.
    fn dois_quadrados() -> RgbImage {
        RgbImage::from_fn(600, 300, |x, y| {
            let dentro = |x0: u32| (x0..x0 + 100).contains(&x) && (100..200).contains(&y);
            if dentro(50) || dentro(400) {
                image::Rgb([220, 20, 20])
            } else {
                image::Rgb([20, 40, 200])
            }
        })
    }

    #[test]
    fn a_varinha_pega_a_cor_continua_ou_toda() {
        let foto = dois_quadrados();
        let s = Selecao::por_cor(&foto, (100, 150), 32, true);
        assert_eq!(s.valor(60, 110), 255);
        assert_eq!(s.valor(450, 150), 0, "o outro quadrado não encosta");
        assert_eq!(s.valor(10, 10), 0);
        let todas = Selecao::por_cor(&foto, (100, 150), 32, false);
        assert_eq!(todas.valor(450, 150), 255, "não contígua pega os dois");
        // Tolerância 0 num pixel diferente por 1: fica de fora.
        let mut quase = foto.clone();
        quase.put_pixel(70, 150, image::Rgb([221, 20, 20]));
        assert_eq!(
            Selecao::por_cor(&quase, (100, 150), 0, true).valor(70, 150),
            0
        );
        assert_eq!(
            Selecao::por_cor(&quase, (100, 150), 1, true).valor(70, 150),
            255
        );
    }

    #[test]
    fn o_alfa_da_camada_e_a_mascara_viram_selecao() {
        let mut c = CamadaDePixels::nova(600, 300);
        c.tile_mut((0, 0))[3] = 255;
        c.tile_mut((0, 0))[7] = 100;
        let s = Selecao::do_alfa(&c);
        assert_eq!((s.valor(0, 0), s.valor(1, 0), s.valor(2, 0)), (255, 100, 0));
        assert_eq!(s.valor(500, 200), 0);

        let mut m = Mascara::nova(255, 600, 300);
        m.pixels.tile_mut((1, 0))[..4].copy_from_slice(&[0, 0, 0, 255]);
        let s = Selecao::da_mascara(&m);
        assert_eq!(s.valor(256, 0), 0, "o preto não é selecionado");
        assert_eq!(s.valor(10, 10), 255, "o fundo branco é");
    }

    // ------------------------------------------- estilo e modificadores

    fn ordenada(c: ((f32, f32), (f32, f32))) -> (f32, f32, f32, f32) {
        let ((ax, ay), (bx, by)) = c;
        (ax.min(bx), ay.min(by), ax.max(bx), ay.max(by))
    }

    #[test]
    fn o_arrasto_normal_com_shift_e_alt() {
        let n = Estilo::Normal;
        assert_eq!(
            ordenada(caixa_do_arrasto(
                (10.0, 10.0),
                (50.0, 30.0),
                n,
                false,
                false
            )),
            (10.0, 10.0, 50.0, 30.0)
        );
        // ⇧: quadrado pelo lado maior, para o lado do arrasto.
        assert_eq!(
            ordenada(caixa_do_arrasto((10.0, 10.0), (50.0, 30.0), n, true, false)),
            (10.0, 10.0, 50.0, 50.0)
        );
        assert_eq!(
            ordenada(caixa_do_arrasto(
                (100.0, 100.0),
                (60.0, 90.0),
                n,
                true,
                false
            )),
            (60.0, 60.0, 100.0, 100.0)
        );
        // ⌥: o começo é o centro.
        assert_eq!(
            ordenada(caixa_do_arrasto(
                (100.0, 100.0),
                (130.0, 110.0),
                n,
                false,
                true
            )),
            (70.0, 90.0, 130.0, 110.0)
        );
        // ⇧⌥: quadrado do centro.
        assert_eq!(
            ordenada(caixa_do_arrasto(
                (100.0, 100.0),
                (130.0, 110.0),
                n,
                true,
                true
            )),
            (70.0, 70.0, 130.0, 130.0)
        );
    }

    #[test]
    fn a_proporcao_fixa_prende_a_razao_e_ignora_o_shift() {
        let p = Estilo::Proporcao {
            largura: 16.0,
            altura: 9.0,
        };
        // Arrasto mais largo: a largura manda.
        let (x0, y0, x1, y1) =
            ordenada(caixa_do_arrasto((0.0, 0.0), (160.0, 10.0), p, false, false));
        assert_eq!((x0, y0, x1, y1), (0.0, 0.0, 160.0, 90.0));
        // Arrasto mais alto: a altura manda (a caixa cobre o ponteiro).
        let (_, _, x1, y1) = ordenada(caixa_do_arrasto((0.0, 0.0), (10.0, 90.0), p, false, false));
        assert_eq!((x1, y1), (160.0, 90.0));
        // Para cima e para a esquerda.
        assert_eq!(
            ordenada(caixa_do_arrasto(
                (200.0, 200.0),
                (40.0, 190.0),
                p,
                false,
                false
            )),
            (40.0, 110.0, 200.0, 200.0)
        );
        // ⇧ não muda nada: a razão já está presa.
        assert_eq!(
            caixa_do_arrasto((0.0, 0.0), (160.0, 10.0), p, true, false),
            caixa_do_arrasto((0.0, 0.0), (160.0, 10.0), p, false, false)
        );
        // ⌥: do centro, com a razão.
        assert_eq!(
            ordenada(caixa_do_arrasto(
                (100.0, 100.0),
                (116.0, 100.0),
                p,
                false,
                true
            )),
            (84.0, 91.0, 116.0, 109.0)
        );
        // 1:1 é o quadrado.
        let q = Estilo::Proporcao {
            largura: 1.0,
            altura: 1.0,
        };
        assert_eq!(
            medida_da_caixa(
                (0.0, 0.0),
                caixa_do_arrasto((0.0, 0.0), (33.0, 12.0), q, false, false).1
            ),
            (33, 33)
        );
    }

    #[test]
    fn o_tamanho_fixo_segue_o_ponteiro_e_o_alt_centraliza() {
        let t = Estilo::Tamanho {
            largura: 300,
            altura: 200,
        };
        // O canto de cima à esquerda vai onde o ponteiro está, e anda com ele.
        let (a, b) = caixa_do_arrasto((10.0, 10.0), (50.4, 70.6), t, false, false);
        assert_eq!((a, b), ((50.0, 71.0), (350.0, 271.0)));
        assert_eq!(medida_da_caixa(a, b), (300, 200));
        // ⇧ não muda; ⌥ centraliza no ponteiro.
        assert_eq!(
            caixa_do_arrasto((10.0, 10.0), (50.4, 70.6), t, true, false),
            (a, b)
        );
        let (a, b) = caixa_do_arrasto((10.0, 10.0), (500.0, 500.0), t, false, true);
        assert_eq!((a, b), ((350.0, 400.0), (650.0, 600.0)));
        // Trocar largura e altura.
        assert_eq!(
            t.trocado(),
            Estilo::Tamanho {
                largura: 200,
                altura: 300
            }
        );
    }

    // ------------------------------------------------------- acabamento

    #[test]
    fn a_elipse_sem_antisserrilhado_e_so_dentro_ou_fora() {
        let forma = Forma::Elipse(Retangulo::novo(100, 100, 201, 99));
        let seca = Selecao::da_forma_com(
            400,
            400,
            &forma,
            Acabamento {
                suavizar: false,
                difusao: 0,
            },
        );
        let lisa = Selecao::da_forma_com(400, 400, &forma, Acabamento::default());
        let meios = |s: &Selecao| {
            (90..310)
                .flat_map(|x| (90..210).map(move |y| (x, y)))
                .filter(|&(x, y)| (1..255).contains(&s.valor(x, y)))
                .count()
        };
        assert_eq!(meios(&seca), 0);
        assert!(meios(&lisa) > 100, "a lisa tem a rampa de um pixel");
        assert_eq!(seca.valor(200, 150), 255);
    }

    #[test]
    fn a_elipse_que_passa_da_foto_nao_achata() {
        // A caixa vai de −100 a 100: o centro da elipse fica no canto (0, 0).
        let forma = Forma::ElipseNaCaixa(-100.0, -100.0, 100.0, 100.0);
        let s = Selecao::da_forma_com(
            300,
            300,
            &forma,
            Acabamento {
                suavizar: false,
                difusao: 0,
            },
        );
        assert_eq!(s.valor(0, 0), 255);
        assert_eq!(s.valor(69, 69), 255, "dentro do círculo de raio 100");
        assert_eq!(s.valor(72, 72), 0);
        assert_eq!(s.valor(99, 0), 255);
        assert_eq!(s.valor(101, 0), 0);
    }

    #[test]
    fn o_laco_antisserrilhado_cobre_a_fracao_da_borda() {
        // Um triângulo com a hipotenusa na diagonal.
        let tri = Forma::Laco(vec![(10.0, 10.0), (110.0, 10.0), (10.0, 110.0)]);
        let seco = Selecao::da_forma_com(
            200,
            200,
            &tri,
            Acabamento {
                suavizar: false,
                difusao: 0,
            },
        );
        let liso = Selecao::da_forma_com(200, 200, &tri, Acabamento::default());
        // O pixel cortado ao meio pela diagonal (x + y + 1 = 120): metade.
        let meio = liso.valor(59, 60);
        assert!((100..=155).contains(&meio), "meio pixel coberto: {meio}");
        assert!([0, 255].contains(&seco.valor(59, 60)));
        assert_eq!(liso.valor(20, 20), 255);
        assert_eq!(liso.valor(100, 100), 0);
        // Borda reta em pixel inteiro: exata nas duas.
        assert_eq!((liso.valor(10, 50), liso.valor(9, 50)), (255, 0));
    }

    #[test]
    fn a_difusao_da_forma_e_zero_e_a_de_valor() {
        let r = Forma::Retangulo(Retangulo::novo(100, 100, 100, 100));
        let zero = Selecao::da_forma_com(
            400,
            400,
            &r,
            Acabamento {
                suavizar: true,
                difusao: 0,
            },
        );
        assert_eq!(
            zero,
            Selecao::da_forma(400, 400, &r),
            "difusão 0 é a borda exata"
        );
        let dez = Selecao::da_forma_com(
            400,
            400,
            &r,
            Acabamento {
                suavizar: true,
                difusao: 10,
            },
        );
        assert!((100..160).contains(&dez.valor(100, 150)));
        assert!(dez.valor(150, 150) > 250);
    }

    /// A difusão calculada só em volta do selecionado dá o mesmo que a da
    /// foto inteira (a conta de antes da otimização).
    #[test]
    fn a_difusao_no_recorte_e_igual_a_da_foto_inteira() {
        let s = Selecao::da_forma(
            700,
            500,
            &Forma::Laco(vec![(300.0, 200.0), (420.0, 230.0), (350.0, 330.0)]),
        );
        for raio in [1, 7, 30] {
            let rapida = s.difusa(raio);
            let (l, a) = (700usize, 500usize);
            let mut d: Vec<u16> = s.densa().iter().map(|v| *v as u16).collect();
            let r = (raio as usize).div_ceil(2).max(1);
            for _ in 0..3 {
                caixa(&mut d, l, a, r, true);
                caixa(&mut d, l, a, r, false);
            }
            let inteira: Vec<u8> = d.iter().map(|v| *v as u8).collect();
            assert_eq!(rapida.densa(), inteira, "raio {raio}");
        }
    }

    // ------------------------------------------------------------ varinha

    #[test]
    fn a_varinha_na_camada_ve_o_transparente_e_nao_a_base() {
        let mut c = CamadaDePixels::nova(300, 300);
        // Um quadrado vermelho opaco em (50..100), e um vermelho meio apagado
        // logo ao lado (100..150) — o resto é transparente, com lixo de cor.
        for y in 50..100 {
            for x in 0..300u32 {
                let i = crate::tiles::indice(x % LADO_DO_TILE, y % LADO_DO_TILE);
                let p = if (50..100).contains(&x) {
                    [200, 0, 0, 255]
                } else if (100..150).contains(&x) {
                    [200, 0, 0, 128]
                } else {
                    [13, 250, 77, 0]
                };
                c.tile_mut((x / LADO_DO_TILE, y / LADO_DO_TILE))[i..i + 4].copy_from_slice(&p);
            }
        }
        let amostra = Amostra::da_camada(&c, 300, 300);
        let vermelho = Selecao::por_cor_em(&amostra, (60, 60), 32, true, false);
        assert_eq!(vermelho.valor(99, 60), 255);
        assert_eq!(vermelho.valor(100, 60), 0, "o meio apagado tem outro alfa");
        // Clique no transparente: todo o transparente ligado, com o lixo de cor
        // e os tiles que nem existem.
        let vazio = Selecao::por_cor_em(&amostra, (5, 5), 0, true, false);
        assert_eq!(vazio.valor(299, 299), 255);
        assert_eq!(vazio.valor(10, 60), 255, "o pixel apagado com cor guardada");
        assert_eq!(vazio.valor(60, 60), 0);
        assert_eq!(vazio.valor(120, 60), 0);
    }

    #[test]
    fn a_varinha_suavizada_tem_meio_caminho_na_borda() {
        let foto = dois_quadrados();
        let s = Selecao::por_cor_em(&Amostra::da_imagem(&foto), (100, 150), 32, true, true);
        assert_eq!(s.valor(100, 150), 255);
        let (dentro, fora) = (s.valor(50, 150), s.valor(49, 150));
        assert!(dentro > 128 && dentro < 255, "{dentro}");
        assert!(fora > 0 && fora < 128, "{fora}");
        assert_eq!(s.valor(40, 150), 0);
    }

    // ------------------------------------------------- transformar seleção

    #[test]
    fn o_molde_anda_e_amplia_a_selecao() {
        let s = Selecao::da_forma(
            600,
            400,
            &Forma::Retangulo(Retangulo::novo(100, 100, 50, 40)),
        );
        let m = Molde::de(&s);
        assert_eq!(m.caixa(), Retangulo::novo(100, 100, 50, 40));
        // Identidade: a mesma seleção.
        let igual = m.transformado(
            &Retangulo::inteiro(600, 400),
            (0.0, 0.0),
            (1.0, 0.0),
            (0.0, 1.0),
        );
        assert_eq!(igual.densa(), s.densa());
        // Andar 30 para a direita: (u, v) = (x − 30, y).
        let andou = m.transformado(
            &Retangulo::inteiro(600, 400),
            (-30.0, 0.0),
            (1.0, 0.0),
            (0.0, 1.0),
        );
        assert_eq!(
            (
                andou.valor(130, 110),
                andou.valor(129, 110),
                andou.valor(179, 139)
            ),
            (255, 0, 255)
        );
        // O dobro em volta da origem: (u, v) = (x/2, y/2).
        let dobro = m.transformado(
            &Retangulo::inteiro(600, 400),
            (0.0, 0.0),
            (0.5, 0.0),
            (0.0, 0.5),
        );
        // A borda a meio caminho fica no dobro exato (a bilinear faz um pixel
        // de rampa em volta).
        assert!(dobro.valor(200, 250) >= 128 && dobro.valor(199, 250) < 128);
        assert!(dobro.valor(299, 250) >= 128 && dobro.valor(300, 250) < 128);
        assert!(dobro.valor(250, 279) >= 128 && dobro.valor(250, 280) < 128);
    }

    #[test]
    fn a_difusao_faz_rampa_e_expandir_contrair_mexem_na_borda() {
        let r = Selecao::da_forma(
            600,
            300,
            &Forma::Retangulo(Retangulo::novo(200, 100, 100, 100)),
        );
        let d = r.difusa(10);
        assert!(d.valor(250, 150) > 250, "o meio continua cheio");
        let borda = d.valor(200, 150);
        assert!((100..160).contains(&borda), "a borda fica a meio ({borda})");
        assert!(
            d.valor(193, 150) > 0 && d.valor(193, 150) < borda,
            "a rampa passa da borda"
        );
        assert_eq!(d.valor(150, 150), 0);

        let e = r.expandida(5);
        assert_eq!(e.valor(195, 150), 255);
        assert_eq!(e.valor(194, 150), 0);
        assert!(!e.limites().vazio());
        let c = r.expandida(-5);
        assert_eq!(c.valor(205, 150), 255);
        assert_eq!(c.valor(204, 150), 0);
        // Contrair o que encosta na borda da foto não recua dela.
        let tudo = Selecao::da_forma(600, 300, &Forma::Retangulo(Retangulo::novo(0, 0, 100, 300)));
        assert_eq!(tudo.expandida(-5).valor(0, 0), 255);
        assert_eq!(tudo.expandida(-5).valor(95, 0), 0);
        // Contrair até sumir: nada.
        assert!(r.expandida(-60).nada());
    }
}
