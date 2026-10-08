//! A difusão da máscara (Propriedades → Difusão), **sem tocar nos pixels dela**.
//!
//! O Photoshop desfoca a máscara na hora de compor: os pixels pintados ficam
//! como estão, e mudar a difusão de 20 para 0 devolve a borda dura. Aqui é o
//! mesmo — o valor de cada pixel na composição vem de um **mapa desfocado**,
//! guardado ao lado da máscara e refeito só onde ela mudou.
//!
//! ```text
//! valor (0–255, a máscara crua)  →  média em blocos k × k  →  3 caixas por eixo (≈ gaussiana de σ/k)
//!                                                         →  bilinear de volta a pixels da foto
//! ```
//!
//! 🔑 **O bloco `k` cresce com a difusão** (σ/2, no mínimo 1): o desfoque roda
//! numa grade pequena, e uma difusão de 200 px numa foto de 24 MP custa o mesmo
//! que uma de 4. A gaussiana é lisa; a bilinear de volta não deixa degrau.
//!
//! 🔑 **O mapa sabe de que tiles ele veio** (os `Arc` dos tiles da máscara no
//! momento da conta): na próxima composição, os tiles que trocaram de `Arc`
//! dizem onde refazer — sem ninguém precisar avisar. O pincel na máscara com
//! difusão refaz só a vizinhança do traço.

use std::sync::{Arc, Mutex};

use crate::retangulo::Retangulo;
use crate::tiles::{indice, retangulo_do_tile, CamadaDePixels, Posicao, Tile, LADO_DO_TILE};

/// O maior raio da difusão, em pixels da foto (o Photoshop vai a 1000; 250
/// cobre todo retoque de pele e de fundo, e a grade fica pequena).
pub const DIFUSAO_MAXIMA: f32 = 250.0;

/// O mapa desfocado de uma máscara.
#[derive(Clone, Debug)]
pub struct MapaDifuso {
    /// O lado do bloco, em pixels da foto.
    k: u32,
    largura: u32,
    altura: u32,
    /// A grade antes do desfoque: a média de cada bloco.
    media: Vec<u8>,
    /// A grade desfocada.
    valores: Vec<u8>,
    /// O raio de cada uma das três caixas, em blocos.
    raio: usize,
    fundo: u8,
    difusao: f32,
    /// Os tiles da máscara (dentro da foto) de quando a conta foi feita.
    assinatura: Vec<(Posicao, Tile)>,
}

impl MapaDifuso {
    /// Até onde, em pixels da foto, uma mudança num pixel da máscara chega
    /// no mapa: o bloco dele (e o vizinho, pela bilinear) e as três caixas.
    pub fn alcance(difusao: f32) -> u32 {
        let k = Self::lado_do_bloco(difusao);
        let r = Self::raio_da_caixa(difusao / k as f32) as u32;
        (3 * r + 2) * k + 2
    }

    fn lado_do_bloco(difusao: f32) -> u32 {
        ((difusao / 2.0).floor() as u32).max(1)
    }

    /// O raio da caixa que, passada três vezes, dá a gaussiana de `sigma`
    /// (variância de 3 caixas de largura `w`: `(w² − 1) / 4`).
    fn raio_da_caixa(sigma: f32) -> usize {
        let w = (4.0 * sigma * sigma + 1.0).sqrt();
        (((w - 1.0) / 2.0).round() as usize).max(1)
    }

    /// O mapa inteiro de `pixels` (valor de quem não foi pintado = `fundo`).
    pub fn novo(pixels: &CamadaDePixels, fundo: u8, difusao: f32) -> Self {
        let (largura, altura) = (pixels.largura(), pixels.altura());
        let k = Self::lado_do_bloco(difusao);
        let (lm, am) = (largura.div_ceil(k).max(1), altura.div_ceil(k).max(1));
        let mut mapa = Self {
            k,
            largura: lm,
            altura: am,
            media: vec![fundo; (lm * am) as usize],
            valores: vec![fundo; (lm * am) as usize],
            raio: Self::raio_da_caixa(difusao / k as f32),
            fundo,
            difusao,
            assinatura: assinatura(pixels),
        };
        mapa.refazer_medias(pixels, &Retangulo::inteiro(largura, altura));
        mapa.desfocar(&Retangulo::inteiro(lm, am));
        mapa
    }

    /// Serve para `pixels` com este fundo e esta difusão?
    fn do_mesmo_jeito(&self, fundo: u8, difusao: f32) -> bool {
        self.fundo == fundo && self.difusao == difusao
    }

    /// Os tiles que mudaram desde a conta, como retângulo da foto. `None` =
    /// nada mudou.
    fn o_que_mudou(&self, pixels: &CamadaDePixels) -> Option<Retangulo> {
        let (largura, altura) = (pixels.largura(), pixels.altura());
        let agora: Vec<(&Posicao, &Tile)> = pixels.existentes().collect();
        let mut sujo = Retangulo::default();
        let (mut i, mut j) = (0, 0);
        // As duas listas vêm ordenadas por posição (BTreeMap).
        while i < self.assinatura.len() || j < agora.len() {
            let a = self.assinatura.get(i);
            let b = agora.get(j);
            let posicao = match (a, b) {
                (Some((pa, ta)), Some((pb, tb))) if pa == *pb => {
                    i += 1;
                    j += 1;
                    if Arc::ptr_eq(ta, tb) {
                        continue;
                    }
                    *pa
                }
                (Some((pa, _)), Some((pb, _))) if pa < *pb => {
                    i += 1;
                    *pa
                }
                (Some((pa, _)), None) => {
                    i += 1;
                    *pa
                }
                (_, Some((pb, _))) => {
                    j += 1;
                    **pb
                }
                (None, None) => break,
            };
            sujo = sujo.uniao(&retangulo_do_tile(posicao, largura, altura));
        }
        (!sujo.vazio()).then_some(sujo)
    }

    /// Refaz o mapa onde `pixels` mudou desde a conta.
    fn atualizar(&mut self, pixels: &CamadaDePixels) {
        let Some(sujo) = self.o_que_mudou(pixels) else {
            return;
        };
        self.assinatura = assinatura(pixels);
        let k = self.k;
        let blocos = Retangulo::novo(
            sujo.x / k,
            sujo.y / k,
            sujo.direita().div_ceil(k) - sujo.x / k,
            sujo.baixo().div_ceil(k) - sujo.y / k,
        )
        .limitado(self.largura, self.altura);
        self.refazer_medias(pixels, &sujo);
        // O desfoque espalha a mudança por três raios de cada lado.
        let s = (3 * self.raio) as u32;
        let alcance = Retangulo::novo(
            blocos.x.saturating_sub(s),
            blocos.y.saturating_sub(s),
            blocos.largura + 2 * s,
            blocos.altura + 2 * s,
        )
        .limitado(self.largura, self.altura);
        self.desfocar(&alcance);
    }

    /// A média de cada bloco que toca `ret` (pixels da foto).
    fn refazer_medias(&mut self, pixels: &CamadaDePixels, ret: &Retangulo) {
        let (largura, altura) = (pixels.largura(), pixels.altura());
        let k = self.k;
        let (bx0, by0) = (ret.x / k, ret.y / k);
        let bx1 = ret.direita().div_ceil(k).min(self.largura);
        let by1 = ret.baixo().div_ceil(k).min(self.altura);
        if bx1 <= bx0 || by1 <= by0 {
            return;
        }
        let fundo = self.fundo;
        let lm = self.largura as usize;
        // Uma faixa de linhas de blocos por thread: na foto inteira com k = 1
        // são 24 milhões de leituras.
        let linhas: Vec<u32> = (by0..by1).collect();
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let por = linhas.len().div_ceil(threads).max(1);
        let media = &mut self.media;
        let pedacos: Vec<&mut [u8]> = media[by0 as usize * lm..by1 as usize * lm]
            .chunks_mut(lm)
            .collect();
        let mut grupos: Vec<Vec<(u32, &mut [u8])>> = Vec::new();
        for (n, (by, linha)) in linhas.iter().zip(pedacos).enumerate() {
            if n % por == 0 {
                grupos.push(Vec::new());
            }
            grupos.last_mut().unwrap().push((*by, linha));
        }
        let fazer = |grupo: Vec<(u32, &mut [u8])>| {
            for (by, linha) in grupo {
                if k == 1 {
                    // Bloco de um pixel: o valor direto, um tile por vez.
                    let ty = by / LADO_DO_TILE;
                    let mut x = bx0;
                    while x < bx1 {
                        let tx = x / LADO_DO_TILE;
                        let fim = ((tx + 1) * LADO_DO_TILE).min(bx1);
                        match pixels.tile((tx as i32, ty as i32)) {
                            None => linha[x as usize..fim as usize].fill(fundo),
                            Some(t) => {
                                for xx in x..fim {
                                    let i = indice(xx % LADO_DO_TILE, by % LADO_DO_TILE);
                                    linha[xx as usize] = crate::documento::Mascara::valor_do_pixel(
                                        fundo,
                                        [t[i], t[i + 1], t[i + 2], t[i + 3]],
                                    );
                                }
                            }
                        }
                        x = fim;
                    }
                    continue;
                }
                let (y0, y1) = (by * k, ((by + 1) * k).min(altura));
                for bx in bx0..bx1 {
                    let (x0, x1) = (bx * k, ((bx + 1) * k).min(largura));
                    linha[bx as usize] = media_do_bloco(pixels, fundo, x0, x1, y0, y1);
                }
            }
        };
        if grupos.len() <= 1 {
            grupos.into_iter().for_each(fazer);
        } else {
            std::thread::scope(|escopo| {
                for grupo in grupos {
                    let fazer = &fazer;
                    escopo.spawn(move || fazer(grupo));
                }
            });
        }
    }

    /// Desfoca `alcance` (em blocos) a partir das médias: três caixas em x,
    /// depois três em y, lendo três raios além de cada lado (o que estiver
    /// dentro da grade — na borda, o valor da borda se estende).
    ///
    /// 🔑 **Em inteiros, sem dividir entre as passadas** — a soma é exata, e o
    /// pedaço refeito dá o mesmo byte que a grade refeita inteira. Com o bloco
    /// de σ/2, o raio da caixa fica em até 4 (largura 9), e `255 · 9⁶` cabe em
    /// 32 bits. **Em faixas de linhas, uma por thread**: cada faixa lê três
    /// raios além dela, como o pedaço.
    fn desfocar(&mut self, alcance: &Retangulo) {
        let r = self.raio;
        debug_assert!(r <= 4, "a soma de 32 bits só cabe até o raio 4");
        let s = 3 * r as u32;
        let entrada = Retangulo::novo(
            alcance.x.saturating_sub(s),
            alcance.y.saturating_sub(s),
            alcance.largura + 2 * s,
            alcance.altura + 2 * s,
        )
        .limitado(self.largura, self.altura);
        let (w, h) = (entrada.largura as usize, entrada.altura as usize);
        if w == 0 || h == 0 || alcance.vazio() {
            return;
        }
        let lm = self.largura as usize;
        // As três caixas em x, linha a linha (as linhas não se tocam).
        let mut grade: Vec<u32> = vec![0; w * h];
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let por = h.div_ceil(threads).max(16);
        {
            let media = &self.media;
            let fazer = |(k, pedaco): (usize, &mut [u32])| {
                let mut aux = vec![0u32; w];
                for (j, linha) in pedaco.chunks_mut(w).enumerate() {
                    let y = entrada.y as usize + k * por + j;
                    let inicio = y * lm + entrada.x as usize;
                    for (d, v) in linha.iter_mut().zip(&media[inicio..inicio + w]) {
                        *d = *v as u32;
                    }
                    for _ in 0..3 {
                        caixa(linha, r, &mut aux);
                    }
                }
            };
            std::thread::scope(|escopo| {
                for parte in grade.chunks_mut(por * w).enumerate() {
                    let fazer = &fazer;
                    escopo.spawn(move || fazer(parte));
                }
            });
        }
        // As três em y, por faixas do alcance: cada uma lê `s` linhas além.
        let (y0, y1) = (alcance.y as usize, alcance.baixo() as usize);
        let (x0, x1) = (
            (alcance.x - entrada.x) as usize,
            (alcance.direita() - entrada.x) as usize,
        );
        let divisor = ((2 * r + 1) as u32).pow(6);
        let linhas_do_alcance = y1 - y0;
        let por = linhas_do_alcance.div_ceil(threads).max(16);
        let grade = &grade;
        let faixas: Vec<(usize, Vec<u8>)> = std::thread::scope(|escopo| {
            let tarefas: Vec<_> = (y0..y1)
                .step_by(por)
                .map(|a| {
                    escopo.spawn(move || {
                        let b = (a + por).min(y1);
                        // A janela em linhas da entrada, com a folga.
                        let ja = a.saturating_sub(s as usize).max(entrada.y as usize);
                        let jb = (b + s as usize).min(entrada.baixo() as usize);
                        let n = jb - ja;
                        let mut janela: Vec<u32> = grade
                            [(ja - entrada.y as usize) * w..(jb - entrada.y as usize) * w]
                            .to_vec();
                        let mut saida = vec![0u32; n * w];
                        for _ in 0..3 {
                            caixa_vertical(&janela, &mut saida, w, n, r);
                            std::mem::swap(&mut janela, &mut saida);
                        }
                        let mut bytes = Vec::with_capacity((b - a) * (x1 - x0));
                        for y in a..b {
                            let linha = &janela[(y - ja) * w..(y - ja + 1) * w];
                            bytes.extend(
                                linha[x0..x1]
                                    .iter()
                                    .map(|g| ((g + divisor / 2) / divisor).min(255) as u8),
                            );
                        }
                        (a, bytes)
                    })
                })
                .collect();
            tarefas.into_iter().filter_map(|t| t.join().ok()).collect()
        });
        let largura_do_alcance = x1 - x0;
        for (a, bytes) in faixas {
            for (k, linha) in bytes.chunks(largura_do_alcance).enumerate() {
                let inicio = (a + k) * lm + alcance.x as usize;
                self.valores[inicio..inicio + largura_do_alcance].copy_from_slice(linha);
            }
        }
    }

    /// O valor no pixel `(x, y)` da foto.
    #[inline]
    pub fn valor(&self, x: i64, y: i64) -> u8 {
        let k = self.k as f32;
        if self.k == 1 {
            let xc = x.clamp(0, self.largura as i64 - 1) as usize;
            let yc = y.clamp(0, self.altura as i64 - 1) as usize;
            return self.valores[yc * self.largura as usize + xc];
        }
        let u = (x as f32 + 0.5) / k - 0.5;
        let v = (y as f32 + 0.5) / k - 0.5;
        let (u0, v0) = (u.floor(), v.floor());
        let (fu, fv) = (u - u0, v - v0);
        let lm = self.largura as i64;
        let am = self.altura as i64;
        let ler = |a: i64, b: i64| {
            self.valores[(b.clamp(0, am - 1) * lm + a.clamp(0, lm - 1)) as usize] as f32
        };
        let (u0, v0) = (u0 as i64, v0 as i64);
        let topo = ler(u0, v0) * (1.0 - fu) + ler(u0 + 1, v0) * fu;
        let base = ler(u0, v0 + 1) * (1.0 - fu) + ler(u0 + 1, v0 + 1) * fu;
        (topo * (1.0 - fv) + base * fv).round() as u8
    }
}

/// Os tiles (dentro da foto) e os `Arc` deles — o que o mapa compara.
fn assinatura(pixels: &CamadaDePixels) -> Vec<(Posicao, Tile)> {
    pixels.existentes().map(|(p, t)| (*p, t.clone())).collect()
}

/// A média do valor da máscara em `[x0, x1) × [y0, y1)`.
fn media_do_bloco(pixels: &CamadaDePixels, fundo: u8, x0: u32, x1: u32, y0: u32, y1: u32) -> u8 {
    let lado = LADO_DO_TILE;
    let mut soma = 0u64;
    let mut y = y0;
    while y < y1 {
        let ty = y / lado;
        let yb = ((ty + 1) * lado).min(y1);
        let mut x = x0;
        while x < x1 {
            let tx = x / lado;
            let xb = ((tx + 1) * lado).min(x1);
            match pixels.tile((tx as i32, ty as i32)) {
                None => soma += fundo as u64 * ((xb - x) * (yb - y)) as u64,
                Some(t) => {
                    for yy in y..yb {
                        for xx in x..xb {
                            let i = indice(xx % lado, yy % lado);
                            soma += crate::documento::Mascara::valor_do_pixel(
                                fundo,
                                [t[i], t[i + 1], t[i + 2], t[i + 3]],
                            ) as u64;
                        }
                    }
                }
            }
            x = xb;
        }
        y = yb;
    }
    let n = ((x1 - x0) * (y1 - y0)).max(1) as u64;
    ((soma + n / 2) / n) as u8
}

/// Uma passada da caixa de raio `r` sobre `v` (a soma, sem dividir), com a
/// borda estendida.
fn caixa(v: &mut [u32], r: usize, aux: &mut [u32]) {
    let n = v.len();
    if n == 0 {
        return;
    }
    let ler = |i: isize| v[i.clamp(0, n as isize - 1) as usize];
    let mut soma: u32 = (-(r as isize)..=r as isize).map(ler).sum();
    for (i, a) in aux.iter_mut().enumerate().take(n) {
        *a = soma;
        let i = i as isize;
        soma = soma + ler(i + r as isize + 1) - ler(i - r as isize);
    }
    v.copy_from_slice(&aux[..n]);
}

/// A mesma caixa em y, sobre `n` linhas de `w` colunas, linha a linha (a soma
/// de cada coluna anda junto, sem pular na memória).
fn caixa_vertical(entrada: &[u32], saida: &mut [u32], w: usize, n: usize, r: usize) {
    let linha = |y: isize| {
        let y = y.clamp(0, n as isize - 1) as usize;
        &entrada[y * w..(y + 1) * w]
    };
    let mut soma = vec![0u32; w];
    for d in -(r as isize)..=r as isize {
        for (s, v) in soma.iter_mut().zip(linha(d)) {
            *s += *v;
        }
    }
    for y in 0..n {
        saida[y * w..(y + 1) * w].copy_from_slice(&soma);
        let (entra, sai) = (
            linha(y as isize + r as isize + 1),
            linha(y as isize - r as isize),
        );
        for ((s, e), t) in soma.iter_mut().zip(entra).zip(sai) {
            *s = *s + *e - *t;
        }
    }
}

/// O lugar do mapa ao lado da máscara: compartilhado pelas cópias dela (a
/// lupa compõe uma cópia do documento) e conferido a cada uso.
#[derive(Clone, Debug, Default)]
pub struct Guarda(Arc<Mutex<Option<Arc<MapaDifuso>>>>);

impl Guarda {
    /// O mapa de `pixels` como está agora, refeito onde mudou.
    pub fn mapa(&self, pixels: &CamadaDePixels, fundo: u8, difusao: f32) -> Arc<MapaDifuso> {
        let mut guardado = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match guardado.as_mut() {
            Some(mapa) if mapa.do_mesmo_jeito(fundo, difusao) => {
                if mapa.o_que_mudou(pixels).is_some() {
                    Arc::make_mut(mapa).atualizar(pixels);
                }
                mapa.clone()
            }
            _ => {
                let mapa = Arc::new(MapaDifuso::novo(pixels, fundo, difusao));
                *guardado = Some(mapa.clone());
                mapa
            }
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn com_quadrado(largura: u32, altura: u32) -> CamadaDePixels {
        let mut c = CamadaDePixels::nova(largura, altura);
        for y in 100..300u32 {
            for x in 100..300u32 {
                let t = c.tile_mut(((x / 256) as i32, (y / 256) as i32));
                let i = indice(x % 256, y % 256);
                t[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        c
    }

    #[test]
    fn a_difusao_amacia_a_borda_e_conserva_o_meio_e_o_longe() {
        let pixels = com_quadrado(600, 400);
        let mapa = MapaDifuso::novo(&pixels, 0, 10.0);
        assert_eq!(mapa.valor(200, 200), 255, "o meio continua branco");
        assert_eq!(mapa.valor(500, 50), 0, "longe continua preto");
        let borda = mapa.valor(100, 200);
        assert!(
            (100..=155).contains(&borda),
            "a borda fica no meio: {borda}"
        );
        // Anda devagar de dentro para fora.
        let perfil: Vec<u8> = (80..120).map(|x| mapa.valor(x, 200)).collect();
        assert!(perfil.windows(2).all(|w| w[1] >= w[0]), "{perfil:?}");
        assert!(perfil[0] > 0 && perfil[39] < 255);
    }

    #[test]
    fn o_mapa_refeito_onde_mudou_e_o_mesmo_que_o_refeito_inteiro() {
        for difusao in [3.0, 30.0] {
            let mut pixels = com_quadrado(900, 700);
            let guarda = Guarda::default();
            guarda.mapa(&pixels, 0, difusao);
            // Um traço longe do quadrado.
            for x in 600..640u32 {
                let t = pixels.tile_mut(((x / 256) as i32, 2));
                let i = indice(x % 256, 600 % 256);
                t[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
            let parcial = guarda.mapa(&pixels, 0, difusao);
            let inteiro = MapaDifuso::novo(&pixels, 0, difusao);
            assert_eq!(parcial.valores, inteiro.valores, "difusão {difusao}");
            // Apagar um tile também conta.
            pixels.definir((0, 0), None);
            let parcial = guarda.mapa(&pixels, 0, difusao);
            let inteiro = MapaDifuso::novo(&pixels, 0, difusao);
            assert_eq!(parcial.valores, inteiro.valores, "difusão {difusao}");
        }
    }

    #[test]
    fn a_mudanca_num_tile_nao_passa_do_alcance() {
        let (largura, altura) = (1400, 1000);
        for difusao in [1.0, 2.0, 3.5, 5.0, 9.0, 20.0, 80.0, 250.0] {
            let antes = CamadaDePixels::nova(largura, altura);
            let mut depois = antes.clone();
            // Um ponto de 12 × 12 dentro de um tile só (um pixel some na média
            // dos blocos grandes).
            let (px, py) = (700u32, 500u32);
            for y in py..py + 12 {
                for x in px..px + 12 {
                    let i = indice(x % 256, y % 256);
                    depois.tile_mut(((x / 256) as i32, (y / 256) as i32))[i..i + 4]
                        .copy_from_slice(&[255, 255, 255, 255]);
                }
            }
            let a = MapaDifuso::novo(&antes, 0, difusao);
            let b = MapaDifuso::novo(&depois, 0, difusao);
            let alcance = MapaDifuso::alcance(difusao) as i64;
            // O tile inteiro do pixel conta como mudado (é o que a vista suja).
            let (tx0, ty0) = ((px / 256 * 256) as i64, (py / 256 * 256) as i64);
            let mut mudou = false;
            for y in (0..altura as i64).step_by(3) {
                for x in (0..largura as i64).step_by(3) {
                    if a.valor(x, y) != b.valor(x, y) {
                        mudou = true;
                        let dx = (tx0 - x).max(x - (tx0 + 255)).max(0);
                        let dy = (ty0 - y).max(y - (ty0 + 255)).max(0);
                        assert!(
                            dx <= alcance && dy <= alcance,
                            "difusão {difusao}: ({x}, {y}) mudou a {dx}, {dy} do tile (alcance {alcance})"
                        );
                    }
                }
            }
            // Com 250 px, um ponto de 12 px se dilui abaixo de meio tom.
            assert!(
                mudou || difusao > 80.0,
                "difusão {difusao}: o ponto aparece no mapa"
            );
        }
    }

    #[test]
    fn trocar_a_difusao_ou_o_fundo_refaz_o_mapa() {
        let pixels = com_quadrado(600, 400);
        let guarda = Guarda::default();
        let a = guarda.mapa(&pixels, 0, 10.0);
        let b = guarda.mapa(&pixels, 0, 40.0);
        assert!(b.valor(90, 200) > a.valor(90, 200));
        let c = guarda.mapa(&pixels, 255, 40.0);
        assert_eq!(c.valor(500, 50), 255);
    }
}
