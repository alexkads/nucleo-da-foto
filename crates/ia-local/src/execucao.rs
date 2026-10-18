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

/// Onde o ONNX Runtime executa o modelo.
///
/// ```rust
/// use ia_local::execucao::Backend;
///
/// // A preferência gravada volta pela chave; a CPU existe em toda compilação.
/// assert_eq!(Backend::da_chave("cpu"), Some(Backend::Cpu));
/// assert_eq!(Backend::da_chave(Backend::Automatico.chave()), Some(Backend::Automatico));
/// assert_eq!(Backend::da_chave("tpu"), None);
/// assert!(Backend::Cpu.disponivel().is_ok());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Backend {
    /// O que o modelo recomenda (medido por quem o integrou).
    Automatico,
    /// A CPU: sempre disponível, e o destino da volta quando outro falha.
    Cpu,
    /// CoreML, no macOS: Neural Engine e GPU da Apple.
    CoreMl,
    /// DirectML, no Windows: qualquer GPU com DirectX 12.
    DirectMl,
    /// CUDA, em placa NVIDIA. Só com a feature `cuda`, e não testado.
    Cuda,
}

impl Backend {
    /// O nome que o operador lê no seletor e no "rodou em …".
    pub fn nome(self) -> &'static str {
        match self {
            Backend::Automatico => "Automático",
            Backend::Cpu => "CPU",
            Backend::CoreMl => "CoreML (Apple)",
            Backend::DirectMl => "DirectML (Windows)",
            Backend::Cuda => "CUDA (NVIDIA)",
        }
    }

    /// O nome estável, para as preferências (`"cpu"`, `"coreml"` …).
    pub fn chave(self) -> &'static str {
        match self {
            Backend::Automatico => "automatico",
            Backend::Cpu => "cpu",
            Backend::CoreMl => "coreml",
            Backend::DirectMl => "directml",
            Backend::Cuda => "cuda",
        }
    }

    /// O backend de uma [`chave`](Self::chave) gravada — só entre os
    /// [`compilados`](Self::compilados). A preferência `"coreml"` lida num
    /// Windows dá `None`, e quem chama volta ao automático.
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
    ///
    /// Para um acelerado, carrega o ONNX Runtime e pergunta a ele. Nunca
    /// baixa nada: sem o runtime presente (Windows com MSYS2, antes do
    /// primeiro [`runtime::baixar`](crate::runtime::baixar)), só a CPU e o
    /// automático respondem que sim.
    ///
    /// # Erros
    ///
    /// O porquê, em frase para a tela: runtime ainda não baixado, backend não
    /// encontrado na máquina ou não compilado nesta versão.
    pub fn disponivel(self) -> Result<(), String> {
        if !crate::runtime::presente() {
            return match self {
                Backend::Automatico | Backend::Cpu => Ok(()),
                _ => Err("o ONNX Runtime ainda não foi baixado".into()),
            };
        }
        if matches!(self, Backend::Automatico | Backend::Cpu) {
            return Ok(());
        }
        // 🚨 Antes de qualquer pergunta ao `ort`, o runtime **nosso** carregado.
        // No Windows (`load-dynamic`), sem o `init_from` o `ort` carrega o
        // `onnxruntime.dll` que o sistema achar — o do System32, mais velho — e
        // entra em pânico (`MissingApi`) dentro do laço de mensagens, que não
        // desenrola: abrir o editor fechava o app (0.1.122, Windows 10).
        crate::runtime::garantir()?;
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

/// Por que a sessão não abriu ou a inferência não terminou.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErroDeExecucao {
    /// O arquivo não abre como modelo (corrompido, outro formato).
    ModeloInvalido(String),
    /// `cancelado` foi ligado antes ou durante a inferência.
    Cancelado,
    /// A inferência falhou (memória, operador sem suporte…).
    Processamento(String),
}

/// Uma sessão pronta: o modelo carregado num backend.
pub struct Sessao {
    sessao: Mutex<Session>,
    /// O arquivo do modelo carregado.
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
///
/// Uma sessão só fica em cache: pedir outro modelo ou outro backend solta a
/// anterior antes de carregar.
///
/// # Erros
///
/// [`ErroDeExecucao::ModeloInvalido`] se o modelo não carregar nem na CPU;
/// [`ErroDeExecucao::Processamento`] se o ONNX Runtime não puder ser
/// carregado.
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
    /// As dimensões, da mais externa à mais interna (`[1, 3, 512, 512]`).
    pub forma: Vec<i64>,
    /// Os valores; o comprimento é o produto da forma.
    pub dados: Vec<f32>,
}

impl Sessao {
    /// Roda o modelo com as entradas nomeadas e devolve as saídas, na ordem
    /// do modelo. 🔑 **Cancelamento real**: enquanto roda, uma vigia olha
    /// `cancelado` e pede ao ONNX Runtime que pare (`RunOptions::terminate`).
    ///
    /// Bloqueia até o fim e serializa: duas chamadas na mesma sessão rodam
    /// uma depois da outra.
    ///
    /// # Erros
    ///
    /// [`ErroDeExecucao::Cancelado`] se `cancelado` foi ligado;
    /// [`ErroDeExecucao::Processamento`] se um tensor não monta (forma que não
    /// bate com os dados), se a inferência falha ou se uma saída não é `f32`.
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
///
/// Carrega o modelo na CPU, fora do cache — não derruba a sessão em uso.
///
/// # Erros
///
/// O porquê, em frase, se o runtime não carregar ou o arquivo não abrir como
/// modelo ONNX.
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

    // ── Um modelo ONNX de verdade, minúsculo, escrito aqui ───────────────
    //
    // `y = a + b` sobre `f32[n]`. O ONNX é um protobuf; os campos usados são
    // poucos e estão em onnx/onnx.proto (ModelProto, GraphProto, NodeProto,
    // ValueInfoProto, TypeProto, TensorShapeProto). Assim a execução inteira
    // — sessão, cache, backend, inferência, cancelamento — é testada sem os
    // 208 MB da LaMa e sem arquivo binário no repositório.

    fn varint(mut v: u64, b: &mut Vec<u8>) {
        while v >= 0x80 {
            b.push((v as u8) | 0x80);
            v >>= 7;
        }
        b.push(v as u8);
    }

    fn numero(campo: u64, v: u64) -> Vec<u8> {
        let mut b = Vec::new();
        varint(campo << 3, &mut b);
        varint(v, &mut b);
        b
    }

    fn bytes(campo: u64, conteudo: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        varint((campo << 3) | 2, &mut b);
        varint(conteudo.len() as u64, &mut b);
        b.extend_from_slice(conteudo);
        b
    }

    /// `ValueInfoProto` de um tensor `f32` com uma dimensão livre `n`.
    fn tensor_f32_livre(nome: &str) -> Vec<u8> {
        let dimensao = bytes(2, b"n"); // Dimension.dim_param
        let forma = bytes(1, &dimensao); // TensorShapeProto.dim
        let tensor = [numero(1, 1), bytes(2, &forma)].concat(); // elem_type FLOAT, shape
        let tipo = bytes(1, &tensor); // TypeProto.tensor_type
        [bytes(1, nome.as_bytes()), bytes(2, &tipo)].concat()
    }

    fn modelo_soma() -> Vec<u8> {
        let no = [
            bytes(1, b"a"),
            bytes(1, b"b"),
            bytes(2, b"y"),
            bytes(4, b"Add"),
        ]
        .concat();
        let grafo = [
            bytes(1, &no),
            bytes(2, b"soma"),
            bytes(11, &tensor_f32_livre("a")),
            bytes(11, &tensor_f32_livre("b")),
            bytes(12, &tensor_f32_livre("y")),
        ]
        .concat();
        let opset = numero(2, 13);
        [numero(1, 8), bytes(7, &grafo), bytes(8, &opset)].concat()
    }

    fn arquivo(pasta: &tempfile::TempDir, nome: &str, conteudo: &[u8]) -> PathBuf {
        let caminho = pasta.path().join(nome);
        std::fs::write(&caminho, conteudo).unwrap();
        caminho
    }

    /// O cache de sessão é um só no processo: os testes que o tocam não
    /// podem rodar ao mesmo tempo.
    static EM_FILA: Mutex<()> = Mutex::new(());

    fn vetor(dados: &[f32]) -> Tensor32 {
        Tensor32 {
            forma: vec![dados.len() as i64],
            dados: dados.to_vec(),
        }
    }

    #[test]
    fn a_chave_de_cada_backend_compilado_volta_ao_mesmo_backend() {
        let compilados = Backend::compilados();
        assert_eq!(compilados[..2], [Backend::Automatico, Backend::Cpu]);
        assert_eq!(
            compilados.contains(&Backend::CoreMl),
            cfg!(target_os = "macos")
        );
        assert_eq!(compilados.contains(&Backend::DirectMl), cfg!(windows));
        assert_eq!(compilados.contains(&Backend::Cuda), cfg!(feature = "cuda"));
        for b in compilados {
            assert_eq!(Backend::da_chave(b.chave()), Some(b));
            assert!(!b.nome().is_empty());
        }
        assert_eq!(Backend::da_chave("tpu"), None);
    }

    #[test]
    fn a_cpu_e_o_automatico_estao_sempre_disponiveis() {
        assert_eq!(Backend::Cpu.disponivel(), Ok(()));
        assert_eq!(Backend::Automatico.disponivel(), Ok(()));
    }

    #[test]
    fn um_backend_de_outro_sistema_nao_esta_disponivel_e_diz_por_que() {
        let alheio = if cfg!(windows) {
            Backend::CoreMl
        } else {
            Backend::DirectMl
        };
        let motivo = alheio.disponivel().unwrap_err();
        assert!(motivo.contains("não compilado"), "{motivo}");
    }

    #[test]
    fn a_assinatura_le_entradas_e_saidas_sem_rodar() {
        let pasta = tempfile::tempdir().unwrap();
        let modelo = arquivo(&pasta, "soma.onnx", &modelo_soma());
        let (entradas, saidas) = assinatura(&modelo).unwrap();
        assert_eq!(
            entradas,
            vec![("a".to_string(), vec![-1]), ("b".to_string(), vec![-1])]
        );
        assert_eq!(
            saidas,
            vec![("y".to_string(), vec![-1])],
            "-1: a dimensão livre"
        );
    }

    #[test]
    fn um_arquivo_que_nao_e_modelo_e_recusado_na_assinatura_e_na_sessao() {
        let _fila = EM_FILA.lock().unwrap_or_else(|e| e.into_inner());
        let pasta = tempfile::tempdir().unwrap();
        let lixo = arquivo(&pasta, "lixo.onnx", b"isto nao e um protobuf de modelo");
        assert!(assinatura(&lixo).is_err());
        assert!(matches!(
            sessao(&lixo, Backend::Cpu, Backend::Cpu),
            Err(ErroDeExecucao::ModeloInvalido(_))
        ));
        let ausente = pasta.path().join("nao-existe.onnx");
        assert!(matches!(
            sessao(&ausente, Backend::Cpu, Backend::Cpu),
            Err(ErroDeExecucao::ModeloInvalido(_))
        ));
    }

    #[test]
    fn roda_o_modelo_e_devolve_as_saidas_na_ordem() {
        let _fila = EM_FILA.lock().unwrap_or_else(|e| e.into_inner());
        let pasta = tempfile::tempdir().unwrap();
        let modelo = arquivo(&pasta, "soma.onnx", &modelo_soma());
        let s = sessao(&modelo, Backend::Cpu, Backend::Cpu).unwrap();
        assert_eq!((s.backend, s.aviso.as_deref()), (Backend::Cpu, None));
        assert_eq!(s.modelo, modelo);
        let nao = AtomicBool::new(false);
        let saidas = s
            .rodar(
                vec![
                    ("a", vetor(&[1.0, 2.0, 3.0])),
                    ("b", vetor(&[10.0, 20.0, 30.0])),
                ],
                &nao,
            )
            .unwrap();
        assert_eq!(saidas, vec![vetor(&[11.0, 22.0, 33.0])]);
        liberar();
    }

    #[test]
    fn o_automatico_vira_o_backend_que_o_modelo_recomenda() {
        let _fila = EM_FILA.lock().unwrap_or_else(|e| e.into_inner());
        let pasta = tempfile::tempdir().unwrap();
        let modelo = arquivo(&pasta, "soma.onnx", &modelo_soma());
        let s = sessao(&modelo, Backend::Automatico, Backend::Cpu).unwrap();
        assert_eq!(s.backend, Backend::Cpu);
        // O pedido explícito pelo mesmo alvo reaproveita a sessão resolvida.
        let explicita = sessao(&modelo, Backend::Cpu, Backend::Cuda).unwrap();
        assert!(Arc::ptr_eq(&s, &explicita));
        liberar();
    }

    #[test]
    fn a_sessao_e_reaproveitada_ate_o_modelo_mudar_ou_ser_liberada() {
        let _fila = EM_FILA.lock().unwrap_or_else(|e| e.into_inner());
        let pasta = tempfile::tempdir().unwrap();
        let um = arquivo(&pasta, "um.onnx", &modelo_soma());
        let outro = arquivo(&pasta, "outro.onnx", &modelo_soma());

        let a = sessao(&um, Backend::Cpu, Backend::Cpu).unwrap();
        let b = sessao(&um, Backend::Cpu, Backend::Cpu).unwrap();
        assert!(Arc::ptr_eq(&a, &b), "a mesma sessão, sem recarregar");

        let c = sessao(&outro, Backend::Cpu, Backend::Cpu).unwrap();
        assert!(!Arc::ptr_eq(&a, &c), "outro modelo: outra sessão");
        assert_eq!(c.modelo, outro);

        liberar();
        let d = sessao(&outro, Backend::Cpu, Backend::Cpu).unwrap();
        assert!(!Arc::ptr_eq(&c, &d), "liberada: carrega de novo");
        liberar();
    }

    #[test]
    fn backend_indisponivel_volta_a_cpu_e_diz_isso_no_aviso() {
        let _fila = EM_FILA.lock().unwrap_or_else(|e| e.into_inner());
        let pasta = tempfile::tempdir().unwrap();
        let modelo = arquivo(&pasta, "soma.onnx", &modelo_soma());
        let alheio = if cfg!(windows) {
            Backend::CoreMl
        } else {
            Backend::DirectMl
        };
        let s = sessao(&modelo, alheio, Backend::Cpu).unwrap();
        assert_eq!(s.backend, Backend::Cpu);
        let aviso = s.aviso.clone().expect("o aviso da volta");
        assert!(
            aviso.contains(alheio.nome()) && aviso.contains("rodando na CPU"),
            "{aviso}"
        );
        // E o resultado é o mesmo: a volta à CPU não muda a conta.
        let nao = AtomicBool::new(false);
        let y = s
            .rodar(vec![("a", vetor(&[0.5])), ("b", vetor(&[0.25]))], &nao)
            .unwrap();
        assert_eq!(y, vec![vetor(&[0.75])]);
        liberar();
    }

    #[test]
    fn cancelado_antes_nao_roda_e_entrada_errada_e_erro_de_processamento() {
        let _fila = EM_FILA.lock().unwrap_or_else(|e| e.into_inner());
        let pasta = tempfile::tempdir().unwrap();
        let modelo = arquivo(&pasta, "soma.onnx", &modelo_soma());
        let s = sessao(&modelo, Backend::Cpu, Backend::Cpu).unwrap();

        let sim = AtomicBool::new(true);
        let r = s.rodar(vec![("a", vetor(&[1.0])), ("b", vetor(&[1.0]))], &sim);
        assert_eq!(r, Err(ErroDeExecucao::Cancelado));

        let nao = AtomicBool::new(false);
        let r = s.rodar(vec![("x", vetor(&[1.0])), ("b", vetor(&[1.0]))], &nao);
        assert!(matches!(r, Err(ErroDeExecucao::Processamento(_))), "{r:?}");

        // Forma que não bate com os dados: recusada antes do runtime.
        let torto = Tensor32 {
            forma: vec![3],
            dados: vec![1.0],
        };
        let r = s.rodar(vec![("a", torto), ("b", vetor(&[1.0]))], &nao);
        assert!(matches!(r, Err(ErroDeExecucao::Processamento(_))), "{r:?}");
        liberar();
    }
}
