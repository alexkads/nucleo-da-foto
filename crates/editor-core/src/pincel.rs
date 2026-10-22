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

/// A ferramenta que pinta com o [`Pincel`]: todas usam a mesma ponta (raio,
/// dureza, fluxo, espaçamento) e a mesma máscara do traço; muda o que vai para
/// cada pixel. As letras entre parênteses são os atalhos do Photoshop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ferramenta {
    /// Pinta a [`cor`](Pincel::cor) de frente no [`modo`](Pincel::modo) da
    /// ferramenta (B).
    Pincel,
    /// Tira alfa da camada e deixa a cor (E); numa máscara, pinta a
    /// [`cor_de_fundo`](Pincel::cor_de_fundo).
    Borracha,
    /// Copia da [`Fonte`] em vez de pintar uma cor — o carimbo (S).
    Carimbo,
    /// O pincel de recuperação com origem manual (J, ⌥ + clique na origem):
    /// copia como o carimbo durante o traço e, ao soltar, adapta a cor e a
    /// luz da cópia ao destino em volta (`recuperacao.rs`).
    Recuperacao,
    /// Clareia a foto onde passa, na faixa de tons escolhida (O).
    Subexposicao(Faixa),
    /// Escurece a foto onde passa, na faixa de tons escolhida (⇧O).
    Superexposicao(Faixa),
    /// Suaviza (R).
    Desfoque,
    /// Realça o detalhe (⇧R).
    Nitidez,
    /// O Pincel misturador (B, no grupo do pincel): a tinta do reservatório
    /// se mistura com a cor da tela (`misturador.rs`).
    Misturador,
}

impl Ferramenta {
    /// A ferramenta lê a foto (a [`Fonte`]) para decidir a cor de cada pixel —
    /// o carimbo e as de tom e de foco.
    pub fn le_a_foto(self) -> bool {
        !matches!(self, Ferramenta::Pincel | Ferramenta::Borracha)
    }

    /// Copia de uma origem escolhida com ⌥ + clique (carimbo e recuperação).
    pub fn copia_da_origem(self) -> bool {
        matches!(self, Ferramenta::Carimbo | Ferramenta::Recuperacao)
    }
}

/// Os tons que a subexposição e a superexposição mexem — as do Photoshop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Faixa {
    /// Os tons escuros: peso `(1 − l)²`, cheio no preto e nulo no branco.
    Sombras,
    /// Os tons do meio: peso `1 − (2l − 1)²`, cheio no cinza 50% e nulo nas
    /// pontas. O padrão, como no Photoshop.
    #[default]
    MeiosTons,
    /// Os tons claros: peso `l²`, cheio no branco e nulo no preto.
    Realces,
}

impl Faixa {
    /// As três, na ordem do menu "Faixa" da barra.
    pub const TODAS: [Faixa; 3] = [Faixa::Sombras, Faixa::MeiosTons, Faixa::Realces];

    /// O nome em português que a barra mostra.
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

/// As opções da ponta e da tinta — a barra de opções do pincel do Photoshop,
/// valendo para todas as [`Ferramenta`]s de pintura. Copiado em cada
/// [`Traco`]: mexer no pincel no meio do traço não muda o traço em curso.
///
/// Os campos não são validados aqui: o raio é cortado entre [`RAIO_MINIMO`] e
/// [`RAIO_MAXIMO`] por quem edita, e o espaçamento entre
/// [`ESPACAMENTO_MINIMO`] e [`ESPACAMENTO_MAXIMO`] pelo traço. O
/// [`Default`] é a ponta redonda de 80 px de diâmetro, dureza 80%, preto
/// sobre branco.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pincel {
    /// O que o traço faz com os pixels (padrão [`Ferramenta::Pincel`]).
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
    /// A difusão do pincel de recuperação, de 1 a 7 (a do Photoshop): quanto
    /// a borda do traço é suavizada antes de a cor se adaptar a ela. Não tem
    /// nada com a difusão da seleção.
    pub difusao: u8,
}

/// A difusão padrão da recuperação (a do Photoshop).
pub const DIFUSAO_PADRAO: u8 = 5;

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
            difusao: DIFUSAO_PADRAO,
        }
    }
}

/// 25% do diâmetro — a ponta redonda do Photoshop.
pub const ESPACAMENTO_PADRAO: f32 = 0.25;
/// O espaçamento mínimo e o máximo (1% e 1000%, como no painel de pincéis).
pub const ESPACAMENTO_MINIMO: f32 = 0.01;
/// O maior espaçamento, 10 diâmetros (1000%): os carimbos ficam soltos, um a
/// cada dez diâmetros.
pub const ESPACAMENTO_MAXIMO: f32 = 10.0;

/// O comprimento do cordão com a suavização em 100%, em **pontos da tela** —
/// quem traça converte pela escala de agora (ver [`Traco::com_cordao`]).
pub const CORDAO_MAXIMO_NA_TELA: f32 = 60.0;

/// Uma predefinição de pincel: o que ela muda e o nome na lista.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Predefinicao {
    /// O nome na lista de predefinições.
    pub nome: &'static str,
    /// O [`Pincel::raio`], em pixels da foto.
    pub raio: f32,
    /// A [`Pincel::dureza`], `0..=1`.
    pub dureza: f32,
    /// A [`Pincel::opacidade`], `0..=1`.
    pub opacidade: f32,
    /// O [`Pincel::fluxo`], `0..=1`.
    pub fluxo: f32,
    /// O [`Pincel::espacamento`], em fração do diâmetro.
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
pub const PREDEFINICOES: [Predefinicao; 7] = [
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
        nome: "Dodge & Burn (fluxo 5%)",
        raio: 30.0,
        dureza: 0.0,
        opacidade: 1.0,
        fluxo: 0.05,
        espacamento: 0.1,
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
/// O maior raio, em pixels da foto (uma ponta de 4000 px de diâmetro).
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

    #[inline]
    fn em(&self, d2: f32) -> u8 {
        let t = d2 / (self.alcance * self.alcance);
        if t >= 1.0 {
            return 0;
        }
        // O `round` de sempre, sem a chamada à libm a cada pixel da ponta.
        self.valores[crate::arredondado(t * TAMANHO_DA_TABELA as f32) as usize]
    }
}

/// O que o traço mudou: o tile de antes e o de depois, por posição — é o passo
/// do desfazer.
#[derive(Clone, Debug, PartialEq)]
pub struct Mudanca {
    /// Cada tile tocado como era antes do traço (`None` = não existia, era
    /// transparente) — o que o desfazer põe de volta.
    pub antes: Vec<(Posicao, Option<Tile>)>,
    /// Os mesmos tiles, na mesma ordem, como o traço os deixou (`None` = o
    /// traço os apagou inteiros) — o que o refazer põe de volta.
    pub depois: Vec<(Posicao, Option<Tile>)>,
}

/// Um traço em curso: do ponteiro descer ao ponteiro subir, sobre uma
/// [`CamadaDePixels`] (a camada ou a máscara dela).
///
/// Monta-se com [`novo`](Traco::novo) e os `com_*`/`dentro_de`/`copiando_de`,
/// alimenta-se com [`ate`](Traco::ate) a cada evento de ponteiro (em pixels da
/// foto, que já pinta na camada e devolve o retângulo a redesenhar) e fecha-se
/// com [`terminar`](Traco::terminar), que devolve a [`Mudanca`] para o
/// histórico. Quem conduz no app é a [`Sessao`](crate::Sessao).
///
/// ```rust
/// use editor_core::pincel::Traco;
/// use editor_core::{CamadaDePixels, Pincel};
///
/// let mut camada = CamadaDePixels::nova(200, 100);
/// let pincel = Pincel { raio: 10.0, dureza: 1.0, cor: [255, 0, 0], ..Pincel::default() };
///
/// let mut traco = Traco::novo(pincel);
/// traco.ate(&mut camada, 20.0, 50.0);
/// let sujo = traco.ate(&mut camada, 180.0, 50.0);
/// assert!(!sujo.vazio());
///
/// let mudanca = traco.terminar(&mut camada).expect("o traço pintou");
/// assert!(!mudanca.antes.is_empty());
/// assert_eq!(camada.pixel(100, 50), [255, 0, 0, 255]);
/// assert_eq!(camada.pixel(100, 5), [0, 0, 0, 0]);
/// ```
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
    /// "Bloquear pixels transparentes": a tinta muda a cor e deixa o alfa de
    /// cada pixel como estava — onde era transparente, nada acontece.
    alfa_travado: bool,
    /// A conta da recuperação: aditiva na alta frequência.
    adaptacao: crate::recuperacao::Adaptacao,
    /// O Pincel misturador: a tinta que ele carrega e as opções.
    misturador: Option<(
        crate::misturador::Tinta,
        crate::misturador::OpcoesDoMisturador,
    )>,
}

impl Traco {
    /// Um traço com este pincel, sem nada pintado: sem seleção, sem fonte, sem
    /// cordão (suavização desligada até [`com_cordao`](Traco::com_cordao)) e
    /// com a transparência livre. O primeiro [`ate`](Traco::ate) já carimba.
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
            alfa_travado: false,
            adaptacao: crate::recuperacao::Adaptacao::Multiplicativa,
            misturador: None,
        }
    }

    /// O Pincel misturador com esta tinta e estas opções; `fonte` é a foto
    /// composta ("Amostrar todas as camadas"), ou nada (a própria camada).
    pub fn misturando(
        mut self,
        tinta: crate::misturador::Tinta,
        opcoes: crate::misturador::OpcoesDoMisturador,
    ) -> Self {
        self.misturador = Some((tinta, opcoes));
        self
    }

    /// A tinta como o traço a deixou (o pincel sujo segue para o próximo).
    pub fn tinta(&self) -> Option<&crate::misturador::Tinta> {
        self.misturador.as_ref().map(|(t, _)| t)
    }

    /// A recuperação com esta conta (a aditiva na alta frequência).
    pub fn com_adaptacao(mut self, adaptacao: crate::recuperacao::Adaptacao) -> Self {
        self.adaptacao = adaptacao;
        self
    }

    /// Com a transparência da camada bloqueada (ver `Traco::alfa_travado`).
    pub fn com_alfa_travado(mut self, travado: bool) -> Self {
        self.alfa_travado = travado;
        self
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

    /// O pincel com que o traço começou (a cópia dele).
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
        self.carimbar_com(camada, cx, cy, crate::nucleos().unwrap_or(1))
    }

    /// O carimbo, com até `nucleos` threads na conta dos pixels.
    fn carimbar_com(
        &mut self,
        camada: &mut CamadaDePixels,
        cx: f32,
        cy: f32,
        nucleos: usize,
    ) -> Retangulo {
        if self.pincel.ferramenta == Ferramenta::Misturador {
            return self.carimbar_misturando(camada, cx, cy);
        }
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
        let ponta = Ponta {
            pincel: self.pincel,
            tabela: &self.tabela,
            alfa_travado: self.alfa_travado,
            centro: (cx, cy),
            fluxo: self.pincel.fluxo.clamp(0.0, 1.0),
            opacidade: self.pincel.opacidade.clamp(0.0, 1.0),
            xs: (x0, x1),
            ys: (y0, y1),
        };
        // 🧵 **A ponta grande em faixas, uma thread por núcleo.** Os tiles
        // de antes (do desfazer), a cobertura e o tile para escrever são
        // registrados antes, em série e na ordem de sempre; cada pixel só lê
        // o que é dele (e a fonte, que todas as threads leem), então a conta
        // de cada um é a mesma, em qualquer ordem — a camada sai igual byte
        // a byte.
        let paralelo =
            nucleos > 1 && area.largura as usize * area.altura as usize >= PIXELS_PARA_DIVIDIR;
        let mut divididos = Vec::new();
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
            if paralelo {
                divididos.push((posicao, antes, mascara));
                continue;
            }
            ponta.no_tile(
                self.fonte.as_ref(),
                posicao,
                0,
                mascara,
                antes.as_deref().map(Vec::as_slice),
                tile,
                cobertura,
            );
        }
        if divididos.is_empty() {
            return area;
        }
        // Cada tile em faixas de linhas, e as faixas repartidas entre as
        // threads (a borda da ponta tem menos pixels que o miolo).
        let posicoes: Vec<Posicao> = divididos.iter().map(|(p, _, _)| *p).collect();
        let tiles = camada.varios_tiles_mut(&posicoes);
        let coberturas = varios_mut(&mut self.cobertura, &posicoes);
        let lado = LADO_DO_TILE as usize;
        let mut faixas = Vec::new();
        for (((posicao, antes, mascara), tile), cobertura) in
            divididos.iter().zip(tiles).zip(coberturas)
        {
            let ty0 = posicao.1 as u32 * LADO_DO_TILE;
            let (ly0, ly1) = (y0.max(ty0) - ty0, y1.min(ty0 + LADO_DO_TILE) - ty0);
            for (k, (linhas, cobertura)) in tile
                .chunks_mut(LINHAS_POR_FAIXA * lado * 4)
                .zip(cobertura.chunks_mut(LINHAS_POR_FAIXA * lado))
                .enumerate()
            {
                let primeira = (k * LINHAS_POR_FAIXA) as u32;
                if primeira >= ly1 || primeira + LINHAS_POR_FAIXA as u32 <= ly0 {
                    continue;
                }
                faixas.push((
                    *posicao,
                    primeira,
                    *mascara,
                    antes.as_deref(),
                    linhas,
                    cobertura,
                ));
            }
        }
        let n = nucleos.min(faixas.len());
        let mut lotes: Vec<Vec<_>> = (0..n).map(|_| Vec::new()).collect();
        for (k, faixa) in faixas.into_iter().enumerate() {
            lotes[k % n].push(faixa);
        }
        let fonte = self.fonte.as_ref();
        let carimbar = |lote: Vec<_>| {
            for (posicao, primeira, mascara, antes, linhas, cobertura) in lote {
                let antes: Option<&Vec<u8>> = antes;
                ponta.no_tile(
                    fonte,
                    posicao,
                    primeira,
                    mascara,
                    antes.map(Vec::as_slice),
                    linhas,
                    cobertura,
                );
            }
        };
        std::thread::scope(|escopo| {
            let mut lotes = lotes.into_iter();
            let aqui = lotes.next();
            for lote in lotes {
                escopo.spawn(move || carimbar(lote));
            }
            if let Some(lote) = aqui {
                carimbar(lote);
            }
        });
        area
    }

    /// Um carimbo do Pincel misturador: a ponta inteira lê a tela de agora
    /// (a camada, com o que este traço já pintou — é o que borra) ou a foto
    /// do começo do traço, e a [`crate::misturador::Tinta`] decide cada pixel.
    fn carimbar_misturando(&mut self, camada: &mut CamadaDePixels, cx: f32, cy: f32) -> Retangulo {
        let Some((mut tinta, opcoes)) = self.misturador.take() else {
            return Retangulo::default();
        };
        let r = self.tabela.alcance.ceil() as i64;
        let lado = (2 * r + 1) as usize;
        let (ox, oy) = (cx.floor() as i64 - r, cy.floor() as i64 - r);
        let (largura, altura) = (camada.largura() as i64, camada.altura() as i64);
        let forca = self.pincel.fluxo.clamp(0.0, 1.0) * self.pincel.opacidade.clamp(0.0, 1.0);
        let mut cobertura = vec![0f32; lado * lado];
        let mut amostra = vec![None; lado * lado];
        let mut tela = vec![None; lado * lado];
        // O quadrado da ponta dentro da foto, por trechos de linha que não
        // saem de um tile: o tile da camada e o da seleção são procurados
        // uma vez por trecho, e não a cada pixel.
        let (x0, x1) = (ox.max(0), (ox + lado as i64).min(largura));
        let (y0, y1) = (oy.max(0), (oy + lado as i64).min(altura));
        for y in y0..y1 {
            let linha = (y - oy) as usize * lado;
            let y = y as u32;
            for (inicio, fim) in trechos(x0, x1) {
                let posicao = crate::tiles::tile_de(inicio, y);
                let tile = camada.tile(posicao);
                // A seleção: o tile dela (ou o valor do tile inteiro), e 0
                // além da largura e da altura dela — como no `Selecao::valor`.
                let mascara = self.selecao.as_deref().map(|s| {
                    let limite = if y < s.altura() { s.largura() } else { 0 };
                    (s.do_tile(posicao), limite)
                });
                for x in inicio..fim {
                    let j = linha + (x as i64 - ox) as usize;
                    let k = ((y % LADO_DO_TILE) * LADO_DO_TILE + x % LADO_DO_TILE) as usize;
                    let i = 4 * k;
                    let atual = tile.map_or([0; 4], |t| [t[i], t[i + 1], t[i + 2], t[i + 3]]);
                    tela[j] = Some(atual);
                    amostra[j] = match self.fonte.as_mut() {
                        Some(f) => f.no_lugar(x, y),
                        None => Some(atual),
                    };
                    let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                    let c = crate::SOBRE_255[self.tabela.em(dx * dx + dy * dy) as usize];
                    let m = match &mascara {
                        None => 1.0,
                        Some((_, limite)) if x >= *limite => 0.0,
                        Some((Ok(t), _)) => crate::SOBRE_255[t[k] as usize],
                        Some((Err(v), _)) => crate::SOBRE_255[*v as usize],
                    };
                    cobertura[j] = c * m * forca;
                }
            }
        }
        let antes = tela.clone();
        tinta.carimbar(
            &opcoes,
            lado,
            &cobertura,
            &amostra,
            &mut tela,
            self.alfa_travado,
        );
        self.misturador = Some((tinta, opcoes));
        // Grava o que mudou, trecho a trecho: o tile de antes (do desfazer)
        // e o tile para escrever saem uma vez por trecho que mudou — e só
        // nos que mudaram, como pixel a pixel.
        let mut caixa: Option<(u32, u32, u32, u32)> = None;
        for y in y0..y1 {
            let linha = (y - oy) as usize * lado;
            let y = y as u32;
            for (inicio, fim) in trechos(x0, x1) {
                let j = |x: u32| linha + (x as i64 - ox) as usize;
                let mudou = |x: &u32| tela[j(*x)] != antes[j(*x)];
                let Some(primeiro) = (inicio..fim).find(mudou) else {
                    continue;
                };
                let posicao = crate::tiles::tile_de(inicio, y);
                self.antes
                    .entry(posicao)
                    .or_insert_with(|| camada.tile(posicao).cloned());
                let tile = camada.tile_mut(posicao);
                let mut ultimo = primeiro;
                for x in (primeiro..fim).filter(mudou) {
                    let Some(novo) = tela[j(x)] else { continue };
                    let i = indice(x % LADO_DO_TILE, y % LADO_DO_TILE);
                    tile[i..i + 4].copy_from_slice(&novo);
                    ultimo = x;
                }
                caixa = Some(match caixa {
                    None => (primeiro, y, ultimo, y),
                    Some((xa, ya, xb, _)) => (xa.min(primeiro), ya, xb.max(ultimo), y),
                });
            }
        }
        caixa.map_or(Retangulo::default(), |(xa, ya, xb, yb)| {
            Retangulo::novo(xa, ya, xb - xa + 1, yb - ya + 1)
        })
    }

    /// A recuperação (`Ferramenta::Recuperacao`), no soltar: o traço já
    /// copiou a origem; aqui cada pixel tocado é refeito a partir do de antes
    /// do traço com a cor **adaptada** (`recuperacao::adaptar`), com a mesma
    /// cobertura. Devolve a região refeita.
    pub fn recuperar(&mut self, camada: &mut CamadaDePixels) -> Retangulo {
        let Some(fonte) = self.fonte.as_mut() else {
            return Retangulo::default();
        };
        if self.pincel.ferramenta != Ferramenta::Recuperacao {
            return Retangulo::default();
        }
        let (largura, altura) = (camada.largura(), camada.altura());
        // A caixa do traço, com folga para a borda e a suavização dela.
        let mut caixa = Retangulo::default();
        for (posicao, cob) in &self.cobertura {
            if cob.iter().any(|m| *m > 0) {
                caixa = caixa.uniao(&crate::tiles::retangulo_do_tile(*posicao, largura, altura));
            }
        }
        if caixa.vazio() {
            return caixa;
        }
        let folga = 2 + 3 * self.pincel.difusao.clamp(1, 7) as u32;
        let x0 = caixa.x.saturating_sub(folga);
        let y0 = caixa.y.saturating_sub(folga);
        let caixa = Retangulo::novo(
            x0,
            y0,
            (caixa.direita() + folga).min(largura) - x0,
            (caixa.baixo() + folga).min(altura) - y0,
        );
        let (w, h) = (caixa.largura as usize, caixa.altura as usize);
        if w * h > crate::recuperacao::LIMITE_DE_PIXELS || w < 3 || h < 3 {
            return Retangulo::default();
        }
        let cobertura_em = |x: u32, y: u32| -> u16 {
            let posicao = crate::tiles::tile_de(x, y);
            self.cobertura.get(&posicao).map_or(0, |c| {
                c[((y % LADO_DO_TILE) * LADO_DO_TILE + x % LADO_DO_TILE) as usize]
            })
        };
        let mut origem = Vec::with_capacity(w * h);
        let mut destino = Vec::with_capacity(w * h);
        let mut livre = Vec::with_capacity(w * h);
        let mut alfa = Vec::with_capacity(w * h);
        for y in caixa.y..caixa.baixo() {
            for x in caixa.x..caixa.direita() {
                let d = fonte.no_lugar(x, y).unwrap_or([0; 4]);
                let o = fonte.cor_com_alfa(x, y);
                let d3 = [d[0] as f32, d[1] as f32, d[2] as f32];
                origem.push(o.map_or(d3, |o| [o[0] as f32, o[1] as f32, o[2] as f32]));
                destino.push(d3);
                alfa.push(o.map_or(0, |o| o[3]));
                // A beira do retângulo é sempre borda.
                let na_beira = x == caixa.x
                    || y == caixa.y
                    || x + 1 == caixa.direita()
                    || y + 1 == caixa.baixo();
                livre.push(!na_beira && cobertura_em(x, y) > 0);
            }
        }
        let resultado = crate::recuperacao::adaptar_com(
            &origem,
            &destino,
            &livre,
            w,
            h,
            self.pincel.difusao,
            self.adaptacao,
        );
        let opacidade = self.pincel.opacidade.clamp(0.0, 1.0);
        let pincel = self.pincel;
        for (posicao, cob) in &self.cobertura {
            let antes = self.antes.get(posicao).cloned().flatten();
            let (tx0, ty0) = (
                posicao.0 as u32 * LADO_DO_TILE,
                posicao.1 as u32 * LADO_DO_TILE,
            );
            let tile = camada.tile_mut(*posicao);
            for (k, m) in cob.iter().enumerate() {
                if *m == 0 {
                    continue;
                }
                let (lx, ly) = (k as u32 % LADO_DO_TILE, k as u32 / LADO_DO_TILE);
                let (x, y) = (tx0 + lx, ty0 + ly);
                if x < caixa.x || y < caixa.y || x >= caixa.direita() || y >= caixa.baixo() {
                    continue;
                }
                let j = ((y - caixa.y) as usize) * w + (x - caixa.x) as usize;
                let a = *m as f32 / CHEIO as f32 * opacidade * alfa[j] as f32 / 255.0;
                let i = indice(lx, ly);
                let de_antes = match &antes {
                    Some(t) => [t[i], t[i + 1], t[i + 2], t[i + 3]],
                    None => [0; 4],
                };
                if a <= 0.0 || (self.alfa_travado && de_antes[3] == 0) {
                    continue;
                }
                let cor = resultado[j].map(|v| v.round() as u8);
                let novo = if self.alfa_travado {
                    let [r, g, b, alfa] = de_antes;
                    let [r, g, b, _] = pintar(&Pincel { cor, ..pincel }, [r, g, b, 255], a);
                    [r, g, b, alfa]
                } else {
                    pintar(&Pincel { cor, ..pincel }, de_antes, a)
                };
                tile[i..i + 4].copy_from_slice(&novo);
            }
        }
        caixa
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
            // Os três canais lado a lado, para o compilador fazer as contas
            // (e as divisões) juntas; a de cada canal é a de sempre.
            let v: [f32; 3] = std::array::from_fn(|k| {
                (pincel.cor[k] as f32 * a + antes[k] as f32 * alfa_antes * (1.0 - a)) / alfa
            });
            let [r, g, b] = v.map(crate::byte_arredondado);
            [r, g, b, quantizar(alfa)]
        }
    }
}

/// Uma ponta de área (em pixels) a partir da qual o carimbo divide a conta
/// entre os núcleos: abaixo dela, abrir as threads custa mais que o ganho.
const PIXELS_PARA_DIVIDIR: usize = 128 * 128;

/// Quantas linhas de um tile vão em cada faixa do carimbo dividido.
const LINHAS_POR_FAIXA: usize = 16;

/// O que o carimbo usa em cada pixel — o mesmo para todas as faixas.
#[derive(Clone, Copy)]
struct Ponta<'a> {
    pincel: Pincel,
    tabela: &'a Tabela,
    alfa_travado: bool,
    centro: (f32, f32),
    fluxo: f32,
    opacidade: f32,
    /// A área do carimbo na foto: `x0..x1`, `y0..y1`.
    xs: (u32, u32),
    ys: (u32, u32),
}

impl Ponta<'_> {
    /// Carimba a parte da área que cai no tile em `posicao`, nas linhas de
    /// `tile` e `cobertura` — que começam na linha `primeira` do tile (0 e o
    /// tile inteiro, numa thread só). `mascara` e `antes` são do tile
    /// inteiro.
    #[allow(clippy::too_many_arguments)]
    fn no_tile(
        &self,
        fonte: Option<&Fonte>,
        posicao: Posicao,
        primeira: u32,
        mascara: Option<Result<&[u8], u8>>,
        antes: Option<&[u8]>,
        tile: &mut [u8],
        cobertura: &mut [u16],
    ) {
        let (cx, cy) = self.centro;
        let (tx0, ty0) = (
            posicao.0 as u32 * LADO_DO_TILE,
            posicao.1 as u32 * LADO_DO_TILE,
        );
        let linhas = cobertura.len() as u32 / LADO_DO_TILE;
        let (px0, px1) = (self.xs.0.max(tx0), self.xs.1.min(tx0 + LADO_DO_TILE));
        let (py0, py1) = (
            self.ys.0.max(ty0 + primeira),
            self.ys.1.min(ty0 + primeira + linhas),
        );
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
                // A cobertura e o tile para escrever, nas linhas desta faixa.
                let kf = k - (primeira * LADO_DO_TILE) as usize;
                // m ← m + fluxo·c·(1 − m), em 16 bits.
                let m = cobertura[kf] as f32 / CHEIO as f32;
                let novo_m = m + self.fluxo * crate::SOBRE_255[c as usize] * (1.0 - m);
                let q = crate::arredondado(novo_m * CHEIO as f32).min(CHEIO as f32) as u16;
                if q <= cobertura[kf] {
                    continue;
                }
                cobertura[kf] = q;
                let a = q as f32 / CHEIO as f32 * self.opacidade;
                let i = indice(lx, ly);
                let de_antes = match antes {
                    Some(t) => [t[i], t[i + 1], t[i + 2], t[i + 3]],
                    None => [0; 4],
                };
                let alfa_de_antes = de_antes[3];
                let de_antes = match self.alfa_travado {
                    true if alfa_de_antes == 0 => continue,
                    // A conta como se o pixel fosse opaco: só a cor muda.
                    true => [de_antes[0], de_antes[1], de_antes[2], 255],
                    false => de_antes,
                };
                let mut novo = match (self.pincel.ferramenta, fonte) {
                    (Ferramenta::Carimbo | Ferramenta::Recuperacao, Some(fonte)) => {
                        match fonte.ler_cor_com_alfa(px, py) {
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
                        }
                    }
                    (f, Some(fonte)) if f.le_a_foto() => {
                        let (Some(foto), Some(suave)) =
                            (fonte.ler_cor(px, py), fonte.ler_desfocada(px, py))
                        else {
                            continue;
                        };
                        let cor = cor_da_ferramenta(f, foto, suave);
                        aplicar_alfa(&Pincel { cor, ..self.pincel }, de_antes, a)
                    }
                    _ => pintar(&self.pincel, de_antes, a),
                };
                if self.alfa_travado {
                    novo[3] = alfa_de_antes;
                }
                let i = indice(lx, ly - primeira);
                tile[i..i + 4].copy_from_slice(&novo);
            }
        }
    }
}

/// Os valores destas chaves (que existem e não se repetem), todos de uma
/// vez, na ordem pedida.
fn varios_mut<'a, V>(mapa: &'a mut BTreeMap<Posicao, V>, chaves: &[Posicao]) -> Vec<&'a mut V> {
    let mut achados: Vec<Option<&mut V>> = chaves.iter().map(|_| None).collect();
    for (chave, valor) in mapa.iter_mut() {
        if let Some(k) = chaves.iter().position(|c| c == chave) {
            achados[k] = Some(valor);
        }
    }
    achados
        .into_iter()
        .map(|v| v.expect("a chave existe"))
        .collect()
}

/// Os trechos `[início, fim)` de `x0..x1` (com `0 ≤ x0`) que não saem de uma
/// coluna de tiles.
fn trechos(x0: i64, x1: i64) -> impl Iterator<Item = (u32, u32)> {
    let lado = LADO_DO_TILE as i64;
    let mut x = x0;
    std::iter::from_fn(move || {
        (x < x1).then(|| {
            let fim = ((x / lado + 1) * lado).min(x1);
            let trecho = (x as u32, fim as u32);
            x = fim;
            trecho
        })
    })
}

/// `(alfa · 255).round().clamp(0, 255)`, sem a libm (`byte_arredondado`).
fn quantizar(alfa: f32) -> u8 {
    crate::byte_arredondado(alfa * 255.0)
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

    /// `Tabela::em` de antes de 2026-10-22: o `round` da libm.
    fn em_antigo(tabela: &Tabela, d2: f32) -> u8 {
        let t = d2 / (tabela.alcance * tabela.alcance);
        if t >= 1.0 {
            return 0;
        }
        tabela.valores[(t * TAMANHO_DA_TABELA as f32).round() as usize]
    }

    /// `aplicar_alfa` de antes de 2026-10-22: canal a canal, com o `round` da
    /// libm.
    fn aplicar_alfa_antigo(pincel: &Pincel, antes: [u8; 4], a: f32) -> [u8; 4] {
        let quantizar = |alfa: f32| (alfa * 255.0).round().clamp(0.0, 255.0) as u8;
        let alfa_antes = antes[3] as f32 / 255.0;
        match pincel.ferramenta {
            Ferramenta::Borracha => {
                let alfa = alfa_antes * (1.0 - a);
                [antes[0], antes[1], antes[2], quantizar(alfa)]
            }
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

    fn pintar_antigo(pincel: &Pincel, antes: [u8; 4], a: f32) -> [u8; 4] {
        if pincel.modo == Modo::Normal || pincel.ferramenta == Ferramenta::Borracha {
            return aplicar_alfa_antigo(pincel, antes, a);
        }
        let [r, g, b] = pincel.cor;
        crate::mesclagem::mesclar_em_camada(antes, [r, g, b, 255], a, pincel.modo)
    }

    /// `Traco::carimbar` de antes de 2026-10-22 (fora o Misturador): o
    /// `round` da libm na tabela, na cobertura e nos canais, e `c / 255` pela
    /// divisão.
    fn carimbar_antigo(t: &mut Traco, camada: &mut CamadaDePixels, cx: f32, cy: f32) -> Retangulo {
        let raio = t.tabela.alcance;
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
            t.antes
                .entry(posicao)
                .or_insert_with(|| camada.tile(posicao).cloned());
            let antes = t.antes.get(&posicao).cloned().flatten();
            let cobertura = t
                .cobertura
                .entry(posicao)
                .or_insert_with(|| vec![0; (LADO_DO_TILE * LADO_DO_TILE) as usize]);
            let fluxo = t.pincel.fluxo.clamp(0.0, 1.0);
            let opacidade = t.pincel.opacidade.clamp(0.0, 1.0);
            let mascara = t.selecao.as_ref().map(|s| s.do_tile(posicao));
            if let Some(Err(0)) = mascara {
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
                    let mut c = em_antigo(&t.tabela, dx * dx + dy * dy);
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
                    let alfa_de_antes = de_antes[3];
                    let de_antes = match t.alfa_travado {
                        true if alfa_de_antes == 0 => continue,
                        true => [de_antes[0], de_antes[1], de_antes[2], 255],
                        false => de_antes,
                    };
                    let mut novo = match (t.pincel.ferramenta, t.fonte.as_mut()) {
                        (Ferramenta::Carimbo | Ferramenta::Recuperacao, Some(fonte)) => {
                            match fonte.cor_com_alfa(px, py) {
                                Some([r, g, b, alfa]) => {
                                    let a = a * alfa as f32 / 255.0;
                                    if a <= 0.0 {
                                        continue;
                                    }
                                    pintar_antigo(
                                        &Pincel {
                                            cor: [r, g, b],
                                            ..t.pincel
                                        },
                                        de_antes,
                                        a,
                                    )
                                }
                                None => continue,
                            }
                        }
                        (f, Some(fonte)) if f.le_a_foto() => {
                            let (Some(foto), Some(suave)) =
                                (fonte.cor(px, py), fonte.desfocada(px, py))
                            else {
                                continue;
                            };
                            let cor = cor_da_ferramenta(f, foto, suave);
                            aplicar_alfa_antigo(&Pincel { cor, ..t.pincel }, de_antes, a)
                        }
                        _ => pintar_antigo(&t.pincel, de_antes, a),
                    };
                    if t.alfa_travado {
                        novo[3] = alfa_de_antes;
                    }
                    tile[i..i + 4].copy_from_slice(&novo);
                }
            }
        }
        area
    }

    /// Uma camada de 700 × 520 com cor e alfa variando (zero inclusive), uns
    /// tiles cheios e outros que não existem.
    fn camada_variada() -> CamadaDePixels {
        let mut c = CamadaDePixels::nova(700, 520);
        for posicao in [(0, 0), (1, 0), (1, 1), (2, 1), (0, 2)] {
            let (tx, ty) = (posicao.0 as u32 * 256, posicao.1 as u32 * 256);
            for (k, p) in c
                .tile_mut(posicao)
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .enumerate()
            {
                let (x, y) = (tx + k as u32 % 256, ty + k as u32 / 256);
                let alfa = if (x / 29 + y / 31) % 4 == 0 {
                    0
                } else {
                    (x * 3 + y) as u8 | 1
                };
                *p = [(x * 5) as u8, (y * 3) as u8, (x ^ y) as u8, alfa];
            }
        }
        c
    }

    /// 🔑 **O carimbo de agora pinta o que o de antes pintava, byte a byte**,
    /// carimbo a carimbo — a camada, a cobertura do traço e os tiles de antes,
    /// e o passo do desfazer no fim —, com todas as ferramentas que passam por
    /// ele (o pincel, a borracha, o carimbo, a recuperação, as de tom e as de
    /// foco), pontas pequena e dura, média e grande e macia, os modos Normal
    /// e Multiplicação com fluxo e opacidade cheios e parciais, sem seleção e
    /// com uma elipse difusa (e invertida, e num canto), com e sem o alfa
    /// travado, carimbos que saem da foto, e a ponta grande numa thread e
    /// dividida em três.
    #[test]
    fn o_carimbo_e_o_de_antes() {
        use crate::documento::{BaseRef, Documento};
        use crate::selecao::{Acabamento, Forma};
        let (l, a) = (700, 520);
        let base = Arc::new(image::RgbImage::from_fn(l, a, |x, y| {
            image::Rgb([(x * 3) as u8, (y * 7) as u8, (x ^ y) as u8])
        }));
        let doc = Documento::novo(BaseRef::da_imagem(&base));
        let elipse = Arc::new(Selecao::da_forma_com(
            l,
            a,
            &Forma::Elipse(Retangulo::novo(80, 60, 520, 380)),
            Acabamento {
                suavizar: true,
                difusao: 8,
            },
        ));
        let mut invertida = (*elipse).clone();
        invertida.inverter();
        let invertida = Arc::new(invertida);
        // Uma seleção num canto: com a ponta grande, quase todo tile fica
        // inteiro fora dela.
        let canto = Arc::new(Selecao::da_forma_com(
            l,
            a,
            &Forma::Elipse(Retangulo::novo(40, 30, 160, 170)),
            Acabamento {
                suavizar: true,
                difusao: 8,
            },
        ));
        let ferramentas = [
            Ferramenta::Pincel,
            Ferramenta::Borracha,
            Ferramenta::Carimbo,
            Ferramenta::Recuperacao,
            Ferramenta::Subexposicao(Faixa::MeiosTons),
            Ferramenta::Superexposicao(Faixa::Realces),
            Ferramenta::Desfoque,
            Ferramenta::Nitidez,
        ];
        let centros = [
            (350.0, 260.0),
            (352.3, 261.7),
            (100.0, 400.0),
            (690.0, 10.0),
            (-30.0, 250.0),
            (500.5, 300.25),
        ];
        for ferramenta in ferramentas {
            for (raio, dureza) in [(3.5, 1.0), (40.0, 0.8), (190.0, 0.0)] {
                for (modo, fluxo, opacidade) in
                    [(Modo::Normal, 1.0, 1.0), (Modo::Multiplicacao, 0.07, 0.6)]
                {
                    // A ponta grande se divide entre as threads: numa e em
                    // três (com a fonte, as três leem a mesma).
                    let grande = raio > 100.0;
                    let dividida = grande && !ferramenta.le_a_foto();
                    let selecoes = if dividida {
                        vec![
                            None,
                            Some(elipse.clone()),
                            Some(invertida.clone()),
                            Some(canto.clone()),
                        ]
                    } else if raio > 100.0 {
                        vec![None, Some(elipse.clone())]
                    } else {
                        vec![None, Some(elipse.clone()), Some(invertida.clone())]
                    };
                    let combinacoes: &[(bool, usize)] = if dividida {
                        &[(false, 1), (true, 1), (false, 3), (true, 3)]
                    } else if grande {
                        &[(false, 1), (true, 1), (false, 3)]
                    } else {
                        &[(false, 1), (true, 1)]
                    };
                    for selecao in selecoes {
                        for &(travado, nucleos) in combinacoes {
                            let pincel = Pincel {
                                ferramenta,
                                raio,
                                dureza,
                                modo,
                                fluxo,
                                opacidade,
                                cor: [200, 40, 90],
                                ..Pincel::default()
                            };
                            let traco = || {
                                let mut t = Traco::novo(pincel)
                                    .dentro_de(selecao.clone())
                                    .com_alfa_travado(travado);
                                if ferramenta.le_a_foto() {
                                    let deslocamento = if ferramenta.copia_da_origem() {
                                        (-120.0, 35.0)
                                    } else {
                                        (0.0, 0.0)
                                    };
                                    t = t.copiando_de(Fonte::nova(
                                        base.clone(),
                                        &doc,
                                        0,
                                        deslocamento,
                                    ));
                                }
                                t
                            };
                            let (mut novo, mut antigo) = (traco(), traco());
                            let (mut c_novo, mut c_antigo) = (camada_variada(), camada_variada());
                            let caso = format!(
                                "{ferramenta:?} raio {raio} {modo:?} seleção {} travado {travado} \
                                 em {nucleos} thread(s)",
                                selecao.is_some()
                            );
                            for (cx, cy) in centros {
                                let sujo = novo.carimbar_com(&mut c_novo, cx, cy, nucleos);
                                let sujo_antes =
                                    carimbar_antigo(&mut antigo, &mut c_antigo, cx, cy);
                                assert_eq!(sujo, sujo_antes, "{caso} em ({cx}, {cy})");
                                assert!(c_novo == c_antigo, "{caso} em ({cx}, {cy})");
                                assert!(
                                    novo.cobertura == antigo.cobertura
                                        && novo.antes == antigo.antes,
                                    "{caso} em ({cx}, {cy}): o estado do traço"
                                );
                            }
                            assert!(
                                novo.terminar(&mut c_novo) == antigo.terminar(&mut c_antigo),
                                "{caso}: o passo do desfazer"
                            );
                            assert!(c_novo == c_antigo, "{caso}: a camada no fim");
                        }
                    }
                }
            }
        }
    }

    /// 🧵 **A ponta grande dividida entre as threads pinta o que uma thread
    /// só pinta**, carimbo a carimbo: centros que levam o começo e o fim da
    /// área a cada linha de uma faixa (e de um tile), ponta dura e macia,
    /// com e sem seleção, o pincel e as ferramentas que leem a foto (o
    /// carimbo, de outro lugar; o desfoque, no lugar), em três e em sete
    /// threads.
    #[test]
    fn a_ponta_dividida_e_a_de_uma_thread() {
        use crate::documento::{BaseRef, Documento};
        use crate::selecao::{Acabamento, Forma};
        let (l, a) = (700, 520);
        let base = Arc::new(image::RgbImage::from_fn(l, a, |x, y| {
            image::Rgb([(x * 3) as u8, (y * 7) as u8, (x ^ y) as u8])
        }));
        let doc = Documento::novo(BaseRef::da_imagem(&base));
        let elipse = Arc::new(Selecao::da_forma_com(
            l,
            a,
            &Forma::Elipse(Retangulo::novo(80, 60, 520, 380)),
            Acabamento {
                suavizar: true,
                difusao: 8,
            },
        ));
        let casos = [
            (Ferramenta::Pincel, 70.0, 1.0),
            (Ferramenta::Pincel, 150.0, 0.5),
            (Ferramenta::Carimbo, 90.0, 0.7),
            (Ferramenta::Desfoque, 90.0, 0.7),
        ];
        for (ferramenta, raio, dureza) in casos {
            for selecao in [None, Some(elipse.clone())] {
                for nucleos in [3, 7] {
                    let pincel = Pincel {
                        ferramenta,
                        raio,
                        dureza,
                        cor: [200, 40, 90],
                        ..Pincel::default()
                    };
                    let traco = || {
                        let t = Traco::novo(pincel).dentro_de(selecao.clone());
                        match ferramenta {
                            Ferramenta::Carimbo => {
                                t.copiando_de(Fonte::nova(base.clone(), &doc, 0, (-120.0, 35.0)))
                            }
                            Ferramenta::Desfoque => {
                                t.copiando_de(Fonte::nova(base.clone(), &doc, 0, (0.0, 0.0)))
                            }
                            _ => t,
                        }
                    };
                    let (mut um, mut varios) = (traco(), traco());
                    let (mut c_um, mut c_varios) = (camada_variada(), camada_variada());
                    for k in 0..25 {
                        let (cx, cy) = (200.0 + k as f32 * 11.3, 230.0 + k as f32 * 0.71);
                        let caso = format!(
                            "{ferramenta:?} raio {raio} em {nucleos} threads, ({cx}, {cy})"
                        );
                        let sujo = varios.carimbar_com(&mut c_varios, cx, cy, nucleos);
                        assert_eq!(sujo, um.carimbar_com(&mut c_um, cx, cy, 1), "{caso}");
                        assert!(c_varios == c_um, "{caso}");
                        assert!(
                            varios.cobertura == um.cobertura && varios.antes == um.antes,
                            "{caso}: o estado do traço"
                        );
                    }
                }
            }
        }
    }

    /// `Traco::carimbar_misturando` de antes de 2026-10-22: pixel a pixel,
    /// com a camada, a seleção, o tile de antes e o de escrever procurados a
    /// cada um, e a tinta de antes.
    fn carimbar_misturando_antigo(
        t: &mut Traco,
        camada: &mut CamadaDePixels,
        cx: f32,
        cy: f32,
    ) -> Retangulo {
        let Some((mut tinta, opcoes)) = t.misturador.take() else {
            return Retangulo::default();
        };
        let r = t.tabela.alcance.ceil() as i64;
        let lado = (2 * r + 1) as usize;
        let (ox, oy) = (cx.floor() as i64 - r, cy.floor() as i64 - r);
        let (largura, altura) = (camada.largura() as i64, camada.altura() as i64);
        let forca = t.pincel.fluxo.clamp(0.0, 1.0) * t.pincel.opacidade.clamp(0.0, 1.0);
        let mut cobertura = vec![0f32; lado * lado];
        let mut amostra = vec![None; lado * lado];
        let mut tela = vec![None; lado * lado];
        for j in 0..lado * lado {
            let (x, y) = (ox + (j % lado) as i64, oy + (j / lado) as i64);
            if x < 0 || y < 0 || x >= largura || y >= altura {
                continue;
            }
            let (x, y) = (x as u32, y as u32);
            let atual = camada.pixel(x, y);
            tela[j] = Some(atual);
            amostra[j] = match t.fonte.as_mut() {
                Some(f) => f.no_lugar(x, y),
                None => Some(atual),
            };
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let c = em_antigo(&t.tabela, dx * dx + dy * dy) as f32 / 255.0;
            let m = t
                .selecao
                .as_ref()
                .map_or(1.0, |s| s.valor(x, y) as f32 / 255.0);
            cobertura[j] = c * m * forca;
        }
        let antes = tela.clone();
        tinta.carimbar_antigo(
            &opcoes,
            lado,
            &cobertura,
            &amostra,
            &mut tela,
            t.alfa_travado,
        );
        t.misturador = Some((tinta, opcoes));
        let mut sujo = Retangulo::default();
        for j in 0..lado * lado {
            let (Some(novo), Some(velho)) = (tela[j], antes[j]) else {
                continue;
            };
            if novo == velho {
                continue;
            }
            let (x, y) = (
                (ox + (j % lado) as i64) as u32,
                (oy + (j / lado) as i64) as u32,
            );
            let posicao = crate::tiles::tile_de(x, y);
            t.antes
                .entry(posicao)
                .or_insert_with(|| camada.tile(posicao).cloned());
            let i = indice(x % LADO_DO_TILE, y % LADO_DO_TILE);
            camada.tile_mut(posicao)[i..i + 4].copy_from_slice(&novo);
            sujo = sujo.uniao(&Retangulo::novo(x, y, 1, 1));
        }
        sujo
    }

    /// 🔑 **O Misturador de agora pinta o que o de antes pintava, byte a
    /// byte**, carimbo a carimbo — a camada, os tiles de antes, a tinta que
    /// segue no pincel e o passo do desfazer —, com umidade, carga e mistura
    /// variadas, amostrando a camada ou a foto composta, o pincel limpo e o
    /// carregado, pontas pequena, média e grande, fluxo e opacidade cheios e
    /// parciais, sem seleção, com uma elipse difusa (e invertida) e com uma
    /// seleção menor que a camada (e invertida: fora dela não se pinta), com
    /// e sem o alfa travado, e carimbos que saem da foto (ou caem fora
    /// dela).
    #[test]
    fn o_misturador_e_o_de_antes() {
        use crate::carimbo::AmostraDoCarimbo;
        use crate::documento::{BaseRef, Camada, Documento};
        use crate::misturador::{OpcoesDoMisturador, Tinta};
        use crate::selecao::{Acabamento, Forma};
        let (l, a) = (700, 520);
        let base = Arc::new(image::RgbImage::from_fn(l, a, |x, y| {
            image::Rgb([(x * 3) as u8, (y * 7) as u8, (x ^ y) as u8])
        }));
        let mut doc = Documento::novo(BaseRef::da_imagem(&base));
        let mut camada = Camada::nova("Pintada", l, a);
        camada.pixels = camada_variada();
        doc.camadas.push(camada);
        let difusa = Acabamento {
            suavizar: true,
            difusao: 8,
        };
        let elipse = Selecao::da_forma_com(
            l,
            a,
            &Forma::Elipse(Retangulo::novo(80, 60, 520, 380)),
            difusa,
        );
        let pequena = Selecao::da_forma_com(
            500,
            400,
            &Forma::Elipse(Retangulo::novo(200, 150, 400, 300)),
            difusa,
        );
        let invertida = |s: &Selecao| {
            let mut s = s.clone();
            s.inverter();
            Arc::new(s)
        };
        let (elipse_invertida, pequena_invertida) = (invertida(&elipse), invertida(&pequena));
        let (elipse, pequena) = (Arc::new(elipse), Arc::new(pequena));
        let opcoes = [
            OpcoesDoMisturador::default(),
            OpcoesDoMisturador {
                umidade: 1.0,
                carga: 0.0,
                mistura: 1.0,
                ..OpcoesDoMisturador::default()
            },
            OpcoesDoMisturador {
                umidade: 0.0,
                carga: 1.0,
                mistura: 0.0,
                ..OpcoesDoMisturador::default()
            },
            OpcoesDoMisturador {
                umidade: 0.3,
                carga: 0.2,
                mistura: 0.8,
                todas_as_camadas: true,
                ..OpcoesDoMisturador::default()
            },
        ];
        let centros = [
            (350.0, 260.0),
            (352.3, 261.7),
            (100.0, 400.0),
            (-900.0, -900.0),
            (690.0, 10.0),
            (-30.0, 250.0),
            (500.5, 300.25),
        ];
        for (raio, dureza) in [(3.5, 1.0), (40.0, 0.6), (120.0, 0.0)] {
            let grande = raio > 100.0;
            let selecoes = if grande {
                vec![None, Some(pequena_invertida.clone())]
            } else {
                vec![
                    None,
                    Some(elipse.clone()),
                    Some(elipse_invertida.clone()),
                    Some(pequena.clone()),
                    Some(pequena_invertida.clone()),
                ]
            };
            let tintas = if grande {
                &[true][..]
            } else {
                &[true, false][..]
            };
            let forcas = if grande {
                &[(1.0, 1.0)][..]
            } else {
                &[(1.0, 1.0), (0.4, 0.7)][..]
            };
            for opcoes in opcoes {
                for selecao in &selecoes {
                    for &carregada in tintas {
                        for &(fluxo, opacidade) in forcas {
                            for travado in [false, true] {
                                let pincel = Pincel {
                                    ferramenta: Ferramenta::Misturador,
                                    raio,
                                    dureza,
                                    fluxo,
                                    opacidade,
                                    ..Pincel::default()
                                };
                                let traco = || {
                                    let mut tinta = Tinta::default();
                                    if carregada {
                                        tinta.carregar([200, 40, 90]);
                                    }
                                    let mut t = Traco::novo(pincel)
                                        .dentro_de(selecao.clone())
                                        .com_alfa_travado(travado)
                                        .misturando(tinta, opcoes);
                                    if opcoes.todas_as_camadas {
                                        t = t.copiando_de(Fonte::da_amostra(
                                            base.clone(),
                                            &doc,
                                            0,
                                            AmostraDoCarimbo::Todas,
                                            (0.0, 0.0),
                                        ));
                                    }
                                    t
                                };
                                let (mut novo, mut antigo) = (traco(), traco());
                                let (mut c_novo, mut c_antigo) =
                                    (camada_variada(), camada_variada());
                                let caso = format!(
                                    "raio {raio} {opcoes:?} seleção {} carregada {carregada} \
                                     fluxo {fluxo} travado {travado}",
                                    selecao.is_some()
                                );
                                for (cx, cy) in centros {
                                    let sujo = novo.carimbar(&mut c_novo, cx, cy);
                                    let sujo_antes = carimbar_misturando_antigo(
                                        &mut antigo,
                                        &mut c_antigo,
                                        cx,
                                        cy,
                                    );
                                    assert_eq!(sujo, sujo_antes, "{caso} em ({cx}, {cy})");
                                    assert!(c_novo == c_antigo, "{caso} em ({cx}, {cy})");
                                    assert!(
                                        novo.antes == antigo.antes,
                                        "{caso} em ({cx}, {cy}): os tiles de antes"
                                    );
                                    assert_eq!(
                                        novo.tinta(),
                                        antigo.tinta(),
                                        "{caso} em ({cx}, {cy}): a tinta"
                                    );
                                }
                                assert!(
                                    novo.terminar(&mut c_novo) == antigo.terminar(&mut c_antigo),
                                    "{caso}: o passo do desfazer"
                                );
                                assert!(c_novo == c_antigo, "{caso}: a camada no fim");
                            }
                        }
                    }
                }
            }
        }
    }

    /// A conta de cada pixel, sozinha: todo alfa de antes, cores variadas e
    /// a cobertura de 0 a 1 em passos finos (inclusive os que caem no meio
    /// de um byte), no pincel e na borracha.
    #[test]
    fn a_conta_do_pixel_e_a_de_antes() {
        for ferramenta in [Ferramenta::Pincel, Ferramenta::Borracha] {
            for alfa in 0..=255u8 {
                for k in 0..=600u32 {
                    let a = k as f32 / 600.0;
                    let cor = [(k * 7) as u8, (k * 13) as u8 ^ alfa, 255 - alfa];
                    let antes = [alfa.wrapping_mul(3), (k % 256) as u8, 128, alfa];
                    let pincel = Pincel {
                        ferramenta,
                        cor,
                        ..Pincel::default()
                    };
                    assert_eq!(
                        aplicar_alfa(&pincel, antes, a),
                        aplicar_alfa_antigo(&pincel, antes, a),
                        "{ferramenta:?} alfa {alfa} a {a}"
                    );
                }
            }
        }
    }

    /// 🔑 **O `arredondado` é o `round`, bit a bit.** Os 2³² `f32` foram
    /// conferidos à parte (4 s em release, quatro núcleos); aqui vai um a cada
    /// 4099 padrões de bits (todo expoente, os dois sinais, NaN e infinito),
    /// cada meio de −3 a 70 000 com os vizinhos dele, e as bordas do 2²³.
    #[test]
    fn o_arredondado_e_o_round() {
        let igual = |x: f32| {
            let (antes, agora) = (x.round(), crate::arredondado(x));
            antes.to_bits() == agora.to_bits() || (antes.is_nan() && agora.is_nan())
        };
        for bits in (0..=u32::MAX).step_by(4099) {
            let x = f32::from_bits(bits);
            assert!(igual(x), "{x:e} ({bits:#x})");
        }
        for n in -3..=70_000 {
            for x in [n as f32, n as f32 + 0.5] {
                for x in [x.next_down(), x, x.next_up()] {
                    assert!(igual(x), "{x:e}");
                }
            }
        }
        for x in [
            0.0,
            -0.0,
            f32::MIN_POSITIVE,
            0.499_999_97,
            8_388_607.5,
            8_388_608.0,
            8_388_608.0f32.next_down(),
            f32::INFINITY,
            f32::NAN,
        ] {
            assert!(igual(x), "{x:e}");
        }
    }
}
