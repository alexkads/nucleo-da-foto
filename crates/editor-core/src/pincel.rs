//! O pincel e a borracha.
//!
//! Um traço é uma sequência de carimbos circulares ao longo do caminho do
//! ponteiro, a cada `espacamento × diâmetro` (25% no Photoshop) — medidos **ao
//! longo do caminho**, e não por evento: o mesmo caminho dá os mesmos carimbos
//! com 10 ou 1000 eventos de ponteiro.
//!
//! 🔑 **Opacidade e fluxo são coisas diferentes** (etapa 13; a definição da Adobe
//! em "Painting tools": o fluxo é a taxa com que a tinta se acumula enquanto o
//! botão está apertado, até o teto da opacidade, e o traço seguinte compõe por
//! cima). O traço tem uma **máscara própria** `m` por pixel, de 0 a 1; cada
//! carimbo de cobertura `c` (a queda da ponta) soma `fluxo · c` a ela:
//!
//! ```text
//! carimbo   m ← m + fluxo · c · (1 − m)          (nunca passa de 1)
//! pixel     a = m · opacidade                     (o teto do traço)
//! pincel    alfa = a + alfa_antes · (1 − a)
//!           cor  = (cor · a + cor_antes · alfa_antes · (1 − a)) / alfa
//! borracha  alfa = alfa_antes · (1 − a)          (a cor fica)
//! ```
//!
//! O pixel é recalculado do tile **de antes do traço** com essa máscara: passar
//! de novo no mesmo gesto acumula pelo fluxo, mas nunca passa da opacidade. Com
//! fluxo 100% e ponta dura, a primeira passada já enche (`m = 1`). É o modelo
//! "buffer do traço + opacidade" que editores abertos usam para imitar o
//! Photoshop; a curva exata da Adobe não é publicada.
//!
//! A borracha só mexe na **camada**: a base não está aqui, e nunca muda (C28).

use std::collections::BTreeMap;

use std::sync::Arc;

use crate::carimbo::Fonte;
use crate::mesclagem::Modo;
use crate::retangulo::Retangulo;
use crate::selecao::Selecao;
use crate::tiles::{indice, CamadaDePixels, Posicao, Tile, LADO_DO_TILE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ferramenta {
    Pincel,
    Borracha,
    /// Copia da [`Fonte`] em vez de pintar uma cor — o carimbo (S).
    Carimbo,
    /// Clareia a foto onde passa, na faixa de tons escolhida (O).
    Subexposicao(Faixa),
    /// Escurece a foto onde passa, na faixa de tons escolhida (⇧O).
    Superexposicao(Faixa),
    /// Suaviza (R).
    Desfoque,
    /// Realça o detalhe (⇧R).
    Nitidez,
}

impl Ferramenta {
    /// A ferramenta lê a foto (a [`Fonte`]) para decidir a cor de cada pixel —
    /// o carimbo e as de tom e de foco.
    pub fn le_a_foto(self) -> bool {
        !matches!(self, Ferramenta::Pincel | Ferramenta::Borracha)
    }
}

/// Os tons que a subexposição e a superexposição mexem — as do Photoshop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Faixa {
    Sombras,
    #[default]
    MeiosTons,
    Realces,
}

impl Faixa {
    pub const TODAS: [Faixa; 3] = [Faixa::Sombras, Faixa::MeiosTons, Faixa::Realces];

    pub fn nome(self) -> &'static str {
        match self {
            Faixa::Sombras => "Sombras",
            Faixa::MeiosTons => "Meios-tons",
            Faixa::Realces => "Realces",
        }
    }

    /// Quanto um tom de luminância `l` (`0..=1`) é desta faixa.
    pub fn peso(self, l: f32) -> f32 {
        match self {
            Faixa::Sombras => (1.0 - l) * (1.0 - l),
            Faixa::MeiosTons => 1.0 - (2.0 * l - 1.0) * (2.0 * l - 1.0),
            Faixa::Realces => l * l,
        }
    }
}

/// O tanto que uma passada de subexposição ou superexposição mexe, no tom
/// mais da faixa (a opacidade do pincel é a "exposição" por cima disso).
///
/// 0,25: com 0,5 uma passada só já deixava um disco claro visível no rosto
/// (visto no app real, 06/out/2026); como no Photoshop, o efeito se constrói
/// passada a passada — cada traço novo lê a foto com o anterior.
const FORCA_DO_TOM: f32 = 0.25;

/// A cor de um pixel depois da ferramenta de tom ou de foco, a partir da foto
/// (`foto`) e da foto desfocada no mesmo ponto (`suave`).
pub fn cor_da_ferramenta(ferramenta: Ferramenta, foto: [u8; 3], suave: [u8; 3]) -> [u8; 3] {
    let n = |v: u8| v as f32 / 255.0;
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let l = 0.3 * n(foto[0]) + 0.59 * n(foto[1]) + 0.11 * n(foto[2]);
    match ferramenta {
        Ferramenta::Subexposicao(faixa) => {
            let k = FORCA_DO_TOM * faixa.peso(l);
            foto.map(|v| q(n(v) + k * (1.0 - n(v))))
        }
        Ferramenta::Superexposicao(faixa) => {
            let k = FORCA_DO_TOM * faixa.peso(l);
            foto.map(|v| q(n(v) - k * n(v)))
        }
        Ferramenta::Desfoque => suave,
        Ferramenta::Nitidez => {
            let mut saida = [0u8; 3];
            for i in 0..3 {
                saida[i] = q(n(foto[i]) + (n(foto[i]) - n(suave[i])));
            }
            saida
        }
        _ => foto,
    }
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
    /// A cor de frente — a do pincel.
    pub cor: [u8; 3],
    /// A cor de fundo do Photoshop: X troca as duas, D volta a preto e
    /// branco. Na máscara, a borracha pinta com ela.
    pub cor_de_fundo: [u8; 3],
    /// `0..=1`: quanto cada carimbo soma à máscara do traço (ver o topo).
    pub fluxo: f32,
    /// A distância entre carimbos, em fração do **diâmetro** (0,25 = 25%,
    /// o padrão da ponta redonda do Photoshop).
    pub espacamento: f32,
    /// `0..=1`: a suavização do traço (o "cordão" do Photoshop: a ponta só
    /// anda quando o ponteiro se afasta mais que o comprimento dele).
    pub suavizacao: f32,
    /// O modo da **ferramenta** (o "Modo" da barra do pincel e do carimbo no
    /// Photoshop), separado do modo da camada: como a tinta se mistura com o
    /// que a camada já tem. Sobre transparente não há com o que misturar, e a
    /// tinta entra como no Normal (a conta do W3C, `mesclar_em_camada`).
    pub modo: Modo,
}

impl Default for Pincel {
    fn default() -> Self {
        Self {
            ferramenta: Ferramenta::Pincel,
            raio: 40.0,
            dureza: 0.8,
            opacidade: 1.0,
            cor: [0, 0, 0],
            cor_de_fundo: [255, 255, 255],
            fluxo: 1.0,
            espacamento: ESPACAMENTO_PADRAO,
            // O padrão do Photoshop é 10%.
            suavizacao: 0.1,
            modo: Modo::Normal,
        }
    }
}

/// 25% do diâmetro — a ponta redonda do Photoshop.
pub const ESPACAMENTO_PADRAO: f32 = 0.25;
/// O espaçamento mínimo e o máximo (1% e 1000%, como no painel de pincéis).
pub const ESPACAMENTO_MINIMO: f32 = 0.01;
pub const ESPACAMENTO_MAXIMO: f32 = 10.0;

/// O comprimento do cordão com a suavização em 100%, em **pontos da tela** —
/// quem traça converte pela escala de agora (ver [`Traco::com_cordao`]).
pub const CORDAO_MAXIMO_NA_TELA: f32 = 60.0;

/// Uma predefinição de pincel: o que ela muda e o nome na lista.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Predefinicao {
    pub nome: &'static str,
    pub raio: f32,
    pub dureza: f32,
    pub opacidade: f32,
    pub fluxo: f32,
    pub espacamento: f32,
}

impl Predefinicao {
    /// O pincel com as medidas desta predefinição (cor, ferramenta e
    /// suavização ficam).
    pub fn aplicada(&self, pincel: Pincel) -> Pincel {
        Pincel {
            raio: self.raio,
            dureza: self.dureza,
            opacidade: self.opacidade,
            fluxo: self.fluxo,
            espacamento: self.espacamento,
            ..pincel
        }
    }

    /// O pincel está com as medidas desta predefinição?
    pub fn e_a_de(&self, p: &Pincel) -> bool {
        let perto = |a: f32, b: f32| (a - b).abs() < 1e-3;
        perto(p.raio, self.raio)
            && perto(p.dureza, self.dureza)
            && perto(p.opacidade, self.opacidade)
            && perto(p.fluxo, self.fluxo)
            && perto(p.espacamento, self.espacamento)
    }
}

/// As predefinições de fábrica — as pontas redondas básicas do Photoshop
/// (dura, macia) e as de retoque que o balcão mais usa.
pub const PREDEFINICOES: [Predefinicao; 6] = [
    Predefinicao {
        nome: "Redondo duro",
        raio: 15.0,
        dureza: 1.0,
        opacidade: 1.0,
        fluxo: 1.0,
        espacamento: 0.25,
    },
    Predefinicao {
        nome: "Redondo macio",
        raio: 40.0,
        dureza: 0.0,
        opacidade: 1.0,
        fluxo: 1.0,
        espacamento: 0.25,
    },
    Predefinicao {
        nome: "Aerógrafo macio (fluxo 10%)",
        raio: 60.0,
        dureza: 0.0,
        opacidade: 1.0,
        fluxo: 0.1,
        espacamento: 0.1,
    },
    Predefinicao {
        nome: "Retoque de pele (opac. 30%)",
        raio: 25.0,
        dureza: 0.2,
        opacidade: 0.3,
        fluxo: 1.0,
        espacamento: 0.25,
    },
    Predefinicao {
        nome: "Máscara: borda média",
        raio: 50.0,
        dureza: 0.5,
        opacidade: 1.0,
        fluxo: 1.0,
        espacamento: 0.25,
    },
    Predefinicao {
        nome: "Detalhe fino",
        raio: 3.0,
        dureza: 0.9,
        opacidade: 1.0,
        fluxo: 1.0,
        espacamento: 0.15,
    },
];

/// O menor e o maior raio, em pixels da foto.
pub const RAIO_MINIMO: f32 = 0.5;
pub const RAIO_MAXIMO: f32 = 2000.0;

impl Pincel {
    /// A cobertura de um carimbo à distância `d` do centro, `0..=1`.
    ///
    /// 🔑 **A dureza é o núcleo cheio** (revisto na etapa 13): até
    /// `dureza · raio` o carimbo cobre tudo, e daí até o raio cai **suave até
    /// zero** (`1 − smoothstep`). Com dureza 0 a queda começa no centro; com 1
    /// é um disco cheio. A **geometria** tem um pixel de anti-aliasing em volta
    /// do raio (`raio ± 0,5`), que só pesa com a ponta quase dura.
    ///
    /// ⚠️ A regra anterior (do PaintFE) fazia da dureza a *opacidade da borda*
    /// e cortava no raio: com dureza 50% a borda ficava em 50% e caía de uma
    /// vez — um degrau que a ponta do Photoshop não tem (a de lá vai a zero na
    /// borda em toda dureza menor que 100%). A curva exata da Adobe não é
    /// publicada; esta é a forma que ela descreve (o "centro duro").
    pub fn queda(&self, d: f32) -> f32 {
        let raio = self.raio.max(RAIO_MINIMO);
        let dura = self.dureza.clamp(0.0, 1.0);
        let nucleo = dura * raio;
        // Até o núcleo (e com a ponta inteira dura, que não tem queda), cheio.
        let material = if d <= nucleo || raio - nucleo <= 1e-6 {
            1.0
        } else {
            let t = ((d - nucleo) / (raio - nucleo)).clamp(0.0, 1.0);
            1.0 - t * t * (3.0 - 2.0 * t)
        };
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
    /// A máscara do traço por pixel (0..=[`CHEIO`], `m` do topo), por tile
    /// tocado. 16 bits: com fluxo de 1% um carimbo soma ~2,5 em 255, e 8 bits
    /// arredondariam o acúmulo para zero.
    cobertura: BTreeMap<Posicao, Vec<u16>>,
    ultimo: Option<(f32, f32)>,
    /// O comprimento do cordão da suavização, em pixels da foto (0 = sem).
    cordao: f32,
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
            cordao: 0.0,
            resto: 0.0,
            selecao: None,
            fonte: None,
        }
    }

    /// A suavização do pincel com a tela de agora: `escala` são pontos da
    /// tela por pixel da foto (o cordão é medido na tela, como no Photoshop,
    /// e não muda de tamanho com o zoom).
    pub fn com_cordao(mut self, escala: f32) -> Self {
        let s = self.pincel.suavizacao.clamp(0.0, 1.0);
        self.cordao = s * CORDAO_MAXIMO_NA_TELA / escala.max(1e-3);
        self
    }

    /// Onde a ponta está agora (a do cordão, com suavização) — o fim do traço.
    pub fn ponta(&self) -> Option<(f32, f32)> {
        self.ultimo
    }

    /// ⇧ + clique: uma reta de `de` (o fim do traço anterior) até `ate`, sem
    /// cordão — a ponta segue a reta exata.
    pub fn reta(
        &mut self,
        camada: &mut CamadaDePixels,
        de: (f32, f32),
        ate: (f32, f32),
    ) -> Retangulo {
        let cordao = std::mem::take(&mut self.cordao);
        let mut sujo = self.ate(camada, de.0, de.1);
        sujo = sujo.uniao(&self.ate(camada, ate.0, ate.1));
        self.cordao = cordao;
        sujo
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
        let espacamento = self
            .pincel
            .espacamento
            .clamp(ESPACAMENTO_MINIMO, ESPACAMENTO_MAXIMO);
        let passo = (2.0 * self.pincel.raio * espacamento).max(0.5);
        let Some((x0, y0)) = self.ultimo else {
            self.ultimo = Some((x, y));
            self.resto = 0.0;
            return self.carimbar(camada, x, y);
        };
        // 🧵 O cordão: a ponta só anda quando o ponteiro passa do comprimento
        // dele, e anda até ficar a esse comprimento — o caminho fica liso sem
        // depender do tempo nem de quantos eventos chegaram.
        let (x, y) = if self.cordao > 0.0 {
            let (px, py) = (x - x0, y - y0);
            let longe = (px * px + py * py).sqrt();
            if longe <= self.cordao {
                return Retangulo::default();
            }
            let k = (longe - self.cordao) / longe;
            (x0 + px * k, y0 + py * k)
        } else {
            (x, y)
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
            let fluxo = self.pincel.fluxo.clamp(0.0, 1.0);
            let opacidade = self.pincel.opacidade.clamp(0.0, 1.0);
            let mascara = self.selecao.as_ref().map(|s| s.do_tile(posicao));
            if let Some(Err(0)) = mascara {
                // O tile inteiro está fora da seleção.
                continue;
            }
            let tile = camada.tile_mut(posicao);
            let (tx0, ty0) = (
                posicao.0 as u32 * LADO_DO_TILE,
                posicao.1 as u32 * LADO_DO_TILE,
            );
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
                    // m ← m + fluxo·c·(1 − m), em 16 bits.
                    let m = cobertura[k] as f32 / CHEIO as f32;
                    let novo_m = m + fluxo * (c as f32 / 255.0) * (1.0 - m);
                    let q = (novo_m * CHEIO as f32).round().min(CHEIO as f32) as u16;
                    if q <= cobertura[k] {
                        continue;
                    }
                    cobertura[k] = q;
                    let a = q as f32 / CHEIO as f32 * opacidade;
                    let i = indice(lx, ly);
                    let de_antes = match &antes {
                        Some(t) => [t[i], t[i + 1], t[i + 2], t[i + 3]],
                        None => [0; 4],
                    };
                    let novo = match (self.pincel.ferramenta, self.fonte.as_mut()) {
                        (Ferramenta::Carimbo, Some(fonte)) => match fonte.cor_com_alfa(px, py) {
                            // O alfa da origem (a camada atual tem
                            // transparência) entra na cobertura.
                            Some([r, g, b, alfa]) => {
                                let a = a * alfa as f32 / 255.0;
                                if a <= 0.0 {
                                    continue;
                                }
                                pintar(
                                    &Pincel {
                                        cor: [r, g, b],
                                        ..self.pincel
                                    },
                                    de_antes,
                                    a,
                                )
                            }
                            // A origem caiu fora da foto: este pixel fica.
                            None => continue,
                        },
                        (f, Some(fonte)) if f.le_a_foto() => {
                            let (Some(foto), Some(suave)) =
                                (fonte.cor(px, py), fonte.desfocada(px, py))
                            else {
                                continue;
                            };
                            let cor = cor_da_ferramenta(f, foto, suave);
                            aplicar_alfa(&Pincel { cor, ..self.pincel }, de_antes, a)
                        }
                        _ => pintar(&self.pincel, de_antes, a),
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

/// A máscara do traço cheia (`m = 1`).
const CHEIO: u16 = u16::MAX;

/// Um pixel da camada depois do traço, a partir do pixel de antes e da
/// cobertura do traço nele (vezes a opacidade do pincel).
pub fn aplicar(pincel: &Pincel, antes: [u8; 4], cobertura: u8) -> [u8; 4] {
    let a = cobertura as f32 / 255.0 * pincel.opacidade.clamp(0.0, 1.0);
    aplicar_alfa(pincel, antes, a)
}

/// A tinta no modo da ferramenta: no Normal (e na borracha) a conta de
/// [`aplicar_alfa`], bit a bit a de antes; nos outros modos, a do W3C com o
/// pixel da camada como fundo.
fn pintar(pincel: &Pincel, antes: [u8; 4], a: f32) -> [u8; 4] {
    if pincel.modo == Modo::Normal || pincel.ferramenta == Ferramenta::Borracha {
        return aplicar_alfa(pincel, antes, a);
    }
    let [r, g, b] = pincel.cor;
    crate::mesclagem::mesclar_em_camada(antes, [r, g, b, 255], a, pincel.modo)
}

/// O mesmo, com o `a` (`0..=1`) já pronto — a máscara do traço vezes a
/// opacidade.
fn aplicar_alfa(pincel: &Pincel, antes: [u8; 4], a: f32) -> [u8; 4] {
    let alfa_antes = antes[3] as f32 / 255.0;
    match pincel.ferramenta {
        Ferramenta::Borracha => {
            let alfa = alfa_antes * (1.0 - a);
            [antes[0], antes[1], antes[2], quantizar(alfa)]
        }
        // O carimbo e as de tom e de foco pintam como o pincel, com a cor que
        // a fonte deu ao pixel.
        _ => {
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
            ..Pincel::default()
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
    fn a_dureza_e_o_nucleo_cheio() {
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
        // Dureza 50%: cheio até a metade do raio e **sem degrau** na borda —
        // a regra anterior deixava a borda em 50% e cortava de uma vez.
        p.dureza = 0.5;
        assert_eq!(p.queda(4.9), 1.0, "o núcleo cheio");
        assert!(
            p.queda(9.4) < 0.05,
            "chega à borda quase em zero: {}",
            p.queda(9.4)
        );
        let mut antes = 1.0;
        for i in 0..=110 {
            let q = p.queda(i as f32 / 10.0);
            assert!(
                q <= antes + 1e-6,
                "a queda nunca sobe (d = {})",
                i as f32 / 10.0
            );
            antes = q;
        }
    }

    /// A máscara do traço num pixel, de 0 a 255, depois de `passadas` idas e
    /// voltas sobre ele.
    fn alfa_depois(p: Pincel, passadas: usize) -> u8 {
        let mut camada = CamadaDePixels::nova(400, 200);
        let mut traco = Traco::novo(p);
        traco.ate(&mut camada, 50.0, 100.0);
        for i in 0..passadas {
            let x = if i % 2 == 0 { 350.0 } else { 50.0 };
            traco.ate(&mut camada, x, 100.0);
        }
        traco.terminar(&mut camada);
        // Os carimbos caem em 50, 70, …, 350 na ida e nos mesmos na volta: o
        // pixel 190 leva exatamente um por passada.
        camada.pixel(190, 100)[3]
    }

    #[test]
    fn opacidade_e_teto_e_fluxo_e_taxa() {
        let base = Pincel {
            raio: 10.0,
            dureza: 1.0,
            espacamento: 1.0, // um carimbo a cada 20 px
            ..pincel(Ferramenta::Pincel)
        };
        // Opacidade 50%, fluxo 100%: uma passada já chega ao teto, e ir e
        // voltar no mesmo traço não passa dele.
        let meia_opacidade = Pincel {
            opacidade: 0.5,
            ..base
        };
        assert_eq!(alfa_depois(meia_opacidade, 1), 128);
        assert_eq!(alfa_depois(meia_opacidade, 6), 128);
        // Fluxo 25%, opacidade 100%: um carimbo dá 25%, e o mesmo traço
        // acumula passada a passada.
        let fluxo_baixo = Pincel {
            fluxo: 0.25,
            ..base
        };
        let uma = alfa_depois(fluxo_baixo, 1);
        assert!((uma as i32 - 64).abs() <= 1, "uma passada: {uma}");
        let duas = alfa_depois(fluxo_baixo, 2);
        let seis = alfa_depois(fluxo_baixo, 6);
        assert!(duas > uma && seis > duas, "acumula: {uma} {duas} {seis}");
        // As duas juntas: o fluxo acumula até a opacidade, e nunca acima.
        let as_duas = Pincel {
            opacidade: 0.5,
            fluxo: 0.25,
            ..base
        };
        let muitas = alfa_depois(as_duas, 40);
        assert!((124..=128).contains(&muitas), "perto do teto: {muitas}");
        assert!(alfa_depois(as_duas, 1) < 40);
    }

    #[test]
    fn com_fluxo_baixo_a_borda_macia_nao_some_no_arredondamento() {
        // 1% de fluxo: cada carimbo soma ~2,5/255 — em 8 bits a borda da ponta
        // macia arredondaria para zero; a máscara de 16 bits acumula.
        let p = Pincel {
            raio: 20.0,
            dureza: 0.0,
            fluxo: 0.01,
            espacamento: 0.01,
            ..pincel(Ferramenta::Pincel)
        };
        let mut camada = CamadaDePixels::nova(300, 100);
        let mut traco = Traco::novo(p);
        traco.ate(&mut camada, 50.0, 50.0);
        traco.ate(&mut camada, 250.0, 50.0);
        traco.terminar(&mut camada);
        assert!(camada.pixel(150, 62)[3] > 0, "a borda macia recebeu tinta");
    }

    #[test]
    fn o_traco_nao_depende_de_quantos_eventos_chegaram() {
        // O mesmo caminho em 1, em 7 e em 300 eventos (passos irregulares):
        // os carimbos caem nos mesmos lugares, e com fluxo baixo — onde cada
        // carimbo a mais apareceria — a camada sai igual.
        let p = Pincel {
            raio: 18.0,
            dureza: 0.3,
            fluxo: 0.3,
            opacidade: 0.9,
            ..pincel(Ferramenta::Pincel)
        };
        let tracar = |pontos: &[f32]| {
            let mut camada = CamadaDePixels::nova(500, 200);
            let mut traco = Traco::novo(p);
            traco.ate(&mut camada, 40.0, 100.0);
            for x in pontos {
                traco.ate(&mut camada, *x, 100.0 + (*x - 40.0) * 0.2);
            }
            traco.terminar(&mut camada);
            camada
        };
        let um = tracar(&[440.0]);
        let sete = tracar(&[41.0, 90.0, 91.5, 200.0, 333.3, 439.0, 440.0]);
        let muitos: Vec<f32> = (1..=300)
            .map(|i| 40.0 + 400.0 * (i as f32 / 300.0))
            .collect();
        let trezentos = tracar(&muitos);
        let mut maior = 0;
        for y in 0..200 {
            for x in 0..500 {
                for outro in [&sete, &trezentos] {
                    let (a, b) = (um.pixel(x, y), outro.pixel(x, y));
                    for k in 0..4 {
                        maior = maior.max((a[k] as i32 - b[k] as i32).abs());
                    }
                }
            }
        }
        assert!(maior <= 2, "diferença máxima entre as frequências: {maior}");
    }

    #[test]
    fn o_espacamento_e_fracao_do_diametro() {
        // 200% de 10 px de diâmetro: um carimbo a cada 20 px. Entre dois
        // carimbos (x = 10) não chega tinta; no carimbo (x = 20), chega.
        let p = Pincel {
            raio: 5.0,
            dureza: 1.0,
            espacamento: 2.0,
            ..pincel(Ferramenta::Pincel)
        };
        let mut camada = CamadaDePixels::nova(200, 50);
        let mut traco = Traco::novo(p);
        traco.ate(&mut camada, 0.5, 25.0);
        traco.ate(&mut camada, 100.5, 25.0);
        traco.terminar(&mut camada);
        assert_eq!(camada.pixel(10, 25)[3], 0, "entre carimbos");
        assert_eq!(camada.pixel(20, 25)[3], 255, "no carimbo");
        assert_eq!(camada.pixel(80, 25)[3], 255);
    }

    #[test]
    fn o_cordao_segura_a_ponta_e_e_medido_na_tela() {
        let p = Pincel {
            suavizacao: 0.5,
            ..pincel(Ferramenta::Pincel)
        };
        // 50% de 60 pontos = 30 pontos; com a foto a 1:1, 30 px.
        let mut camada = CamadaDePixels::nova(400, 200);
        let mut traco = Traco::novo(p).com_cordao(1.0);
        traco.ate(&mut camada, 100.0, 100.0);
        assert!(
            traco.ate(&mut camada, 125.0, 100.0).vazio(),
            "dentro do cordão"
        );
        assert_eq!(traco.ponta(), Some((100.0, 100.0)));
        traco.ate(&mut camada, 200.0, 100.0);
        let (x, y) = traco.ponta().unwrap();
        assert!(
            (x - 170.0).abs() < 1e-3 && y == 100.0,
            "a 30 px do ponteiro: {x}"
        );
        // Com a foto ampliada 2×, o mesmo cordão na tela são 15 px da foto.
        let mut traco = Traco::novo(p).com_cordao(2.0);
        traco.ate(&mut camada, 100.0, 50.0);
        traco.ate(&mut camada, 200.0, 50.0);
        assert!((traco.ponta().unwrap().0 - 185.0).abs() < 1e-3);
    }

    #[test]
    fn a_reta_liga_dois_pontos_sem_o_cordao() {
        let p = Pincel {
            suavizacao: 1.0,
            raio: 4.0,
            dureza: 1.0,
            ..pincel(Ferramenta::Pincel)
        };
        let mut camada = CamadaDePixels::nova(400, 200);
        let mut traco = Traco::novo(p).com_cordao(1.0);
        traco.reta(&mut camada, (50.0, 50.0), (350.0, 150.0));
        traco.terminar(&mut camada);
        for t in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
            let (x, y) = (50.0 + 300.0 * t, 50.0 + 100.0 * t);
            assert_eq!(camada.pixel(x as u32, y as u32)[3], 255, "t = {t}");
        }
        assert_eq!(camada.pixel(200, 50)[3], 0, "fora da reta");
    }

    #[test]
    fn as_predefinicoes_mudam_so_as_medidas() {
        let p = Pincel {
            cor: [1, 2, 3],
            ferramenta: Ferramenta::Borracha,
            suavizacao: 0.4,
            ..Pincel::default()
        };
        for pre in PREDEFINICOES {
            let q = pre.aplicada(p);
            assert!(pre.e_a_de(&q), "{}", pre.nome);
            assert_eq!(
                (q.cor, q.ferramenta, q.suavizacao),
                (p.cor, p.ferramenta, p.suavizacao)
            );
        }
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
            ..Pincel::default()
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

    #[test]
    fn as_ferramentas_de_tom_e_de_foco() {
        let meio = [128u8, 128, 128];
        let clara = cor_da_ferramenta(Ferramenta::Subexposicao(Faixa::MeiosTons), meio, meio);
        let escura = cor_da_ferramenta(Ferramenta::Superexposicao(Faixa::MeiosTons), meio, meio);
        assert!(clara[0] > 150 && escura[0] < 110, "{clara:?} {escura:?}");
        // Nas sombras, um meio-tom quase não mexe com a faixa dos realces.
        let realce =
            cor_da_ferramenta(Ferramenta::Subexposicao(Faixa::Realces), [20, 20, 20], meio);
        assert!(realce[0] < 25, "{realce:?}");
        // Desfoque devolve a suave; nitidez se afasta dela.
        assert_eq!(
            cor_da_ferramenta(Ferramenta::Desfoque, [200, 0, 0], [100, 50, 0]),
            [100, 50, 0]
        );
        assert_eq!(
            cor_da_ferramenta(Ferramenta::Nitidez, [150, 100, 0], [100, 100, 0]),
            [200, 100, 0]
        );
    }
}
