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
//! ímpar). Somar (⇧) é o máximo das duas, subtrair (⌥) é `a · (1 − b)`.
//!
//! A seleção **não entra no desfazer** — ela não muda pixel; o que se faz com
//! ela (apagar, preencher, pintar) entra.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::retangulo::Retangulo;
use crate::tiles::{retangulo_do_tile, Posicao, LADO_DO_TILE};

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
}
