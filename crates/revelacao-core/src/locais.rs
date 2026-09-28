//! A revelação local: máscaras (pincel, gradientes) com os ajustes delas, e os
//! retoques (Clone e Heal) — tudo como **parâmetro**, nunca como pixel.
//!
//! ## 🔑 A fonte de verdade é esta struct, e não uma textura
//!
//! O bruto não muda, e os pixels de origem também não. O que se guarda é o gesto:
//! cada stroke com seus pontos, cada gradiente com suas alças, cada carimbo com
//! origem e caminho. As texturas de máscara (`R8Unorm`) e as intermediárias do
//! retoque são **cache** — o motor as refaz a partir daqui, no tamanho da imagem
//! que está revelando. É o que deixa a exportação em 6016 px sair com a mesma
//! máscara do preview em 2048 px, sem ampliar bitmap nenhum (é o mesmo contrato
//! da foto: bruto, parâmetros, revelada e caches nunca se substituem).
//!
//! ## Coordenadas e unidades
//!
//! - **Posição**: `x`, `y` em `0..1` da **foto inteira de pé** — a imagem que
//!   entra no shader, antes de espelho, giro de 90°, endireitamento e recorte
//!   (`transformacao::aplicar` vem depois). Por isso cortar ou girar não tira a
//!   máscara do lugar: ela é da foto, não do quadro.
//! - **Raio**: fração do **maior lado** da foto inteira —
//!   `raio_px = raio × max(largura, altura)`. É isotrópico em pixels (círculo
//!   continua círculo numa foto 3:2) e é a mesma régua de
//!   `Motor::definir_escala_do_original`: o mesmo stroke cobre o mesmo pedaço da
//!   foto em qualquer resolução.
//! - **Feather**: fração do raio (`0..1`) em que a borda cai de 1 a 0, por
//!   `smoothstep`. Zero é borda dura.
//! - **Pressão**: multiplica a intensidade do trecho (`0..1`); não muda o raio.
//!   Quem não tem pressão (mouse) manda 1.
//!
//! ## 🚨 Um stroke não acumula consigo mesmo
//!
//! Os trechos entre dois pontos são cápsulas, e dentro de um stroke elas se
//! combinam por **máximo**: passar duas vezes no mesmo lugar, no mesmo gesto, não
//! escurece mais. Só o stroke pronto entra na camada, com a opacidade dele:
//!
//! ```text
//! Somar     m = s·op + m·(1 − s·op)
//! Subtrair  m = m·(1 − s·op)
//! ```
//!
//! Strokes diferentes compõem entre si (o segundo gesto por cima do primeiro
//! soma), que é o que o pincel do Lightroom faz.
//!
//! ## A referência em CPU
//!
//! [`mascara_em_cpu`] é a especificação executável da máscara: o shader
//! (`shaders/mascara.wgsl`) faz a mesma conta, e os testes do motor comparam os
//! dois. Ela **não** é caminho de produção — sem GPU a exportação falha, como
//! sempre falhou (ver `Motor::abrir`).

use serde::{Deserialize, Serialize};

/// A versão do formato. Uma revelação de versão maior é recusada por
/// [`ParametrosLocais::de_json`], e quem abre a foto a guarda intacta.
pub const VERSAO: u32 = 1;
/// Quantas camadas de máscara uma foto pode ter — o tamanho do `array` do WGSL.
pub const MAXIMO_DE_CAMADAS: usize = 8;
/// Quantos componentes cabem numa camada.
pub const MAXIMO_DE_COMPONENTES: usize = 512;
/// Quantos pontos cabem num stroke ou num caminho de retoque.
pub const MAXIMO_DE_PONTOS: usize = 4096;
/// Quantos retoques cabem numa foto.
pub const MAXIMO_DE_RETOQUES: usize = 256;
/// A faixa da exposição local, em EV.
pub const EXPOSICAO_MAXIMA: f32 = 5.0;
/// O menor raio aceito (fração do maior lado) — abaixo disso é menos de um
/// pixel numa foto de 6000 px.
pub const RAIO_MINIMO: f32 = 0.0002;

/// Tudo o que é local numa foto: as camadas de máscara e os retoques.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParametrosLocais {
    pub versao: u32,
    #[serde(default)]
    pub camadas: Vec<Camada>,
    #[serde(default)]
    pub retoques: Vec<Retoque>,
}

impl Default for ParametrosLocais {
    fn default() -> Self {
        Self {
            versao: VERSAO,
            camadas: Vec::new(),
            retoques: Vec::new(),
        }
    }
}

/// Uma máscara e o que ela aplica.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camada {
    /// O nome na lista da Revelação local ("Máscara 1").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub nome: String,
    /// Oculta (o olho fechado): fica na revelação, e o motor não a aplica.
    #[serde(default = "verdadeiro", skip_serializing_if = "e_verdadeiro")]
    pub visivel: bool,
    #[serde(default)]
    pub ajustes: AjustesLocais,
    #[serde(default)]
    pub componentes: Vec<Componente>,
    /// A máscara invertida: o ajuste vale **fora** do que foi pintado.
    #[serde(default)]
    pub invertida: bool,
}

impl Default for Camada {
    fn default() -> Self {
        Self {
            nome: String::new(),
            visivel: true,
            ajustes: AjustesLocais::default(),
            componentes: Vec::new(),
            invertida: false,
        }
    }
}

fn verdadeiro() -> bool {
    true
}

fn e_verdadeiro(v: &bool) -> bool {
    *v
}

/// O tamanho máximo do nome de uma máscara, em caracteres.
pub const MAXIMO_DO_NOME: usize = 80;

/// Os ajustes que uma camada aplica onde a máscara vale.
///
/// Começa só com a exposição; os próximos entram aqui, com `serde(default)`
/// no neutro, e a versão do formato não precisa mudar.
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AjustesLocais {
    /// Em EV, aplicada em RGB **linear**: `+1` dobra a luz onde a máscara é 1.
    pub exposicao_ev: f32,
}

/// Se o componente acrescenta à máscara ou apaga dela.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modo {
    #[default]
    Somar,
    Subtrair,
}

/// Um pedaço da máscara: a forma e o modo.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Componente {
    #[serde(default)]
    pub modo: Modo,
    #[serde(flatten)]
    pub forma: Forma,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Forma {
    Pincel(BrushStroke),
    Linear(GradienteLinear),
    Radial(GradienteRadial),
    Laco(Laco),
}

/// Um laço: o polígono fechado que o operador cercou — livre (arrastando) ou
/// poligonal (clique a clique), é o mesmo polígono.
///
/// O último ponto se liga ao primeiro. A borda é suave por **distância à
/// borda**: `feather` é a largura da transição, em fração do maior lado da foto
/// (a mesma régua do raio do pincel), centrada na linha do laço — metade para
/// dentro, metade para fora. Zero é borda dura.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Laco {
    pub pontos: Vec<[f32; 2]>,
    #[serde(default)]
    pub feather: f32,
}

/// Quantos vértices o laço leva à GPU — o tamanho do `array` do WGSL (dois por
/// `vec4`). Um laço livre tem centenas de pontos; ele é simplificado
/// (Douglas–Peucker) até caber, com a menor tolerância que basta.
pub const VERTICES_DO_LACO: usize = 256;

/// O laço em pixels de uma imagem `largura × altura`, simplificado até
/// [`VERTICES_DO_LACO`]. A GPU e a referência em CPU desenham **este**
/// polígono, e por isso concordam.
pub fn laco_em_pixels(pontos: &[[f32; 2]], largura: u32, altura: u32) -> Vec<[f32; 2]> {
    let (w, h) = (largura as f32, altura as f32);
    let px: Vec<[f32; 2]> = pontos.iter().map(|p| [p[0] * w, p[1] * h]).collect();
    let mut tolerancia = 0.5;
    loop {
        let simples = douglas_peucker(&px, tolerancia);
        if simples.len() <= VERTICES_DO_LACO {
            return simples;
        }
        tolerancia *= 2.0;
    }
}

fn douglas_peucker(pontos: &[[f32; 2]], tolerancia: f32) -> Vec<[f32; 2]> {
    if pontos.len() <= 3 {
        return pontos.to_vec();
    }
    let mut manter = vec![false; pontos.len()];
    manter[0] = true;
    manter[pontos.len() - 1] = true;
    let mut pilha = vec![(0usize, pontos.len() - 1)];
    while let Some((a, b)) = pilha.pop() {
        let (mut pior, mut onde) = (0.0f32, a);
        for (i, p) in pontos.iter().enumerate().take(b).skip(a + 1) {
            let d = distancia_ao_segmento(*p, pontos[a], pontos[b]);
            if d > pior {
                pior = d;
                onde = i;
            }
        }
        if pior > tolerancia {
            manter[onde] = true;
            pilha.push((a, onde));
            pilha.push((onde, b));
        }
    }
    pontos
        .iter()
        .zip(manter)
        .filter_map(|(p, m)| m.then_some(*p))
        .collect()
}

fn distancia_ao_segmento(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let l2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if l2 > 1e-12 {
        ((ap[0] * ab[0] + ap[1] * ab[1]) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let d = [ap[0] - ab[0] * t, ap[1] - ab[1] * t];
    (d[0] * d[0] + d[1] * d[1]).sqrt()
}

/// A distância **com sinal** de `p` à borda do polígono (em pixels): positiva
/// dentro, negativa fora. Dentro/fora pela regra par-ímpar — um laço que se
/// cruza fica com as partes cruzadas de fora, como no Photoshop.
///
/// 🔑 A mesma conta, na mesma ordem, de `fs_laco` em `shaders/laco.wgsl`.
pub fn distancia_ao_laco(poligono: &[[f32; 2]], p: [f32; 2]) -> f32 {
    let n = poligono.len();
    let mut dentro = false;
    let mut menor = f32::MAX;
    for i in 0..n {
        let a = poligono[i];
        let b = poligono[(i + 1) % n];
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0];
            if p[0] < x {
                dentro = !dentro;
            }
        }
        menor = menor.min(distancia_ao_segmento(p, a, b));
    }
    if dentro {
        menor
    } else {
        -menor
    }
}

/// A cobertura do laço em `p` (pixels), com a transição centrada na borda.
pub fn cobertura_do_laco(poligono: &[[f32; 2]], feather_px: f32, p: [f32; 2]) -> f32 {
    if poligono.len() < 3 {
        return 0.0;
    }
    suave(
        -feather_px * 0.5,
        feather_px * 0.5,
        distancia_ao_laco(poligono, p),
    )
}

/// Um gesto de pincel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrushStroke {
    /// Fração do maior lado da foto inteira (ver o módulo).
    pub raio: f32,
    /// Fração do raio em que a borda cai.
    pub feather: f32,
    /// A opacidade do stroke inteiro — o teto da cobertura dele.
    pub opacidade: f32,
    /// `[x, y, pressão]`, na ordem do gesto.
    pub pontos: Vec<[f32; 3]>,
}

/// Um gradiente linear: 100% antes de `inicio`, 0% depois de `fim`, e a
/// transição (`smoothstep`) entre os dois.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradienteLinear {
    pub inicio: [f32; 2],
    pub fim: [f32; 2],
}

/// Um gradiente radial: uma elipse, 100% dentro e 0% fora (ou o contrário).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradienteRadial {
    pub centro: [f32; 2],
    /// Os semi-eixos, em fração do maior lado — a mesma régua do raio do pincel.
    pub raio_x: f32,
    pub raio_y: f32,
    /// Em graus, no sentido horário da tela.
    #[serde(default)]
    pub angulo: f32,
    /// Fração do semi-eixo em que a borda cai.
    pub feather: f32,
    /// O efeito vale fora da elipse — o padrão do "radial" do Lightroom.
    #[serde(default)]
    pub fora: bool,
}

/// Um retoque.
///
/// - **Clone** (carimbo) copia os pixels da origem.
/// - **Heal** (band-aid) copia a textura da origem e adapta cor e luz à
///   vizinhança do destino.
/// - **Preencher** (Content-Aware) não tem origem: sintetiza a área com
///   patches da vizinhança (`preenchimento.rs`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Retoque {
    Clone(Carimbo),
    Heal(Carimbo),
    Preencher(Preenchimento),
}

impl Retoque {
    /// A origem e o caminho — `None` no Content-Aware, que não tem origem.
    pub fn carimbo(&self) -> Option<&Carimbo> {
        match self {
            Retoque::Clone(c) | Retoque::Heal(c) => Some(c),
            Retoque::Preencher(_) => None,
        }
    }

    /// A máscara do destino, como stroke de opacidade cheia.
    pub fn traco(&self) -> BrushStroke {
        match self {
            Retoque::Clone(c) | Retoque::Heal(c) => c.como_traco(),
            Retoque::Preencher(p) => BrushStroke {
                raio: p.raio,
                feather: p.feather,
                opacidade: 1.0,
                pontos: p.caminho.iter().map(|q| [q[0], q[1], 1.0]).collect(),
            },
        }
    }

    pub fn opacidade(&self) -> f32 {
        match self {
            Retoque::Clone(c) | Retoque::Heal(c) => c.opacidade,
            Retoque::Preencher(p) => p.opacidade,
        }
    }
}

/// A área a preencher pelo conteúdo em volta — o caminho pintado, sem origem.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preenchimento {
    /// O caminho pintado (com `raio`) — vazio quando a área é um laço.
    #[serde(default)]
    pub caminho: Vec<[f32; 2]>,
    pub raio: f32,
    pub feather: f32,
    pub opacidade: f32,
    /// A área cercada por um laço, em vez de pintada. Com laço, `caminho` e
    /// `raio` não contam, e `feather` é o do laço (fração do maior lado).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub laco: Vec<[f32; 2]>,
}

impl Preenchimento {
    fn saneado(mut self) -> Option<Self> {
        self.caminho = self
            .caminho
            .into_iter()
            .filter_map(posicao)
            .take(MAXIMO_DE_PONTOS)
            .collect();
        self.laco = self
            .laco
            .into_iter()
            .filter_map(posicao)
            .take(MAXIMO_DE_PONTOS)
            .collect();
        if self.laco.len() < 3 {
            self.laco.clear();
        }
        self.raio = finito_ou(self.raio, RAIO_MINIMO).clamp(RAIO_MINIMO, 0.25);
        self.feather = finito_ou(self.feather, 0.0).clamp(0.0, 1.0);
        self.opacidade = finito_ou(self.opacidade, 1.0).clamp(0.0, 1.0);
        (!self.caminho.is_empty() || !self.laco.is_empty()).then_some(self)
    }
}

/// De onde vem e para onde vai um retoque.
///
/// O deslocamento é fixo durante o gesto: `origem − destino_inicial`. Cada ponto
/// `p` do caminho recebe o que está em `p + deslocamento`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Carimbo {
    pub origem: [f32; 2],
    pub destino_inicial: [f32; 2],
    /// O caminho do destino, começando em `destino_inicial`.
    pub caminho: Vec<[f32; 2]>,
    pub raio: f32,
    pub feather: f32,
    pub opacidade: f32,
}

impl Carimbo {
    /// `origem − destino_inicial`, em coordenadas normalizadas.
    pub fn deslocamento(&self) -> [f32; 2] {
        [
            self.origem[0] - self.destino_inicial[0],
            self.origem[1] - self.destino_inicial[1],
        ]
    }

    /// O caminho do destino como stroke de opacidade cheia e pressão 1 — é a
    /// máscara do destino, rasterizada pelo mesmo código do pincel.
    pub fn como_traco(&self) -> BrushStroke {
        BrushStroke {
            raio: self.raio,
            feather: self.feather,
            opacidade: 1.0,
            pontos: self.caminho.iter().map(|p| [p[0], p[1], 1.0]).collect(),
        }
    }
}

/// Quantas amostras o anel do Heal leva à GPU — o tamanho do `array` do WGSL.
pub const AMOSTRAS_DO_ANEL: usize = 96;

/// O anel do Heal: pontos **logo fora** da máscara de destino, em pixels de uma
/// imagem `largura × altura`, onde a diferença de cor entre destino e fonte é
/// medida. É só geometria — sai dos parâmetros, sem ler pixel nenhum —, e por
/// isso a mesma conta serve ao desktop e ao navegador.
///
/// Cada ponto do caminho (espaçados de meio raio) contribui com 16 pontos num
/// círculo de `1,25 × raio + 2 px`. Saem os que caem dentro de outra cápsula
/// do caminho, fora da foto, ou cuja fonte (`ponto + deslocamento`) cai fora
/// da foto. Passando de [`AMOSTRAS_DO_ANEL`], fica um a cada tanto.
pub fn anel_do_carimbo(carimbo: &Carimbo, largura: u32, altura: u32) -> Vec<[f32; 2]> {
    let (w, h) = (largura as f32, altura as f32);
    let r = carimbo.raio * w.max(h);
    let raio_do_anel = r * 1.25 + 2.0;
    let [dx, dy] = carimbo.deslocamento();
    let (dx, dy) = (dx * w, dy * h);
    let caminho: Vec<[f32; 2]> = carimbo
        .caminho
        .iter()
        .map(|p| [p[0] * w, p[1] * h])
        .collect();

    // Os centros: o caminho reamostrado a cada meio raio.
    let passo = (r * 0.5).max(1.0);
    let mut centros = vec![caminho[0]];
    for par in caminho.windows(2) {
        let (a, b) = (par[0], par[1]);
        let comprimento = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let n = (comprimento / passo).ceil() as usize;
        for k in 1..=n {
            let t = k as f32 / n as f32;
            centros.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    let dentro_da_mascara = |q: [f32; 2]| {
        let trechos: Vec<([f32; 2], [f32; 2])> = if caminho.len() == 1 {
            vec![(caminho[0], caminho[0])]
        } else {
            caminho.windows(2).map(|p| (p[0], p[1])).collect()
        };
        trechos.iter().any(|(a, b)| {
            let ab = [b[0] - a[0], b[1] - a[1]];
            let aq = [q[0] - a[0], q[1] - a[1]];
            let l2 = ab[0] * ab[0] + ab[1] * ab[1];
            let t = if l2 > 1e-9 {
                ((aq[0] * ab[0] + aq[1] * ab[1]) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let d = [aq[0] - ab[0] * t, aq[1] - ab[1] * t];
            (d[0] * d[0] + d[1] * d[1]).sqrt() < r * 1.15
        })
    };
    let na_foto = |x: f32, y: f32| x >= 0.5 && y >= 0.5 && x <= w - 0.5 && y <= h - 0.5;

    let mut anel = Vec::new();
    for c in &centros {
        for k in 0..16 {
            let a = k as f32 / 16.0 * std::f32::consts::TAU;
            let q = [c[0] + raio_do_anel * a.cos(), c[1] + raio_do_anel * a.sin()];
            if na_foto(q[0], q[1]) && na_foto(q[0] + dx, q[1] + dy) && !dentro_da_mascara(q) {
                anel.push(q);
            }
        }
    }
    if anel.len() > AMOSTRAS_DO_ANEL {
        let passo = anel.len() as f32 / AMOSTRAS_DO_ANEL as f32;
        anel = (0..AMOSTRAS_DO_ANEL)
            .map(|i| anel[(i as f32 * passo) as usize])
            .collect();
    }
    anel
}

/// Por que uma revelação não pôde ser lida.
#[derive(Debug, Clone, PartialEq)]
pub enum ErroDosParametros {
    /// Gravada por uma versão mais nova do app. Quem abre a foto não a
    /// sobrescreve: devolve o texto que leu.
    VersaoNova(u32),
    /// Não é JSON, ou não tem a forma.
    Invalida(String),
}

impl std::fmt::Display for ErroDosParametros {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErroDosParametros::VersaoNova(v) => {
                write!(
                    f,
                    "revelação local da versão {v}; este app lê até a {VERSAO}"
                )
            }
            ErroDosParametros::Invalida(e) => write!(f, "revelação local inválida: {e}"),
        }
    }
}

impl std::error::Error for ErroDosParametros {}

fn finito_ou(v: f32, padrao: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        padrao
    }
}

/// Posição aceita um pouco além da foto: um stroke que começa fora da borda
/// tem de pintar a borda inteira.
fn posicao(p: [f32; 2]) -> Option<[f32; 2]> {
    (p[0].is_finite() && p[1].is_finite()).then(|| [p[0].clamp(-1.0, 2.0), p[1].clamp(-1.0, 2.0)])
}

impl BrushStroke {
    fn saneado(mut self) -> Option<Self> {
        self.raio = finito_ou(self.raio, RAIO_MINIMO).clamp(RAIO_MINIMO, 1.0);
        self.feather = finito_ou(self.feather, 0.0).clamp(0.0, 1.0);
        self.opacidade = finito_ou(self.opacidade, 1.0).clamp(0.0, 1.0);
        self.pontos = self
            .pontos
            .into_iter()
            .filter_map(|[x, y, p]| {
                let [x, y] = posicao([x, y])?;
                Some([x, y, finito_ou(p, 1.0).clamp(0.0, 1.0)])
            })
            .take(MAXIMO_DE_PONTOS)
            .collect();
        (!self.pontos.is_empty()).then_some(self)
    }

    /// O retângulo que o stroke pode tocar, em pixels de uma imagem
    /// `largura × altura`: `(x0, y0, x1, y1)`, já limitado à imagem. `None`
    /// quando cai inteiro fora.
    pub fn caixa_em_pixels(&self, largura: u32, altura: u32) -> Option<(u32, u32, u32, u32)> {
        caixa_de_pontos(
            self.pontos.iter().map(|p| [p[0], p[1]]),
            self.raio,
            largura,
            altura,
        )
    }
}

/// A caixa de uma sequência de pontos com raio — ver [`BrushStroke::caixa_em_pixels`].
pub fn caixa_de_pontos(
    pontos: impl Iterator<Item = [f32; 2]>,
    raio: f32,
    largura: u32,
    altura: u32,
) -> Option<(u32, u32, u32, u32)> {
    let (w, h) = (largura as f32, altura as f32);
    let r = raio * w.max(h);
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for [x, y] in pontos {
        x0 = x0.min(x * w - r);
        y0 = y0.min(y * h - r);
        x1 = x1.max(x * w + r);
        y1 = y1.max(y * h + r);
    }
    let x0 = x0.floor().max(0.0);
    let y0 = y0.floor().max(0.0);
    let x1 = x1.ceil().min(w);
    let y1 = y1.ceil().min(h);
    (x1 > x0 && y1 > y0).then_some((x0 as u32, y0 as u32, x1 as u32, y1 as u32))
}

impl Componente {
    fn saneado(self) -> Option<Self> {
        let forma = match self.forma {
            Forma::Pincel(t) => Forma::Pincel(t.saneado()?),
            Forma::Linear(g) => Forma::Linear(GradienteLinear {
                inicio: posicao(g.inicio)?,
                fim: posicao(g.fim)?,
            }),
            Forma::Laco(l) => {
                let pontos: Vec<[f32; 2]> = l
                    .pontos
                    .into_iter()
                    .filter_map(posicao)
                    .take(MAXIMO_DE_PONTOS)
                    .collect();
                if pontos.len() < 3 {
                    return None;
                }
                Forma::Laco(Laco {
                    pontos,
                    feather: finito_ou(l.feather, 0.0).clamp(0.0, 0.25),
                })
            }
            Forma::Radial(g) => Forma::Radial(GradienteRadial {
                centro: posicao(g.centro)?,
                raio_x: finito_ou(g.raio_x, RAIO_MINIMO).clamp(RAIO_MINIMO, 2.0),
                raio_y: finito_ou(g.raio_y, RAIO_MINIMO).clamp(RAIO_MINIMO, 2.0),
                angulo: finito_ou(g.angulo, 0.0).rem_euclid(360.0),
                feather: finito_ou(g.feather, 0.0).clamp(0.0, 1.0),
                fora: g.fora,
            }),
        };
        Some(Self {
            modo: self.modo,
            forma,
        })
    }
}

impl Carimbo {
    fn saneado(mut self) -> Option<Self> {
        self.origem = posicao(self.origem)?;
        self.destino_inicial = posicao(self.destino_inicial)?;
        self.caminho = self
            .caminho
            .into_iter()
            .filter_map(posicao)
            .take(MAXIMO_DE_PONTOS)
            .collect();
        if self.caminho.is_empty() {
            self.caminho.push(self.destino_inicial);
        }
        self.raio = finito_ou(self.raio, RAIO_MINIMO).clamp(RAIO_MINIMO, 1.0);
        self.feather = finito_ou(self.feather, 0.0).clamp(0.0, 1.0);
        self.opacidade = finito_ou(self.opacidade, 1.0).clamp(0.0, 1.0);
        Some(self)
    }
}

impl ParametrosLocais {
    /// Sem camada e sem retoque — a foto sai bit a bit a de antes.
    pub fn vazia(&self) -> bool {
        self.camadas.is_empty() && self.retoques.is_empty()
    }

    /// O que o motor aplica: as camadas visíveis e com componentes, e todos os
    /// retoques. A ordem se mantém.
    pub fn para_o_motor(&self) -> ParametrosLocais {
        ParametrosLocais {
            versao: self.versao,
            camadas: self
                .camadas
                .iter()
                .filter(|c| c.visivel && !c.componentes.is_empty())
                .cloned()
                .collect(),
            retoques: self.retoques.clone(),
        }
    }

    /// A posição, no motor, da camada `i` desta revelação — `None` se ela está
    /// oculta ou vazia (e por isso não é desenhada).
    pub fn indice_no_motor(&self, i: usize) -> Option<usize> {
        let camada = self.camadas.get(i)?;
        if !camada.visivel || camada.componentes.is_empty() {
            return None;
        }
        Some(
            self.camadas[..i]
                .iter()
                .filter(|c| c.visivel && !c.componentes.is_empty())
                .count(),
        )
    }

    /// Lê o JSON gravado, já saneado. Versão maior que [`VERSAO`] é recusada.
    pub fn de_json(json: &str) -> Result<Self, ErroDosParametros> {
        let valor: serde_json::Value =
            serde_json::from_str(json).map_err(|e| ErroDosParametros::Invalida(e.to_string()))?;
        let versao = valor
            .get("versao")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(VERSAO as u64) as u32;
        if versao > VERSAO {
            return Err(ErroDosParametros::VersaoNova(versao));
        }
        let parametros: ParametrosLocais = serde_json::from_value(valor)
            .map_err(|e| ErroDosParametros::Invalida(e.to_string()))?;
        Ok(parametros.saneada())
    }

    /// O JSON que se grava. Revelação vazia vira `None` — a coluna fica `NULL`.
    pub fn em_json(&self) -> Option<String> {
        (!self.vazia()).then(|| serde_json::to_string(self).expect("a revelação local vira JSON"))
    }

    /// Limites e números finitos: o que a GPU recebe nunca é `NaN`, e nenhuma
    /// revelação cresce além do que o shader e a memória comportam.
    pub fn saneada(self) -> Self {
        let camadas = self
            .camadas
            .into_iter()
            .take(MAXIMO_DE_CAMADAS)
            .map(|c| Camada {
                nome: c.nome.chars().take(MAXIMO_DO_NOME).collect(),
                visivel: c.visivel,
                ajustes: AjustesLocais {
                    exposicao_ev: finito_ou(c.ajustes.exposicao_ev, 0.0)
                        .clamp(-EXPOSICAO_MAXIMA, EXPOSICAO_MAXIMA),
                },
                componentes: c
                    .componentes
                    .into_iter()
                    .filter_map(Componente::saneado)
                    .take(MAXIMO_DE_COMPONENTES)
                    .collect(),
                invertida: c.invertida,
            })
            .collect();
        let retoques = self
            .retoques
            .into_iter()
            .filter_map(|r| match r {
                Retoque::Clone(c) => c.saneado().map(Retoque::Clone),
                Retoque::Heal(c) => c.saneado().map(Retoque::Heal),
                Retoque::Preencher(p) => p.saneado().map(Retoque::Preencher),
            })
            .take(MAXIMO_DE_RETOQUES)
            .collect();
        Self {
            versao: VERSAO,
            camadas,
            retoques,
        }
    }
}

// ---------------------------------------------------------------------------
// A matemática da máscara — a mesma de `shaders/mascara.wgsl`, linha a linha.
// ---------------------------------------------------------------------------

/// `smoothstep` do WGSL, com a borda dura quando as duas pontas coincidem (o
/// WGSL não define esse caso, e o shader faz o mesmo desvio).
pub fn suave(borda0: f32, borda1: f32, x: f32) -> f32 {
    if borda1 - borda0 <= 1e-6 {
        return if x < borda1 { 0.0 } else { 1.0 };
    }
    let t = ((x - borda0) / (borda1 - borda0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A cobertura de uma cápsula `a → b` no ponto `p` (tudo em pixels), com
/// pressão interpolada ao longo do trecho.
pub fn cobertura_da_capsula(
    p: [f32; 2],
    a: [f32; 2],
    b: [f32; 2],
    pressao_a: f32,
    pressao_b: f32,
    raio_px: f32,
    feather: f32,
) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let ap = [p[0] - a[0], p[1] - a[1]];
    let comprimento2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if comprimento2 > 1e-12 {
        ((ap[0] * ab[0] + ap[1] * ab[1]) / comprimento2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let d = [ap[0] - ab[0] * t, ap[1] - ab[1] * t];
    let distancia = (d[0] * d[0] + d[1] * d[1]).sqrt();
    let pressao = pressao_a + (pressao_b - pressao_a) * t;
    let interno = raio_px * (1.0 - feather);
    pressao * (1.0 - suave(interno, raio_px, distancia))
}

/// Os trechos de um stroke: pares consecutivos de pontos, e um trecho de
/// comprimento zero quando o stroke é um clique só.
pub fn trechos(pontos: &[[f32; 3]]) -> impl Iterator<Item = ([f32; 3], [f32; 3])> + '_ {
    let unico = (pontos.len() == 1).then(|| (pontos[0], pontos[0]));
    unico
        .into_iter()
        .chain(pontos.windows(2).map(|par| (par[0], par[1])))
}

/// A cobertura de um gradiente no ponto `p` (em pixels) de uma imagem
/// `largura × altura`.
pub fn cobertura_do_gradiente(forma: &Forma, p: [f32; 2], largura: f32, altura: f32) -> f32 {
    let lado = largura.max(altura);
    match forma {
        Forma::Linear(g) => {
            let a = [g.inicio[0] * largura, g.inicio[1] * altura];
            let b = [g.fim[0] * largura, g.fim[1] * altura];
            let ab = [b[0] - a[0], b[1] - a[1]];
            let comprimento2 = ab[0] * ab[0] + ab[1] * ab[1];
            if comprimento2 <= 1e-6 {
                return 0.0;
            }
            let t = ((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / comprimento2;
            1.0 - suave(0.0, 1.0, t)
        }
        Forma::Radial(g) => {
            let c = [g.centro[0] * largura, g.centro[1] * altura];
            let (seno, cosseno) = (-g.angulo.to_radians()).sin_cos();
            let d = [p[0] - c[0], p[1] - c[1]];
            let local = [d[0] * cosseno - d[1] * seno, d[0] * seno + d[1] * cosseno];
            let q = ((local[0] / (g.raio_x * lado)).powi(2)
                + (local[1] / (g.raio_y * lado)).powi(2))
            .sqrt();
            let dentro = 1.0 - suave(1.0 - g.feather, 1.0, q);
            if g.fora {
                1.0 - dentro
            } else {
                dentro
            }
        }
        Forma::Laco(l) => cobertura_do_laco(
            &laco_em_pixels(&l.pontos, largura as u32, altura as u32),
            l.feather * lado,
            p,
        ),
        Forma::Pincel(_) => 0.0,
    }
}

/// Um stroke rasterizado sozinho, **sem** a opacidade: o máximo das cápsulas.
pub fn traco_em_cpu(traco: &BrushStroke, largura: u32, altura: u32) -> Vec<f32> {
    let (w, h) = (largura as f32, altura as f32);
    let raio_px = traco.raio * w.max(h);
    let mut saida = vec![0.0f32; (largura * altura) as usize];
    let Some((x0, y0, x1, y1)) = traco.caixa_em_pixels(largura, altura) else {
        return saida;
    };
    for y in y0..y1 {
        for x in x0..x1 {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let mut s = 0.0f32;
            for (a, b) in trechos(&traco.pontos) {
                s = s.max(cobertura_da_capsula(
                    p,
                    [a[0] * w, a[1] * h],
                    [b[0] * w, b[1] * h],
                    a[2],
                    b[2],
                    raio_px,
                    traco.feather,
                ));
            }
            saida[(y * largura + x) as usize] = s;
        }
    }
    saida
}

/// Compõe uma cobertura `s` sobre a máscara `m`, no modo dado.
pub fn compor(m: f32, s: f32, modo: Modo) -> f32 {
    match modo {
        Modo::Somar => s + m * (1.0 - s),
        Modo::Subtrair => m * (1.0 - s),
    }
}

/// A máscara de uma camada, em `0..1`, **sem** a inversão (que o shader de
/// revelação aplica, lendo o `invertida` da camada).
///
/// É a referência: sem quantização de 8 bits, que a GPU tem e os testes
/// toleram.
pub fn mascara_em_cpu(camada: &Camada, largura: u32, altura: u32) -> Vec<f32> {
    let (w, h) = (largura as f32, altura as f32);
    let mut m = vec![0.0f32; (largura * altura) as usize];
    for componente in &camada.componentes {
        match &componente.forma {
            Forma::Pincel(traco) => {
                let s = traco_em_cpu(traco, largura, altura);
                for (mi, si) in m.iter_mut().zip(s) {
                    *mi = compor(*mi, si * traco.opacidade, componente.modo);
                }
            }
            forma => {
                for y in 0..altura {
                    for x in 0..largura {
                        let i = (y * largura + x) as usize;
                        let s =
                            cobertura_do_gradiente(forma, [x as f32 + 0.5, y as f32 + 0.5], w, h);
                        m[i] = compor(m[i], s, componente.modo);
                    }
                }
            }
        }
    }
    m
}

#[cfg(test)]
mod testes {
    use super::*;

    fn traco(pontos: Vec<[f32; 3]>, raio: f32, feather: f32, opacidade: f32) -> BrushStroke {
        BrushStroke {
            raio,
            feather,
            opacidade,
            pontos,
        }
    }

    fn camada_com(componentes: Vec<Componente>) -> Camada {
        Camada {
            ajustes: AjustesLocais { exposicao_ev: 1.0 },
            componentes,
            ..Default::default()
        }
    }

    fn pincel(t: BrushStroke, modo: Modo) -> Componente {
        Componente {
            modo,
            forma: Forma::Pincel(t),
        }
    }

    fn maximo(v: &[f32]) -> f32 {
        v.iter().copied().fold(0.0, f32::max)
    }

    /// 🚨 O requisito do pincel: passar e voltar no mesmo gesto não escurece.
    ///
    /// Um stroke que vai e volta três vezes sobre o mesmo trecho, com opacidade
    /// 0,5, não pode passar de 0,5 em lugar nenhum — somar as cápsulas daria
    /// quase 1.
    #[test]
    fn circulos_sobrepostos_no_mesmo_stroke_nao_passam_da_opacidade() {
        let pontos = (0..60)
            .map(|i| {
                let t = (i % 20) as f32 / 19.0;
                let x = if (i / 20) % 2 == 0 { t } else { 1.0 - t };
                [0.2 + 0.6 * x, 0.5, 1.0]
            })
            .collect();
        let camada = camada_com(vec![pincel(traco(pontos, 0.1, 0.5, 0.5), Modo::Somar)]);
        let m = mascara_em_cpu(&camada, 64, 64);
        let pico = maximo(&m);
        assert!((pico - 0.5).abs() < 1e-5, "pico {pico}, esperava 0,5");
    }

    /// Dois gestos, sim, compõem: é o segundo passe do Lightroom.
    #[test]
    fn dois_strokes_compoem_como_camadas() {
        let t = || traco(vec![[0.5, 0.5, 1.0]], 0.2, 0.0, 0.5);
        let camada = camada_com(vec![pincel(t(), Modo::Somar), pincel(t(), Modo::Somar)]);
        let m = mascara_em_cpu(&camada, 32, 32);
        let centro = m[16 * 32 + 16];
        assert!(
            (centro - 0.75).abs() < 1e-5,
            "0,5 sobre 0,5 dá 0,75, deu {centro}"
        );
    }

    #[test]
    fn o_feather_cai_monotonico_e_para_no_raio() {
        let camada = camada_com(vec![pincel(
            traco(vec![[0.5, 0.5, 1.0]], 0.25, 0.6, 1.0),
            Modo::Somar,
        )]);
        let lado = 128u32;
        let m = mascara_em_cpu(&camada, lado, lado);
        let linha: Vec<f32> = (64..128).map(|x| m[(64 * lado + x) as usize]).collect();
        assert!(
            linha.windows(2).all(|p| p[1] <= p[0] + 1e-6),
            "não cresce para fora"
        );
        // Raio de 32 px, feather 0,6: 1 até 12,8 px do centro, 0 a partir de 32.
        assert!((linha[0] - 1.0).abs() < 1e-6);
        assert!(linha[10] > 0.99);
        assert_eq!(linha[33], 0.0);
        assert!(linha[20] > 0.0 && linha[20] < 1.0);
    }

    #[test]
    fn feather_zero_e_borda_dura() {
        let camada = camada_com(vec![pincel(
            traco(vec![[0.5, 0.5, 1.0]], 0.25, 0.0, 1.0),
            Modo::Somar,
        )]);
        let m = mascara_em_cpu(&camada, 64, 64);
        assert!(m.iter().all(|&v| v == 0.0 || v == 1.0));
    }

    #[test]
    fn a_pressao_escala_a_intensidade_e_nao_o_raio() {
        let forte = mascara_em_cpu(
            &camada_com(vec![pincel(
                traco(vec![[0.5, 0.5, 1.0]], 0.2, 0.0, 1.0),
                Modo::Somar,
            )]),
            32,
            32,
        );
        let fraca = mascara_em_cpu(
            &camada_com(vec![pincel(
                traco(vec![[0.5, 0.5, 0.4]], 0.2, 0.0, 1.0),
                Modo::Somar,
            )]),
            32,
            32,
        );
        for (f, d) in forte.iter().zip(&fraca) {
            assert!((f * 0.4 - d).abs() < 1e-6);
        }
        // A pressão varia ao longo do trecho.
        let rampa = mascara_em_cpu(
            &camada_com(vec![pincel(
                traco(vec![[0.1, 0.5, 0.0], [0.9, 0.5, 1.0]], 0.05, 0.0, 1.0),
                Modo::Somar,
            )]),
            64,
            64,
        );
        let (esquerda, direita) = (rampa[32 * 64 + 10], rampa[32 * 64 + 54]);
        assert!(esquerda < direita, "{esquerda} < {direita}");
    }

    #[test]
    fn stroke_na_borda_pinta_a_borda_e_nao_quebra() {
        let camada = camada_com(vec![pincel(
            traco(vec![[-0.2, -0.2, 1.0], [0.05, 0.05, 1.0]], 0.1, 0.0, 1.0),
            Modo::Somar,
        )]);
        let m = mascara_em_cpu(&camada, 40, 30);
        assert_eq!(m[0], 1.0, "o canto");
        assert_eq!(m[29 * 40 + 39], 0.0, "o outro canto");
        // Inteiro fora da foto: nada, e sem pânico.
        let fora = camada_com(vec![pincel(
            traco(vec![[1.9, 1.9, 1.0]], 0.05, 0.0, 1.0),
            Modo::Somar,
        )]);
        assert!(mascara_em_cpu(&fora, 40, 30).iter().all(|&v| v == 0.0));
    }

    /// O raio é fração do maior lado: o mesmo stroke cobre a mesma fração da
    /// foto em 2048 e em 6016 px.
    #[test]
    fn o_raio_e_o_mesmo_pedaco_da_foto_em_qualquer_resolucao() {
        let camada = camada_com(vec![pincel(
            traco(vec![[0.3, 0.4, 1.0], [0.6, 0.5, 1.0]], 0.08, 0.3, 1.0),
            Modo::Somar,
        )]);
        let area = |w: u32, h: u32| {
            let m = mascara_em_cpu(&camada, w, h);
            m.iter().sum::<f32>() / (w * h) as f32
        };
        let (pequena, grande) = (area(150, 100), area(600, 400));
        assert!(
            (pequena - grande).abs() < 0.003,
            "{pequena} vs {grande}: a cobertura relativa tem de ser a mesma"
        );
    }

    #[test]
    fn subtrair_apaga_o_que_foi_pintado() {
        let camada = camada_com(vec![
            pincel(traco(vec![[0.5, 0.5, 1.0]], 0.3, 0.0, 1.0), Modo::Somar),
            pincel(traco(vec![[0.5, 0.5, 1.0]], 0.1, 0.0, 1.0), Modo::Subtrair),
        ]);
        let m = mascara_em_cpu(&camada, 40, 40);
        assert_eq!(m[20 * 40 + 20], 0.0, "o centro foi apagado");
        assert_eq!(m[20 * 40 + 12], 1.0, "o anel ficou");
    }

    #[test]
    fn o_gradiente_linear_vai_de_cheio_a_vazio() {
        let camada = camada_com(vec![Componente {
            modo: Modo::Somar,
            forma: Forma::Linear(GradienteLinear {
                inicio: [0.25, 0.5],
                fim: [0.75, 0.5],
            }),
        }]);
        let m = mascara_em_cpu(&camada, 100, 10);
        assert_eq!(m[5 * 100 + 10], 1.0);
        assert_eq!(m[5 * 100 + 90], 0.0);
        assert!((m[5 * 100 + 50] - 0.5).abs() < 0.05);
    }

    #[test]
    fn o_radial_vale_fora_quando_pedido() {
        let radial = |fora| Componente {
            modo: Modo::Somar,
            forma: Forma::Radial(GradienteRadial {
                centro: [0.5, 0.5],
                raio_x: 0.3,
                raio_y: 0.3,
                angulo: 0.0,
                feather: 0.2,
                fora,
            }),
        };
        let dentro = mascara_em_cpu(&camada_com(vec![radial(false)]), 50, 50);
        let fora = mascara_em_cpu(&camada_com(vec![radial(true)]), 50, 50);
        assert_eq!(dentro[25 * 50 + 25], 1.0);
        assert_eq!(fora[25 * 50 + 25], 0.0);
        assert_eq!(fora[0], 1.0);
    }

    #[test]
    fn os_parametros_vao_e_volta_pelo_json() {
        let parametros = ParametrosLocais {
            versao: VERSAO,
            camadas: vec![camada_com(vec![
                pincel(
                    traco(vec![[0.1, 0.2, 0.7], [0.3, 0.4, 1.0]], 0.02, 0.5, 0.8),
                    Modo::Somar,
                ),
                Componente {
                    modo: Modo::Subtrair,
                    forma: Forma::Radial(GradienteRadial {
                        centro: [0.5, 0.5],
                        raio_x: 0.2,
                        raio_y: 0.1,
                        angulo: 30.0,
                        feather: 0.5,
                        fora: true,
                    }),
                },
            ])],
            retoques: vec![Retoque::Heal(Carimbo {
                origem: [0.2, 0.2],
                destino_inicial: [0.6, 0.6],
                caminho: vec![[0.6, 0.6], [0.62, 0.6]],
                raio: 0.01,
                feather: 0.3,
                opacidade: 1.0,
            })],
        };
        let json = parametros.em_json().expect("não vazia");
        assert_eq!(ParametrosLocais::de_json(&json), Ok(parametros));
        assert!(json.contains("\"tipo\":\"pincel\""), "{json}");
    }

    /// Nome e olho vão e voltam; a máscara oculta fica na revelação e sai do
    /// que o motor aplica, e os índices do motor pulam as ocultas.
    #[test]
    fn nome_e_visibilidade_vao_e_voltam_e_o_motor_pula_as_ocultas() {
        let pincel = || Componente {
            modo: Modo::Somar,
            forma: Forma::Pincel(traco(vec![[0.5, 0.5, 1.0]], 0.1, 0.0, 1.0)),
        };
        let parametros = ParametrosLocais {
            camadas: vec![
                Camada {
                    nome: "Céu".into(),
                    componentes: vec![pincel()],
                    ..Default::default()
                },
                Camada {
                    nome: "Rosto".into(),
                    visivel: false,
                    componentes: vec![pincel()],
                    ..Default::default()
                },
                Camada {
                    nome: "Vazia".into(),
                    ..Default::default()
                },
                Camada {
                    nome: "Fundo".into(),
                    componentes: vec![pincel()],
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let json = parametros.em_json().unwrap();
        assert!(
            json.contains("\"visivel\":false"),
            "só a oculta escreve o campo: {json}"
        );
        assert_eq!(ParametrosLocais::de_json(&json).unwrap(), parametros);
        // Revelação antiga, sem os campos: visível e sem nome.
        let antiga =
            ParametrosLocais::de_json(r#"{"versao":1,"camadas":[{"componentes":[]}]}"#).unwrap();
        assert!(antiga.camadas[0].visivel && antiga.camadas[0].nome.is_empty());

        let motor = parametros.para_o_motor();
        let nomes: Vec<&str> = motor.camadas.iter().map(|c| c.nome.as_str()).collect();
        assert_eq!(nomes, ["Céu", "Fundo"]);
        assert_eq!(
            (0..4)
                .map(|i| parametros.indice_no_motor(i))
                .collect::<Vec<_>>(),
            [Some(0), None, None, Some(1)]
        );
    }

    #[test]
    fn parametros_vazios_nao_grava_nada() {
        assert_eq!(ParametrosLocais::default().em_json(), None);
    }

    /// Versão mais nova não é lida — e quem abre a foto guarda o texto intacto.
    #[test]
    fn versao_nova_e_recusada_e_nao_vira_vazia() {
        assert_eq!(
            ParametrosLocais::de_json(r#"{"versao": 2, "camadas": []}"#),
            Err(ErroDosParametros::VersaoNova(2))
        );
        assert!(matches!(
            ParametrosLocais::de_json("não é json"),
            Err(ErroDosParametros::Invalida(_))
        ));
    }

    #[test]
    fn o_saneamento_tira_nan_e_limita() {
        let json = serde_json::json!({
            "versao": 1,
            "camadas": (0..12).map(|_| serde_json::json!({
                "ajustes": { "exposicao_ev": 99.0 },
                "componentes": [
                    { "tipo": "pincel", "raio": 5.0, "feather": -1.0, "opacidade": 2.0,
                      "pontos": [[0.5, 0.5, 3.0], [0.1, 0.1, 1.0]] },
                    { "tipo": "pincel", "raio": 0.1, "feather": 0.1, "opacidade": 1.0, "pontos": [] }
                ]
            })).collect::<Vec<_>>()
        });
        let r = ParametrosLocais::de_json(&json.to_string()).unwrap();
        assert_eq!(r.camadas.len(), MAXIMO_DE_CAMADAS);
        let c = &r.camadas[0];
        assert_eq!(c.ajustes.exposicao_ev, EXPOSICAO_MAXIMA);
        assert_eq!(c.componentes.len(), 1, "o stroke sem ponto sai");
        let Forma::Pincel(t) = &c.componentes[0].forma else {
            panic!()
        };
        assert_eq!((t.raio, t.feather, t.opacidade), (1.0, 0.0, 1.0));
        assert_eq!(t.pontos[0][2], 1.0);
    }

    #[test]
    fn o_laco_pinta_dentro_e_a_borda_suave_fica_na_linha() {
        let quadrado = vec![[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]];
        let camada = camada_com(vec![Componente {
            modo: Modo::Somar,
            forma: Forma::Laco(Laco {
                pontos: quadrado.clone(),
                feather: 0.0,
            }),
        }]);
        let m = mascara_em_cpu(&camada, 40, 40);
        assert_eq!(m[20 * 40 + 20], 1.0);
        assert_eq!(m[20 * 40 + 5], 0.0);
        assert_eq!(m[20 * 40 + 11], 1.0, "logo dentro da borda (x = 10)");
        assert_eq!(m[20 * 40 + 9], 0.0, "logo fora");

        let suave = mascara_em_cpu(
            &camada_com(vec![Componente {
                modo: Modo::Somar,
                forma: Forma::Laco(Laco {
                    pontos: quadrado,
                    feather: 0.2, // 8 px de transição
                }),
            }]),
            40,
            40,
        );
        let borda = suave[20 * 40 + 10];
        assert!(
            borda > 0.3 && borda < 0.7,
            "meio da transição na linha: {borda}"
        );
        assert_eq!(suave[20 * 40 + 20], 1.0);
        assert_eq!(suave[20 * 40 + 2], 0.0);
    }

    /// Um laço livre de 2000 pontos cabe nos vértices da GPU e continua o mesmo
    /// contorno (a tolerância da simplificação é de meio pixel para cima).
    #[test]
    fn o_laco_livre_e_simplificado_sem_perder_a_forma() {
        let circulo: Vec<[f32; 2]> = (0..2000)
            .map(|i| {
                let a = i as f32 / 2000.0 * std::f32::consts::TAU;
                [0.5 + 0.3 * a.cos(), 0.5 + 0.3 * a.sin()]
            })
            .collect();
        let px = laco_em_pixels(&circulo, 400, 400);
        assert!(
            px.len() <= VERTICES_DO_LACO && px.len() > 16,
            "{}",
            px.len()
        );
        for p in &px {
            let r = ((p[0] - 200.0).powi(2) + (p[1] - 200.0).powi(2)).sqrt();
            assert!((r - 120.0).abs() < 0.01, "os vértices ficam no círculo");
        }
        // E no meio de cada lado o erro é pequeno.
        let d = distancia_ao_laco(&px, [200.0 + 120.0, 200.0]).abs();
        assert!(d < 2.0, "{d}");
    }

    #[test]
    fn o_anel_fica_fora_da_mascara_e_dentro_da_foto() {
        let c = Carimbo {
            origem: [0.2, 0.5],
            destino_inicial: [0.6, 0.5],
            caminho: vec![[0.6, 0.5], [0.8, 0.5]],
            raio: 0.05,
            feather: 0.3,
            opacidade: 1.0,
        };
        let anel = anel_do_carimbo(&c, 200, 100);
        assert!(!anel.is_empty() && anel.len() <= AMOSTRAS_DO_ANEL);
        let r = 0.05 * 200.0;
        for q in &anel {
            // Longe do caminho (y = 50, x de 120 a 160) e dentro da foto,
            // com a fonte (40 px à esquerda) dentro também.
            let dx = if q[0] < 120.0 {
                120.0 - q[0]
            } else if q[0] > 160.0 {
                q[0] - 160.0
            } else {
                0.0
            };
            let d = (dx * dx + (q[1] - 50.0).powi(2)).sqrt();
            assert!(d >= r * 1.15, "{q:?} a {d} px do caminho");
            assert!(q[0] - 80.0 >= 0.5 && q[1] >= 0.5 && q[1] <= 99.5);
        }
    }

    #[test]
    fn o_deslocamento_e_origem_menos_destino_inicial() {
        let c = Carimbo {
            origem: [0.2, 0.3],
            destino_inicial: [0.5, 0.5],
            caminho: vec![],
            raio: 0.01,
            feather: 0.0,
            opacidade: 1.0,
        }
        .saneado()
        .unwrap();
        let [dx, dy] = c.deslocamento();
        assert!((dx + 0.3).abs() < 1e-6 && (dy + 0.2).abs() < 1e-6);
        assert_eq!(
            c.caminho,
            vec![[0.5, 0.5]],
            "caminho vazio começa no destino"
        );
    }
}
