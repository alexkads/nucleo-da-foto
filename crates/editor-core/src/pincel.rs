//! O pincel e a borracha.
//!
//! Um traço é uma sequência de carimbos circulares ao longo do caminho do
//! ponteiro, espaçados de ¼ do raio.
//!
//! 🚨 **Um traço não acumula consigo mesmo** — o mesmo contrato do pincel das
//! máscaras (`revelacao-core/src/locais.rs`). Dentro do traço, a cobertura de
//! cada pixel é o **máximo** dos carimbos que passaram por ele; o pixel é
//! recalculado do tile **de antes do traço** com essa cobertura. Passar duas vezes
//! no mesmo lugar, no mesmo gesto, não escurece nem apaga mais — e o resultado não
//! depende de quantos eventos de ponteiro chegaram. Traços diferentes compõem
//! entre si, como no Photoshop e no Lightroom.
//!
//! ```text
//! pincel    a = cobertura · opacidade
//!           alfa = a + alfa_antes · (1 − a)
//!           cor  = (cor · a + cor_antes · alfa_antes · (1 − a)) / alfa
//! borracha  alfa = alfa_antes · (1 − a)          (a cor fica)
//! ```
//!
//! A borracha só mexe na **camada**: a base não está aqui, e nunca muda (C28).

use std::collections::BTreeMap;

use std::sync::Arc;

use crate::carimbo::Fonte;
use crate::retangulo::Retangulo;
use crate::selecao::Selecao;
use crate::tiles::{indice, CamadaDePixels, Posicao, Tile, LADO_DO_TILE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ferramenta {
    Pincel,
    Borracha,
    /// Copia da [`Fonte`] em vez de pintar uma cor — o carimbo (S).
    Carimbo,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pincel {
    pub ferramenta: Ferramenta,
    /// Raio em pixels **da foto**.
    pub raio: f32,
    /// `0..=1`: a fração do raio que pinta cheio antes de a borda cair.
    pub dureza: f32,
    /// `0..=1`.
    pub opacidade: f32,
    pub cor: [u8; 3],
}

impl Default for Pincel {
    fn default() -> Self {
        Self {
            ferramenta: Ferramenta::Pincel,
            raio: 40.0,
            dureza: 0.8,
            opacidade: 1.0,
            cor: [0, 0, 0],
        }
    }
}

/// O menor e o maior raio, em pixels da foto.
pub const RAIO_MINIMO: f32 = 0.5;
pub const RAIO_MAXIMO: f32 = 2000.0;

impl Pincel {
    /// A cobertura de um carimbo à distância `d` do centro, `0..=1`.
    ///
    /// 🔑 **A regra do PaintFE** (`brush_render.rs`, `compute_brush_alpha`), que
    /// é a do Photoshop: a **dureza é a opacidade da borda** — `material = 1 +
    /// (dureza − 1)·smoothstep(d/raio)` —, e a **geometria** tem um pixel de
    /// anti-aliasing em volta do raio (`raio ± 0,5`). Com dureza 1 o carimbo é
    /// um disco cheio de borda lisa; sem o anti-aliasing seria serrilhado.
    pub fn queda(&self, d: f32) -> f32 {
        let raio = self.raio.max(RAIO_MINIMO);
        let dura = self.dureza.clamp(0.0, 1.0);
        let t = (d / raio).clamp(0.0, 1.0);
        let material = 1.0 + (dura - 1.0) * t * t * (3.0 - 2.0 * t);
        let (fora, dentro) = (raio + 0.5, raio - 0.5);
        let geometria = if d <= dentro {
            1.0
        } else if d >= fora {
            0.0
        } else {
            let x = ((d - fora) / (dentro - fora)).clamp(0.0, 1.0);
            x * x * (3.0 - 2.0 * x)
        };
        material * geometria
    }

    /// Até onde o carimbo alcança, com a borda do anti-aliasing.
    pub fn alcance(&self) -> f32 {
        self.raio.max(RAIO_MINIMO) + 0.5
    }
}

/// A queda do carimbo tabelada pela **distância ao quadrado** — a tabela do
/// PaintFE (`rebuild_brush_lut`): um carimbo de raio 200 são 125 mil pixels, e
/// a tabela troca uma raiz e um smoothstep por pixel por uma leitura.
struct Tabela {
    alcance: f32,
    valores: [u8; TAMANHO_DA_TABELA + 1],
}

const TAMANHO_DA_TABELA: usize = 1024;

impl Tabela {
    fn de(pincel: &Pincel) -> Self {
        let alcance = pincel.alcance();
        let mut valores = [0u8; TAMANHO_DA_TABELA + 1];
        for (i, v) in valores.iter_mut().enumerate() {
            let d = (i as f32 / TAMANHO_DA_TABELA as f32).sqrt() * alcance;
            *v = (pincel.queda(d) * 255.0).round() as u8;
        }
        Self { alcance, valores }
    }

    fn em(&self, d2: f32) -> u8 {
        let t = d2 / (self.alcance * self.alcance);
        if t >= 1.0 {
            return 0;
        }
        self.valores[(t * TAMANHO_DA_TABELA as f32).round() as usize]
    }
}

/// O que o traço mudou: o tile de antes e o de depois, por posição — é o passo
/// do desfazer.
#[derive(Clone, Debug, PartialEq)]
pub struct Mudanca {
    pub antes: Vec<(Posicao, Option<Tile>)>,
    pub depois: Vec<(Posicao, Option<Tile>)>,
}

/// Um traço em curso.
pub struct Traco {
    pincel: Pincel,
    tabela: Tabela,
    /// O tile de cada posição tocada **antes** do traço (`None` = não existia).
    antes: BTreeMap<Posicao, Option<Tile>>,
    /// A cobertura do traço por pixel (0..=255), por tile tocado.
    cobertura: BTreeMap<Posicao, Vec<u8>>,
    ultimo: Option<(f32, f32)>,
    /// Quanto do caminho sobrou desde o último carimbo.
    resto: f32,
    /// Com seleção, a cobertura de cada pixel é multiplicada pela máscara: o
    /// pincel e a borracha não passam da borda dela.
    selecao: Option<Arc<Selecao>>,
    /// De onde o carimbo copia (`Ferramenta::Carimbo`).
    fonte: Option<Fonte>,
}

impl Traco {
    pub fn novo(pincel: Pincel) -> Self {
        Self {
            tabela: Tabela::de(&pincel),
            pincel,
            antes: BTreeMap::new(),
            cobertura: BTreeMap::new(),
            ultimo: None,
            resto: 0.0,
            selecao: None,
            fonte: None,
        }
    }

    /// O carimbo copia daqui.
    pub fn copiando_de(mut self, fonte: Fonte) -> Self {
        self.fonte = Some(fonte);
        self
    }

    /// O traço fica dentro da seleção.
    pub fn dentro_de(mut self, selecao: Option<Arc<Selecao>>) -> Self {
        self.selecao = selecao;
        self
    }

    pub fn pincel(&self) -> &Pincel {
        &self.pincel
    }

    /// O ponteiro chegou em `(x, y)` (pixels da foto). Devolve a região suja.
    pub fn ate(&mut self, camada: &mut CamadaDePixels, x: f32, y: f32) -> Retangulo {
        let passo = (self.pincel.raio * 0.25).max(0.5);
        let Some((x0, y0)) = self.ultimo else {
            self.ultimo = Some((x, y));
            self.resto = 0.0;
            return self.carimbar(camada, x, y);
        };
        let (dx, dy) = (x - x0, y - y0);
        let distancia = (dx * dx + dy * dy).sqrt();
        let mut sujo = Retangulo::default();
        let mut andado = passo - self.resto;
        while andado <= distancia {
            let t = andado / distancia;
            sujo = sujo.uniao(&self.carimbar(camada, x0 + dx * t, y0 + dy * t));
            andado += passo;
        }
        self.resto = distancia - (andado - passo);
        self.ultimo = Some((x, y));
        sujo
    }

    fn carimbar(&mut self, camada: &mut CamadaDePixels, cx: f32, cy: f32) -> Retangulo {
        let raio = self.tabela.alcance;
        let (largura, altura) = (camada.largura(), camada.altura());
        let x0 = (cx - raio).floor().max(0.0) as u32;
        let y0 = (cy - raio).floor().max(0.0) as u32;
        let x1 = ((cx + raio).ceil().max(0.0) as u32).min(largura);
        let y1 = ((cy + raio).ceil().max(0.0) as u32).min(altura);
        if x1 <= x0 || y1 <= y0 {
            return Retangulo::default();
        }
        let area = Retangulo::novo(x0, y0, x1 - x0, y1 - y0);
        for posicao in camada.tiles_do_retangulo(&area) {
            self.antes
                .entry(posicao)
                .or_insert_with(|| camada.tile(posicao).cloned());
            let antes = self.antes.get(&posicao).cloned().flatten();
            let cobertura = self
                .cobertura
                .entry(posicao)
                .or_insert_with(|| vec![0; (LADO_DO_TILE * LADO_DO_TILE) as usize]);
            let mascara = self.selecao.as_ref().map(|s| s.do_tile(posicao));
            if let Some(Err(0)) = mascara {
                // O tile inteiro está fora da seleção.
                continue;
            }
            let tile = camada.tile_mut(posicao);
            let (tx0, ty0) = (posicao.0 * LADO_DO_TILE, posicao.1 * LADO_DO_TILE);
            let (px0, px1) = (x0.max(tx0), x1.min(tx0 + LADO_DO_TILE));
            let (py0, py1) = (y0.max(ty0), y1.min(ty0 + LADO_DO_TILE));
            for py in py0..py1 {
                for px in px0..px1 {
                    let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
                    let mut c = self.tabela.em(dx * dx + dy * dy);
                    if c == 0 {
                        continue;
                    }
                    let (lx, ly) = (px - tx0, py - ty0);
                    let k = (ly * LADO_DO_TILE + lx) as usize;
                    match mascara {
                        None | Some(Err(255)) => {}
                        Some(Err(m)) => c = ((c as u32 * m as u32 + 127) / 255) as u8,
                        Some(Ok(m)) => c = ((c as u32 * m[k] as u32 + 127) / 255) as u8,
                    }
                    if c == 0 {
                        continue;
                    }
                    if c <= cobertura[k] {
                        continue;
                    }
                    cobertura[k] = c;
                    let i = indice(lx, ly);
                    let de_antes = match &antes {
                        Some(t) => [t[i], t[i + 1], t[i + 2], t[i + 3]],
                        None => [0; 4],
                    };
                    let novo = match (self.pincel.ferramenta, self.fonte.as_mut()) {
                        (Ferramenta::Carimbo, Some(fonte)) => match fonte.cor(px, py) {
                            Some(cor) => aplicar(&Pincel { cor, ..self.pincel }, de_antes, c),
                            // A origem caiu fora da foto: este pixel fica.
                            None => continue,
                        },
                        _ => aplicar(&self.pincel, de_antes, c),
                    };
                    tile[i..i + 4].copy_from_slice(&novo);
                }
            }
        }
        area
    }

    /// Fecha o traço. `None` quando ele não mudou nada (clique fora da foto).
    pub fn terminar(self, camada: &mut CamadaDePixels) -> Option<Mudanca> {
        let mut antes = Vec::new();
        let mut depois = Vec::new();
        for (posicao, velho) in self.antes {
            camada.enxugar(posicao);
            let novo = camada.tile(posicao).cloned();
            let igual = match (&velho, &novo) {
                (None, None) => true,
                (Some(a), Some(b)) => a == b,
                _ => false,
            };
            if !igual {
                antes.push((posicao, velho));
                depois.push((posicao, novo));
            }
        }
        (!antes.is_empty()).then_some(Mudanca { antes, depois })
    }
}

/// Um pixel da camada depois do traço, a partir do pixel de antes e da
/// cobertura do traço nele.
pub fn aplicar(pincel: &Pincel, antes: [u8; 4], cobertura: u8) -> [u8; 4] {
    let a = cobertura as f32 / 255.0 * pincel.opacidade.clamp(0.0, 1.0);
    let alfa_antes = antes[3] as f32 / 255.0;
    match pincel.ferramenta {
        Ferramenta::Borracha => {
            let alfa = alfa_antes * (1.0 - a);
            [antes[0], antes[1], antes[2], quantizar(alfa)]
        }
        // O carimbo pinta como o pincel, com a cor que a fonte deu ao pixel.
        Ferramenta::Pincel | Ferramenta::Carimbo => {
            let alfa = a + alfa_antes * (1.0 - a);
            if alfa <= 0.0 {
                return [0; 4];
            }
            let canal = |cor: u8, velho: u8| -> u8 {
                let v = (cor as f32 * a + velho as f32 * alfa_antes * (1.0 - a)) / alfa;
                v.round().clamp(0.0, 255.0) as u8
            };
            [
                canal(pincel.cor[0], antes[0]),
                canal(pincel.cor[1], antes[1]),
                canal(pincel.cor[2], antes[2]),
                quantizar(alfa),
            ]
        }
    }
}

fn quantizar(alfa: f32) -> u8 {
    (alfa * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod testes {
    use super::*;

    fn pincel(ferramenta: Ferramenta) -> Pincel {
        Pincel {
            ferramenta,
            raio: 10.0,
            dureza: 1.0,
            opacidade: 1.0,
            cor: [200, 10, 20],
        }
    }

    #[test]
    fn o_pincel_pinta_dentro_do_raio_e_nada_fora() {
        let mut camada = CamadaDePixels::nova(400, 300);
        let mut traco = Traco::novo(pincel(Ferramenta::Pincel));
        let sujo = traco.ate(&mut camada, 100.0, 100.0);
        traco.terminar(&mut camada).expect("mudou");
        assert_eq!(camada.pixel(100, 100), [200, 10, 20, 255]);
        assert_eq!(camada.pixel(108, 100), [200, 10, 20, 255]);
        assert_eq!(camada.pixel(111, 100)[3], 0, "fora do raio");
        assert_eq!(camada.pixel(100, 111)[3], 0);
        let borda = camada.pixel(106, 107)[3];
        assert!(
            borda > 0 && borda < 255,
            "a borda tem anti-aliasing: {borda}"
        );
        assert!(sujo.x <= 90 && sujo.direita() >= 110);
        for y in 0..300 {
            for x in 0..400 {
                let (dx, dy) = (x as f32 + 0.5 - 100.0, y as f32 + 0.5 - 100.0);
                if (dx * dx + dy * dy).sqrt() >= 10.5 {
                    assert_eq!(camada.pixel(x, y)[3], 0, "({x},{y}) fora do carimbo");
                }
            }
        }
    }

    #[test]
    fn um_traco_nao_acumula_consigo_mesmo() {
        let mut p = pincel(Ferramenta::Pincel);
        p.opacidade = 0.5;
        let mut camada = CamadaDePixels::nova(300, 300);
        let mut traco = Traco::novo(p);
        // Vai e volta três vezes sobre o mesmo lugar.
        for _ in 0..3 {
            traco.ate(&mut camada, 50.0, 50.0);
            traco.ate(&mut camada, 150.0, 50.0);
        }
        traco.terminar(&mut camada);
        assert_eq!(camada.pixel(100, 50)[3], 128, "meia opacidade, uma vez só");

        // Um segundo traço, sim, compõe por cima.
        let mut segundo = Traco::novo(p);
        segundo.ate(&mut camada, 100.0, 50.0);
        segundo.terminar(&mut camada);
        assert_eq!(camada.pixel(100, 50)[3], 192);
    }

    #[test]
    fn o_traco_cruza_a_fronteira_dos_tiles() {
        let mut camada = CamadaDePixels::nova(600, 600);
        let mut traco = Traco::novo(pincel(Ferramenta::Pincel));
        traco.ate(&mut camada, 250.0, 250.0);
        traco.ate(&mut camada, 262.0, 262.0);
        let mudanca = traco.terminar(&mut camada).unwrap();
        assert_eq!(
            mudanca.antes.len(),
            4,
            "os quatro tiles em volta de (256,256)"
        );
        assert!(mudanca.antes.iter().all(|(_, t)| t.is_none()));
        assert_eq!(camada.pixel(256, 256)[3], 255);
    }

    #[test]
    fn a_borracha_apaga_a_camada() {
        let mut camada = CamadaDePixels::nova(300, 300);
        let mut traco = Traco::novo(pincel(Ferramenta::Pincel));
        traco.ate(&mut camada, 100.0, 100.0);
        traco.terminar(&mut camada);
        let mut borracha = Traco::novo(Pincel {
            raio: 12.0,
            ..pincel(Ferramenta::Borracha)
        });
        borracha.ate(&mut camada, 100.0, 100.0);
        let mudanca = borracha.terminar(&mut camada).unwrap();
        assert!(camada.vazia());
        assert_eq!(camada.quantos(), 0, "o tile transparente sai");
        assert!(mudanca.depois.iter().all(|(_, t)| t.is_none()));
    }

    #[test]
    fn o_carimbo_fora_da_foto_nao_muda_nada() {
        let mut camada = CamadaDePixels::nova(100, 100);
        let mut traco = Traco::novo(pincel(Ferramenta::Pincel));
        traco.ate(&mut camada, -50.0, -50.0);
        assert!(traco.terminar(&mut camada).is_none());
    }

    #[test]
    fn a_dureza_e_a_opacidade_da_borda() {
        let mut p = pincel(Ferramenta::Pincel);
        p.dureza = 0.0;
        assert_eq!(p.queda(0.0), 1.0);
        assert!(p.queda(5.0) > 0.0 && p.queda(5.0) < 1.0);
        assert!(p.queda(9.4) < 0.05, "macio: some perto da borda");
        assert_eq!(p.queda(10.5), 0.0);
        p.dureza = 1.0;
        assert_eq!(p.queda(9.4), 1.0, "duro: cheio até a borda");
        assert!(
            p.queda(10.0) > 0.4 && p.queda(10.0) < 0.6,
            "o pixel da borda"
        );
    }

    #[test]
    fn a_tabela_segue_a_conta() {
        for dureza in [0.0, 0.3, 1.0] {
            let p = Pincel {
                dureza,
                raio: 37.0,
                ..pincel(Ferramenta::Pincel)
            };
            let tabela = Tabela::de(&p);
            for d in [0.0f32, 5.0, 20.0, 36.0, 37.0, 37.4] {
                let exata = (p.queda(d) * 255.0).round() as i32;
                assert!((tabela.em(d * d) as i32 - exata).abs() <= 3, "d = {d}");
            }
        }
    }

    #[test]
    fn o_carimbo_copia_a_foto_deslocada() {
        use crate::documento::{BaseRef, Documento};
        let base = std::sync::Arc::new(image::RgbImage::from_fn(600, 400, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 200) as u8, 40])
        }));
        let doc = Documento::novo(BaseRef::da_imagem(&base));
        let mut camada = CamadaDePixels::nova(600, 400);
        let p = Pincel {
            ferramenta: Ferramenta::Carimbo,
            raio: 20.0,
            dureza: 1.0,
            opacidade: 1.0,
            cor: [0, 0, 0],
        };
        let fonte = Fonte::nova(base.clone(), &doc, 0, (100.0, 50.0));
        let mut traco = Traco::novo(p).copiando_de(fonte);
        traco.ate(&mut camada, 300.0, 200.0);
        traco.terminar(&mut camada);
        assert_eq!(
            camada.pixel(300, 200),
            [144, 50, 40, 255],
            "o pixel de (400, 250)"
        );
        assert_eq!(camada.pixel(330, 200)[3], 0, "fora do raio");
    }
}
