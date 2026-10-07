//! Deformar (o *Warp* do Photoshop): o conteúdo tirado da camada pelo ⌘T
//! (`transformar::Conteudo`) é levado por uma **malha** em vez de uma
//! transformação afim.
//!
//! ## O modelo
//!
//! A grade de **3 × 3 células** é um único **retalho de Bézier bicúbico**
//! (produto tensorial), com **4 × 4 = 16 pontos de controle** — a grade padrão
//! do Warp do Photoshop:
//!
//! ```text
//! S(u, v) = Σᵢ Σⱼ Bᵢ(u) · Bⱼ(v) · P[j][i]        u, v ∈ [0, 1]
//! Bₖ(t)   = C(3, k) · tᵏ · (1 − t)³⁻ᵏ            (Bernstein de grau 3)
//! ```
//!
//! - `P[0][0]`, `P[0][3]`, `P[3][3]`, `P[3][0]` — os **cantos** (por onde a
//!   superfície passa);
//! - os outros oito da borda — as **alças** dos cantos (a tangente da borda);
//! - `P[1][1]`, `P[1][2]`, `P[2][1]`, `P[2][2]` — os **pontos internos**.
//!
//! Com os pontos igualmente espaçados na caixa, `S` é a identidade (o
//! polinômio de Bernstein reproduz o linear): sem mexer, nada muda. Por ser
//! **um** polinômio, a deformação é suave em toda a caixa — não há células
//! transformadas cada uma por si, nem emendas entre elas. As linhas da grade
//! que a tela desenha são `u = 0, ⅓, ⅔, 1` e `v = 0, ⅓, ⅔, 1` sobre a
//! superfície.
//!
//! ## O desenho
//!
//! Inverter `S` exatamente não tem fórmula. O desenho é **direto, sem buracos**:
//! o quadrado `(u, v)` é dividido em quadradinhos pequenos (≤ ~6 px na caixa),
//! cada um em dois triângulos; cada triângulo é levado por `S` nos vértices e
//! rasterizado no destino; dentro dele `(u, v)` é interpolado pelas
//! coordenadas baricêntricas (afim por triângulo — o erro é o da corda de um
//! arco de 6 px), e a cor vem do conteúdo **original** por amostragem bilinear
//! de alfa pré-multiplicado (a mesma do ⌘T: a borda difusa fica difusa, sem
//! halo). Triângulos vizinhos dividem as arestas e o teste de dentro tem uma
//! folga de ε: um centro de pixel na aresta é pintado pelos dois com o mesmo
//! valor, nunca por nenhum.
//!
//! **Dobras** (um ponto arrastado por cima de outro): os triângulos se
//! sobrepõem; vale o **último** desenhado (de cima para baixo, da esquerda para
//! a direita no retalho) — nunca se soma alfa. Triângulo degenerado (área ~0)
//! é pulado. Pontos não finitos são recusados ([`Malha::valida`]); o destino
//! é limitado a uma margem em volta da foto, como o ⌘T.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::tiles::{indice, CamadaDePixels, Posicao, BYTES_DO_TILE, LADO_DO_TILE};
use crate::transformar::{Caixa, Conteudo, Transformacao};

/// Quanto o desenho guarda além da borda da foto, em fração do maior lado
/// (a mesma regra do ⌘T).
const MARGEM_FORA_DA_FOTO: f32 = 1.0;

/// O lado máximo, em pixels da caixa, de um quadradinho da tesselação.
const LADO_DO_QUADRADINHO: f32 = 6.0;

/// O que o ponteiro pegou na malha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pega {
    /// Um dos 16 pontos: `(linha, coluna)`, de 0 a 3.
    Ponto(usize, usize),
}

/// Os 16 pontos de controle, em pixels **do documento** (`[linha][coluna]`,
/// linha 0 em cima, coluna 0 à esquerda).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Malha {
    pub pontos: [[(f32, f32); 4]; 4],
}

#[inline]
fn bernstein(t: f32) -> [f32; 4] {
    let s = 1.0 - t;
    [s * s * s, 3.0 * t * s * s, 3.0 * t * t * s, t * t * t]
}

impl Malha {
    /// A grade sem deformação sobre a caixa: os pontos a ⅓ e ⅔.
    pub fn da_caixa(caixa: &Caixa) -> Self {
        let (x, y) = (caixa.x as f32, caixa.y as f32);
        let (l, a) = (caixa.largura as f32, caixa.altura as f32);
        let mut pontos = [[(0.0, 0.0); 4]; 4];
        for (j, linha) in pontos.iter_mut().enumerate() {
            for (i, p) in linha.iter_mut().enumerate() {
                *p = (x + l * i as f32 / 3.0, y + a * j as f32 / 3.0);
            }
        }
        Self { pontos }
    }

    /// A grade levada pela transformação livre: uma transformação afim leva
    /// um retalho de Bézier exatamente aos pontos de controle transformados —
    /// passar do ⌘T ao Deformar não muda nenhum pixel.
    pub fn da_transformacao(caixa: &Caixa, t: &Transformacao) -> Self {
        let mut m = Self::da_caixa(caixa);
        for p in m.pontos.iter_mut().flatten() {
            *p = t.aplicar(caixa, p.0, p.1);
        }
        m
    }

    /// O ponto da superfície em `(u, v)`.
    pub fn ponto(&self, u: f32, v: f32) -> (f32, f32) {
        let (bu, bv) = (bernstein(u), bernstein(v));
        let mut s = (0.0, 0.0);
        for (j, linha) in self.pontos.iter().enumerate() {
            for (i, p) in linha.iter().enumerate() {
                let w = bu[i] * bv[j];
                s.0 += w * p.0;
                s.1 += w * p.1;
            }
        }
        s
    }

    /// Todos os pontos são números finitos e cabem num limite largo em volta
    /// da foto (um arrasto absurdo não aloca uma camada gigante).
    pub fn valida(&self, largura: u32, altura: u32) -> bool {
        let limite = 4.0 * largura.max(altura) as f32 + 1.0;
        self.pontos
            .iter()
            .flatten()
            .all(|p| p.0.is_finite() && p.1.is_finite() && p.0.abs() < limite && p.1.abs() < limite)
    }

    /// As duas malhas são a mesma, a menos de um milésimo de pixel.
    pub fn quase_igual(&self, outra: &Malha) -> bool {
        self.pontos
            .iter()
            .flatten()
            .zip(outra.pontos.iter().flatten())
            .all(|(a, b)| (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3)
    }

    /// Leva o ponto `(linha, coluna)` de `(dx, dy)`. 🔑 Um **canto** leva as
    /// duas alças dele e o ponto interno vizinho junto (o bloco 2 × 2 do
    /// canto): a curva perto do canto anda com ele, como no Photoshop, sem
    /// virar uma ponta.
    pub fn mover_ponto(&mut self, linha: usize, coluna: usize, dx: f32, dy: f32) {
        if linha > 3 || coluna > 3 {
            return;
        }
        let canto = (linha == 0 || linha == 3) && (coluna == 0 || coluna == 3);
        let mut mover = |j: usize, i: usize| {
            let p = &mut self.pontos[j][i];
            p.0 += dx;
            p.1 += dy;
        };
        if canto {
            let (jj, ii) = (if linha == 0 { 1 } else { 2 }, if coluna == 0 { 1 } else { 2 });
            mover(linha, coluna);
            mover(linha, ii);
            mover(jj, coluna);
            mover(jj, ii);
        } else {
            mover(linha, coluna);
        }
    }

    /// Arrastar **por dentro** da malha: o ponto da superfície em `(u, v)` vai
    /// de `(dx, dy)`, mexendo nos 16 pontos o mínimo possível (a solução de
    /// menor norma de `Σ wᵢⱼ·δᵢⱼ = d`, com `wᵢⱼ = Bᵢ(u)·Bⱼ(v)`: cada ponto anda
    /// `d · wᵢⱼ / Σw²`). Os pontos perto do agarrado andam mais, os longe quase
    /// nada — o "puxar a área" do Warp.
    pub fn puxar(&mut self, u: f32, v: f32, dx: f32, dy: f32) {
        let (u, v) = (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
        let (bu, bv) = (bernstein(u), bernstein(v));
        let mut soma = 0.0;
        for wv in bv {
            for wu in bu {
                soma += (wu * wv) * (wu * wv);
            }
        }
        if soma <= 1e-12 {
            return;
        }
        for (j, linha) in self.pontos.iter_mut().enumerate() {
            for (i, p) in linha.iter_mut().enumerate() {
                let k = bu[i] * bv[j] / soma;
                p.0 += dx * k;
                p.1 += dy * k;
            }
        }
    }

    /// O ponto de controle a até `raio` (pixels do documento) de `(x, y)` — o
    /// mais perto. Os cantos ganham empate (ficam por cima na tela).
    pub fn pegar(&self, x: f32, y: f32, raio: f32) -> Option<Pega> {
        let mut melhor: Option<(f32, Pega)> = None;
        for (j, linha) in self.pontos.iter().enumerate() {
            for (i, p) in linha.iter().enumerate() {
                let d = ((p.0 - x).powi(2) + (p.1 - y).powi(2)).sqrt();
                let canto = (j == 0 || j == 3) && (i == 0 || i == 3);
                let d = if canto { d - 0.01 } else { d };
                if d <= raio && melhor.is_none_or(|(m, _)| d < m) {
                    melhor = Some((d, Pega::Ponto(j, i)));
                }
            }
        }
        melhor.map(|(_, p)| p)
    }

    /// O `(u, v)` da superfície que cai em `(x, y)` — para o arrasto por
    /// dentro. Busca numa tesselação de 24 × 24 (a primeira camada da dobra
    /// ganha). `None` fora da malha.
    pub fn onde(&self, x: f32, y: f32) -> Option<(f32, f32)> {
        const N: usize = 24;
        let grade = self.grade(N, N);
        for j in 0..N {
            for i in 0..N {
                let (a, b, c, d) = (
                    grade[j * (N + 1) + i],
                    grade[j * (N + 1) + i + 1],
                    grade[(j + 1) * (N + 1) + i + 1],
                    grade[(j + 1) * (N + 1) + i],
                );
                for tri in [[a, b, c], [a, c, d]] {
                    if let Some(w) = baricentricas(tri.map(|v| v.0), (x, y)) {
                        if w.iter().all(|k| *k >= -1e-4) {
                            let u = w[0] * tri[0].1 .0 + w[1] * tri[1].1 .0 + w[2] * tri[2].1 .0;
                            let v = w[0] * tri[0].1 .1 + w[1] * tri[1].1 .1 + w[2] * tri[2].1 .1;
                            return Some((u, v));
                        }
                    }
                }
            }
        }
        None
    }

    /// As linhas da grade (as quatro em `u` constante e as quatro em `v`), cada
    /// uma com `passos + 1` pontos do documento — o que a tela desenha.
    pub fn linhas_da_grade(&self, passos: usize) -> Vec<Vec<(f32, f32)>> {
        let passos = passos.max(1);
        let mut linhas = Vec::with_capacity(8);
        for k in 0..4 {
            let t = k as f32 / 3.0;
            linhas.push(
                (0..=passos)
                    .map(|p| self.ponto(t, p as f32 / passos as f32))
                    .collect(),
            );
            linhas.push(
                (0..=passos)
                    .map(|p| self.ponto(p as f32 / passos as f32, t))
                    .collect(),
            );
        }
        linhas
    }

    /// A superfície amostrada numa grade `(nu + 1) × (nv + 1)`: cada vértice
    /// com o ponto do documento e o `(u, v)` dele.
    fn grade(&self, nu: usize, nv: usize) -> Vec<((f32, f32), (f32, f32))> {
        self.grade_com_folga(nu, nv, 0.0, 0.0)
    }

    /// A grade de `−fu` a `1 + fu` em `u` (e o mesmo em `v`).
    fn grade_com_folga(
        &self,
        nu: usize,
        nv: usize,
        fu: f32,
        fv: f32,
    ) -> Vec<((f32, f32), (f32, f32))> {
        let mut g = Vec::with_capacity((nu + 1) * (nv + 1));
        for j in 0..=nv {
            let v = -fv + (1.0 + 2.0 * fv) * j as f32 / nv as f32;
            for i in 0..=nu {
                let u = -fu + (1.0 + 2.0 * fu) * i as f32 / nu as f32;
                g.push((self.ponto(u, v), (u, v)));
            }
        }
        g
    }
}

/// As coordenadas baricêntricas de `p` no triângulo `t`; `None` se ele é
/// degenerado.
fn baricentricas(t: [(f32, f32); 3], p: (f32, f32)) -> Option<[f32; 3]> {
    let [(x0, y0), (x1, y1), (x2, y2)] = t;
    let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
    if area.abs() < 1e-6 {
        return None;
    }
    let w1 = ((p.0 - x0) * (y2 - y0) - (x2 - x0) * (p.1 - y0)) / area;
    let w2 = ((x1 - x0) * (p.1 - y0) - (p.0 - x0) * (y1 - y0)) / area;
    Some([1.0 - w1 - w2, w1, w2])
}

/// O conteúdo levado pela malha, numa camada transparente do tamanho da foto
/// (com o que sai dela guardado fora, até a margem). Sempre a partir do
/// conteúdo **original** da operação: a prévia de cada arrasto não reamostra
/// a anterior.
pub fn desenhar(conteudo: &Conteudo, malha: &Malha, largura: u32, altura: u32) -> CamadaDePixels {
    let mut saida = CamadaDePixels::nova(largura, altura);
    let caixa = conteudo.caixa;
    if caixa.vazia() || !malha.valida(largura, altura) {
        return saida;
    }
    let (lc, ac) = (caixa.largura as f32, caixa.altura as f32);
    // A tesselação: quadradinhos de até ~6 px na caixa (e na deformação
    // comum, perto disso no destino), no máximo 256 por lado.
    let comprimento = |pontos: &[(f32, f32)]| -> f32 {
        pontos
            .windows(2)
            .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
            .sum()
    };
    let mut maior_u = lc;
    let mut maior_v = ac;
    for k in 0..4 {
        let t = k as f32 / 3.0;
        let em_u: Vec<_> = (0..=16).map(|p| malha.ponto(p as f32 / 16.0, t)).collect();
        let em_v: Vec<_> = (0..=16).map(|p| malha.ponto(t, p as f32 / 16.0)).collect();
        maior_u = maior_u.max(comprimento(&em_u));
        maior_v = maior_v.max(comprimento(&em_v));
    }
    let nu = ((maior_u / LADO_DO_QUADRADINHO).ceil() as usize).clamp(3, 256);
    let nv = ((maior_v / LADO_DO_QUADRADINHO).ceil() as usize).clamp(3, 256);
    // Meio pixel de folga além da caixa (a superfície continua além de
    // `[0, 1]`): a borda do conteúdo esticado sai antisserrilhada pela
    // bilinear, como no ⌘T, e não cortada no triângulo.
    let grade = malha.grade_com_folga(nu, nv, 1.0 / lc, 1.0 / ac);

    let margem = (largura.max(altura) as f32 * MARGEM_FORA_DA_FOTO).ceil();
    let (lim_x0, lim_y0) = (-margem, -margem);
    let (lim_x1, lim_y1) = (largura as f32 + margem, altura as f32 + margem);

    // Os triângulos, na ordem do desenho, com a caixa de cada um no destino.
    struct Triangulo {
        xy: [(f32, f32); 3],
        uv: [(f32, f32); 3],
        y0: i64,
        y1: i64,
    }
    let mut triangulos = Vec::with_capacity(nu * nv * 2);
    for j in 0..nv {
        for i in 0..nu {
            let a = grade[j * (nu + 1) + i];
            let b = grade[j * (nu + 1) + i + 1];
            let c = grade[(j + 1) * (nu + 1) + i + 1];
            let d = grade[(j + 1) * (nu + 1) + i];
            for tri in [[a, b, c], [a, c, d]] {
                let xy = tri.map(|v| v.0);
                let area = (xy[1].0 - xy[0].0) * (xy[2].1 - xy[0].1)
                    - (xy[2].0 - xy[0].0) * (xy[1].1 - xy[0].1);
                if area.abs() < 1e-6 {
                    continue;
                }
                let ys = xy.map(|p| p.1);
                let (ymin, ymax) = (ys.iter().cloned().fold(f32::MAX, f32::min), ys.iter().cloned().fold(f32::MIN, f32::max));
                triangulos.push(Triangulo {
                    xy,
                    uv: tri.map(|v| v.1),
                    y0: (ymin.max(lim_y0) - 0.5).floor() as i64,
                    y1: (ymax.min(lim_y1) + 0.5).ceil() as i64,
                });
            }
        }
    }
    if triangulos.is_empty() {
        return saida;
    }
    let y_min = triangulos.iter().map(|t| t.y0).min().unwrap_or(0);
    let y_max = triangulos.iter().map(|t| t.y1).max().unwrap_or(0);

    let lado = LADO_DO_TILE as i64;
    let faixa = |l0: i64| -> Vec<(Posicao, Vec<u8>)> {
        let mut tiles: BTreeMap<Posicao, Vec<u8>> = BTreeMap::new();
        let (fa, fb) = (l0 * lado, (l0 + 1) * lado);
        for t in triangulos.iter().filter(|t| t.y1 >= fa && t.y0 < fb) {
            let [(x0, y0), (x1, y1), (x2, y2)] = t.xy;
            let area = (x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0);
            let eps = 1e-4;
            let xs = [x0, x1, x2];
            let xa = (xs.iter().cloned().fold(f32::MAX, f32::min).max(lim_x0) - 0.5).floor() as i64;
            let xb = (xs.iter().cloned().fold(f32::MIN, f32::max).min(lim_x1) + 0.5).ceil() as i64;
            for y in t.y0.max(fa)..t.y1.min(fb) {
                let py = y as f32 + 0.5;
                for x in xa..xb {
                    let px = x as f32 + 0.5;
                    let w1 = ((px - x0) * (y2 - y0) - (x2 - x0) * (py - y0)) / area;
                    let w2 = ((x1 - x0) * (py - y0) - (px - x0) * (y1 - y0)) / area;
                    let w0 = 1.0 - w1 - w2;
                    if w0 < -eps || w1 < -eps || w2 < -eps {
                        continue;
                    }
                    let u = w0 * t.uv[0].0 + w1 * t.uv[1].0 + w2 * t.uv[2].0;
                    let v = w0 * t.uv[0].1 + w1 * t.uv[1].1 + w2 * t.uv[2].1;
                    let p = conteudo.amostra(u * lc, v * ac);
                    let posicao = (x.div_euclid(lado) as i32, y.div_euclid(lado) as i32);
                    let i = indice(x.rem_euclid(lado) as u32, y.rem_euclid(lado) as u32);
                    if p[3] == 0 {
                        // A dobra: o de cima apaga o de baixo também onde é
                        // transparente.
                        if let Some(tile) = tiles.get_mut(&posicao) {
                            tile[i..i + 4].copy_from_slice(&p);
                        }
                        continue;
                    }
                    let tile = tiles
                        .entry(posicao)
                        .or_insert_with(|| vec![0; BYTES_DO_TILE]);
                    tile[i..i + 4].copy_from_slice(&p);
                }
            }
        }
        tiles.into_iter().collect()
    };
    let faixas: Vec<i64> = (y_min.div_euclid(lado)..=(y_max - 1).div_euclid(lado)).collect();
    let resultados: Vec<Vec<_>> = std::thread::scope(|escopo| {
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
    });
    for (posicao, tile) in resultados.into_iter().flatten() {
        if tile.iter().skip(3).step_by(4).any(|a| *a != 0) {
            saida.definir(posicao, Some(Arc::new(tile)));
        }
    }
    saida
}
