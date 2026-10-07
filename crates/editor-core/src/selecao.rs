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
//! elipse com um pixel de anti-aliasing, laço pelo centro de cada pixel (par e
//! ímpar). Somar (⇧) é o máximo das duas, subtrair (⌥) é `a · (1 − b)`, cruzar
//! (⇧⌥) é o mínimo.
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

const BYTES: usize = (LADO_DO_TILE * LADO_DO_TILE) as usize;

/// Uma forma desenhada, em pixels da foto.
#[derive(Clone, Debug, PartialEq)]
pub enum Forma {
    Retangulo(Retangulo),
    /// A elipse inscrita no retângulo.
    Elipse(Retangulo),
    /// Os vértices do laço, fechado do último ao primeiro.
    Laco(Vec<(f32, f32)>),
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

    /// A máscara de uma forma.
    pub fn da_forma(largura: u32, altura: u32, forma: &Forma) -> Self {
        let mut s = Self::vazia(largura, altura);
        match forma {
            Forma::Retangulo(r) => {
                let r = r.limitado(largura, altura);
                s.pintar(&r, |_, _| 255);
            }
            Forma::Elipse(r) => {
                let r = r.limitado(largura, altura);
                let (rx, ry) = (r.largura as f32 / 2.0, r.altura as f32 / 2.0);
                if rx < 0.5 || ry < 0.5 {
                    return s;
                }
                let (cx, cy) = (r.x as f32 + rx, r.y as f32 + ry);
                let menor = rx.min(ry);
                s.pintar(&r, |x, y| {
                    let (dx, dy) = ((x as f32 + 0.5 - cx) / rx, (y as f32 + 0.5 - cy) / ry);
                    let raio = (dx * dx + dy * dy).sqrt();
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
                // Por linha: os cruzamentos das arestas com o centro da linha,
                // ordenados; dentro é entre o 1º e o 2º, o 3º e o 4º…
                let mut linhas: BTreeMap<u32, Vec<f32>> = BTreeMap::new();
                for y in caixa.y..caixa.baixo() {
                    let yc = y as f32 + 0.5;
                    let mut cortes = Vec::new();
                    for i in 0..pontos.len() {
                        let (ax, ay) = pontos[i];
                        let (bx, by) = pontos[(i + 1) % pontos.len()];
                        if (ay <= yc) != (by <= yc) {
                            cortes.push(ax + (yc - ay) / (by - ay) * (bx - ax));
                        }
                    }
                    cortes.sort_by(f32::total_cmp);
                    linhas.insert(y, cortes);
                }
                s.pintar(&caixa, |x, y| {
                    let xc = x as f32 + 0.5;
                    let cortes = &linhas[&y];
                    let antes = cortes.iter().take_while(|c| **c <= xc).count();
                    if antes % 2 == 1 {
                        255
                    } else {
                        0
                    }
                });
            }
        }
        s.enxugar();
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

    /// A varinha mágica (W): os pixels de `foto` cuja cor difere da de `(x, y)`
    /// no máximo `tolerancia` em cada canal (32 no Photoshop). Contígua, só a
    /// área ligada ao ponto; senão, todos os parecidos da foto.
    pub fn por_cor(foto: &RgbImage, (x, y): (u32, u32), tolerancia: u8, contigua: bool) -> Self {
        let (largura, altura) = (foto.width(), foto.height());
        let mut s = Self::vazia(largura, altura);
        if x >= largura || y >= altura {
            return s;
        }
        let alvo = foto.get_pixel(x, y).0;
        let tol = tolerancia as i16;
        let (l, a) = (largura as usize, altura as usize);
        let bruto = foto.as_raw();
        let parecido =
            |i: usize| (0..3).all(|c| (bruto[i * 3 + c] as i16 - alvo[c] as i16).abs() <= tol);
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
        let densa: Vec<u8> = marcado.iter().map(|m| if *m { 255 } else { 0 }).collect();
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
        let (l, a) = (self.largura as usize, self.altura as usize);
        let mut d = vec![self.padrao; l * a];
        for (posicao, tile) in &self.tiles {
            let r = retangulo_do_tile(*posicao, self.largura, self.altura);
            for y in 0..r.altura {
                let de = (y * LADO_DO_TILE) as usize;
                let para = (r.y + y) as usize * l + r.x as usize;
                d[para..para + r.largura as usize]
                    .copy_from_slice(&tile[de..de + r.largura as usize]);
            }
        }
        d
    }

    /// Troca o conteúdo pelo de uma máscara densa (do tamanho da foto).
    fn carregar_densa(&mut self, d: &[u8]) {
        self.padrao = 0;
        self.tiles.clear();
        let colunas = self.largura.div_ceil(LADO_DO_TILE);
        let linhas = self.altura.div_ceil(LADO_DO_TILE);
        let l = self.largura as usize;
        for tl in 0..linhas {
            for tc in 0..colunas {
                let r = retangulo_do_tile((tc, tl), self.largura, self.altura);
                let mut tile = vec![0u8; BYTES];
                let mut algum = false;
                for y in 0..r.altura {
                    let de = (r.y + y) as usize * l + r.x as usize;
                    let linha = &d[de..de + r.largura as usize];
                    algum |= linha.iter().any(|v| *v != 0);
                    let para = (y * LADO_DO_TILE) as usize;
                    tile[para..para + r.largura as usize].copy_from_slice(linha);
                }
                if algum {
                    self.tiles.insert((tc, tl), Arc::new(tile));
                }
            }
        }
    }

    /// Difusão (⇧F6): a borda vira uma rampa de `raio` pixels — três passadas
    /// de caixa em cada direção, perto de um desfoque gaussiano.
    pub fn difusa(&self, raio: u32) -> Selecao {
        if raio == 0 || self.nada() {
            return self.clone();
        }
        let (l, a) = (self.largura as usize, self.altura as usize);
        let mut d: Vec<u16> = self.densa().iter().map(|v| *v as u16).collect();
        let r = (raio as usize).div_ceil(2).max(1);
        for _ in 0..3 {
            caixa(&mut d, l, a, r, true);
            caixa(&mut d, l, a, r, false);
        }
        let mut s = Selecao::vazia(self.largura, self.altura);
        s.carregar_densa(&d.iter().map(|v| *v as u8).collect::<Vec<_>>());
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
