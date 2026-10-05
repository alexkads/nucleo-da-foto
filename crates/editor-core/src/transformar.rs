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
use crate::tiles::{indice, CamadaDePixels, LADO_DO_TILE};

/// O conteúdo tirado da camada: RGBA de alfa reto, do tamanho da caixa.
#[derive(Clone, Debug, PartialEq)]
pub struct Conteudo {
    pub caixa: Retangulo,
    rgba: Vec<u8>,
}

impl Conteudo {
    /// O que a camada tem (vezes a seleção, se houver). `None` quando não
    /// sobra pixel nenhum.
    pub fn da_camada(camada: &CamadaDePixels, selecao: Option<&Selecao>) -> Option<Self> {
        // A caixa justa: só os pixels com alfa.
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
        let valor = |x: u32, y: u32| selecao.map_or(255, |s| s.valor(x, y));
        for (posicao, tile) in camada.existentes() {
            let (tx, ty) = (posicao.0 * LADO_DO_TILE, posicao.1 * LADO_DO_TILE);
            for ly in 0..LADO_DO_TILE {
                for lx in 0..LADO_DO_TILE {
                    let (x, y) = (tx + lx, ty + ly);
                    if x >= camada.largura() || y >= camada.altura() {
                        continue;
                    }
                    if tile[indice(lx, ly) + 3] > 0 && valor(x, y) > 0 {
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
        let caixa = Retangulo::novo(x0, y0, x1 - x0, y1 - y0);
        let mut rgba = Vec::with_capacity((caixa.largura * caixa.altura * 4) as usize);
        for y in y0..y1 {
            for x in x0..x1 {
                let mut p = camada.pixel(x, y);
                let m = valor(x, y) as u32;
                p[3] = ((p[3] as u32 * m + 127) / 255) as u8;
                rgba.extend_from_slice(&p);
            }
        }
        Some(Self { caixa, rgba })
    }

    /// O pixel `(x, y)` da caixa, **pré-multiplicado**, em `0..=1`; fora é
    /// transparente.
    fn premultiplicado(&self, x: i64, y: i64) -> [f32; 4] {
        if x < 0 || y < 0 || x >= self.caixa.largura as i64 || y >= self.caixa.altura as i64 {
            return [0.0; 4];
        }
        let k = ((y as u32 * self.caixa.largura + x as u32) * 4) as usize;
        let a = self.rgba[k + 3] as f32 / 255.0;
        [
            self.rgba[k] as f32 / 255.0 * a,
            self.rgba[k + 1] as f32 / 255.0 * a,
            self.rgba[k + 2] as f32 / 255.0 * a,
            a,
        ]
    }

    /// Lê bilinear no ponto `(u, v)` em pixels da caixa (o centro do pixel
    /// `(0, 0)` é `(0,5, 0,5)`). Devolve RGBA de alfa reto.
    fn amostra(&self, u: f32, v: f32) -> [u8; 4] {
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
    pub dx: f32,
    pub dy: f32,
    pub escala_x: f32,
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

    fn centro(caixa: &Retangulo) -> (f32, f32) {
        (
            caixa.x as f32 + caixa.largura as f32 / 2.0,
            caixa.y as f32 + caixa.altura as f32 / 2.0,
        )
    }

    /// Um ponto da caixa (pixels da foto) para onde ele vai.
    pub fn aplicar(&self, caixa: &Retangulo, x: f32, y: f32) -> (f32, f32) {
        let (cx, cy) = Self::centro(caixa);
        let (px, py) = ((x - cx) * self.escala_x, (y - cy) * self.escala_y);
        let (s, c) = self.angulo.sin_cos();
        (
            cx + self.dx + px * c - py * s,
            cy + self.dy + px * s + py * c,
        )
    }

    /// De onde, na caixa, vem o ponto `(x, y)` do destino.
    pub fn inversa(&self, caixa: &Retangulo, x: f32, y: f32) -> (f32, f32) {
        let (cx, cy) = Self::centro(caixa);
        let (px, py) = (x - cx - self.dx, y - cy - self.dy);
        let (s, c) = self.angulo.sin_cos();
        let (rx, ry) = (px * c + py * s, -px * s + py * c);
        (
            cx + rx / self.escala_x.max(1e-4),
            cy + ry / self.escala_y.max(1e-4),
        )
    }

    /// Os quatro cantos da caixa transformada (sentido do relógio a partir do
    /// de cima à esquerda) — o que a tela desenha com as alças.
    pub fn cantos(&self, caixa: &Retangulo) -> [(f32, f32); 4] {
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
pub fn desenhar(
    conteudo: &Conteudo,
    t: &Transformacao,
    largura: u32,
    altura: u32,
) -> CamadaDePixels {
    let mut saida = CamadaDePixels::nova(largura, altura);
    let caixa = conteudo.caixa;
    let cantos = t.cantos(&caixa);
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in cantos {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let x0 = (x0.floor() - 1.0).max(0.0) as u32;
    let y0 = (y0.floor() - 1.0).max(0.0) as u32;
    let x1 = ((x1.ceil() + 1.0).max(0.0) as u32).min(largura);
    let y1 = ((y1.ceil() + 1.0).max(0.0) as u32).min(altura);
    for y in y0..y1 {
        for x in x0..x1 {
            let (u, v) = t.inversa(&caixa, x as f32 + 0.5, y as f32 + 0.5);
            let p = conteudo.amostra(u - caixa.x as f32, v - caixa.y as f32);
            if p[3] == 0 {
                continue;
            }
            let posicao = (x / LADO_DO_TILE, y / LADO_DO_TILE);
            let i = indice(x % LADO_DO_TILE, y % LADO_DO_TILE);
            saida.tile_mut(posicao)[i..i + 4].copy_from_slice(&p);
        }
    }
    saida
}

/// `cima` sobre `fundo` (Normal), só onde `cima` tem tile.
pub fn sobre(fundo: &CamadaDePixels, cima: &CamadaDePixels) -> CamadaDePixels {
    let mut saida = fundo.clone();
    for (posicao, tile) in cima.existentes() {
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

#[cfg(test)]
mod testes {
    use super::*;
    use crate::selecao::Forma;

    fn camada_com_quadrado() -> CamadaDePixels {
        let mut c = CamadaDePixels::nova(600, 400);
        for y in 100..140 {
            for x in 200..260 {
                let i = indice(x % 256, y % 256);
                c.tile_mut((x / 256, y / 256))[i..i + 4].copy_from_slice(&[
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
        assert_eq!(tudo.caixa, Retangulo::novo(200, 100, 60, 40));
        let s = Selecao::da_forma(
            600,
            400,
            &Forma::Retangulo(Retangulo::novo(230, 0, 100, 400)),
        );
        let metade = Conteudo::da_camada(&c, Some(&s)).unwrap();
        assert_eq!(metade.caixa, Retangulo::novo(230, 100, 30, 40));
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
        let caixa = Retangulo::novo(200, 100, 60, 40);
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
    fn por_cima_do_fundo() {
        let mut fundo = CamadaDePixels::nova(300, 300);
        fundo.tile_mut((0, 0))[..4].copy_from_slice(&[10, 10, 10, 255]);
        let mut cima = CamadaDePixels::nova(300, 300);
        cima.tile_mut((0, 0))[4..8].copy_from_slice(&[200, 0, 0, 255]);
        let s = sobre(&fundo, &cima);
        assert_eq!(s.pixel(0, 0), [10, 10, 10, 255]);
        assert_eq!(s.pixel(1, 0), [200, 0, 0, 255]);
    }
}
