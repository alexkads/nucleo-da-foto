//! A execução de um modelo ONNX: o backend, a sessão reaproveitada e a
//! inferência cancelável.
//!
//! 🔑 **O modelo não é o backend.** O backend é onde o ONNX Runtime roda o
//! grafo: a CPU sempre; CoreML no macOS e DirectML no Windows quando
//! compilados e disponíveis; CUDA com a feature `cuda`. Um backend aceita
//! **parte** do grafo e devolve o resto à CPU — por isso a escolha automática
//! é de quem conhece o modelo e o mediu (a LaMa é mais rápida na CPU do Mac
//! do que no CoreML), e a tela diz onde rodou de verdade.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(any(target_os = "macos", windows, feature = "cuda"))]
use ort::ep::ExecutionProvider;
use ort::session::{RunOptions, Session};
use ort::value::Tensor;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Backend {
    /// O que o modelo recomenda (medido por quem o integrou).
    Automatico,
    Cpu,
    CoreMl,
    DirectMl,
    Cuda,
}

impl Backend {
    pub fn nome(self) -> &'static str {
        match self {
            Backend::Automatico => "Automático",
            Backend::Cpu => "CPU",
            Backend::CoreMl => "CoreML (Apple)",
            Backend::DirectMl => "DirectML (Windows)",
            Backend::Cuda => "CUDA (NVIDIA)",
        }
    }

    pub fn chave(self) -> &'static str {
        match self {
            Backend::Automatico => "automatico",
            Backend::Cpu => "cpu",
            Backend::CoreMl => "coreml",
            Backend::DirectMl => "directml",
            Backend::Cuda => "cuda",
        }
    }

    pub fn da_chave(chave: &str) -> Option<Backend> {
        Backend::compilados()
            .into_iter()
            .find(|b| b.chave() == chave)
    }

    /// Os que esta compilação conhece (a escolha manual oferece estes).
    pub fn compilados() -> Vec<Backend> {
        let mut v = vec![Backend::Automatico, Backend::Cpu];
        if cfg!(target_os = "macos") {
            v.push(Backend::CoreMl);
        }
        if cfg!(windows) {
            v.push(Backend::DirectMl);
        }
        if cfg!(feature = "cuda") {
            v.push(Backend::Cuda);
        }
        v
    }

    /// Está disponível nesta máquina? (A CPU, sempre.)
    pub fn disponivel(self) -> Result<(), String> {
        if !crate::runtime::presente() {
            return match self {
                Backend::Automatico | Backend::Cpu => Ok(()),
                _ => Err("o ONNX Runtime ainda não foi baixado".into()),
            };
        }
        match self {
            Backend::Automatico | Backend::Cpu => Ok(()),
            #[cfg(target_os = "macos")]
            Backend::CoreMl => encontrado(ort::ep::CoreML::default().is_available()),
            #[cfg(windows)]
            Backend::DirectMl => encontrado(ort::ep::DirectML::default().is_available()),
            #[cfg(feature = "cuda")]
            Backend::Cuda => encontrado(ort::ep::CUDA::default().is_available()),
            #[allow(unreachable_patterns)]
            _ => Err("não compilado nesta versão do app".into()),
        }
    }
}

/// A resposta do ONNX Runtime sobre um backend acelerado. Só existe onde há
/// algum compilado — no Linux sem `cuda` não há (a 0.1.106 quebrou ali).
#[cfg(any(target_os = "macos", windows, feature = "cuda"))]
fn encontrado(sim: ort::Result<bool>) -> Result<(), String> {
    match sim {
        Ok(true) => Ok(()),
        Ok(false) => Err("o ONNX Runtime não o encontrou nesta máquina".into()),
        Err(e) => Err(e.to_string()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErroDeExecucao {
    /// O arquivo não abre como modelo (corrompido, outro formato).
    ModeloInvalido(String),
    Cancelado,
    /// A inferência falhou (memória, operador sem suporte…).
    Processamento(String),
}

/// Uma sessão pronta: o modelo carregado num backend.
pub struct Sessao {
    sessao: Mutex<Session>,
    pub modelo: PathBuf,
    /// Onde ela roda de fato.
    pub backend: Backend,
    /// Quando o pedido não pôde ser atendido: o que aconteceu e para onde foi.
    pub aviso: Option<String>,
}

/// A sessão em cache: uma só (o modelo da LaMa ocupa ~1 GB carregado), e
/// trocada quando o modelo ou o backend mudam.
static CACHE: Mutex<Option<(PathBuf, Backend, Arc<Sessao>)>> = Mutex::new(None);

/// A sessão de `modelo` no `pedido` (com `Automatico` resolvido para
/// `automatico`). Reaproveita a do cache; carrega na primeira vez (segundos).
/// Se o backend pedido não estiver disponível ou falhar ao registrar, volta à
/// CPU e diz isso em [`Sessao::aviso`].
pub fn sessao(
    modelo: &Path,
    pedido: Backend,
    automatico: Backend,
) -> Result<Arc<Sessao>, ErroDeExecucao> {
    let alvo = if pedido == Backend::Automatico {
        automatico
    } else {
        pedido
    };
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((caminho, b, s)) = cache.as_ref() {
        if caminho == modelo && *b == alvo {
            return Ok(s.clone());
        }
    }
    // Solta a anterior antes de carregar a nova: duas na memória não cabem.
    *cache = None;
    crate::runtime::garantir().map_err(ErroDeExecucao::Processamento)?;
    let (sessao, backend, aviso) = match carregar(modelo, alvo) {
        Ok(s) => (s, alvo, None),
        Err(motivo) if alvo != Backend::Cpu => {
            let s = carregar(modelo, Backend::Cpu).map_err(ErroDeExecucao::ModeloInvalido)?;
            let aviso = format!(
                "{} não pôde ser usado ({motivo}) — rodando na CPU",
                alvo.nome()
            );
            (s, Backend::Cpu, Some(aviso))
        }
        Err(motivo) => return Err(ErroDeExecucao::ModeloInvalido(motivo)),
    };
    let s = Arc::new(Sessao {
        sessao: Mutex::new(sessao),
        modelo: modelo.to_path_buf(),
        backend,
        aviso,
    });
    *cache = Some((modelo.to_path_buf(), alvo, s.clone()));
    Ok(s)
}

/// Solta a sessão em cache — ao fechar a janela que a usava.
pub fn liberar() {
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn carregar(modelo: &Path, backend: Backend) -> Result<Session, String> {
    backend.disponivel()?;
    let mut construtor = Session::builder().map_err(|e| e.to_string())?;
    let ep: Option<ort::ep::ExecutionProviderDispatch> = match backend {
        #[cfg(target_os = "macos")]
        Backend::CoreMl => Some(ort::ep::CoreML::default().build().error_on_failure()),
        #[cfg(windows)]
        Backend::DirectMl => Some(ort::ep::DirectML::default().build().error_on_failure()),
        #[cfg(feature = "cuda")]
        Backend::Cuda => Some(ort::ep::CUDA::default().build().error_on_failure()),
        _ => None,
    };
    if let Some(ep) = ep {
        construtor = construtor
            .with_execution_providers([ep])
            .map_err(|e| e.to_string())?;
    }
    construtor
        .commit_from_file(modelo)
        .map_err(|e| e.to_string())
}

/// Um tensor `f32`: a forma e os dados (em ordem de linha).
#[derive(Clone, Debug, PartialEq)]
pub struct Tensor32 {
    pub forma: Vec<i64>,
    pub dados: Vec<f32>,
}

impl Sessao {
    /// Roda o modelo com as entradas nomeadas e devolve as saídas, na ordem
    /// do modelo. 🔑 **Cancelamento real**: enquanto roda, uma vigia olha
    /// `cancelado` e pede ao ONNX Runtime que pare (`RunOptions::terminate`).
    pub fn rodar(
        &self,
        entradas: Vec<(&str, Tensor32)>,
        cancelado: &AtomicBool,
    ) -> Result<Vec<Tensor32>, ErroDeExecucao> {
        if cancelado.load(Ordering::Relaxed) {
            return Err(ErroDeExecucao::Cancelado);
        }
        let opcoes =
            Arc::new(RunOptions::new().map_err(|e| ErroDeExecucao::Processamento(e.to_string()))?);
        let terminou = AtomicBool::new(false);
        let mut valores = Vec::with_capacity(entradas.len());
        for (nome, t) in entradas {
            let forma: Vec<usize> = t.forma.iter().map(|v| *v as usize).collect();
            let tensor = Tensor::from_array((forma, t.dados))
                .map_err(|e| ErroDeExecucao::Processamento(e.to_string()))?;
            valores.push((nome.to_string(), tensor));
        }
        std::thread::scope(|escopo| {
            let vigia_opcoes = opcoes.clone();
            let terminou = &terminou;
            escopo.spawn(move || {
                while !terminou.load(Ordering::Relaxed) {
                    if cancelado.load(Ordering::Relaxed) {
                        let _ = vigia_opcoes.terminate();
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(30));
                }
            });
            let resultado = (|| {
                let mut sessao = self.sessao.lock().unwrap_or_else(|e| e.into_inner());
                let entradas: Vec<(String, ort::value::DynValue)> = valores
                    .into_iter()
                    .map(|(n, t)| (n, t.into_dyn()))
                    .collect();
                let saidas = sessao.run_with_options(entradas, &*opcoes).map_err(|e| {
                    if cancelado.load(Ordering::Relaxed) {
                        ErroDeExecucao::Cancelado
                    } else {
                        ErroDeExecucao::Processamento(e.to_string())
                    }
                })?;
                let mut v = Vec::new();
                for (_, valor) in saidas.iter() {
                    let (forma, dados) = valor
                        .try_extract_tensor::<f32>()
                        .map_err(|e| ErroDeExecucao::Processamento(e.to_string()))?;
                    v.push(Tensor32 {
                        forma: forma.iter().copied().collect(),
                        dados: dados.to_vec(),
                    });
                }
                Ok(v)
            })();
            terminou.store(true, Ordering::Relaxed);
            resultado
        })
    }
}

/// As entradas e as saídas de um modelo, `(nome, forma)`.
pub type Assinatura = (Vec<(String, Vec<i64>)>, Vec<(String, Vec<i64>)>);

/// A assinatura de um modelo: as entradas e as saídas, `(nome, forma)` (−1
/// na dimensão livre). Para conferir um arquivo importado sem rodá-lo.
pub fn assinatura(modelo: &Path) -> Result<Assinatura, String> {
    crate::runtime::garantir()?;
    let s = Session::builder()
        .map_err(|e| e.to_string())?
        .commit_from_file(modelo)
        .map_err(|e| e.to_string())?;
    let forma = |t: &ort::value::ValueType| match t {
        ort::value::ValueType::Tensor { shape, .. } => shape.iter().copied().collect(),
        _ => Vec::new(),
    };
    Ok((
        s.inputs()
            .iter()
            .map(|i| (i.name().to_string(), forma(i.dtype())))
            .collect(),
        s.outputs()
            .iter()
            .map(|o| (o.name().to_string(), forma(o.dtype())))
            .collect(),
    ))
}

#[cfg(test)]
mod testes {
    use super::*;

    /// 🧪 Com um modelo de verdade (`VLB_MODELO_ONNX`): pedir um backend que
    /// esta máquina não tem volta à CPU **e diz isso**, e a sessão é
    /// reaproveitada no pedido seguinte.
    #[test]
    #[ignore]
    fn backend_indisponivel_volta_a_cpu_com_aviso_e_a_sessao_e_reaproveitada() {
        let modelo = PathBuf::from(std::env::var("VLB_MODELO_ONNX").expect("VLB_MODELO_ONNX"));
        let ausente = if cfg!(windows) {
            Backend::CoreMl
        } else {
            Backend::DirectMl
        };
        let s = sessao(&modelo, ausente, Backend::Cpu).unwrap();
        assert_eq!(s.backend, Backend::Cpu);
        let aviso = s.aviso.clone().expect("o aviso da volta");
        assert!(aviso.contains("rodando na CPU"), "{aviso}");
        let de_novo = sessao(&modelo, ausente, Backend::Cpu).unwrap();
        assert!(Arc::ptr_eq(&s, &de_novo), "a mesma sessão, sem recarregar");
        liberar();
        // Cancelado antes de rodar: não roda.
        let cancelado = AtomicBool::new(true);
        let r = de_novo.rodar(Vec::new(), &cancelado);
        assert_eq!(r.err(), Some(ErroDeExecucao::Cancelado));
    }
}
