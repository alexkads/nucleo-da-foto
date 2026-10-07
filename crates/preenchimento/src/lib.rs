//! 🧩 Os motores do Preenchimento sensível ao conteúdo do editor em camadas.
//!
//! Um **motor** recebe uma região da foto e a máscara de destino (o que será
//! reconstruído) e devolve o remendo da caixa do destino. O contrato
//! ([`Motor`]) é o mesmo para a síntese por patches e para um modelo:
//!
//! | | PatchMatch | LaMa local |
//! |---|---|---|
//! | O que é | síntese por patches (`revelacao-core`) | rede neural (big-lama, ONNX) |
//! | Região de amostragem | **sim**, respeitada pixel a pixel | **não** — o modelo vê o contexto inteiro |
//! | Resolução | a da foto | 512 × 512 (reduz e amplia acima disso) |
//! | Precisa de modelo | não | sim (208 MB, baixado pelo operador) |
//! | Progresso | por etapa | indeterminado (o runtime não informa) |
//!
//! 🔑 **Três máscaras que não se confundem:** o **destino** e a
//! **amostragem** entram aqui; o **peso de aplicação** (a borda suave da
//! seleção) é de quem aplica o remendo — e nenhuma delas é a máscara de
//! camada do editor.
//!
//! O **modelo** (LaMa) não é o **backend** (CPU, CoreML, DirectML, CUDA): o
//! backend é onde o ONNX Runtime executa o modelo — isso, o catálogo de
//! modelos e a sessão reaproveitada são do crate `ia-local`, comum a outras
//! tarefas de IA.

use std::sync::atomic::{AtomicBool, Ordering};

pub mod patchmatch;

#[cfg(feature = "lama")]
pub mod lama;

/// Os métodos que o editor oferece — só os que têm motor de verdade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Metodo {
    PatchMatch,
    LaMa,
}

impl Metodo {
    /// Os disponíveis nesta compilação, na ordem do seletor.
    pub fn disponiveis() -> Vec<Metodo> {
        let mut m = vec![Metodo::PatchMatch];
        if cfg!(feature = "lama") {
            m.push(Metodo::LaMa);
        }
        m
    }

    pub fn chave(self) -> &'static str {
        match self {
            Metodo::PatchMatch => "patchmatch",
            Metodo::LaMa => "lama",
        }
    }

    pub fn da_chave(chave: &str) -> Option<Metodo> {
        Metodo::disponiveis()
            .into_iter()
            .find(|m| m.chave() == chave)
    }

    pub fn nome(self) -> &'static str {
        match self {
            Metodo::PatchMatch => "Conteúdo tradicional (PatchMatch)",
            Metodo::LaMa => "IA local (LaMa)",
        }
    }

    pub fn capacidades(self) -> Capacidades {
        match self {
            Metodo::PatchMatch => Capacidades {
                amostragem: true,
                lado_nativo: None,
                precisa_de_modelo: false,
                progresso_interno: true,
                contexto_ajustavel: false,
            },
            Metodo::LaMa => Capacidades {
                amostragem: false,
                lado_nativo: Some(512),
                precisa_de_modelo: true,
                progresso_interno: false,
                contexto_ajustavel: true,
            },
        }
    }
}

/// O que um motor sabe fazer — a tela mostra os controles por aqui.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capacidades {
    /// Respeita uma região de amostragem (de onde as fontes podem vir).
    pub amostragem: bool,
    /// A resolução em que o motor trabalha (`None`: a da foto). Acima dela,
    /// o recorte é reduzido e o resultado ampliado — a tela avisa.
    pub lado_nativo: Option<u32>,
    pub precisa_de_modelo: bool,
    /// Informa progresso por etapa; sem isso, a barra é indeterminada.
    pub progresso_interno: bool,
    /// Aceita a margem de contexto em volta do destino.
    pub contexto_ajustavel: bool,
}

/// A entrada de um motor: a região de trabalho e as máscaras dela.
pub struct Entrada<'a> {
    /// RGBA `largura × altura` (o alfa não conta).
    pub rgba: &'a [u8],
    pub largura: u32,
    pub altura: u32,
    /// O destino: o que será reconstruído (um por pixel).
    pub destino: &'a [bool],
    /// De onde as fontes podem vir (`None`: tudo fora do destino). Motor sem
    /// [`Capacidades::amostragem`] ignora.
    pub amostragem: Option<&'a [bool]>,
    pub semente: u64,
    /// A margem de contexto em volta do destino, em frações do lado dele
    /// (0,5 = meio destino de cada lado). Motor sem
    /// [`Capacidades::contexto_ajustavel`] ignora.
    pub contexto: f32,
}

/// O remendo: a caixa do destino, em coordenadas da região de entrada. Fora
/// do destino, os pixels são os da própria entrada — quem aplica usa o peso.
#[derive(Clone, Debug, PartialEq)]
pub struct Saida {
    pub x0: u32,
    pub y0: u32,
    pub largura: u32,
    pub altura: u32,
    pub rgba: Vec<u8>,
    /// Quanto o motor reduziu o recorte para caber na resolução dele (1 =
    /// resolução nativa da foto). Acima de 1, o miolo foi ampliado.
    pub reducao: f32,
    /// Onde rodou, como o runtime informou ("CPU", "CoreML" …).
    pub executado_em: String,
}

/// Onde a síntese está. `fracao = None`: o motor não informa (indeterminado).
#[derive(Clone, Debug, PartialEq)]
pub struct Progresso {
    pub fracao: Option<f32>,
    pub etapa: String,
}

/// Cancelamento cooperativo e aviso de progresso.
pub struct Controle<'a> {
    pub cancelado: &'a AtomicBool,
    pub progresso: &'a (dyn Fn(Progresso) + Sync),
}

impl Controle<'_> {
    pub fn foi_cancelado(&self) -> bool {
        self.cancelado.load(Ordering::Relaxed)
    }
}

/// Por que não saiu remendo — cada um com a sua mensagem para o operador.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Erro {
    SemDestino,
    /// Nenhuma fonte válida na região de amostragem.
    SemFontes,
    Cancelado,
    /// O modelo do método não está instalado.
    ModeloAusente,
    /// O arquivo do modelo não abre (corrompido ou incompatível).
    ModeloInvalido(String),
    /// O backend pedido não está disponível nesta máquina.
    BackendIndisponivel {
        backend: String,
        motivo: String,
    },
    /// Erro do próprio processamento (memória, runtime).
    Processamento(String),
}

impl Erro {
    pub fn mensagem(&self) -> String {
        match self {
            Erro::SemDestino => "Selecione o que será removido".into(),
            Erro::SemFontes => {
                "Não há de onde tirar o preenchimento: pinte mais área verde (de onde os pedaços podem vir)".into()
            }
            Erro::Cancelado => "Cancelado".into(),
            Erro::ModeloAusente => "O modelo da IA não está instalado — baixe ou importe nas opções".into(),
            Erro::ModeloInvalido(m) => format!("O arquivo do modelo não serve: {m}"),
            Erro::BackendIndisponivel { backend, motivo } => {
                format!("{backend} não está disponível nesta máquina ({motivo})")
            }
            Erro::Processamento(m) => format!("O processamento falhou: {m}"),
        }
    }
}

/// O contrato de um motor de preenchimento.
pub trait Motor: Send + Sync {
    fn metodo(&self) -> Metodo;

    fn capacidades(&self) -> Capacidades {
        self.metodo().capacidades()
    }

    /// Reconstrói o destino. Roda fora da thread da tela; olha o
    /// cancelamento sempre que o motor permite.
    fn preencher(&self, entrada: &Entrada, controle: &Controle) -> Result<Saida, Erro>;
}

/// A caixa `(x0, y0, x1, y1)` (inclusiva) do destino, se houver.
pub(crate) fn caixa_do_destino(destino: &[bool], largura: u32) -> Option<(u32, u32, u32, u32)> {
    let mut caixa: Option<(u32, u32, u32, u32)> = None;
    for (i, _) in destino.iter().enumerate().filter(|(_, d)| **d) {
        let (x, y) = (i as u32 % largura, i as u32 / largura);
        caixa = Some(match caixa {
            None => (x, y, x, y),
            Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x), d.max(y)),
        });
    }
    caixa
}
