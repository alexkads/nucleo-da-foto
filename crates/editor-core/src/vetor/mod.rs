//! ✒️ Geometria vetorial: os caminhos da Caneta, como os do Photoshop.
//!
//! ```text
//! Caminho ── nome, id, regra de preenchimento
//!   └─ Subcaminho (componente) ── aberto | fechado, operação do componente
//!        └─ Âncora ── ponto, alça de entrada, alça de saída, ligação das alças
//! ```
//!
//! 🔑 **Tudo em pixels do documento, fracionário (`f64`).** Nada aqui sabe de
//! tela, zoom ou giro: a janela converte o ponteiro antes de chegar, e as
//! tolerâncias de clique chegam já divididas pela escala da vista.
//!
//! 🔑 **O segmento é uma Bézier cúbica** da âncora `a` até a seguinte `b`:
//! `P0 = a.ponto`, `P1 = a.saida`, `P2 = b.entrada`, `P3 = b.ponto`. A alça
//! ausente vale o ponto da própria âncora (o controle coincide com ela) — um
//! segmento sem nenhuma alça é uma reta. O subcaminho fechado tem mais um
//! segmento, da última âncora à primeira.
//!
//! 🔑 **A ligação das alças é uma regra de edição, não a forma de agora**:
//! - [`Ligacao::Canto`] — as alças são independentes (ou não existem);
//! - [`Ligacao::Suave`] — colineares e opostas, **cada uma com o seu
//!   comprimento**: mexer numa gira a outra sem mudar o tamanho dela;
//! - [`Ligacao::Simetrico`] — opostas e do mesmo comprimento.
//!
//! O arrasto que cria uma âncora puxa as duas alças iguais (é o gesto), mas a
//! âncora nasce [`Ligacao::Suave`]: ajustar depois uma das alças não obriga a
//! outra a acompanhar o comprimento.
//!
//! O que fica em outros arquivos: a geometria das curvas (`geometria.rs`), a
//! rasterização em cobertura (`cobertura.rs`), as edições (`edicao.rs`) e a
//! máquina de estados da ferramenta (`caneta.rs`).

pub mod caneta;
pub mod cobertura;
pub mod edicao;
pub mod geometria;
#[cfg(test)]
mod testes_da_caneta;

use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::retangulo::Retangulo;
use crate::selecao::Selecao;

/// Um ponto no espaço do documento (pixels da foto, fracionário).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ponto {
    pub x: f64,
    pub y: f64,
}

impl Ponto {
    pub const fn novo(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn mais(self, d: (f64, f64)) -> Self {
        Self::novo(self.x + d.0, self.y + d.1)
    }

    pub fn menos(self, outro: Ponto) -> (f64, f64) {
        (self.x - outro.x, self.y - outro.y)
    }

    pub fn distancia(self, outro: Ponto) -> f64 {
        (self.x - outro.x).hypot(self.y - outro.y)
    }

    /// O ponto do outro lado de `centro`, à mesma distância.
    pub fn espelhado_em(self, centro: Ponto) -> Ponto {
        Ponto::novo(2.0 * centro.x - self.x, 2.0 * centro.y - self.y)
    }

    pub(crate) fn kurbo(self) -> kurbo::Point {
        kurbo::Point::new(self.x, self.y)
    }

    pub(crate) fn de_kurbo(p: kurbo::Point) -> Self {
        Self::novo(p.x, p.y)
    }
}

/// Como as duas alças de uma âncora se prendem uma à outra ao serem editadas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ligacao {
    /// Independentes (o ponto de canto do Photoshop; com ou sem alças).
    #[default]
    Canto,
    /// Colineares e opostas, cada uma com o seu comprimento.
    Suave,
    /// Opostas e de comprimento igual.
    Simetrico,
}

impl Ligacao {
    pub fn nome(self) -> &'static str {
        match self {
            Ligacao::Canto => "Canto",
            Ligacao::Suave => "Suave",
            Ligacao::Simetrico => "Simétrico",
        }
    }
}

/// Uma âncora do subcaminho.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ancora {
    /// Estável dentro do caminho: sobrevive a edições, ao desfazer e à
    /// gravação.
    pub id: u64,
    pub ponto: Ponto,
    /// O controle do segmento que **chega** à âncora (P2 dele). `None` = o
    /// controle coincide com a âncora.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrada: Option<Ponto>,
    /// O controle do segmento que **sai** da âncora (P1 dele).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saida: Option<Ponto>,
    #[serde(default)]
    pub ligacao: Ligacao,
}

/// Qual alça.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lado {
    Entrada,
    Saida,
}

impl Lado {
    pub fn oposto(self) -> Lado {
        match self {
            Lado::Entrada => Lado::Saida,
            Lado::Saida => Lado::Entrada,
        }
    }
}

/// Uma alça mais curta que isto (em pixels do documento) conta como ausente.
pub const ALCA_NULA: f64 = 1e-6;

impl Ancora {
    pub fn de_canto(id: u64, ponto: Ponto) -> Self {
        Self {
            id,
            ponto,
            entrada: None,
            saida: None,
            ligacao: Ligacao::Canto,
        }
    }

    pub fn alca(&self, lado: Lado) -> Option<Ponto> {
        match lado {
            Lado::Entrada => self.entrada,
            Lado::Saida => self.saida,
        }
    }

    pub fn alca_mut(&mut self, lado: Lado) -> &mut Option<Ponto> {
        match lado {
            Lado::Entrada => &mut self.entrada,
            Lado::Saida => &mut self.saida,
        }
    }

    /// O controle efetivo do lado: a alça, ou a própria âncora.
    pub fn controle(&self, lado: Lado) -> Ponto {
        self.alca(lado).unwrap_or(self.ponto)
    }

    /// Alças que coincidem com a âncora viram ausentes.
    pub fn normalizar(&mut self) {
        let p = self.ponto;
        for lado in [Lado::Entrada, Lado::Saida] {
            let alca = self.alca_mut(lado);
            if alca.is_some_and(|a| a.distancia(p) < ALCA_NULA) {
                *alca = None;
            }
        }
    }

    /// A âncora andou: as alças vão junto.
    pub fn transladar(&mut self, d: (f64, f64)) {
        self.ponto = self.ponto.mais(d);
        self.entrada = self.entrada.map(|a| a.mais(d));
        self.saida = self.saida.map(|a| a.mais(d));
    }

    /// Coloca a alça `lado` em `alvo` e aplica a ligação à outra:
    /// [`Ligacao::Suave`] gira a oposta para ficar colinear mantendo o
    /// comprimento dela; [`Ligacao::Simetrico`] a espelha; [`Ligacao::Canto`]
    /// não a toca. `independente` (o ⌥ do Photoshop) ignora a ligação **neste
    /// gesto** — quem chama decide se a âncora vira canto.
    pub fn mover_alca(&mut self, lado: Lado, alvo: Ponto, independente: bool) {
        *self.alca_mut(lado) = Some(alvo);
        if !independente {
            self.aplicar_ligacao_a_partir_de(lado);
        }
        self.normalizar();
    }

    /// Recoloca a alça oposta a `lado` conforme a ligação.
    pub fn aplicar_ligacao_a_partir_de(&mut self, lado: Lado) {
        let p = self.ponto;
        let Some(mexida) = self.alca(lado) else {
            return;
        };
        let (dx, dy) = mexida.menos(p);
        let comprimento = dx.hypot(dy);
        let oposta = lado.oposto();
        match self.ligacao {
            Ligacao::Canto => {}
            Ligacao::Simetrico => {
                *self.alca_mut(oposta) = Some(mexida.espelhado_em(p));
            }
            Ligacao::Suave => {
                let Some(outra) = self.alca(oposta) else {
                    return;
                };
                if comprimento < ALCA_NULA {
                    return;
                }
                let l = outra.distancia(p);
                *self.alca_mut(oposta) = Some(Ponto::novo(
                    p.x - dx / comprimento * l,
                    p.y - dy / comprimento * l,
                ));
            }
        }
    }

    /// As duas alças são colineares e opostas (a âncora **é** suave, seja qual
    /// for a regra de edição).
    pub fn alcas_colineares(&self) -> bool {
        let (Some(e), Some(s)) = (self.entrada, self.saida) else {
            return false;
        };
        let (a, b) = (e.menos(self.ponto), s.menos(self.ponto));
        let (la, lb) = (a.0.hypot(a.1), b.0.hypot(b.1));
        if la < ALCA_NULA || lb < ALCA_NULA {
            return false;
        }
        let cruz = (a.0 * b.1 - a.1 * b.0) / (la * lb);
        let ponto = (a.0 * b.0 + a.1 * b.1) / (la * lb);
        cruz.abs() < 1e-6 && ponto < 0.0
    }
}

/// Como o componente entra na área do caminho (os botões "Operações do
/// caminho" do Photoshop).
///
/// 🔑 **Ordem de avaliação**: de baixo para cima, na ordem dos subcaminhos. A
/// área começa vazia; cada componente, rasterizado sozinho com a regra de
/// preenchimento do caminho, se junta ao acumulado pela operação dele. O
/// primeiro componente com [`Self::Subtrair`] parte da área cheia (subtrair do
/// nada não deixaria nada); com [`Self::Intersectar`] ou [`Self::Excluir`],
/// conta como [`Self::Somar`] (não há com o que cruzar).
///
/// Com cobertura `a` (acumulado) e `b` (componente) em `0..=1`: somar é
/// `max(a, b)`, subtrair `a·(1 − b)`, intersectar `min(a, b)`, excluir
/// `|a − b|` — as mesmas contas da seleção (somar, tirar, cruzar), e exatas
/// para pixels inteiros dentro ou fora.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperacaoDoComponente {
    #[default]
    Somar,
    Subtrair,
    Intersectar,
    Excluir,
}

impl OperacaoDoComponente {
    pub const TODAS: [OperacaoDoComponente; 4] = [
        OperacaoDoComponente::Somar,
        OperacaoDoComponente::Subtrair,
        OperacaoDoComponente::Intersectar,
        OperacaoDoComponente::Excluir,
    ];

    pub fn nome(self) -> &'static str {
        match self {
            OperacaoDoComponente::Somar => "Combinar formas",
            OperacaoDoComponente::Subtrair => "Subtrair forma da frente",
            OperacaoDoComponente::Intersectar => "Fazer interseção das áreas",
            OperacaoDoComponente::Excluir => "Excluir sobreposição",
        }
    }

    /// A conta em `0..=255`.
    #[inline]
    pub fn juntar(self, a: u8, b: u8) -> u8 {
        match self {
            OperacaoDoComponente::Somar => a.max(b),
            OperacaoDoComponente::Subtrair => ((a as u32 * (255 - b as u32) + 127) / 255) as u8,
            OperacaoDoComponente::Intersectar => a.min(b),
            OperacaoDoComponente::Excluir => a.abs_diff(b),
        }
    }
}

/// A regra que diz o que é "dentro" de um componente que se cruza consigo
/// mesmo (um oito, uma estrela de cinco pontas desenhada de uma vez).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegraDePreenchimento {
    /// Diferente de zero: dentro é onde o caminho dá voltas num sentido mais
    /// que no outro — a estrela sai cheia.
    #[default]
    NaoZero,
    /// Par ou ímpar: dentro é onde uma reta até o infinito cruza a borda um
    /// número ímpar de vezes — o miolo da estrela sai vazado.
    ParImpar,
}

/// Um componente: um subcaminho aberto ou fechado.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Subcaminho {
    pub id: u64,
    pub ancoras: Vec<Ancora>,
    pub fechado: bool,
    #[serde(default)]
    pub operacao: OperacaoDoComponente,
}

/// Um segmento cúbico (`P0`, `P1`, `P2`, `P3`) e as âncoras das pontas
/// (índices no subcaminho).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segmento {
    pub de: usize,
    pub ate: usize,
    pub p: [Ponto; 4],
}

impl Segmento {
    /// Sem alça nas duas pontas: uma reta.
    pub fn reto(&self) -> bool {
        self.p[1] == self.p[0] && self.p[2] == self.p[3]
    }

    pub fn kurbo(&self) -> kurbo::CubicBez {
        kurbo::CubicBez::new(
            self.p[0].kurbo(),
            self.p[1].kurbo(),
            self.p[2].kurbo(),
            self.p[3].kurbo(),
        )
    }
}

impl Subcaminho {
    pub fn novo(id: u64, operacao: OperacaoDoComponente) -> Self {
        Self {
            id,
            ancoras: Vec::new(),
            fechado: false,
            operacao,
        }
    }

    /// Quantos segmentos: `n − 1` aberto, `n` fechado (com 2+ âncoras).
    pub fn quantos_segmentos(&self) -> usize {
        let n = self.ancoras.len();
        match (n, self.fechado) {
            (0 | 1, _) => 0,
            (_, true) => n,
            (_, false) => n - 1,
        }
    }

    /// O segmento `i` (o fechado termina com o da última à primeira).
    pub fn segmento(&self, i: usize) -> Option<Segmento> {
        if i >= self.quantos_segmentos() {
            return None;
        }
        let de = i;
        let ate = (i + 1) % self.ancoras.len();
        let (a, b) = (&self.ancoras[de], &self.ancoras[ate]);
        Some(Segmento {
            de,
            ate,
            p: [
                a.ponto,
                a.controle(Lado::Saida),
                b.controle(Lado::Entrada),
                b.ponto,
            ],
        })
    }

    pub fn segmentos(&self) -> impl Iterator<Item = Segmento> + '_ {
        (0..self.quantos_segmentos()).filter_map(|i| self.segmento(i))
    }

    pub fn indice_da_ancora(&self, id: u64) -> Option<usize> {
        self.ancoras.iter().position(|a| a.id == id)
    }

    /// O segmento de fechamento **virtual** do aberto — a reta da última à
    /// primeira âncora, que o preenchimento usa sem mudar o caminho.
    pub fn fechamento_virtual(&self) -> Option<Segmento> {
        if self.fechado || self.ancoras.len() < 2 {
            return None;
        }
        let ate = 0;
        let de = self.ancoras.len() - 1;
        let (a, b) = (self.ancoras[de].ponto, self.ancoras[ate].ponto);
        Some(Segmento {
            de,
            ate,
            p: [a, a, b, b],
        })
    }

    /// A curva do componente em `kurbo` (fechada se `fechar`, mesmo aberta —
    /// o fechamento virtual).
    pub fn bez_path(&self, fechar: bool) -> kurbo::BezPath {
        let mut c = kurbo::BezPath::new();
        let Some(primeira) = self.ancoras.first() else {
            return c;
        };
        c.move_to(primeira.ponto.kurbo());
        for s in self.segmentos() {
            c.curve_to(s.p[1].kurbo(), s.p[2].kurbo(), s.p[3].kurbo());
        }
        if self.fechado || fechar {
            c.close_path();
        }
        c
    }

    /// Âncoras e alças, todas (o limite de controle, que contém a curva).
    pub fn pontos_de_controle(&self) -> impl Iterator<Item = Ponto> + '_ {
        self.ancoras
            .iter()
            .flat_map(|a| [Some(a.ponto), a.entrada, a.saida])
            .flatten()
    }

    /// A caixa exata da curva (extremos das cúbicas), `(x0, y0, x1, y1)`.
    pub fn limites(&self) -> Option<(f64, f64, f64, f64)> {
        use kurbo::ParamCurveExtrema as _;
        let mut caixa: Option<kurbo::Rect> = None;
        for a in &self.ancoras {
            let r = kurbo::Rect::from_points(a.ponto.kurbo(), a.ponto.kurbo());
            caixa = Some(caixa.map_or(r, |c| c.union(r)));
        }
        for s in self.segmentos() {
            let r = s.kurbo().bounding_box();
            caixa = Some(caixa.map_or(r, |c| c.union(r)));
        }
        caixa.map(|r| (r.x0, r.y0, r.x1, r.y1))
    }
}

/// Onde um caminho mora: o de trabalho, um nomeado (pelo id) ou a máscara
/// vetorial de uma camada (pelo índice).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "onde", content = "id", rename_all = "snake_case")]
pub enum LugarDoCaminho {
    Trabalho,
    Nomeado(u64),
    Mascara(usize),
}

/// Um caminho: um ou mais componentes, com nome e id estáveis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Caminho {
    pub id: u64,
    pub nome: String,
    pub subcaminhos: Vec<Subcaminho>,
    #[serde(default)]
    pub regra: RegraDePreenchimento,
    /// O próximo id livre para âncoras e subcaminhos deste caminho.
    pub proximo_id: u64,
}

/// O nome do caminho de trabalho, como no Photoshop.
pub const NOME_DO_TRABALHO: &str = "Caminho de trabalho";

impl Caminho {
    pub fn novo(id: u64, nome: &str) -> Self {
        Self {
            id,
            nome: nome.into(),
            subcaminhos: Vec::new(),
            regra: RegraDePreenchimento::NaoZero,
            proximo_id: 1,
        }
    }

    /// Um id novo para âncora ou subcaminho.
    pub fn gerar_id(&mut self) -> u64 {
        let id = self.proximo_id;
        self.proximo_id += 1;
        id
    }

    pub fn vazio(&self) -> bool {
        self.subcaminhos.iter().all(|s| s.ancoras.is_empty())
    }

    pub fn subcaminho(&self, id: u64) -> Option<&Subcaminho> {
        self.subcaminhos.iter().find(|s| s.id == id)
    }

    pub fn subcaminho_mut(&mut self, id: u64) -> Option<&mut Subcaminho> {
        self.subcaminhos.iter_mut().find(|s| s.id == id)
    }

    pub fn indice_do_subcaminho(&self, id: u64) -> Option<usize> {
        self.subcaminhos.iter().position(|s| s.id == id)
    }

    pub fn ancora(&self, r: RefAncora) -> Option<&Ancora> {
        self.subcaminho(r.sub)?
            .ancoras
            .iter()
            .find(|a| a.id == r.ancora)
    }

    pub fn ancora_mut(&mut self, r: RefAncora) -> Option<&mut Ancora> {
        self.subcaminho_mut(r.sub)?
            .ancoras
            .iter_mut()
            .find(|a| a.id == r.ancora)
    }

    /// A caixa da curva, em pixels do documento.
    pub fn limites(&self) -> Option<(f64, f64, f64, f64)> {
        self.subcaminhos
            .iter()
            .filter_map(Subcaminho::limites)
            .reduce(|a, b| (a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3)))
    }

    /// A caixa da curva em pixels inteiros dentro da foto (com um pixel de
    /// folga para o antisserrilhado) — o que muda na foto quando o caminho de
    /// uma máscara muda.
    pub fn retangulo(&self, largura: u32, altura: u32) -> Retangulo {
        let Some((x0, y0, x1, y1)) = self.limites() else {
            return Retangulo::default();
        };
        let (x0, y0) = ((x0.floor() - 1.0).max(0.0), (y0.floor() - 1.0).max(0.0));
        let (x1, y1) = (
            (x1.ceil() + 1.0).min(largura as f64),
            (y1.ceil() + 1.0).min(altura as f64),
        );
        if x1 <= x0 || y1 <= y0 {
            return Retangulo::default();
        }
        Retangulo::novo(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
    }

    /// Algum componente tem área (duas âncoras ou mais).
    pub fn tem_area(&self) -> bool {
        self.subcaminhos.iter().any(|s| s.ancoras.len() >= 2)
    }

    /// Como máscara, só a caixa da curva pode ser revelada: tem área e não
    /// começa subtraindo (o que partiria da foto cheia).
    pub fn alcanca_so_a_caixa(&self) -> bool {
        self.tem_area()
            && self
                .subcaminhos
                .iter()
                .find(|s| s.ancoras.len() >= 2)
                .is_some_and(|s| s.operacao != OperacaoDoComponente::Subtrair)
    }

    /// Uma impressão da geometria (pontos, alças, fechamento, operações e
    /// regra): o que invalida a cobertura guardada. Nome e ids não entram.
    pub fn assinatura(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.regra.hash(&mut h);
        for s in &self.subcaminhos {
            s.fechado.hash(&mut h);
            s.operacao.hash(&mut h);
            s.ancoras.len().hash(&mut h);
            for a in &s.ancoras {
                for p in [Some(a.ponto), a.entrada, a.saida] {
                    match p {
                        Some(p) => {
                            1u8.hash(&mut h);
                            p.x.to_bits().hash(&mut h);
                            p.y.to_bits().hash(&mut h);
                        }
                        None => 0u8.hash(&mut h),
                    }
                }
            }
        }
        h.finish()
    }
}

/// Uma âncora dentro do caminho: o subcaminho e ela, pelos ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RefAncora {
    pub sub: u64,
    pub ancora: u64,
}

/// Os caminhos do documento: o de trabalho (provisório, mas salvo para
/// recuperação) e os nomeados, na ordem do painel Caminhos. As máscaras
/// vetoriais moram nas camadas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Caminhos {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trabalho: Option<Caminho>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nomeados: Vec<Caminho>,
    /// O próximo id livre para caminhos.
    #[serde(default = "um")]
    pub proximo_id: u64,
}

fn um() -> u64 {
    1
}

impl Default for Caminhos {
    fn default() -> Self {
        Self::novo()
    }
}

impl Caminhos {
    pub fn novo() -> Self {
        Self {
            trabalho: None,
            nomeados: Vec::new(),
            proximo_id: 1,
        }
    }

    pub fn gerar_id(&mut self) -> u64 {
        let id = self.proximo_id.max(1);
        self.proximo_id = id + 1;
        id
    }

    pub fn vazio(&self) -> bool {
        self.trabalho.is_none() && self.nomeados.is_empty()
    }

    pub fn nomeado(&self, id: u64) -> Option<&Caminho> {
        self.nomeados.iter().find(|c| c.id == id)
    }

    pub fn indice_do_nomeado(&self, id: u64) -> Option<usize> {
        self.nomeados.iter().position(|c| c.id == id)
    }

    /// "Caminho N", com o N seguinte ao maior que já existe.
    pub fn proximo_nome(&self) -> String {
        let maior = self
            .nomeados
            .iter()
            .filter_map(|c| c.nome.strip_prefix("Caminho ")?.trim().parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("Caminho {}", maior + 1)
    }
}

/// A máscara vetorial de uma camada: o caminho que recorta a camada, editável
/// para sempre. Nunca muda os pixels — entra só na conta da composição.
///
/// 🔑 **A cobertura é rasterizada por demanda e reaproveitada**: em
/// resolução cheia do documento, com antisserrilhado, guardada pela
/// [`Caminho::assinatura`]. A vista reduzida, a lupa e a exportação leem a
/// mesma cobertura, então a borda é a mesma em qualquer zoom.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MascaraVetorial {
    pub caminho: Caminho,
    /// Desligada (⇧ + clique na miniatura), a camada aparece inteira e o
    /// caminho continua lá.
    #[serde(default = "verdadeiro")]
    pub ativa: bool,
    /// A corrente com a camada: o Mover e o ⌘T levam o caminho junto.
    #[serde(default = "verdadeiro")]
    pub vinculada: bool,
    /// `0..=1`: quanto o lado de fora esconde (as Propriedades da máscara
    /// vetorial). Com 0,5, fora da curva a camada aparece pela metade.
    #[serde(default = "um_f32", skip_serializing_if = "e_um")]
    pub densidade: f32,
    /// A difusão da borda, em pixels do documento — a borda da curva vira
    /// rampa sem mudar o caminho.
    #[serde(default, skip_serializing_if = "e_zero")]
    pub difusao: f32,
    #[serde(skip)]
    cache: CacheDaCobertura,
}

fn verdadeiro() -> bool {
    true
}

fn um_f32() -> f32 {
    1.0
}

fn e_um(v: &f32) -> bool {
    *v == 1.0
}

fn e_zero(v: &f32) -> bool {
    *v == 0.0
}

/// A maior difusão da máscara vetorial, em pixels (a do Photoshop é 1000;
/// aqui, a da seleção).
pub const DIFUSAO_MAXIMA_VETORIAL: f32 = 250.0;

impl PartialEq for MascaraVetorial {
    fn eq(&self, outra: &Self) -> bool {
        self.caminho == outra.caminho
            && self.ativa == outra.ativa
            && self.vinculada == outra.vinculada
            && self.densidade == outra.densidade
            && self.difusao == outra.difusao
    }
}

/// A cobertura guardada: a assinatura do caminho (com densidade e
/// difusão), o tamanho da foto e a máscara. Dividida entre as cópias da máscara (o histórico guarda cópias):
/// cada uma confere a assinatura antes de usar.
#[derive(Clone, Debug, Default)]
struct CacheDaCobertura(Arc<Mutex<Option<CoberturaGuardada>>>);

/// A assinatura do caminho, a largura e a altura da foto, e a cobertura.
type CoberturaGuardada = (u64, u32, u32, Arc<Selecao>);

impl MascaraVetorial {
    /// A cobertura com a difusão (a borda vira rampa) e a densidade (o fora
    /// esconde só `densidade`): `255 − densidade · (255 − v)`.
    fn acabada(&self, s: Arc<Selecao>) -> Arc<Selecao> {
        let mut s = s;
        let raio = self.difusao.clamp(0.0, DIFUSAO_MAXIMA_VETORIAL).round() as u32;
        if raio > 0 {
            s = Arc::new(s.difusa(raio));
        }
        let d = self.densidade.clamp(0.0, 1.0);
        if d < 1.0 {
            let mut t = s.as_ref().clone();
            let vazia = Selecao::vazia(t.largura(), t.altura());
            t.combinar_com(&vazia, |v, _| 255 - ((255 - v) as f32 * d).round() as u8);
            s = Arc::new(t);
        }
        s
    }

    /// Quanto uma mudança da curva em `caixa` muda a foto: a difusão leva a
    /// rampa além da curva; com densidade abaixo de 100% ou máscara que vale
    /// fora da caixa, a foto inteira.
    pub fn alcance(&self, caixa: Retangulo, largura: u32, altura: u32) -> Retangulo {
        if self.densidade < 1.0 || !self.caminho.alcanca_so_a_caixa() {
            return Retangulo::inteiro(largura, altura);
        }
        if caixa.vazio() || self.difusao <= 0.0 {
            return caixa;
        }
        let m = (3.0 * self.difusao.min(DIFUSAO_MAXIMA_VETORIAL)).ceil() as u32 + 2;
        Retangulo::novo(
            caixa.x.saturating_sub(m),
            caixa.y.saturating_sub(m),
            caixa.largura + 2 * m,
            caixa.altura + 2 * m,
        )
        .limitado(largura, altura)
    }

    pub fn nova(caminho: Caminho) -> Self {
        Self {
            caminho,
            ativa: true,
            vinculada: true,
            densidade: 1.0,
            difusao: 0.0,
            cache: CacheDaCobertura::default(),
        }
    }

    /// A cobertura do caminho na foto `largura × altura`, de 0 (esconde) a
    /// 255 (revela). Rasterizada só quando a geometria mudou.
    pub fn cobertura(&self, largura: u32, altura: u32) -> Arc<Selecao> {
        let assinatura = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            self.caminho.assinatura().hash(&mut h);
            self.densidade.to_bits().hash(&mut h);
            self.difusao.to_bits().hash(&mut h);
            h.finish()
        };
        let mut guarda = self.cache.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((a, l, h, s)) = guarda.as_ref() {
            if *a == assinatura && *l == largura && *h == altura {
                return s.clone();
            }
        }
        // 🔑 A máscara vetorial sem nenhum componente com área revela tudo
        // (a "Revelar tudo" do Photoshop): criar a máscara e só depois
        // desenhar não some com a camada.
        let s = Arc::new(if self.caminho.tem_area() {
            cobertura::rasterizar(
                &self.caminho,
                largura,
                altura,
                &cobertura::Opcoes::da_mascara(),
            )
        } else {
            Selecao::tudo(largura, altura)
        });
        let s = self.acabada(s);
        *guarda = Some((assinatura, largura, altura, s.clone()));
        s
    }
}
