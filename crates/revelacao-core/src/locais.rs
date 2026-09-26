//! A receita local: máscaras (pincel, gradientes) com os ajustes delas, e os
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

/// A versão do formato. Uma receita de versão maior é recusada por
/// [`ReceitaLocal::de_json`], e quem abre a foto a guarda intacta.
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
pub struct ReceitaLocal {
    pub versao: u32,
    #[serde(default)]
    pub camadas: Vec<Camada>,
    #[serde(default)]
    pub retoques: Vec<Retoque>,
}

impl Default for ReceitaLocal {
    fn default() -> Self {
        Self {
            versao: VERSAO,
            camadas: Vec::new(),
            retoques: Vec::new(),
        }
    }
}

/// Uma máscara e o que ela aplica.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Camada {
    #[serde(default)]
    pub ajustes: AjustesLocais,
    #[serde(default)]
    pub componentes: Vec<Componente>,
    /// A máscara invertida: o ajuste vale **fora** do que foi pintado.
    #[serde(default)]
    pub invertida: bool,
}

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

/// Um retoque. Os dois tipos têm os mesmos parâmetros e fazem coisas
/// diferentes: o Clone copia os pixels; o Heal copia a textura e adapta cor e
/// luz à vizinhança do destino.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Retoque {
    Clone(Carimbo),
    Heal(Carimbo),
}

impl Retoque {
    pub fn carimbo(&self) -> &Carimbo {
        match self {
            Retoque::Clone(c) | Retoque::Heal(c) => c,
        }
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

/// Por que uma receita não pôde ser lida.
#[derive(Debug, Clone, PartialEq)]
pub enum ErroDaReceita {
    /// Gravada por uma versão mais nova do app. Quem abre a foto não a
    /// sobrescreve: devolve o texto que leu.
    VersaoNova(u32),
    /// Não é JSON, ou não tem a forma.
    Invalida(String),
}

impl std::fmt::Display for ErroDaReceita {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErroDaReceita::VersaoNova(v) => {
                write!(f, "receita local da versão {v}; este app lê até a {VERSAO}")
            }
            ErroDaReceita::Invalida(e) => write!(f, "receita local inválida: {e}"),
        }
    }
}

impl std::error::Error for ErroDaReceita {}

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

impl ReceitaLocal {
    /// Sem camada e sem retoque — a foto sai bit a bit a de antes.
    pub fn vazia(&self) -> bool {
        self.camadas.is_empty() && self.retoques.is_empty()
    }

    /// Lê o JSON gravado, já saneado. Versão maior que [`VERSAO`] é recusada.
    pub fn de_json(json: &str) -> Result<Self, ErroDaReceita> {
        let valor: serde_json::Value =
            serde_json::from_str(json).map_err(|e| ErroDaReceita::Invalida(e.to_string()))?;
        let versao = valor
            .get("versao")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(VERSAO as u64) as u32;
        if versao > VERSAO {
            return Err(ErroDaReceita::VersaoNova(versao));
        }
        let receita: ReceitaLocal =
            serde_json::from_value(valor).map_err(|e| ErroDaReceita::Invalida(e.to_string()))?;
        Ok(receita.saneada())
    }

    /// O JSON que se grava. Receita vazia vira `None` — a coluna fica `NULL`.
    pub fn em_json(&self) -> Option<String> {
        (!self.vazia()).then(|| serde_json::to_string(self).expect("a receita local vira JSON"))
    }

    /// Limites e números finitos: o que a GPU recebe nunca é `NaN`, e nenhuma
    /// receita cresce além do que o shader e a memória comportam.
    pub fn saneada(self) -> Self {
        let camadas = self
            .camadas
            .into_iter()
            .take(MAXIMO_DE_CAMADAS)
            .map(|c| Camada {
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
            invertida: false,
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
    fn a_receita_vai_e_volta_pelo_json() {
        let receita = ReceitaLocal {
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
        let json = receita.em_json().expect("não vazia");
        assert_eq!(ReceitaLocal::de_json(&json), Ok(receita));
        assert!(json.contains("\"tipo\":\"pincel\""), "{json}");
    }

    #[test]
    fn receita_vazia_nao_grava_nada() {
        assert_eq!(ReceitaLocal::default().em_json(), None);
    }

    /// Versão mais nova não é lida — e quem abre a foto guarda o texto intacto.
    #[test]
    fn versao_nova_e_recusada_e_nao_vira_vazia() {
        assert_eq!(
            ReceitaLocal::de_json(r#"{"versao": 2, "camadas": []}"#),
            Err(ErroDaReceita::VersaoNova(2))
        );
        assert!(matches!(
            ReceitaLocal::de_json("não é json"),
            Err(ErroDaReceita::Invalida(_))
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
        let r = ReceitaLocal::de_json(&json.to_string()).unwrap();
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
