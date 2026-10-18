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
//!
//! # Exemplo
//!
//! O seletor do editor guarda a **chave** do método nas preferências e volta
//! dela ao abrir:
//!
//! ```rust
//! use preenchimento::Metodo;
//!
//! // O PatchMatch existe em toda compilação, e vem primeiro no seletor.
//! assert_eq!(Metodo::disponiveis()[0], Metodo::PatchMatch);
//! assert_eq!(Metodo::da_chave("patchmatch"), Some(Metodo::PatchMatch));
//!
//! // Chave desconhecida (ou de um método fora desta compilação) não vira nada.
//! assert_eq!(Metodo::da_chave("photoshop"), None);
//!
//! // A ida e volta vale para todo método disponível.
//! for metodo in Metodo::disponiveis() {
//!     assert_eq!(Metodo::da_chave(metodo.chave()), Some(metodo));
//! }
//! ```

#![deny(missing_docs)]

use std::sync::atomic::{AtomicBool, Ordering};

pub mod patchmatch;

#[cfg(feature = "lama")]
pub mod lama;

/// Os métodos que o editor oferece — só os que têm motor de verdade.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Metodo {
    /// A síntese por patches do `revelacao-core` — sem modelo, respeita a
    /// região de amostragem. Sempre disponível.
    PatchMatch,
    /// A rede LaMa em ONNX, pelo `ia-local`. Só existe com a feature `lama`
    /// (ligada por padrão) e só roda com o modelo instalado.
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

    /// O nome estável do método, para as preferências e o registro
    /// (`"patchmatch"`, `"lama"`). Não muda com o texto da tela.
    pub fn chave(self) -> &'static str {
        match self {
            Metodo::PatchMatch => "patchmatch",
            Metodo::LaMa => "lama",
        }
    }

    /// O método de uma [`chave`](Self::chave) gravada. `None` se a chave não
    /// é conhecida **ou** se o método não está nesta compilação — a
    /// preferência de um balcão com LaMa não quebra um sem ela; quem chama
    /// cai no primeiro de [`disponiveis`](Self::disponiveis).
    pub fn da_chave(chave: &str) -> Option<Metodo> {
        Metodo::disponiveis()
            .into_iter()
            .find(|m| m.chave() == chave)
    }

    /// O nome que o operador lê no seletor.
    pub fn nome(self) -> &'static str {
        match self {
            Metodo::PatchMatch => "Conteúdo tradicional (PatchMatch)",
            Metodo::LaMa => "IA local (LaMa)",
        }
    }

    /// O que o método sabe fazer — a mesma resposta de
    /// [`Motor::capacidades`], sem precisar de um motor montado.
    pub fn capacidades(self) -> Capacidades {
        match self {
            Metodo::PatchMatch => Capacidades {
                amostragem: true,
                lado_nativo: None,
                precisa_de_modelo: false,
                progresso_interno: true,
                contexto_ajustavel: false,
                adaptacao_de_cor: true,
            },
            Metodo::LaMa => Capacidades {
                amostragem: false,
                lado_nativo: Some(512),
                precisa_de_modelo: true,
                progresso_interno: false,
                contexto_ajustavel: true,
                adaptacao_de_cor: false,
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
    /// Só roda com um arquivo de modelo instalado: a tela oferece baixar ou
    /// importar antes de habilitar o botão.
    pub precisa_de_modelo: bool,
    /// Informa progresso por etapa; sem isso, a barra é indeterminada.
    pub progresso_interno: bool,
    /// Aceita a margem de contexto em volta do destino.
    pub contexto_ajustavel: bool,
    /// Ajusta o tom do remendo à borda (a "Adaptação de cor" do Photoshop).
    pub adaptacao_de_cor: bool,
}

/// A entrada de um motor: a região de trabalho e as máscaras dela.
pub struct Entrada<'a> {
    /// RGBA `largura × altura` (o alfa não conta).
    pub rgba: &'a [u8],
    /// Largura da região, em pixels.
    pub largura: u32,
    /// Altura da região, em pixels.
    pub altura: u32,
    /// O destino: o que será reconstruído (um por pixel).
    pub destino: &'a [bool],
    /// De onde as fontes podem vir (`None`: tudo fora do destino). Motor sem
    /// [`Capacidades::amostragem`] ignora.
    pub amostragem: Option<&'a [bool]>,
    /// A semente do sorteio: a mesma entrada com a mesma semente dá o mesmo
    /// remendo (o PatchMatch é aleatório; o LaMa ignora).
    pub semente: u64,
    /// A margem de contexto em volta do destino, em frações do lado dele
    /// (0,5 = meio destino de cada lado). Motor sem
    /// [`Capacidades::contexto_ajustavel`] ignora.
    pub contexto: f32,
    /// Ajustar o tom do remendo à borda. Motor sem
    /// [`Capacidades::adaptacao_de_cor`] ignora.
    pub adaptar_cor: bool,
}

/// O remendo: a caixa do destino, em coordenadas da região de entrada. Fora
/// do destino, os pixels são os da própria entrada — quem aplica usa o peso.
#[derive(Clone, Debug, PartialEq)]
pub struct Saida {
    /// A coluna do canto de cima à esquerda da caixa, na região de entrada.
    pub x0: u32,
    /// A linha do canto de cima à esquerda da caixa.
    pub y0: u32,
    /// Largura da caixa, em pixels.
    pub largura: u32,
    /// Altura da caixa, em pixels.
    pub altura: u32,
    /// RGBA `largura × altura`, alfa 255.
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
    /// De 0 a 1, ou `None` quando o motor não sabe quanto falta.
    pub fracao: Option<f32>,
    /// O que está acontecendo, em frase para a tela ("Textura", "Carregando
    /// o modelo").
    pub etapa: String,
}

/// Cancelamento cooperativo e aviso de progresso.
pub struct Controle<'a> {
    /// Ligado pela tela para pedir o fim; o motor olha quando pode e
    /// devolve [`Erro::Cancelado`].
    pub cancelado: &'a AtomicBool,
    /// Chamado da thread do motor a cada etapa: tem de ser barato e não
    /// pode esperar pela tela.
    pub progresso: &'a (dyn Fn(Progresso) + Sync),
}

impl Controle<'_> {
    /// Se a tela já pediu o fim.
    pub fn foi_cancelado(&self) -> bool {
        self.cancelado.load(Ordering::Relaxed)
    }
}

/// Por que não saiu remendo — cada um com a sua mensagem para o operador.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Erro {
    /// A máscara de destino está vazia: não há o que reconstruir.
    SemDestino,
    /// Nenhuma fonte válida na região de amostragem.
    SemFontes,
    /// O operador cancelou antes do fim.
    Cancelado,
    /// O modelo do método não está instalado.
    ModeloAusente,
    /// O arquivo do modelo não abre (corrompido ou incompatível).
    ModeloInvalido(String),
    /// O backend pedido não está disponível nesta máquina.
    BackendIndisponivel {
        /// O nome do backend, como a tela o mostra.
        backend: String,
        /// O porquê, como o runtime o disse.
        motivo: String,
    },
    /// Erro do próprio processamento (memória, runtime).
    Processamento(String),
}

impl Erro {
    /// A frase para o operador, em português e com o que fazer a seguir
    /// quando há o que fazer.
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
///
/// `Send + Sync` porque o editor monta o motor uma vez e o usa da thread de
/// trabalho.
pub trait Motor: Send + Sync {
    /// O método que este motor implementa.
    fn metodo(&self) -> Metodo;

    /// O que o motor sabe fazer — por padrão, as do seu [`Metodo`].
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

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn a_chave_de_cada_metodo_disponivel_volta_ao_mesmo_metodo() {
        let disponiveis = Metodo::disponiveis();
        assert_eq!(
            disponiveis[0],
            Metodo::PatchMatch,
            "o tradicional vem primeiro"
        );
        assert_eq!(disponiveis.contains(&Metodo::LaMa), cfg!(feature = "lama"));
        for m in disponiveis {
            assert_eq!(Metodo::da_chave(m.chave()), Some(m));
            assert!(!m.nome().is_empty());
        }
        assert_eq!(Metodo::da_chave("photoshop"), None);
        assert_eq!(Metodo::da_chave(""), None);
    }

    #[test]
    fn as_capacidades_dizem_o_que_a_tabela_do_crate_diz() {
        let pm = Metodo::PatchMatch.capacidades();
        assert!(pm.amostragem && pm.progresso_interno && pm.adaptacao_de_cor);
        assert!(!pm.precisa_de_modelo && !pm.contexto_ajustavel);
        assert_eq!(pm.lado_nativo, None, "trabalha na resolução da foto");

        let lama = Metodo::LaMa.capacidades();
        assert!(!lama.amostragem && !lama.progresso_interno && !lama.adaptacao_de_cor);
        assert!(lama.precisa_de_modelo && lama.contexto_ajustavel);
        assert_eq!(lama.lado_nativo, Some(512));
    }

    #[test]
    fn as_capacidades_padrao_de_um_motor_sao_as_do_metodo_dele() {
        struct Falso;
        impl Motor for Falso {
            fn metodo(&self) -> Metodo {
                Metodo::LaMa
            }
            fn preencher(&self, _: &Entrada, _: &Controle) -> Result<Saida, Erro> {
                Err(Erro::Cancelado)
            }
        }
        assert_eq!(Falso.capacidades(), Metodo::LaMa.capacidades());
    }

    #[test]
    fn todo_erro_tem_uma_mensagem_para_o_operador_com_o_detalhe() {
        let erros = [
            Erro::SemDestino,
            Erro::SemFontes,
            Erro::Cancelado,
            Erro::ModeloAusente,
            Erro::ModeloInvalido("cabeçalho truncado".into()),
            Erro::BackendIndisponivel {
                backend: "CoreML".into(),
                motivo: "sem Neural Engine".into(),
            },
            Erro::Processamento("sem memória".into()),
        ];
        for e in &erros {
            assert!(!e.mensagem().is_empty(), "{e:?}");
        }
        assert!(erros[4].mensagem().contains("cabeçalho truncado"));
        let backend = erros[5].mensagem();
        assert!(backend.contains("CoreML") && backend.contains("sem Neural Engine"));
        assert!(erros[6].mensagem().contains("sem memória"));
    }

    #[test]
    fn o_controle_le_o_cancelamento_na_hora() {
        let cancelado = AtomicBool::new(false);
        let controle = Controle {
            cancelado: &cancelado,
            progresso: &|_| {},
        };
        assert!(!controle.foi_cancelado());
        cancelado.store(true, Ordering::Relaxed);
        assert!(controle.foi_cancelado());
    }

    #[test]
    fn a_caixa_do_destino_e_inclusiva_e_vazia_sem_destino() {
        let (w, h) = (6u32, 4u32);
        let mut destino = vec![false; (w * h) as usize];
        assert_eq!(caixa_do_destino(&destino, w), None);

        destino[(w + 2) as usize] = true; // (2, 1)
        assert_eq!(
            caixa_do_destino(&destino, w),
            Some((2, 1, 2, 1)),
            "um pixel"
        );

        destino[(3 * w + 5) as usize] = true; // (5, 3), a quina
        destino[(2 * w) as usize] = true; // (0, 2)
        assert_eq!(caixa_do_destino(&destino, w), Some((0, 1, 5, 3)));
    }
}
