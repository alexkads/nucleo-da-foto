//! 🖨️ As páginas de um PDF, desenhadas em pixels — sem tela e sem GPU.
//!
//! # Para quê
//!
//! O balcão gera o fotolivro do ensaio em PDF (crate `fotolivro`) e o manda ao
//! cliente. Até 10/10/2026 o operador só via o livro abrindo o arquivo em outro
//! programa; o dono, olhando o diálogo de exportação: *"Acho que cabe uma
//! visualização de PDF, crie um módulo de visualização de PDF com base nesse
//! projeto https://github.com/storytold/pdfcraft"*. Este crate é a metade sem
//! janela desse módulo; a tela mora em `ui-gpui/src/pdf`.
//!
//! # De onde vem
//!
//! Do **PdfCraft** (github.com/storytold/pdfcraft, MIT OU Apache-2.0), commit
//! `72b3b617`, `crates/render/src/raster.rs`: o `PageRenderer`, os tetos
//! `MAX_SIDE`/`MAX_PIXELS`, o `effective_scale`, o `device_pixels` e a fila
//! que só guarda o trabalho que ainda interessa (`RenderPool::set_queue`).
//! Copiado e adaptado, e não dependência — ele pede edition 2024 e anda dezenas
//! de commits por dia (skill `referencias-storytold`, no e-commerce).
//!
//! Quem desenha é o **`hayro`** (github.com/LaurenzV/hayro, Apache-2.0 OU MIT),
//! que é o que o PdfCraft usa por baixo: Rust puro, CPU, sem biblioteca do
//! sistema. O PdfCraft leva uma cópia remendada dele (ladrilhos, camadas,
//! formulários); aqui vai o do crates.io, porque o PDF é o nosso.
//!
//! # O que fica de fora, de propósito
//!
//! Senha, camadas, formulários, texto selecionável, ladrilhos e a vigia de
//! página travada. O que este crate abre é o livro que o próprio app escreveu:
//! nada disso existe nele. Um PDF de fora que precise delas é outro trabalho.
//!
//! # 🔗 Os links
//!
//! O fotolivro é feito deles — "Abrir minha galeria", "Folhear o álbum", a
//! foto que leva à galeria do cliente — e o dono perguntou por eles no mesmo
//! dia: *"Os clicks nos links do PDF funcionam no novo visualizador?"*. Cada
//! folha traz os seus ([`Link`]): a área, em fração da folha, e o destino —
//! um endereço (`/URI`) ou outra página do documento (`/GoTo` e `/Dest`). É o
//! `Link`/`LinkTarget` do `inspect.rs` do PdfCraft, sem as ações de camada.
//!
//! # Nunca derruba o app
//!
//! Cada página é desenhada dentro de `catch_unwind`, como lá: um pânico do
//! rasterizador vira [`Erro::Desenho`] daquela página, e o documento é lido de
//! novo para a próxima.

use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::object::{Array, Dict, Name, ObjectIdentifier, Rect};
use hayro::hayro_syntax::page::{Page, Rotation};
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{RenderCache, RenderSettings};

/// O maior lado de uma página desenhada: o teto de memória no zoom alto.
pub const LADO_MAXIMO: f32 = 8192.0;
/// O teto de pixels por página (~64 MP ≈ 256 MB em RGBA).
pub const PIXELS_MAXIMOS: f32 = 64.0e6;

/// Uma folha do documento, em pontos (1/72 de polegada), já com a rotação.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Folha {
    pub largura: f32,
    pub altura: f32,
}

impl Folha {
    /// Largura sobre altura — a página deitada do fotolivro dá 1,41.
    pub fn aspecto(&self) -> f32 {
        self.largura / self.altura.max(1.0)
    }
}

/// Para onde um [`Link`] leva.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destino {
    /// Um endereço de fora: abre no navegador.
    Endereco(String),
    /// Outra página deste documento, contada de 0.
    Pagina(usize),
}

/// 🔗 Uma área clicável da folha.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    /// Esquerda, topo, largura e altura, em **fração da folha** (0 a 1, com a
    /// origem no canto de cima à esquerda, já com a rotação): a tela multiplica
    /// pelo tamanho em que a página estiver desenhada.
    pub caixa: [f32; 4],
    pub destino: Destino,
}

/// A ordem dos canais nos pixels entregues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ordem {
    #[default]
    Rgba,
    /// A que o GPUI espera (`imagem.rs` do `ui-gpui`).
    Bgra,
}

/// Uma página desenhada: 4 bytes por pixel, linha a linha, **opaca** (o papel
/// é branco, então não há o que pré-multiplicar).
#[derive(Clone, PartialEq, Eq)]
pub struct Imagem {
    pub largura: u32,
    pub altura: u32,
    pub pixels: Vec<u8>,
    pub ordem: Ordem,
}

impl Imagem {
    /// Os mesmos pixels na outra ordem de canais, no lugar.
    pub fn em(mut self, ordem: Ordem) -> Self {
        if self.ordem != ordem {
            for pixel in self.pixels.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            self.ordem = ordem;
        }
        self
    }
}

impl std::fmt::Debug for Imagem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Imagem({}×{}, {:?})",
            self.largura, self.altura, self.ordem
        )
    }
}

/// Por que o documento não abriu ou a página não saiu — em frase, para a tela.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Erro {
    /// Não é um PDF que dê para ler.
    Ilegivel(String),
    /// O documento não tem essa página (contada de 0).
    SemPagina(usize),
    /// A página existe e não pôde ser desenhada.
    Desenho(String),
}

impl std::fmt::Display for Erro {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // O porquê do leitor é em inglês e de programador ("Invalid"):
            // fica no `Debug`, para o registro, e fora da frase da tela.
            Erro::Ilegivel(_) => write!(f, "o arquivo não é um PDF legível"),
            Erro::SemPagina(pagina) => write!(f, "o documento não tem a página {}", pagina + 1),
            Erro::Desenho(porque) => write!(f, "a página não pôde ser desenhada: {porque}"),
        }
    }
}

impl std::error::Error for Erro {}

/// A escala pedida, limitada para a página caber em [`LADO_MAXIMO`] e em
/// [`PIXELS_MAXIMOS`].
///
/// Os tetos ganham sempre (`effective_scale` do PdfCraft): um piso aqui já
/// deixou um arquivo forjado, com página de 934 milhões de pontos, pedir 9
/// milhões de pixels de largura. Só a escala zero ou não finita é trocada.
pub fn escala_efetiva(largura_pt: f32, altura_pt: f32, escala: f32) -> f32 {
    let (l, a) = (largura_pt.max(1.0), altura_pt.max(1.0));
    let pelo_lado = LADO_MAXIMO / l.max(a);
    let pela_area = (PIXELS_MAXIMOS / (l * a)).sqrt();
    let limitada = escala.min(pelo_lado).min(pela_area);
    if limitada.is_finite() && limitada > 0.0 {
        limitada
    } else {
        pelo_lado.min(pela_area).max(f32::MIN_POSITIVE)
    }
}

/// Os pixels que cobrem `pt` pontos na `escala`: para cima, para a última
/// linha ou coluna parcial ser desenhada em vez de cortada (`device_pixels` do
/// PdfCraft, que segue o `pdftoppm`). Ruído de até 1/100 de pixel não sobe:
/// 612 pt a 150 dpi continuam 1275 px. No mínimo 1.
pub fn pixels_do_dispositivo(pt: f32, escala: f32) -> u32 {
    let px = pt * escala - 0.01;
    if px.is_finite() {
        px.ceil().max(1.0) as u32
    } else {
        1
    }
}

fn frase_do_panico(panico: &Box<dyn std::any::Any + Send>) -> String {
    panico
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| panico.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "falha desconhecida".into())
}

fn ler(bytes: &Arc<Vec<u8>>) -> Result<Pdf, Erro> {
    match catch_unwind(AssertUnwindSafe(|| Pdf::new(bytes.clone()))) {
        Ok(Ok(pdf)) => Ok(pdf),
        Ok(Err(erro)) => Err(Erro::Ilegivel(format!("{erro:?}"))),
        Err(panico) => Err(Erro::Ilegivel(frase_do_panico(&panico))),
    }
}

/// O retângulo da anotação (espaço do PDF, y para cima, dentro da caixa de
/// corte) em fração da folha como ela é desenhada: y para baixo e girada.
fn caixa_na_folha(retangulo: Rect, pagina: &Page<'_>) -> Option<[f32; 4]> {
    let corte = pagina.intersected_crop_box();
    let (largura, altura) = (corte.x1 - corte.x0, corte.y1 - corte.y0);
    if !(largura > 0.0 && altura > 0.0) {
        return None;
    }
    // Os dois cantos, na folha sem girar: u para a direita, v para baixo.
    let canto = |x: f64, y: f64| ((x - corte.x0) / largura, 1.0 - (y - corte.y0) / altura);
    let girar = |(u, v): (f64, f64)| match pagina.rotation() {
        Rotation::None => (u, v),
        Rotation::Horizontal => (1.0 - v, u),
        Rotation::Flipped => (1.0 - u, 1.0 - v),
        Rotation::FlippedHorizontal => (v, 1.0 - u),
    };
    let (a, b) = (
        girar(canto(retangulo.x0, retangulo.y0)),
        girar(canto(retangulo.x1, retangulo.y1)),
    );
    let (esquerda, direita) = (a.0.min(b.0).clamp(0.0, 1.0), a.0.max(b.0).clamp(0.0, 1.0));
    let (topo, base) = (a.1.min(b.1).clamp(0.0, 1.0), a.1.max(b.1).clamp(0.0, 1.0));
    let caixa = [
        esquerda as f32,
        topo as f32,
        (direita - esquerda) as f32,
        (base - topo) as f32,
    ];
    (caixa[2] > 0.0 && caixa[3] > 0.0 && caixa.iter().all(|n| n.is_finite())).then_some(caixa)
}

/// A página de um destino explícito (`[página /XYZ …]`): o primeiro item é a
/// referência ao objeto da página. Destino por nome fica de fora.
fn pagina_do_destino(destino: &Array<'_>, ids: &[Option<ObjectIdentifier>]) -> Option<usize> {
    let alvo: ObjectIdentifier = destino.raw_iter().next()?.as_obj_ref()?.into();
    ids.iter().position(|id| *id == Some(alvo))
}

fn destino_da_anotacao(anotacao: &Dict<'_>, ids: &[Option<ObjectIdentifier>]) -> Option<Destino> {
    if let Some(destino) = anotacao.get::<Array<'_>>(b"Dest") {
        return pagina_do_destino(&destino, ids).map(Destino::Pagina);
    }
    let acao = anotacao.get::<Dict<'_>>(b"A")?;
    let tipo = acao.get::<Name<'_>>(b"S")?;
    match &tipo[..] {
        b"URI" => {
            let endereco = acao.get::<hayro::hayro_syntax::object::String<'_>>(b"URI")?;
            let endereco = String::from_utf8_lossy(endereco.as_bytes())
                .trim()
                .to_string();
            (!endereco.is_empty()).then_some(Destino::Endereco(endereco))
        }
        b"GoTo" => {
            let destino = acao.get::<Array<'_>>(b"D")?;
            pagina_do_destino(&destino, ids).map(Destino::Pagina)
        }
        _ => None,
    }
}

fn links_da_pagina(pagina: &Page<'_>, ids: &[Option<ObjectIdentifier>]) -> Vec<Link> {
    let Some(anotacoes) = pagina.raw().get::<Array<'_>>(b"Annots") else {
        return Vec::new();
    };
    anotacoes
        .iter::<Dict<'_>>()
        .filter(|anotacao| {
            anotacao
                .get::<Name<'_>>(b"Subtype")
                .is_some_and(|tipo| &tipo[..] == b"Link")
        })
        .filter_map(|anotacao| {
            Some(Link {
                caixa: caixa_na_folha(anotacao.get::<Rect>(b"Rect")?, pagina)?,
                destino: destino_da_anotacao(&anotacao, ids)?,
            })
        })
        .collect()
}

/// Um PDF aberto, que desenha uma página por vez na thread de quem chama — o
/// `PageRenderer` do PdfCraft. Para a tela, use a [`Fila`].
pub struct Documento {
    bytes: Arc<Vec<u8>>,
    pdf: Pdf,
}

impl Documento {
    pub fn abrir(bytes: Arc<Vec<u8>>) -> Result<Self, Erro> {
        let pdf = ler(&bytes)?;
        Ok(Self { bytes, pdf })
    }

    /// As folhas, na ordem do documento.
    pub fn folhas(&self) -> Vec<Folha> {
        self.pdf
            .pages()
            .iter()
            .map(|pagina| {
                let (largura, altura) = pagina.render_dimensions();
                Folha { largura, altura }
            })
            .collect()
    }

    /// 🔗 Os links de cada folha, na ordem do documento. Um documento
    /// estragado nas anotações fica sem links, e não sem páginas.
    pub fn links(&self) -> Vec<Vec<Link>> {
        let pdf = &self.pdf;
        let paginas = pdf.pages().len();
        catch_unwind(AssertUnwindSafe(|| {
            let ids: Vec<Option<ObjectIdentifier>> = pdf
                .pages()
                .iter()
                .map(|pagina| pagina.raw().obj_id())
                .collect();
            pdf.pages()
                .iter()
                .map(|pagina| links_da_pagina(pagina, &ids))
                .collect()
        }))
        .unwrap_or_else(|_| vec![Vec::new(); paginas])
    }

    /// Desenha a página (contada de 0) com `escala` pixels por ponto, sobre
    /// papel branco. Nunca entra em pânico.
    pub fn desenhar(&mut self, pagina: usize, escala: f32) -> Result<Imagem, Erro> {
        let pdf = &self.pdf;
        let resultado = catch_unwind(AssertUnwindSafe(|| {
            let folha = pdf.pages().get(pagina).ok_or(Erro::SemPagina(pagina))?;
            let (l, a) = folha.render_dimensions();
            if !(l.is_finite() && a.is_finite()) || l < 0.5 || a < 0.5 {
                return Err(Erro::Desenho(format!(
                    "a caixa da página é vazia ou inválida ({l}×{a} pt)"
                )));
            }
            let escala = escala_efetiva(l, a, escala);
            // O hayro arredonda para baixo quando o tamanho não é dado e
            // perde a borda parcial; a escala já mantém cada lado no teto.
            let lado = |pt: f32| pixels_do_dispositivo(pt, escala).min(LADO_MAXIMO as u32) as u16;
            let ajustes = RenderSettings {
                x_scale: escala,
                y_scale: escala,
                width: Some(lado(l)),
                height: Some(lado(a)),
                bg_color: WHITE,
            };
            let desenho = hayro::render(
                folha,
                &RenderCache::new(),
                &InterpreterSettings::default(),
                &ajustes,
            );
            Ok(Imagem {
                largura: desenho.width().into(),
                altura: desenho.height().into(),
                pixels: desenho.data_as_u8_slice().to_vec(),
                ordem: Ordem::Rgba,
            })
        }));
        match resultado {
            Ok(feito) => feito,
            Err(panico) => {
                // O estado do leitor depois de um pânico não é confiável: a
                // próxima página começa de um documento lido de novo.
                if let Ok(pdf) = ler(&self.bytes) {
                    self.pdf = pdf;
                }
                Err(Erro::Desenho(frase_do_panico(&panico)))
            }
        }
    }
}

/// Uma página pedida à [`Fila`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pedido {
    /// Contada de 0.
    pub pagina: usize,
    /// Pixels por ponto.
    pub escala: f32,
}

/// O que a [`Fila`] devolve.
#[derive(Debug, Clone, PartialEq)]
pub enum Resposta {
    /// O documento abriu: as folhas dele e os links de cada uma. É sempre a
    /// primeira resposta.
    Aberto {
        folhas: Vec<Folha>,
        links: Vec<Vec<Link>>,
    },
    /// O documento não abriu. Nada mais vem depois.
    Falhou(Erro),
    /// Uma página pedida ficou pronta, ou falhou.
    Pagina {
        pedido: Pedido,
        imagem: Result<Imagem, Erro>,
    },
}

#[derive(Default)]
struct Espera {
    pedidos: VecDeque<Pedido>,
    fechada: bool,
}

type Divisa = Arc<(Mutex<Espera>, Condvar)>;

// Um pânico com a trava na mão não envenena a fila para sempre.
fn travar(espera: &Mutex<Espera>) -> MutexGuard<'_, Espera> {
    espera.lock().unwrap_or_else(|veneno| veneno.into_inner())
}

/// 🧵 O documento numa thread própria: a tela pede páginas e colhe as prontas
/// sem nunca esperar pelo desenho.
///
/// A lista de [`Fila::pedir`] **substitui** a anterior (o `set_queue` do
/// PdfCraft): quem rolou vinte páginas não espera as dezenove que passaram. A
/// que já está sendo desenhada termina e é entregue; a tela decide se ainda a
/// quer.
pub struct Fila {
    espera: Divisa,
    respostas: Receiver<Resposta>,
}

impl Fila {
    /// Abre o documento em segundo plano. A primeira [`Resposta`] diz se abriu.
    pub fn abrir(bytes: Arc<Vec<u8>>, ordem: Ordem) -> Self {
        let espera: Divisa = Arc::default();
        let (envio, respostas) = channel();
        let da_thread = espera.clone();
        let nasceu = std::thread::Builder::new()
            .name("visualizador-pdf".into())
            .spawn({
                let envio = envio.clone();
                move || trabalhar(bytes, ordem, da_thread, envio)
            });
        if let Err(erro) = nasceu {
            let _ = envio.send(Resposta::Falhou(Erro::Ilegivel(format!(
                "sem thread para desenhar: {erro}"
            ))));
        }
        Self { espera, respostas }
    }

    /// O que a tela quer agora, do mais urgente para o menos.
    pub fn pedir(&self, pedidos: Vec<Pedido>) {
        travar(&self.espera.0).pedidos = pedidos.into();
        self.espera.1.notify_one();
    }

    /// Uma resposta pronta, se houver. Não espera.
    pub fn colher(&self) -> Option<Resposta> {
        self.respostas.try_recv().ok()
    }
}

impl Drop for Fila {
    fn drop(&mut self) {
        let mut espera = travar(&self.espera.0);
        espera.fechada = true;
        espera.pedidos.clear();
        self.espera.1.notify_one();
    }
}

fn trabalhar(bytes: Arc<Vec<u8>>, ordem: Ordem, espera: Divisa, envio: Sender<Resposta>) {
    let mut documento = match Documento::abrir(bytes) {
        Ok(documento) => documento,
        Err(erro) => {
            let _ = envio.send(Resposta::Falhou(erro));
            return;
        }
    };
    let aberto = Resposta::Aberto {
        folhas: documento.folhas(),
        links: documento.links(),
    };
    if envio.send(aberto).is_err() {
        return;
    }
    loop {
        let pedido = {
            let mut guarda = travar(&espera.0);
            loop {
                if guarda.fechada {
                    return;
                }
                if let Some(pedido) = guarda.pedidos.pop_front() {
                    break pedido;
                }
                guarda = espera
                    .1
                    .wait(guarda)
                    .unwrap_or_else(|veneno| veneno.into_inner());
            }
        };
        let imagem = documento
            .desenhar(pedido.pagina, pedido.escala)
            .map(|imagem| imagem.em(ordem));
        if envio.send(Resposta::Pagina { pedido, imagem }).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::time::{Duration, Instant};

    const GALERIA: &str = "https://recordarfotos.com.br/meus-ensaios/abc";

    /// O livro que o balcão gera: duas fotos lisas, uma levada e uma à venda.
    fn livro() -> Arc<Vec<u8>> {
        let foto = |cor: [u8; 3], nome: &str, levada: bool| fotolivro::FotoDaFolha {
            link: Some(format!("{GALERIA}?foto={nome}")),
            imagem: image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                400,
                300,
                image::Rgb(cor),
            )),
            nome: nome.into(),
            levada,
        };
        let capa = fotolivro::Capa {
            titulo: "Ensaio de teste".into(),
            site: "recordarfotos.com.br".into(),
            galeria: Some(GALERIA.into()),
            ..Default::default()
        };
        Arc::new(
            fotolivro::gerar(
                &capa,
                vec![
                    foto([200, 30, 30], "a.jpg", true),
                    foto([30, 30, 200], "b.jpg", false),
                ],
            )
            .expect("o fotolivro de teste"),
        )
    }

    fn colher_ate(fila: &Fila, quer: impl Fn(&Resposta) -> bool) -> Resposta {
        let prazo = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(resposta) = fila.colher() {
                if quer(&resposta) {
                    return resposta;
                }
            } else {
                assert!(Instant::now() < prazo, "a fila não respondeu em 60 s");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    #[test]
    fn os_tetos_ganham_da_escala_pedida() {
        // A4 em pé a 2×: cabe, e a escala passa como veio.
        assert_eq!(escala_efetiva(595.0, 842.0, 2.0), 2.0);
        // O lado: 842 pt a 100× seriam 84 mil pixels.
        let pelo_lado = escala_efetiva(595.0, 842.0, 100.0);
        assert!(842.0 * pelo_lado <= LADO_MAXIMO + 0.5, "{pelo_lado}");
        // A área: um cartaz quadrado cabe no lado e estoura os pixels.
        let pela_area = escala_efetiva(4000.0, 4000.0, 2.04);
        assert!(
            4000.0 * 4000.0 * pela_area * pela_area <= PIXELS_MAXIMOS * 1.001,
            "{pela_area}"
        );
        // A página forjada do PdfCraft: os tetos ganham, sem piso.
        let forjada = escala_efetiva(934.0e6, 100.0, 1.0);
        assert!(934.0e6 * forjada <= LADO_MAXIMO * 1.001, "{forjada}");
        // Escala sem sentido não vira página de zero pixels nem infinita.
        for ruim in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let escala = escala_efetiva(595.0, 842.0, ruim);
            assert!(escala.is_finite() && escala > 0.0, "{ruim} → {escala}");
        }
    }

    #[test]
    fn a_borda_parcial_sobe_e_o_ruido_nao() {
        // 612 pt a 150 dpi: 1275 exatos, apesar do ruído do ponto flutuante.
        assert_eq!(pixels_do_dispositivo(612.0, 150.0 / 72.0), 1275);
        assert_eq!(pixels_do_dispositivo(100.0, 1.004), 101);
        assert_eq!(pixels_do_dispositivo(0.0, 1.0), 1);
        assert_eq!(pixels_do_dispositivo(f32::NAN, 1.0), 1);
    }

    /// 🔑 O que o balcão vai abrir: o livro do crate `fotolivro`, página a
    /// página, com a foto no lugar.
    #[test]
    fn o_fotolivro_de_verdade_se_desenha() {
        let mut documento = Documento::abrir(livro()).expect("o livro abre");
        let folhas = documento.folhas();
        assert_eq!(
            folhas.len(),
            fotolivro::Plano::de(2).total(),
            "as páginas do plano do livro"
        );
        // A página deitada: A4 de lado, 297 × 210 mm.
        let folha = folhas[0];
        assert!(
            (folha.largura - 841.89).abs() < 1.0 && (folha.altura - 595.28).abs() < 1.0,
            "{folha:?}"
        );
        assert!(folha.aspecto() > 1.4);

        let pagina = fotolivro::Plano::de(2).pagina_da_foto(0) - 1;
        let imagem = documento.desenhar(pagina, 0.5).expect("a página da foto");
        assert_eq!((imagem.largura, imagem.altura), (421, 298));
        assert_eq!(
            imagem.pixels.len(),
            (imagem.largura * imagem.altura * 4) as usize
        );
        // Opaca de ponta a ponta: é o que deixa a imagem ir à tela sem
        // pré-multiplicar.
        assert!(imagem.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
        // A foto vermelha está na página: muito vermelho, pouco azul.
        let vermelhos = imagem
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 150 && p[1] < 90 && p[2] < 90)
            .count();
        assert!(
            vermelhos > 5_000,
            "só {vermelhos} pixels da foto na página desenhada"
        );
        // Na outra ordem de canais, o vermelho troca de lugar com o azul.
        let bgra = imagem.clone().em(Ordem::Bgra);
        let no_azul = bgra
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[2] > 150 && p[1] < 90 && p[0] < 90)
            .count();
        assert_eq!(no_azul, vermelhos);
    }

    /// 🔗 Os links do livro: a capa leva à galeria e à primeira foto, e a
    /// página da foto leva à foto na galeria — cada um com a sua área.
    #[test]
    fn os_links_do_fotolivro_saem_com_area_e_destino() {
        let documento = Documento::abrir(livro()).expect("o livro abre");
        let links = documento.links();
        let plano = fotolivro::Plano::de(2);
        assert_eq!(links.len(), plano.total());

        // A capa: "Abrir minha galeria" (endereço) e "Folhear o álbum" (a
        // primeira página do livro).
        let capa = &links[0];
        assert!(
            capa.iter()
                .any(|l| l.destino == Destino::Endereco(GALERIA.into())),
            "a capa sem o link da galeria: {capa:?}"
        );
        assert!(
            capa.iter()
                .any(|l| l.destino == Destino::Pagina(plano.primeira_do_livro() - 1)),
            "a capa sem o salto para o álbum: {capa:?}"
        );

        // A página da primeira foto: a foto inteira é o link dela.
        let da_foto = &links[plano.pagina_da_foto(0) - 1];
        let foto = da_foto
            .iter()
            .find(|l| l.destino == Destino::Endereco(format!("{GALERIA}?foto=a.jpg")))
            .unwrap_or_else(|| panic!("a foto sem o link dela: {da_foto:?}"));
        let [esquerda, topo, largura, altura] = foto.caixa;
        assert!(
            largura > 0.3 && altura > 0.3,
            "a área da foto é pequena demais: {:?}",
            foto.caixa
        );
        // Dentro da folha, e com o y já para baixo: a foto fica acima da
        // legenda, então a área não encosta no pé da página.
        assert!(esquerda >= 0.0 && topo >= 0.0 && esquerda + largura <= 1.0);
        assert!(topo + altura < 0.95, "{:?}", foto.caixa);

        // Todo salto interno cai numa página que existe.
        for link in links.iter().flatten() {
            if let Destino::Pagina(pagina) = link.destino {
                assert!(pagina < plano.total(), "{link:?}");
            }
        }
    }

    #[test]
    fn o_que_nao_e_pdf_e_a_pagina_que_nao_existe_viram_frase() {
        let lixo = Arc::new(b"isto nao e um pdf".to_vec());
        let erro = Documento::abrir(lixo).err().expect("lixo não abre");
        assert!(matches!(erro, Erro::Ilegivel(_)), "{erro:?}");
        assert!(erro.to_string().starts_with("o arquivo não é um PDF"));

        let mut documento = Documento::abrir(livro()).unwrap();
        let fora = documento.folhas().len();
        assert_eq!(documento.desenhar(fora, 1.0), Err(Erro::SemPagina(fora)));
        assert_eq!(
            Erro::SemPagina(4).to_string(),
            "o documento não tem a página 5"
        );
    }

    /// A tela: abre em segundo plano, pede e colhe; o pedido novo substitui o
    /// antigo.
    #[test]
    fn a_fila_abre_desenha_e_troca_de_pedido() {
        let fila = Fila::abrir(livro(), Ordem::Bgra);
        let Resposta::Aberto { folhas, links } = colher_ate(&fila, |_| true) else {
            panic!("a primeira resposta é a abertura");
        };
        assert_eq!(folhas.len(), fotolivro::Plano::de(2).total());
        assert_eq!(links.len(), folhas.len(), "uma lista de links por folha");

        let ultima = folhas.len() - 1;
        // Tudo pedido, e logo trocado por só a última: ela chega, e a fila
        // não fica devendo as outras.
        fila.pedir(
            (0..folhas.len())
                .map(|pagina| Pedido {
                    pagina,
                    escala: 0.25,
                })
                .collect(),
        );
        let quero = Pedido {
            pagina: ultima,
            escala: 0.3,
        };
        fila.pedir(vec![quero]);
        let Resposta::Pagina { imagem, .. } = colher_ate(
            &fila,
            |r| matches!(r, Resposta::Pagina { pedido, .. } if *pedido == quero),
        ) else {
            unreachable!()
        };
        let imagem = imagem.expect("a última página");
        assert_eq!(imagem.ordem, Ordem::Bgra);
        assert_eq!(
            imagem.largura,
            pixels_do_dispositivo(folhas[ultima].largura, 0.3)
        );

        // O que não é PDF: a fila diz, e não trava.
        let ruim = Fila::abrir(Arc::new(vec![1, 2, 3]), Ordem::Rgba);
        assert!(matches!(
            colher_ate(&ruim, |_| true),
            Resposta::Falhou(Erro::Ilegivel(_))
        ));
    }
}
