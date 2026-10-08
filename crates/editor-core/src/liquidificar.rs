//! Liquidificar — a "Deformação para a frente" do filtro Liquify do Photoshop:
//! um pincel que empurra os pixels na direção do arrasto.
//!
//! ## O modelo
//!
//! Um **campo de deslocamento para trás** `u`: o pixel `q` do resultado mostra
//! o pixel `q + u(q)` da camada **original**. O campo mora numa grade de
//! [`PASSO_DA_GRADE`] px (12 MB numa foto de 24 MP, contra 192 MB num campo por
//! pixel) e é lido por interpolação bilinear — liso por construção.
//!
//! Um passo do pincel em `c`, andando `d`, com peso `w(q)` (a queda do pincel):
//!
//! ```text
//! u'(q) = u(q − w(q)·d) − w(q)·d
//! ```
//!
//! ou seja, o ponto `q` passa a mostrar o que antes estava em `q − w·d`: o
//! conteúdo anda junto com o ponteiro, mais no centro do pincel e nada fora
//! dele. O arrasto é dividido em passos de no máximo ¼ do raio, para o
//! resultado não depender de quantos eventos de ponteiro chegaram.
//!
//! 🔑 **A camada é sempre refeita da original**, só onde o passo mexeu: o
//! resultado nunca é a reamostragem da reamostragem (sem borrar a cada
//! pincelada), e o alfa entra pré-multiplicado na bilinear (borda sem halo).
//! Com seleção, o peso é multiplicado por ela — o de fora fica congelado.

use crate::retangulo::Retangulo;
use crate::selecao::Selecao;
use crate::tiles::{indice, CamadaDePixels, LADO_DO_TILE};

/// O espaçamento da grade do campo, em pixels.
pub const PASSO_DA_GRADE: u32 = 4;

/// O campo de deslocamento e a camada de onde o resultado é refeito.
#[derive(Clone, Debug)]
pub struct Liquido {
    original: CamadaDePixels,
    /// Pontos da grade: `(colunas + 1) × (linhas + 1)`, cada um `(ux, uy)`.
    campo: Vec<[f32; 2]>,
    colunas: u32,
    linhas: u32,
    /// Onde o campo já não é zero (pixels da foto) — o que muda na camada.
    pub area: Retangulo,
}

impl Liquido {
    pub fn novo(original: CamadaDePixels) -> Self {
        let (l, a) = (original.largura(), original.altura());
        let colunas = l.div_ceil(PASSO_DA_GRADE);
        let linhas = a.div_ceil(PASSO_DA_GRADE);
        Self {
            original,
            campo: vec![[0.0; 2]; ((colunas + 1) * (linhas + 1)) as usize],
            colunas,
            linhas,
            area: Retangulo::default(),
        }
    }

    pub fn original(&self) -> &CamadaDePixels {
        &self.original
    }

    /// O campo está todo em zero (nada foi empurrado).
    pub fn parado(&self) -> bool {
        self.campo
            .iter()
            .all(|u| u[0].abs() < 1e-3 && u[1].abs() < 1e-3)
    }

    /// O deslocamento em `(x, y)` (pixels da foto), bilinear na grade.
    pub fn deslocamento(&self, x: f32, y: f32) -> [f32; 2] {
        let g = PASSO_DA_GRADE as f32;
        let (gx, gy) = (
            (x / g).clamp(0.0, self.colunas as f32),
            (y / g).clamp(0.0, self.linhas as f32),
        );
        let (i0, j0) = (gx.floor() as u32, gy.floor() as u32);
        let (i1, j1) = ((i0 + 1).min(self.colunas), (j0 + 1).min(self.linhas));
        let (fx, fy) = (gx - i0 as f32, gy - j0 as f32);
        let p = |i: u32, j: u32| self.campo[(j * (self.colunas + 1) + i) as usize];
        let (a, b, c, d) = (p(i0, j0), p(i1, j0), p(i0, j1), p(i1, j1));
        let mut u = [0.0; 2];
        for k in 0..2 {
            u[k] = a[k] * (1.0 - fx) * (1.0 - fy)
                + b[k] * fx * (1.0 - fy)
                + c[k] * (1.0 - fx) * fy
                + d[k] * fx * fy;
        }
        u
    }

    /// Empurra de `de` até `ate` com o pincel de `raio` e `forca` (0..=1;
    /// 1 leva o centro do pincel junto com o ponteiro inteiro). `dureza`
    /// (0..=1) é a parte do raio que empurra cheio. Devolve o retângulo da
    /// foto que mudou (já refeito em `camada`).
    #[allow(clippy::too_many_arguments)]
    pub fn empurrar(
        &mut self,
        camada: &mut CamadaDePixels,
        de: (f32, f32),
        ate: (f32, f32),
        raio: f32,
        forca: f32,
        dureza: f32,
        selecao: Option<&Selecao>,
    ) -> Retangulo {
        let raio = raio.max(1.0);
        let (dx, dy) = (ate.0 - de.0, ate.1 - de.1);
        let dist = dx.hypot(dy);
        if dist < 1e-3 || !dist.is_finite() {
            return Retangulo::default();
        }
        let passos = (dist / (raio / 4.0)).ceil().max(1.0) as u32;
        let mut sujo = Retangulo::default();
        for k in 0..passos {
            let t = (k as f32 + 0.5) / passos as f32;
            let c = (de.0 + dx * t, de.1 + dy * t);
            let d = (dx / passos as f32 * forca, dy / passos as f32 * forca);
            sujo = sujo.uniao(&self.um_passo(c, d, raio, dureza, selecao));
        }
        if !sujo.vazio() {
            self.redesenhar(camada, &sujo);
        }
        sujo
    }

    /// Um passo do pincel: o campo nos pontos da grade dentro do raio.
    fn um_passo(
        &mut self,
        c: (f32, f32),
        d: (f32, f32),
        raio: f32,
        dureza: f32,
        selecao: Option<&Selecao>,
    ) -> Retangulo {
        let g = PASSO_DA_GRADE as f32;
        let (l, a) = (self.original.largura(), self.original.altura());
        let i0 = ((c.0 - raio) / g).floor().max(0.0) as u32;
        let j0 = ((c.1 - raio) / g).floor().max(0.0) as u32;
        let i1 = (((c.0 + raio) / g).ceil().max(0.0) as u32).min(self.colunas);
        let j1 = (((c.1 + raio) / g).ceil().max(0.0) as u32).min(self.linhas);
        if i1 < i0 || j1 < j0 {
            return Retangulo::default();
        }
        let dureza = dureza.clamp(0.0, 0.99);
        // O campo novo lê o de antes (u'(q) = u(q − w·d) − w·d): calcular
        // tudo antes de escrever.
        let mut novos = Vec::new();
        for j in j0..=j1 {
            for i in i0..=i1 {
                let (x, y) = (i as f32 * g, j as f32 * g);
                let r = (x - c.0).hypot(y - c.1) / raio;
                if r >= 1.0 {
                    continue;
                }
                // Cheio até a dureza, e cai liso (cosseno) até a borda.
                let mut w = if r <= dureza {
                    1.0
                } else {
                    let t = (r - dureza) / (1.0 - dureza);
                    0.5 + 0.5 * (t * std::f32::consts::PI).cos()
                };
                if let Some(s) = selecao {
                    let (sx, sy) = ((x as u32).min(l - 1), (y as u32).min(a - 1));
                    w *= s.valor(sx, sy) as f32 / 255.0;
                }
                if w <= 0.0 {
                    continue;
                }
                let (px_, py_) = (x - w * d.0, y - w * d.1);
                let u = self.deslocamento(px_, py_);
                // `q + u'(q)` = (q − w·d) + u(q − w·d): a origem de onde o
                // ponto empurrado veio.
                novos.push((i, j, [u[0] - w * d.0, u[1] - w * d.1]));
            }
        }
        for (i, j, u) in novos {
            self.campo[(j * (self.colunas + 1) + i) as usize] = u;
        }
        // O que a mudança alcança: o raio, mais um passo da grade.
        let x0 = (c.0 - raio - g).floor().max(0.0) as u32;
        let y0 = (c.1 - raio - g).floor().max(0.0) as u32;
        let x1 = ((c.0 + raio + g).ceil().max(0.0) as u32).min(l);
        let y1 = ((c.1 + raio + g).ceil().max(0.0) as u32).min(a);
        let ret = Retangulo::novo(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0));
        self.area = self.area.uniao(&ret);
        ret
    }

    /// Refaz `ret` da camada a partir da original e do campo.
    pub fn redesenhar(&self, camada: &mut CamadaDePixels, ret: &Retangulo) {
        let ret = ret.limitado(camada.largura(), camada.altura());
        if ret.vazio() {
            return;
        }
        for posicao in camada.tiles_do_retangulo(&ret) {
            let (tx, ty) = (
                posicao.0 as u32 * LADO_DO_TILE,
                posicao.1 as u32 * LADO_DO_TILE,
            );
            let x0 = ret.x.max(tx);
            let y0 = ret.y.max(ty);
            let x1 = ret.direita().min(tx + LADO_DO_TILE);
            let y1 = ret.baixo().min(ty + LADO_DO_TILE);
            let tile = camada.tile_mut(posicao);
            for y in y0..y1 {
                for x in x0..x1 {
                    let (xc, yc) = (x as f32 + 0.5, y as f32 + 0.5);
                    let u = self.deslocamento(xc, yc);
                    let p = if u[0].abs() < 1e-4 && u[1].abs() < 1e-4 {
                        self.original.pixel(x, y)
                    } else {
                        amostra(&self.original, xc + u[0], yc + u[1])
                    };
                    let i = indice(x - tx, y - ty);
                    tile[i..i + 4].copy_from_slice(&p);
                }
            }
            camada.enxugar(posicao);
        }
    }
}

/// Bilinear com alfa pré-multiplicado em `(u, v)` (centro do pixel `(0, 0)`
/// em `(0,5, 0,5)`), de alfa reto na saída; fora da foto é transparente.
fn amostra(camada: &CamadaDePixels, u: f32, v: f32) -> [u8; 4] {
    let (u, v) = (u - 0.5, v - 0.5);
    let (x0, y0) = (u.floor(), v.floor());
    let (fx, fy) = (u - x0, v - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let (l, a) = (camada.largura() as i64, camada.altura() as i64);
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
        let (x, y) = (x0 + dx, y0 + dy);
        if x < 0 || y < 0 || x >= l || y >= a {
            continue;
        }
        let p = camada.pixel(x as u32, y as u32);
        let al = p[3] as f32 / 255.0;
        soma[0] += p[0] as f32 * al * peso;
        soma[1] += p[1] as f32 * al * peso;
        soma[2] += p[2] as f32 * al * peso;
        soma[3] += al * peso;
    }
    if soma[3] <= 1.0 / 512.0 {
        return [0; 4];
    }
    let q = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    [
        q(soma[0] / soma[3]),
        q(soma[1] / soma[3]),
        q(soma[2] / soma[3]),
        q(soma[3] * 255.0),
    ]
}
