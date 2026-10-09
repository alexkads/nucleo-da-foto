//! A prévia da galeria em PDF: um **fotolivro virtual** que o cliente folheia
//! no celular e de onde baixa ou compra com um toque.
//!
//! # 🔄 O pedido (dono, 09/10/2026)
//!
//! *"A escolha de uso é PDF/Arquivo individual, sendo que a marca d'água fica
//! automática baseado no se a foto foi sinalizada com LEVADA"*, *"a idéia é
//! modernizar e simplificar a vida do cliente!"*, *"o PDF faz coisas
//! impressionantes, utilize as possibilidades do mesmo"*, *"no final tem que
//! ter o link para baixar todas"* e *"tem que ser como se fosse um foto livro
//! virtual bastante apelativo e diagramado"*. É o que vai ao cliente no
//! pós-venda.
//!
//! - **A levada sai limpa; a disponível sai com a marca d'água do sistema** —
//!   a mesma que o cliente vê em `/meus-ensaios`, gravada pelo servidor (C27).
//!   Quem decide é o estado da foto, e não uma escolha no modal.
//! - **É diagramado como livro**: páginas deitadas (o livro aberto), e cada
//!   página escolhe um molde pela orientação das fotos — uma foto grande sobre
//!   o fundo escuro, duas em composição de revista, um trio, o retrato com o
//!   texto ao lado ([`diagramar`]). Títulos em serifa, legendas discretas.
//! - 🖼️ **Cada foto numa moldura de álbum antigo** (dono: *"coloque uma
//!   moldura vintage em cada foto, vale a pena gastarmos tempo na qualidade
//!   desse book"*): a margem creme do papel fotográfico, o filete sépia, a
//!   sombra no papel e as cantoneiras pretas nos quatro cantos —
//!   [`Moldura`]. As cantoneiras seguram a margem, **não a foto**.
//! - 🚨 **Nenhuma foto é cortada** (dono: *"normalmente usamos 4x3 e numa
//!   foto de família alguma pessoa pode ficar cortada"*). A moldura do molde
//!   é só o lugar; a foto entra **inteira**, na proporção dela
//!   ([`posicionar`]), e o que sobra é papel. Nada a cobre: a legenda fica
//!   fora da foto, até na página escura.
//! - **Tudo é tocável**: cada foto abre ela mesma na galeria do cliente
//!   (`/meus-ensaios/<galeria>?foto=<id>`, já escolhida quando está à venda);
//!   a capa leva à galeria e ao álbum; o sumário leva à página de cada foto;
//!   a barra lateral do leitor lista tudo (marcadores).
//! - **O fim fecha a venda**: "Baixar todas as minhas fotos" (o ZIP começa ao
//!   abrir a galeria, `?acao=baixar-todas`) e "Comprar as N disponíveis"
//!   (todas já escolhidas, `?acao=comprar-todas`), com as que esperam por ele.
//!
//! - 🎩 **É a lembrança de uma experiência, e não um catálogo** (dono:
//!   *"Gramado é um turismo de experiência… fazer com que eles voltem sempre e
//!   deixem os amigos com inveja do que eles viveram"*). A capa abre como
//!   uma viagem no tempo, com o lugar e a data; o livro termina em "Até a
//!   próxima viagem", que convida a mostrar aos amigos e a agendar o próximo
//!   ensaio.
//!
//! 🔑 **Link, e não arquivo anexado.** O anexo de PDF só abre no Acrobat; o
//! leitor do celular, do Chrome e do WhatsApp o ignora, e levar os originais
//! dentro faria um PDF de centenas de MB. O link funciona em qualquer leitor,
//! e o download continua passando pelo site, que confere quem pagou.
//!
//! ## O corte deste arquivo
//!
//! [`diagramar`] decide os moldes e [`Plano`] numera as páginas — é o que os
//! links internos usam, e o defeito possível (link para a página errada) mora
//! nos dois, testáveis sem escrever um byte. [`gerar`] desenha.

use image::DynamicImage;
use printpdf::{
    Actions, BorderArray, BuiltinFont, Color, ColorArray, Destination, HighlightingMode,
    ImageCompression, ImageOptimizationOptions, LinePoint, LinkAnnotation, Mm, Op, PaintMode,
    PdfDocument, PdfFontHandle, PdfPage, PdfSaveOptions, Point, Polygon, PolygonRing, Pt, RawImage,
    Rect, Rgb, TextItem, WindingOrder, XObjectId, XObjectTransform,
};

/// Uma foto do álbum.
#[derive(Debug, Clone)]
pub struct FotoDaFolha {
    pub imagem: DynamicImage,
    /// O nome do arquivo, como o cliente o vê na galeria.
    pub nome: String,
    /// Levada no balcão (ou comprada no site): sai limpa, "sua foto".
    pub levada: bool,
    /// A foto aberta na galeria do cliente. `None` = sem galeria no site.
    pub link: Option<String>,
}

/// O que vale para o álbum inteiro.
#[derive(Debug, Clone, Default)]
pub struct Capa {
    /// O título do ensaio.
    pub titulo: String,
    /// A galeria inteira no site — o botão "Abrir minha galeria".
    pub galeria: Option<String>,
    /// O endereço que o rodapé mostra, sem `https://`.
    pub site: String,
    /// O lugar e o dia do ensaio — "Gramado · 9 de outubro de 2026". Vazio
    /// = a capa não diz.
    pub lugar_e_data: String,
    /// Onde se agenda o próximo ensaio — o fim do livro convida a voltar.
    pub agendar: Option<String>,
}

/// A página deitada: o livro aberto, e o que a tela do celular deitado e a
/// do computador mostram inteira.
const PAPEL: (f32, f32) = (297.0, 210.0);
const MARGEM: f32 = 16.0;
const VAO: f32 = 8.0;
/// A linha da legenda embaixo das fotos.
const LEGENDA: f32 = 10.0;
/// Miniaturas por página no sumário.
const SUMARIO_COLUNAS: usize = 6;
const SUMARIO_LINHAS: usize = 3;
pub const POR_SUMARIO: usize = SUMARIO_COLUNAS * SUMARIO_LINHAS;

/// Em pé é abaixo disto; deitada, acima.
fn em_pe(aspecto: f32) -> bool {
    aspecto < 0.95
}

/// Os moldes de página do fotolivro. Os números são os índices das fotos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Molde {
    /// Uma foto deitada sangrando na página inteira.
    Destaque(usize),
    /// Duas em pé, lado a lado.
    DuplaEmPe(usize, usize),
    /// Duas deitadas em composição assimétrica: uma grande no alto, a outra
    /// menor embaixo, do outro lado — o lado alterna a cada página
    /// (`true` = a grande à direita).
    DuplaDeitada(usize, usize, bool),
    /// Uma em pé grande à esquerda e duas deitadas empilhadas à direita.
    Trio(usize, usize, usize),
    /// A em pé sozinha, com o número e o convite ao lado.
    RetratoComTexto(usize),
}

impl Molde {
    pub fn fotos(&self) -> Vec<usize> {
        match *self {
            Molde::Destaque(a) | Molde::RetratoComTexto(a) => vec![a],
            Molde::DuplaEmPe(a, b) | Molde::DuplaDeitada(a, b, _) => vec![a, b],
            Molde::Trio(a, b, c) => vec![a, b, c],
        }
    }
}

/// A diagramação: as fotos, **na ordem do ensaio**, distribuídas em moldes.
///
/// 🔑 A ordem nunca muda — é a ordem em que o cliente as viu no balcão. O que
/// varia é o molde: duas em pé seguidas viram dupla; uma em pé seguida de duas
/// deitadas, trio; as deitadas alternam entre o destaque sangrado e a dupla,
/// para o livro ter ritmo e não ser uma grade.
pub fn diagramar(aspectos: &[f32]) -> Vec<Molde> {
    let mut moldes = Vec::new();
    let mut i = 0;
    let mut deitadas = 0usize;
    let mut duplas = 0usize;
    let pe = |k: usize| aspectos.get(k).is_some_and(|&a| em_pe(a));
    let deitada = |k: usize| aspectos.get(k).is_some_and(|&a| !em_pe(a));
    while i < aspectos.len() {
        if pe(i) {
            if pe(i + 1) {
                moldes.push(Molde::DuplaEmPe(i, i + 1));
                i += 2;
            } else if deitada(i + 1) && deitada(i + 2) {
                moldes.push(Molde::Trio(i, i + 1, i + 2));
                i += 3;
            } else {
                moldes.push(Molde::RetratoComTexto(i));
                i += 1;
            }
        } else {
            if !deitadas.is_multiple_of(3) && deitada(i + 1) {
                moldes.push(Molde::DuplaDeitada(i, i + 1, duplas.is_multiple_of(2)));
                duplas += 1;
                i += 2;
            } else {
                moldes.push(Molde::Destaque(i));
                i += 1;
            }
            deitadas += 1;
        }
    }
    moldes
}

/// As páginas do álbum, numeradas a partir de 1 (a régua do PDF): a capa, o
/// sumário, as páginas diagramadas e o fim.
#[derive(Debug, Clone, PartialEq)]
pub struct Plano {
    pub fotos: usize,
    pub moldes: Vec<Molde>,
}

impl Plano {
    pub fn de(aspectos: &[f32]) -> Self {
        Self {
            fotos: aspectos.len(),
            moldes: diagramar(aspectos),
        }
    }

    pub fn paginas_de_sumario(&self) -> usize {
        self.fotos.div_ceil(POR_SUMARIO).max(1)
    }

    pub fn primeira_do_sumario(&self) -> usize {
        2
    }

    /// A página do sumário onde a foto `i` aparece.
    pub fn sumario_da_foto(&self, i: usize) -> usize {
        2 + i / POR_SUMARIO
    }

    /// A página do livro onde a foto `i` (de 0) está.
    pub fn pagina_da_foto(&self, i: usize) -> usize {
        let molde = self
            .moldes
            .iter()
            .position(|m| m.fotos().contains(&i))
            .unwrap_or(0);
        self.primeira_do_livro() + molde
    }

    pub fn primeira_do_livro(&self) -> usize {
        1 + self.paginas_de_sumario() + 1
    }

    /// O fim: baixar todas e comprar as disponíveis.
    pub fn pagina_final(&self) -> usize {
        self.primeira_do_livro() + self.moldes.len()
    }

    /// A despedida: mostrar aos amigos e voltar.
    pub fn despedida(&self) -> usize {
        self.pagina_final() + 1
    }

    pub fn total(&self) -> usize {
        self.despedida()
    }
}

/// Um retângulo, em mm, contado da **base** do papel (a régua do PDF).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caixa {
    pub x: f32,
    pub y: f32,
    pub largura: f32,
    pub altura: f32,
}

impl Caixa {
    fn nova(x: f32, y: f32, largura: f32, altura: f32) -> Self {
        Self {
            x,
            y,
            largura,
            altura,
        }
    }
}

/// As molduras de um molde, na ordem das fotos dele.
pub fn molduras(molde: &Molde) -> Vec<Caixa> {
    let (l, alto) = PAPEL;
    let util_l = l - 2. * MARGEM;
    let base = MARGEM + LEGENDA;
    let util_a = alto - 2. * MARGEM - LEGENDA;
    match molde {
        // A faixa da legenda fica embaixo, fora da foto.
        Molde::Destaque(_) => vec![Caixa::nova(8., 16., l - 16., alto - 22.)],
        Molde::DuplaEmPe(..) => {
            let largura = (util_l - VAO) / 2.;
            vec![
                Caixa::nova(MARGEM, base, largura, util_a),
                Caixa::nova(MARGEM + largura + VAO, base, largura, util_a),
            ]
        }
        Molde::DuplaDeitada(_, _, a_direita) => {
            let grande = Caixa::nova(0., 0., 168., 112.);
            let pequena_l = util_l - grande.largura - VAO;
            let pequena = Caixa::nova(0., base, pequena_l, pequena_l * 0.75);
            let topo = alto - MARGEM - grande.altura;
            if !a_direita {
                vec![
                    Caixa {
                        x: MARGEM,
                        y: topo,
                        ..grande
                    },
                    Caixa {
                        x: MARGEM + grande.largura + VAO,
                        ..pequena
                    },
                ]
            } else {
                vec![
                    Caixa {
                        x: l - MARGEM - grande.largura,
                        y: topo,
                        ..grande
                    },
                    Caixa {
                        x: MARGEM,
                        ..pequena
                    },
                ]
            }
        }
        Molde::Trio(..) => {
            let esquerda = util_a * 0.75;
            let direita = util_l - esquerda - VAO;
            let meia = (util_a - VAO - LEGENDA) / 2.;
            let x = MARGEM + esquerda + VAO;
            vec![
                Caixa::nova(MARGEM, base, esquerda, util_a),
                Caixa::nova(x, base + meia + VAO + LEGENDA, direita, meia),
                Caixa::nova(x, base, direita, meia),
            ]
        }
        Molde::RetratoComTexto(_) => {
            let altura = alto - 2. * MARGEM;
            vec![Caixa::nova(MARGEM, MARGEM, altura * 0.75, altura)]
        }
    }
}

/// A foto inteira dentro da caixa, sem esticar e centrada.
pub fn encaixar(caixa: &Caixa, aspecto: f32) -> Caixa {
    let aspecto = aspecto.max(0.01);
    let (largura, altura) = if caixa.largura / caixa.altura > aspecto {
        (caixa.altura * aspecto, caixa.altura)
    } else {
        (caixa.largura, caixa.largura / aspecto)
    };
    Caixa::nova(
        caixa.x + (caixa.largura - largura) / 2.,
        caixa.y + (caixa.altura - altura) / 2.,
        largura,
        altura,
    )
}

/// A moldura vintage de uma foto: onde vão a sombra, a margem, a foto, o
/// filete e as cantoneiras, em mm.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moldura {
    /// A margem de papel fotográfico em volta da foto.
    pub margem: Caixa,
    /// A foto, inteira.
    pub foto: Caixa,
    /// A largura da margem.
    pub borda: f32,
    /// O cateto das cantoneiras.
    pub cantoneira: f32,
}

/// A moldura que cabe no lugar: a foto inteira, a margem em volta dela e
/// as cantoneiras.
///
/// 🔑 **A cantoneira cobre só a margem.** O cateto é menor que duas bordas,
/// então a diagonal passa antes do canto da foto — nada dela some.
pub fn emoldurar(lugar: &Caixa, aspecto: f32, alinhar: Alinhar) -> Moldura {
    let borda = (lugar.largura.min(lugar.altura) * 0.04).clamp(1.2, 5.5);
    let folga = borda + SOMBRA;
    let por_dentro = Caixa::nova(
        lugar.x + borda,
        lugar.y + folga,
        (lugar.largura - borda - folga).max(1.),
        (lugar.altura - borda - folga).max(1.),
    );
    let foto = posicionar(&por_dentro, aspecto, alinhar);
    let margem = Caixa::nova(
        foto.x - borda,
        foto.y - borda,
        foto.largura + 2. * borda,
        foto.altura + 2. * borda,
    );
    Moldura {
        margem,
        foto,
        borda,
        cantoneira: borda * 1.85,
    }
}

/// O deslocamento da sombra da foto no papel.
const SOMBRA: f32 = 0.9;

/// Para que lado a foto encosta dentro da moldura, quando sobra lugar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alinhar {
    Centro,
    Esquerda,
    Direita,
}

/// 🚨 A foto **inteira** na moldura: [`encaixar`], e depois encostada no lado
/// pedido. Nunca maior que a moldura, nunca cortada, nunca esticada.
pub fn posicionar(moldura: &Caixa, aspecto: f32, alinhar: Alinhar) -> Caixa {
    let mut dentro = encaixar(moldura, aspecto);
    match alinhar {
        Alinhar::Centro => {}
        Alinhar::Esquerda => dentro.x = moldura.x,
        Alinhar::Direita => dentro.x = moldura.x + moldura.largura - dentro.largura,
    }
    dentro
}

/// O resumo: "9 fotos · 3 suas · 6 disponíveis para compra".
pub fn resumo(fotos: &[FotoDaFolha]) -> String {
    let levadas = fotos.iter().filter(|f| f.levada).count();
    let disponiveis = fotos.len() - levadas;
    let mut partes = vec![match fotos.len() {
        1 => "1 foto".to_string(),
        n => format!("{n} fotos"),
    }];
    if levadas > 0 {
        partes.push(match levadas {
            1 => "1 sua".to_string(),
            n => format!("{n} suas"),
        });
    }
    if disponiveis > 0 {
        partes.push(match disponiveis {
            1 => "1 disponível para compra".to_string(),
            n => format!("{n} disponíveis para compra"),
        });
    }
    partes.join(" · ")
}

/// A foto da capa: a primeira levada (sai limpa), senão a primeira.
pub fn foto_da_capa(fotos: &[FotoDaFolha]) -> usize {
    fotos.iter().position(|f| f.levada).unwrap_or(0)
}

/// O endereço da galeria com uma ação do fim do álbum.
pub fn com_acao(galeria: &str, acao: &str) -> String {
    let separador = if galeria.contains('?') { '&' } else { '?' };
    format!("{galeria}{separador}acao={acao}")
}

/// O lado maior com que a foto entra no PDF: a página sangrada num monitor
/// com folga. A marcada já chega com 1400 px do servidor.
const LADO_NA_FOLHA: u32 = 1800;

/// As cores do livro: papel quente, tinta quase preta, o âmbar e o verde do
/// site para o que se compra e o que já é dele.
struct Paleta {
    tinta: Color,
    apagado: Color,
    papel: Color,
    escuro: Color,
    moldura: Color,
    verde: Color,
    ambar: Color,
    branco: Color,
}

fn rgb(r: f32, g: f32, b: f32) -> Color {
    Color::Rgb(Rgb {
        r,
        g,
        b,
        icc_profile: None,
    })
}

impl Paleta {
    fn nova() -> Self {
        Self {
            tinta: rgb(0.13, 0.11, 0.09),
            apagado: rgb(0.47, 0.44, 0.40),
            papel: rgb(0.965, 0.95, 0.925),
            escuro: rgb(0.10, 0.09, 0.08),
            moldura: rgb(0.91, 0.89, 0.85),
            verde: rgb(0.16, 0.47, 0.30),
            ambar: rgb(0.70, 0.43, 0.13),
            branco: rgb(1., 1., 1.),
        }
    }
}

/// As cores da moldura de álbum antigo. A sombra não está aqui: ela é o
/// fundo da página escurecido ([`Pagina::nova`]).
struct MolduraVintage {
    papel: Color,
    filete: Color,
    cantoneira: Color,
}

impl MolduraVintage {
    fn nova() -> Self {
        Self {
            papel: rgb(0.985, 0.972, 0.94),
            filete: rgb(0.55, 0.45, 0.34),
            cantoneira: rgb(0.13, 0.11, 0.10),
        }
    }
}

/// O fundo escurecido — a sombra da foto sobre ele.
fn sombra_de(fundo: &Color) -> Color {
    match fundo {
        Color::Rgb(c) => rgb(c.r * 0.8, c.g * 0.78, c.b * 0.75),
        _ => rgb(0.7, 0.67, 0.62),
    }
}

fn pt(mm: f32) -> Pt {
    Mm(mm).into()
}

#[derive(Clone, Copy, PartialEq)]
enum Letra {
    Sans,
    SansNegrito,
    Serifa,
    SerifaNegrito,
    SerifaItalico,
}

impl Letra {
    fn fonte(self) -> BuiltinFont {
        match self {
            Letra::Sans => BuiltinFont::Helvetica,
            Letra::SansNegrito => BuiltinFont::HelveticaBold,
            Letra::Serifa => BuiltinFont::TimesRoman,
            Letra::SerifaNegrito => BuiltinFont::TimesBold,
            Letra::SerifaItalico => BuiltinFont::TimesItalic,
        }
    }

    /// A largura média de um caractere, em em — para alinhar e quebrar linha.
    fn em(self) -> f32 {
        match self {
            Letra::Sans => 0.5,
            Letra::SansNegrito => 0.56,
            Letra::Serifa | Letra::SerifaItalico => 0.45,
            Letra::SerifaNegrito => 0.49,
        }
    }
}

fn largura_do_texto(texto: &str, tamanho: f32, letra: Letra) -> f32 {
    texto.chars().count() as f32 * tamanho * letra.em() * 25.4 / 72.
}

/// Corta o texto que não cabe, com reticências.
fn caber(texto: &str, tamanho: f32, letra: Letra, largura: f32) -> String {
    if largura_do_texto(texto, tamanho, letra) <= largura {
        return texto.to_string();
    }
    let mut curto: String = texto.chars().collect();
    while !curto.is_empty() && largura_do_texto(&format!("{curto}…"), tamanho, letra) > largura {
        curto.pop();
    }
    format!("{curto}…")
}

/// Quebra o título em linhas que caibam na largura, palavra a palavra.
pub fn quebrar(texto: &str, tamanho: f32, largura: f32, maximo: usize) -> Vec<String> {
    let letra = Letra::SerifaNegrito;
    let mut linhas: Vec<String> = Vec::new();
    let mut atual = String::new();
    for palavra in texto.split_whitespace() {
        let tentativa = if atual.is_empty() {
            palavra.to_string()
        } else {
            format!("{atual} {palavra}")
        };
        if largura_do_texto(&tentativa, tamanho, letra) <= largura || atual.is_empty() {
            atual = tentativa;
        } else {
            linhas.push(std::mem::take(&mut atual));
            atual = palavra.to_string();
        }
    }
    if !atual.is_empty() {
        linhas.push(atual);
    }
    if linhas.len() > maximo {
        let resto = linhas[maximo - 1..].join(" ");
        linhas.truncate(maximo - 1);
        linhas.push(caber(&resto, tamanho, letra, largura));
    }
    linhas
}

struct Pagina {
    ops: Vec<Op>,
    sombra: Color,
    /// Sobre fundo escuro a cantoneira preta some: lá ela é dourada.
    escura: bool,
}

/// O fundo é escuro?
fn escuro(fundo: &Color) -> bool {
    matches!(fundo, Color::Rgb(c) if c.r + c.g + c.b < 1.2)
}

impl Pagina {
    /// Daqui em diante, as fotos estão sobre este fundo (um painel).
    fn sobre(&mut self, fundo: &Color) {
        self.sombra = sombra_de(fundo);
        self.escura = escuro(fundo);
    }

    fn nova(fundo: &Color) -> Self {
        let mut p = Self {
            ops: Vec::new(),
            sombra: sombra_de(fundo),
            escura: escuro(fundo),
        };
        p.retangulo(Caixa::nova(0., 0., PAPEL.0, PAPEL.1), fundo);
        p
    }

    fn texto(&mut self, conteudo: &str, x: f32, y: f32, tamanho: f32, letra: Letra, cor: &Color) {
        self.ops.push(Op::StartTextSection);
        self.ops.push(Op::SetFont {
            font: PdfFontHandle::Builtin(letra.fonte()),
            size: Pt(tamanho),
        });
        self.ops.push(Op::SetFillColor { col: cor.clone() });
        self.ops.push(Op::SetTextCursor {
            pos: Point { x: pt(x), y: pt(y) },
        });
        self.ops.push(Op::ShowText {
            items: vec![TextItem::Text(conteudo.to_string())],
        });
        self.ops.push(Op::EndTextSection);
    }

    /// O sobretítulo em caixa alta, com as letras espaçadas.
    fn espacado(&mut self, conteudo: &str, x: f32, y: f32, tamanho: f32, cor: &Color) {
        let espacado: String = conteudo
            .chars()
            .map(|c| {
                if c == ' ' {
                    "  ".to_string()
                } else {
                    format!("{c} ")
                }
            })
            .collect();
        self.texto(espacado.trim_end(), x, y, tamanho, Letra::SansNegrito, cor);
    }

    fn retangulo(&mut self, caixa: Caixa, cor: &Color) {
        self.ops.push(Op::SaveGraphicsState);
        self.ops.push(Op::SetFillColor { col: cor.clone() });
        self.ops.push(Op::DrawRectangle {
            rectangle: Rect {
                mode: Some(PaintMode::Fill),
                ..Rect::from_xywh(
                    pt(caixa.x),
                    pt(caixa.y),
                    pt(caixa.largura),
                    pt(caixa.altura),
                )
            },
        });
        self.ops.push(Op::RestoreGraphicsState);
    }

    fn filete(&mut self, x: f32, y: f32, largura: f32, cor: &Color) {
        self.retangulo(Caixa::nova(x, y, largura, 0.6), cor);
    }

    fn acao(&mut self, caixa: Caixa, acao: Actions) {
        self.ops.push(Op::LinkAnnotation {
            link: LinkAnnotation::new(
                Rect::from_xywh(
                    pt(caixa.x),
                    pt(caixa.y),
                    pt(caixa.largura),
                    pt(caixa.altura),
                ),
                acao,
                Some(BorderArray::Solid([0., 0., 0.])),
                Some(ColorArray::Transparent),
                Some(HighlightingMode::None),
            ),
        });
    }

    fn link(&mut self, caixa: Caixa, destino: &str) {
        self.acao(caixa, Actions::Uri(destino.to_string()));
    }

    fn ir_para(&mut self, caixa: Caixa, pagina: usize) {
        self.acao(
            caixa,
            Actions::go_to(Destination::Xyz {
                page: pagina,
                left: None,
                top: None,
                zoom: None,
            }),
        );
    }

    fn imagem(&mut self, id: &XObjectId, px: (u32, u32), onde: Caixa) {
        // A régua de 72 dpi: 1 px = 1 pt, e a escala leva ao tamanho pedido.
        let natural = (px.0 as f32 * 25.4 / 72., px.1 as f32 * 25.4 / 72.);
        self.ops.push(Op::UseXobject {
            id: id.clone(),
            transform: XObjectTransform {
                translate_x: Some(pt(onde.x)),
                translate_y: Some(pt(onde.y)),
                scale_x: Some(onde.largura / natural.0),
                scale_y: Some(onde.altura / natural.1),
                dpi: Some(72.),
                ..Default::default()
            },
        });
    }

    fn triangulo(&mut self, a: (f32, f32), b: (f32, f32), c: (f32, f32), cor: &Color) {
        let ponto = |(x, y): (f32, f32)| LinePoint {
            p: Point { x: pt(x), y: pt(y) },
            bezier: false,
        };
        self.ops.push(Op::SaveGraphicsState);
        self.ops.push(Op::SetFillColor { col: cor.clone() });
        self.ops.push(Op::DrawPolygon {
            polygon: Polygon {
                rings: vec![PolygonRing {
                    points: vec![ponto(a), ponto(b), ponto(c)],
                }],
                mode: PaintMode::Fill,
                winding_order: WindingOrder::NonZero,
            },
        });
        self.ops.push(Op::RestoreGraphicsState);
    }

    fn contorno(&mut self, caixa: Caixa, espessura: f32, cor: &Color) {
        self.ops.push(Op::SaveGraphicsState);
        self.ops.push(Op::SetOutlineColor { col: cor.clone() });
        self.ops.push(Op::SetOutlineThickness { pt: pt(espessura) });
        self.ops.push(Op::DrawRectangle {
            rectangle: Rect {
                mode: Some(PaintMode::Stroke),
                ..Rect::from_xywh(
                    pt(caixa.x),
                    pt(caixa.y),
                    pt(caixa.largura),
                    pt(caixa.altura),
                )
            },
        });
        self.ops.push(Op::RestoreGraphicsState);
    }

    /// 🖼️ A foto inteira, na moldura de álbum antigo (ver [`emoldurar`]).
    /// Devolve a margem — é embaixo dela que vai a legenda, e nela o toque.
    fn foto_inteira(
        &mut self,
        id: &XObjectId,
        px: (u32, u32),
        lugar: Caixa,
        alinhar: Alinhar,
    ) -> Caixa {
        let m = emoldurar(&lugar, px.0 as f32 / px.1.max(1) as f32, alinhar);
        let MolduraVintage {
            papel,
            filete,
            cantoneira,
        } = MolduraVintage::nova();
        let sombra = self.sombra.clone();
        let cantoneira = if self.escura {
            rgb(0.74, 0.60, 0.37)
        } else {
            cantoneira
        };
        // A sombra: a foto colada um pouco acima do papel do álbum.
        self.retangulo(
            Caixa {
                x: m.margem.x + SOMBRA,
                y: m.margem.y - SOMBRA,
                ..m.margem
            },
            &sombra,
        );
        self.retangulo(m.margem, &papel);
        self.imagem(id, px, m.foto);
        // O filete sépia, rente à foto, como a borda impressa das cópias de
        // laboratório antigas.
        let recuo = m.borda * 0.42;
        self.contorno(
            Caixa::nova(
                m.foto.x - recuo,
                m.foto.y - recuo,
                m.foto.largura + 2. * recuo,
                m.foto.altura + 2. * recuo,
            ),
            (m.borda * 0.05).clamp(0.12, 0.25),
            &filete,
        );
        // As cantoneiras, nos quatro cantos da margem.
        let (x0, y0) = (m.margem.x, m.margem.y);
        let (x1, y1) = (x0 + m.margem.largura, y0 + m.margem.altura);
        let c = m.cantoneira;
        self.triangulo((x0, y0), (x0 + c, y0), (x0, y0 + c), &cantoneira);
        self.triangulo((x1, y0), (x1 - c, y0), (x1, y0 + c), &cantoneira);
        self.triangulo((x0, y1), (x0 + c, y1), (x0, y1 - c), &cantoneira);
        self.triangulo((x1, y1), (x1 - c, y1), (x1, y1 - c), &cantoneira);
        m.margem
    }

    /// Um botão: o retângulo cheio, o rótulo centrado.
    fn botao(&mut self, caixa: Caixa, rotulo: &str, tamanho: f32, fundo: &Color, letra: &Color) {
        self.retangulo(caixa, fundo);
        let largura = largura_do_texto(rotulo, tamanho, Letra::SansNegrito);
        self.texto(
            rotulo,
            caixa.x + (caixa.largura - largura) / 2.,
            caixa.y + caixa.altura / 2. - tamanho * 0.36 * 25.4 / 72.,
            tamanho,
            Letra::SansNegrito,
            letra,
        );
    }

    /// O número da página, centrado embaixo, como num livro.
    fn folio(&mut self, numero: usize, cor: &Color) {
        let texto = numero.to_string();
        let largura = largura_do_texto(&texto, 8., Letra::Serifa);
        self.texto(&texto, (PAPEL.0 - largura) / 2., 7., 8., Letra::Serifa, cor);
    }
}

/// Encolhe para o tamanho do álbum — nunca amplia.
fn reduzida(imagem: &DynamicImage) -> DynamicImage {
    if imagem.width().max(imagem.height()) <= LADO_NA_FOLHA {
        return imagem.clone();
    }
    imagem.resize(
        LADO_NA_FOLHA,
        LADO_NA_FOLHA,
        image::imageops::FilterType::Lanczos3,
    )
}

type Imagens = Vec<(XObjectId, (u32, u32))>;

struct Livro<'a> {
    capa: &'a Capa,
    fotos: &'a [FotoDaFolha],
    imagens: Imagens,
    plano: Plano,
    titulo: String,
    resumo: String,
    cor: Paleta,
}

impl Livro<'_> {
    /// A legenda de uma foto: o número e o que o toque faz. O nome do
    /// arquivo fica nos marcadores — na página ele só disputaria com a foto.
    fn legenda(&self, p: &mut Pagina, i: usize, x: f32, y: f32, claro: bool) {
        let foto = &self.fotos[i];
        let numero = format!("{:02}", i + 1);
        let cor_numero = if claro {
            &self.cor.branco
        } else {
            &self.cor.tinta
        };
        p.texto(&numero, x, y, 10., Letra::SerifaItalico, cor_numero);
        let (estado, tom) = match (foto.levada, foto.link.is_some()) {
            (true, true) => ("SUA FOTO · TOQUE PARA BAIXAR", &self.cor.verde),
            (true, false) => ("SUA FOTO", &self.cor.verde),
            (false, true) => ("DISPONÍVEL · TOQUE PARA COMPRAR", &self.cor.ambar),
            (false, false) => ("DISPONÍVEL", &self.cor.ambar),
        };
        let tom = if claro { &self.cor.branco } else { tom };
        p.espacado(estado, x + 7., y + 0.2, 6., tom);
    }

    fn tocar_na_foto(&self, p: &mut Pagina, i: usize, caixa: Caixa) {
        if let Some(destino) = &self.fotos[i].link {
            p.link(caixa, destino);
        }
    }

    fn capa(&self) -> PdfPage {
        let cor = &self.cor;
        let mut p = Pagina::nova(&cor.papel);
        // A foto à esquerda, inteira, num painel escuro da altura toda — o
        // passe-partout do livro.
        let i = foto_da_capa(self.fotos);
        let (id, px) = &self.imagens[i];
        let painel = Caixa::nova(0., 0., PAPEL.0 * 0.6, PAPEL.1);
        p.retangulo(painel, &cor.escuro);
        p.sobre(&cor.escuro);
        p.foto_inteira(
            id,
            *px,
            Caixa::nova(10., 10., painel.largura - 20., painel.altura - 20.),
            Alinhar::Centro,
        );

        let x = PAPEL.0 * 0.6 + 16.;
        let largura = PAPEL.0 - x - MARGEM;
        let mut y = PAPEL.1 - 36.;
        p.espacado("UMA VIAGEM NO TEMPO", x, y, 6.5, &cor.ambar);
        y -= 14.;
        for linha in quebrar(&self.titulo, 26., largura, 3) {
            p.texto(&linha, x, y, 26., Letra::SerifaNegrito, &cor.tinta);
            y -= 10.5;
        }
        if !self.capa.lugar_e_data.is_empty() {
            p.texto(
                &caber(&self.capa.lugar_e_data, 11., Letra::SerifaItalico, largura),
                x,
                y + 2.,
                11.,
                Letra::SerifaItalico,
                &cor.apagado,
            );
            y -= 6.;
        }
        y -= 1.;
        p.filete(x, y, 24., &cor.ambar);
        y -= 9.;
        for linha in [
            "Por algumas horas, vocês viveram outra época.",
            "Aqui está ela — para folhear, guardar e",
            "mostrar a quem não estava lá.",
        ] {
            p.texto(linha, x, y, 9.5, Letra::SerifaItalico, &cor.tinta);
            y -= 4.8;
        }
        y -= 3.;
        p.texto(&self.resumo, x, y, 7.5, Letra::Sans, &cor.apagado);

        let mut base = MARGEM + 4.;
        let folhear = Caixa::nova(x, base, largura, 11.);
        p.botao(folhear, "Folhear o álbum  ›", 10., &cor.moldura, &cor.tinta);
        p.ir_para(folhear, self.plano.primeira_do_livro());
        base += 15.;
        if let Some(galeria) = &self.capa.galeria {
            let botao = Caixa::nova(x, base, largura, 11.);
            p.botao(botao, "Abrir minha galeria", 10., &cor.escuro, &cor.branco);
            p.link(botao, galeria);
        }
        PdfPage::new(Mm(PAPEL.0), Mm(PAPEL.1), p.ops)
    }

    fn sumario(&self, numero: usize) -> PdfPage {
        let cor = &self.cor;
        let mut p = Pagina::nova(&cor.papel);
        let topo = PAPEL.1 - MARGEM;
        p.espacado("NESTE ÁLBUM", MARGEM, topo - 4., 6.5, &cor.ambar);
        p.texto(
            &self.titulo,
            MARGEM,
            topo - 13.,
            16.,
            Letra::SerifaNegrito,
            &cor.tinta,
        );
        p.texto(
            &self.resumo,
            MARGEM,
            topo - 19.,
            8.5,
            Letra::Sans,
            &cor.apagado,
        );

        let util_l = PAPEL.0 - 2. * MARGEM;
        let topo_da_grade = topo - 27.;
        let util_a = topo_da_grade - MARGEM - 4.;
        let vao = 5.;
        let lado = ((util_l - vao * (SUMARIO_COLUNAS as f32 - 1.)) / SUMARIO_COLUNAS as f32)
            .min((util_a - (vao + 5.) * (SUMARIO_LINHAS as f32 - 1.) - 5.) / SUMARIO_LINHAS as f32);
        let inicio = numero * POR_SUMARIO;
        for (k, i) in (inicio..self.fotos.len().min(inicio + POR_SUMARIO)).enumerate() {
            let (coluna, linha) = (k % SUMARIO_COLUNAS, k / SUMARIO_COLUNAS);
            let caixa = Caixa::nova(
                MARGEM + coluna as f32 * (lado + vao),
                topo_da_grade - (linha as f32 + 1.) * lado - linha as f32 * (vao + 5.),
                lado,
                lado,
            );
            let (id, px) = &self.imagens[i];
            p.retangulo(caixa, &cor.moldura);
            p.foto_inteira(
                id,
                *px,
                Caixa::nova(
                    caixa.x + 1.5,
                    caixa.y + 1.5,
                    caixa.largura - 3.,
                    caixa.altura - 3.,
                ),
                Alinhar::Centro,
            );
            let tom = if self.fotos[i].levada {
                &cor.verde
            } else {
                &cor.ambar
            };
            p.retangulo(Caixa::nova(caixa.x, caixa.y - 3.6, 2.2, 2.2), tom);
            p.texto(
                &format!("{:02}", i + 1),
                caixa.x + 3.6,
                caixa.y - 3.5,
                7.5,
                Letra::SerifaItalico,
                &cor.tinta,
            );
            p.ir_para(
                Caixa::nova(caixa.x, caixa.y - 5., caixa.largura, caixa.altura + 5.),
                self.plano.pagina_da_foto(i),
            );
        }
        // O que os quadradinhos querem dizer.
        let y = 8.;
        p.retangulo(Caixa::nova(MARGEM, y, 2.2, 2.2), &cor.verde);
        p.texto("sua", MARGEM + 3.6, y + 0.1, 7., Letra::Sans, &cor.apagado);
        p.retangulo(Caixa::nova(MARGEM + 14., y, 2.2, 2.2), &cor.ambar);
        p.texto(
            "disponível para compra",
            MARGEM + 17.6,
            y + 0.1,
            7.,
            Letra::Sans,
            &cor.apagado,
        );
        p.folio(self.plano.primeira_do_sumario() + numero, &cor.apagado);
        PdfPage::new(Mm(PAPEL.0), Mm(PAPEL.1), p.ops)
    }

    fn pagina(&self, molde: &Molde, numero: usize) -> PdfPage {
        let cor = &self.cor;
        let caixas = molduras(molde);
        let fundo = match molde {
            Molde::Destaque(_) => &cor.escuro,
            _ => &cor.papel,
        };
        let mut p = Pagina::nova(fundo);

        match molde {
            Molde::Destaque(i) => {
                let (id, px) = &self.imagens[*i];
                let onde = p.foto_inteira(id, *px, caixas[0], Alinhar::Centro);
                // A legenda no pé, embaixo da foto — nunca sobre ela.
                self.legenda(&mut p, *i, onde.x, onde.y - 6.5, true);
                self.tocar_na_foto(&mut p, *i, onde);
            }
            Molde::RetratoComTexto(i) => {
                let (id, px) = &self.imagens[*i];
                let onde = p.foto_inteira(id, *px, caixas[0], Alinhar::Esquerda);
                self.tocar_na_foto(&mut p, *i, onde);
                let x = onde.x + onde.largura + 20.;
                let largura = PAPEL.0 - x - MARGEM - 6.;
                let foto = &self.fotos[*i];
                let mut y = PAPEL.1 - 62.;
                p.texto(
                    &format!("{:02}", i + 1),
                    x,
                    y,
                    60.,
                    Letra::SerifaItalico,
                    &cor.moldura,
                );
                y -= 16.;
                let (frase, convite, tom) = if foto.levada {
                    (
                        "Esta é sua.",
                        "Toque para baixar em alta resolução.",
                        &cor.verde,
                    )
                } else {
                    (
                        "Esperando por você.",
                        "Leve para casa, sem a marca d'água.",
                        &cor.ambar,
                    )
                };
                p.texto(frase, x, y, 22., Letra::SerifaNegrito, &cor.tinta);
                y -= 8.;
                p.texto(
                    &caber(convite, 10., Letra::SerifaItalico, largura),
                    x,
                    y,
                    10.,
                    Letra::SerifaItalico,
                    &cor.apagado,
                );
                y -= 7.;
                p.texto(
                    &caber(&foto.nome, 7.5, Letra::Sans, largura),
                    x,
                    y,
                    7.5,
                    Letra::Sans,
                    &cor.apagado,
                );
                if let Some(destino) = &foto.link {
                    let rotulo = if foto.levada {
                        "Baixar esta foto"
                    } else {
                        "Comprar esta foto"
                    };
                    let botao = Caixa::nova(x, y - 22., 60., 12.);
                    p.botao(botao, rotulo, 10.5, tom, &cor.branco);
                    p.link(botao, destino);
                }
            }
            _ => {
                let alinhamentos: Vec<Alinhar> = match molde {
                    Molde::DuplaDeitada(_, _, false) => vec![Alinhar::Esquerda, Alinhar::Direita],
                    Molde::DuplaDeitada(_, _, true) => vec![Alinhar::Direita, Alinhar::Esquerda],
                    Molde::DuplaEmPe(..) => vec![Alinhar::Direita, Alinhar::Esquerda],
                    _ => vec![Alinhar::Esquerda; 3],
                };
                let mut lugares = Vec::new();
                for (k, i) in molde.fotos().into_iter().enumerate() {
                    let (id, px) = &self.imagens[i];
                    let onde = p.foto_inteira(id, *px, caixas[k], alinhamentos[k]);
                    self.legenda(&mut p, i, onde.x, onde.y - 6., false);
                    self.tocar_na_foto(&mut p, i, onde);
                    lugares.push(onde);
                }
                if let Molde::DuplaDeitada(a, b, _) = molde {
                    // O vão acima da menor vira o texto da página, como numa
                    // revista: os números grandes e o que cada toque faz.
                    let coluna = lugares[1];
                    let mut y = PAPEL.1 - MARGEM - 26.;
                    p.texto(
                        &format!("{:02}  {:02}", a + 1, b + 1),
                        coluna.x,
                        y,
                        40.,
                        Letra::SerifaItalico,
                        &cor.moldura,
                    );
                    y -= 14.;
                    let frase = match (self.fotos[*a].levada, self.fotos[*b].levada) {
                        (true, true) => "As duas são suas.",
                        (false, false) => "As duas esperam por você.",
                        _ => "Uma é sua; a outra espera por você.",
                    };
                    for linha in quebrar(frase, 15., coluna.largura, 2) {
                        p.texto(&linha, coluna.x, y, 15., Letra::SerifaNegrito, &cor.tinta);
                        y -= 6.5;
                    }
                    y -= 1.;
                    p.filete(coluna.x, y, 18., &cor.ambar);
                    y -= 7.;
                    p.texto(
                        "Toque na foto para abrir no site.",
                        coluna.x,
                        y,
                        9.,
                        Letra::SerifaItalico,
                        &cor.apagado,
                    );
                }
            }
        }
        let folio = if matches!(molde, Molde::Destaque(_)) {
            &cor.moldura
        } else {
            &cor.apagado
        };
        p.folio(self.plano.primeira_do_livro() + numero, folio);
        PdfPage::new(Mm(PAPEL.0), Mm(PAPEL.1), p.ops)
    }

    /// "Até a próxima viagem": a última página. A foto da capa outra vez,
    /// o convite a mostrar aos amigos e a voltar.
    fn despedida(&self) -> PdfPage {
        let cor = &self.cor;
        let mut p = Pagina::nova(&cor.escuro);
        // A última foto do ensaio — de preferência uma dele, que sai limpa.
        let i = (0..self.fotos.len())
            .rev()
            .find(|&i| self.fotos[i].levada)
            .unwrap_or(self.fotos.len() - 1);
        let (id, px) = &self.imagens[i];
        let lugar = Caixa::nova(MARGEM, MARGEM, PAPEL.0 * 0.52, PAPEL.1 - 2. * MARGEM);
        let onde = p.foto_inteira(id, *px, lugar, Alinhar::Centro);
        p.ir_para(onde, self.plano.pagina_da_foto(i));

        let x = PAPEL.0 * 0.52 + MARGEM + 16.;
        let largura = PAPEL.0 - x - MARGEM;
        let claro = &cor.moldura;
        let mut y = PAPEL.1 - 50.;
        p.espacado("FIM DESTA HISTÓRIA", x, y, 6.5, &rgb(0.74, 0.60, 0.37));
        y -= 15.;
        p.texto(
            "Até a próxima",
            x,
            y,
            30.,
            Letra::SerifaNegrito,
            &cor.branco,
        );
        y -= 12.;
        p.texto("viagem.", x, y, 30., Letra::SerifaItalico, &cor.branco);
        y -= 6.;
        p.filete(x, y, 24., &rgb(0.74, 0.60, 0.37));
        y -= 10.;
        for linha in [
            "Mande este livro a quem você ama — ele foi",
            "feito para passar de mão em mão. E quando a",
            "saudade bater, outra época espera por vocês.",
        ] {
            p.texto(linha, x, y, 10., Letra::SerifaItalico, claro);
            y -= 5.2;
        }
        y -= 14.;
        if let Some(agendar) = &self.capa.agendar {
            let botao = Caixa::nova(x, y, largura, 14.);
            p.botao(
                botao,
                "Agendar o próximo ensaio",
                11.5,
                &rgb(0.74, 0.60, 0.37),
                &cor.escuro,
            );
            p.link(botao, agendar);
            y -= 20.;
        }
        if let Some(galeria) = &self.capa.galeria {
            let botao = Caixa::nova(x, y, largura, 11.);
            p.botao(
                botao,
                "Abrir minha galeria",
                10.,
                &rgb(0.2, 0.18, 0.16),
                claro,
            );
            p.link(botao, galeria);
        }
        if !self.capa.site.is_empty() {
            p.texto(&self.capa.site, x, MARGEM, 8., Letra::Sans, &cor.apagado);
        }
        PdfPage::new(Mm(PAPEL.0), Mm(PAPEL.1), p.ops)
    }

    fn fim(&self) -> PdfPage {
        let cor = &self.cor;
        let mut p = Pagina::nova(&cor.papel);
        let levadas = self.fotos.iter().filter(|f| f.levada).count();
        let disponiveis = self.fotos.len() - levadas;

        // À esquerda, as que ainda esperam por ele (e as dele, se sobrar
        // lugar), em mosaico — cada uma leva à sua página.
        let mosaico: Vec<usize> = (0..self.fotos.len())
            .filter(|&i| !self.fotos[i].levada)
            .chain((0..self.fotos.len()).filter(|&i| self.fotos[i].levada))
            .take(4)
            .collect();
        let painel = Caixa::nova(0., 0., PAPEL.0 * 0.5, PAPEL.1);
        p.retangulo(painel, &cor.escuro);
        p.sobre(&cor.escuro);
        let area = Caixa::nova(8., 8., painel.largura - 16., painel.altura - 16.);
        let vao = 4.;
        let (colunas, linhas) = match mosaico.len() {
            1 => (1, 1),
            2 => (1, 2),
            _ => (2, 2),
        };
        let lado_l = (area.largura - vao * (colunas as f32 - 1.)) / colunas as f32;
        let lado_a = (area.altura - vao * (linhas as f32 - 1.)) / linhas as f32;
        for (k, &i) in mosaico.iter().enumerate() {
            let (coluna, linha) = (k % colunas, k / colunas);
            let caixa = Caixa::nova(
                area.x + coluna as f32 * (lado_l + vao),
                area.y + area.altura - (linha as f32 + 1.) * lado_a - linha as f32 * vao,
                lado_l,
                lado_a,
            );
            let (id, px) = &self.imagens[i];
            let onde = p.foto_inteira(id, *px, caixa, Alinhar::Centro);
            p.ir_para(onde, self.plano.pagina_da_foto(i));
        }

        let x = PAPEL.0 * 0.5 + 18.;
        let largura = PAPEL.0 - x - MARGEM;
        let mut y = PAPEL.1 - 44.;
        p.espacado("PARA LEVAR PARA CASA", x, y, 6.5, &cor.ambar);
        y -= 13.;
        p.texto(
            "Leve o seu ensaio",
            x,
            y,
            28.,
            Letra::SerifaNegrito,
            &cor.tinta,
        );
        y -= 4.;
        p.filete(x, y, 24., &cor.ambar);
        y -= 9.;
        for linha in [
            "Com um toque: as suas fotos num arquivo só,",
            "e as outras direto para a compra.",
        ] {
            p.texto(linha, x, y, 10., Letra::SerifaItalico, &cor.apagado);
            y -= 5.;
        }
        y -= 16.;

        if let Some(galeria) = &self.capa.galeria {
            let altura = 14.;
            if levadas > 0 {
                let rotulo = match levadas {
                    1 => "Baixar a minha foto".to_string(),
                    n => format!("Baixar todas as minhas fotos ({n})"),
                };
                let botao = Caixa::nova(x, y, largura, altura);
                p.botao(botao, &rotulo, 11.5, &cor.verde, &cor.branco);
                p.link(botao, &com_acao(galeria, "baixar-todas"));
                y -= altura + 6.;
            }
            if disponiveis > 0 {
                let rotulo = match disponiveis {
                    1 => "Comprar a foto disponível".to_string(),
                    n => format!("Comprar as {n} disponíveis"),
                };
                let botao = Caixa::nova(x, y, largura, altura);
                p.botao(botao, &rotulo, 11.5, &cor.ambar, &cor.branco);
                p.link(botao, &com_acao(galeria, "comprar-todas"));
                y -= altura + 6.;
            }
            let botao = Caixa::nova(x, y, largura, 11.);
            p.botao(botao, "Abrir minha galeria", 10., &cor.moldura, &cor.tinta);
            p.link(botao, galeria);
        }
        p.texto(
            "‹ Voltar ao sumário",
            x,
            MARGEM,
            8.5,
            Letra::SansNegrito,
            &cor.tinta,
        );
        p.ir_para(
            Caixa::nova(x - 2., MARGEM - 3., 40., 8.),
            self.plano.primeira_do_sumario(),
        );
        if !self.capa.site.is_empty() {
            let largura_do_site = largura_do_texto(&self.capa.site, 8., Letra::Sans);
            p.texto(
                &self.capa.site,
                PAPEL.0 - MARGEM - largura_do_site,
                MARGEM,
                8.,
                Letra::Sans,
                &cor.apagado,
            );
        }
        PdfPage::new(Mm(PAPEL.0), Mm(PAPEL.1), p.ops)
    }
}

/// O fotolivro inteiro, em PDF.
pub fn gerar(capa: &Capa, fotos: &[FotoDaFolha]) -> Result<Vec<u8>, String> {
    if fotos.is_empty() {
        return Err("nenhuma foto para a prévia".to_string());
    }
    let titulo = if capa.titulo.trim().is_empty() {
        "O seu ensaio".to_string()
    } else {
        capa.titulo.trim().to_string()
    };
    let linha_de_resumo = resumo(fotos);

    let mut documento = PdfDocument::new(&titulo);
    {
        let info = &mut documento.metadata.info;
        info.document_title = titulo.clone();
        info.author = "RecordarFotos".into();
        info.creator = "VintageLightbox".into();
        info.subject = linha_de_resumo.clone();
        info.keywords = vec!["ensaio".into(), "fotolivro".into(), "galeria".into()];
    }

    // Cada foto entra uma vez no arquivo e aparece em várias páginas: o
    // sumário, a página dela, a capa e o fim.
    let imagens: Imagens = fotos
        .iter()
        .map(|foto| {
            let rgb = reduzida(&foto.imagem).to_rgb8();
            let px = (rgb.width(), rgb.height());
            let id = documento.add_image(&RawImage {
                width: px.0 as usize,
                height: px.1 as usize,
                data_format: printpdf::RawImageFormat::RGB8,
                pixels: printpdf::RawImageData::U8(rgb.into_raw()),
                tag: Vec::new(),
            });
            (id, px)
        })
        .collect();
    let aspectos: Vec<f32> = imagens
        .iter()
        .map(|(_, px)| px.0 as f32 / px.1.max(1) as f32)
        .collect();

    let livro = Livro {
        capa,
        fotos,
        imagens,
        plano: Plano::de(&aspectos),
        titulo,
        resumo: linha_de_resumo,
        cor: Paleta::nova(),
    };

    let mut paginas = Vec::with_capacity(livro.plano.total());
    paginas.push(livro.capa());
    for numero in 0..livro.plano.paginas_de_sumario() {
        paginas.push(livro.sumario(numero));
    }
    for (numero, molde) in livro.plano.moldes.iter().enumerate() {
        paginas.push(livro.pagina(molde, numero));
    }
    paginas.push(livro.fim());
    paginas.push(livro.despedida());

    // Os marcadores: a barra lateral do leitor vira o índice.
    documento.add_bookmark("Capa", 1);
    documento.add_bookmark("Neste álbum", livro.plano.primeira_do_sumario());
    for (i, foto) in fotos.iter().enumerate() {
        let estado = if foto.levada { "sua" } else { "disponível" };
        documento.add_bookmark(
            &format!("{:02} · {} — {}", i + 1, foto.nome, estado),
            livro.plano.pagina_da_foto(i),
        );
    }
    documento.add_bookmark("Baixar e comprar", livro.plano.pagina_final());
    documento.add_bookmark("Até a próxima viagem", livro.plano.despedida());

    let opcoes = PdfSaveOptions {
        image_optimization: Some(ImageOptimizationOptions {
            quality: Some(0.86),
            format: Some(ImageCompression::Jpeg),
            max_image_size: None,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut avisos = Vec::new();
    Ok(documento.with_pages(paginas).save(&opcoes, &mut avisos))
}

#[cfg(test)]
mod testes {
    use super::*;

    fn foto(levada: bool, link: Option<&str>) -> FotoDaFolha {
        FotoDaFolha {
            imagem: DynamicImage::ImageRgb8(image::RgbImage::new(60, 40)),
            nome: "IMG_1.jpg".into(),
            levada,
            link: link.map(str::to_string),
        }
    }

    const DEITADA: f32 = 1.5;
    const EM_PE: f32 = 0.67;

    /// 🔑 A diagramação nunca muda a ordem e nunca perde foto: cada foto cai
    /// num molde, uma vez, na ordem do ensaio.
    #[test]
    fn a_diagramacao_guarda_a_ordem_e_todas_as_fotos() {
        let aspectos = [
            DEITADA, EM_PE, EM_PE, DEITADA, DEITADA, EM_PE, DEITADA, DEITADA, EM_PE,
        ];
        let ordem: Vec<usize> = diagramar(&aspectos)
            .iter()
            .flat_map(|m| m.fotos())
            .collect();
        assert_eq!(ordem, (0..aspectos.len()).collect::<Vec<_>>());
    }

    #[test]
    fn os_moldes_seguem_a_orientacao() {
        assert_eq!(diagramar(&[EM_PE, EM_PE]), [Molde::DuplaEmPe(0, 1)]);
        assert_eq!(
            diagramar(&[EM_PE, DEITADA, DEITADA]),
            [Molde::Trio(0, 1, 2)]
        );
        assert_eq!(diagramar(&[EM_PE]), [Molde::RetratoComTexto(0)]);
        // As deitadas alternam entre o destaque sangrado e a dupla.
        assert_eq!(
            diagramar(&[DEITADA, DEITADA, DEITADA]),
            [Molde::Destaque(0), Molde::DuplaDeitada(1, 2, true)]
        );
    }

    /// 🔑 Os links internos dependem disto: capa, sumário, livro, fim.
    #[test]
    fn o_plano_numera_as_paginas() {
        let plano = Plano::de(&[EM_PE, EM_PE, DEITADA]);
        assert_eq!(plano.paginas_de_sumario(), 1);
        assert_eq!(plano.primeira_do_livro(), 3);
        assert_eq!(plano.pagina_da_foto(0), 3);
        assert_eq!(plano.pagina_da_foto(1), 3, "a dupla divide a página");
        assert_eq!(plano.pagina_da_foto(2), 4);
        assert_eq!(plano.pagina_final(), 5);
        assert_eq!(plano.despedida(), 6);
        assert_eq!(plano.total(), 6);

        let grande = Plano::de(&[EM_PE; 19]);
        assert_eq!(grande.paginas_de_sumario(), 2);
        assert_eq!(grande.sumario_da_foto(18), 3);
        assert_eq!(grande.primeira_do_livro(), 4);
    }

    /// As molduras ficam dentro do papel, e as do mesmo molde não se cruzam.
    #[test]
    fn as_molduras_cabem_no_papel() {
        for molde in [
            Molde::DuplaEmPe(0, 1),
            Molde::DuplaDeitada(0, 1, false),
            Molde::DuplaDeitada(1, 2, true),
            Molde::Trio(0, 1, 2),
            Molde::RetratoComTexto(0),
            Molde::Destaque(0),
        ] {
            let caixas = molduras(&molde);
            assert_eq!(caixas.len(), molde.fotos().len());
            for c in &caixas {
                assert!(
                    c.x >= -0.01 && c.x + c.largura <= PAPEL.0 + 0.01,
                    "{molde:?}"
                );
                assert!(
                    c.y >= -0.01 && c.y + c.altura <= PAPEL.1 + 0.01,
                    "{molde:?}"
                );
            }
            for par in caixas.windows(2) {
                let (a, b) = (par[0], par[1]);
                let separadas = a.x + a.largura <= b.x + 0.01
                    || b.x + b.largura <= a.x + 0.01
                    || a.y + a.altura <= b.y + 0.01
                    || b.y + b.altura <= a.y + 0.01;
                assert!(separadas, "molduras sobrepostas em {molde:?}");
            }
        }
    }

    /// 🖼️ A moldura cabe no lugar, guarda a foto inteira e a cantoneira não
    /// alcança a foto.
    #[test]
    fn a_moldura_cabe_e_a_cantoneira_nao_cobre_a_foto() {
        for lugar in [
            Caixa::nova(10., 10., 120., 90.),
            Caixa::nova(0., 0., 30., 30.),
        ] {
            for aspecto in [4. / 3., 3. / 4., 1.] {
                let m = emoldurar(&lugar, aspecto, Alinhar::Centro);
                assert!((m.foto.largura / m.foto.altura - aspecto).abs() < 0.001);
                assert!(m.margem.x >= lugar.x - 0.01);
                assert!(
                    m.margem.y - SOMBRA >= lugar.y - 0.01,
                    "a sombra saiu do lugar"
                );
                assert!(m.margem.x + m.margem.largura + SOMBRA <= lugar.x + lugar.largura + 0.01);
                assert!(m.margem.y + m.margem.altura <= lugar.y + lugar.altura + 0.01);
                // O canto da foto fica a 2 bordas do canto da margem, somando
                // os catetos; a diagonal da cantoneira passa antes.
                assert!(m.cantoneira < 2. * m.borda, "a cantoneira cobriria a foto");
            }
        }
    }

    /// 🚨 **Nenhuma foto é cortada** (dono, 09/10): em todo molde, a 4×3, a
    /// 3×4 e a quadrada entram inteiras — a proporção da foto se mantém e ela
    /// cabe na moldura, qualquer que seja o lado em que encosta.
    #[test]
    fn nenhuma_foto_e_cortada_em_molde_nenhum() {
        for molde in [
            Molde::Destaque(0),
            Molde::DuplaEmPe(0, 1),
            Molde::DuplaDeitada(0, 1, false),
            Molde::DuplaDeitada(0, 1, true),
            Molde::Trio(0, 1, 2),
            Molde::RetratoComTexto(0),
        ] {
            for moldura in molduras(&molde) {
                for aspecto in [4. / 3., 3. / 4., 1.0, 3. / 2., 16. / 9.] {
                    for alinhar in [Alinhar::Centro, Alinhar::Esquerda, Alinhar::Direita] {
                        let f = posicionar(&moldura, aspecto, alinhar);
                        assert!(
                            (f.largura / f.altura - aspecto).abs() < 0.001,
                            "esticou em {molde:?}"
                        );
                        assert!(f.x >= moldura.x - 0.01, "saiu pela esquerda em {molde:?}");
                        assert!(f.y >= moldura.y - 0.01, "saiu por baixo em {molde:?}");
                        assert!(f.x + f.largura <= moldura.x + moldura.largura + 0.01);
                        assert!(f.y + f.altura <= moldura.y + moldura.altura + 0.01);
                    }
                }
            }
        }
    }

    #[test]
    fn o_titulo_quebra_em_linhas() {
        let linhas = quebrar("Ensaio em Gramado — Família Souza", 26., 80., 3);
        assert!((2..=3).contains(&linhas.len()), "{linhas:?}");
    }

    #[test]
    fn o_resumo_conta_as_suas_e_as_disponiveis() {
        let fotos = [foto(true, None), foto(false, None), foto(false, None)];
        assert_eq!(
            resumo(&fotos),
            "3 fotos · 1 sua · 2 disponíveis para compra"
        );
        assert_eq!(resumo(&fotos[1..2]), "1 foto · 1 disponível para compra");
    }

    #[test]
    fn a_acao_entra_na_galeria() {
        assert_eq!(
            com_acao("https://s/g", "baixar-todas"),
            "https://s/g?acao=baixar-todas"
        );
        assert_eq!(
            com_acao("https://s/g?x=1", "comprar-todas"),
            "https://s/g?x=1&acao=comprar-todas"
        );
    }

    /// A capa prefere a levada: ela sai limpa.
    #[test]
    fn a_capa_prefere_a_levada() {
        let fotos = [foto(false, None), foto(true, None)];
        assert_eq!(foto_da_capa(&fotos), 1);
        assert_eq!(foto_da_capa(&fotos[..1]), 0);
    }

    /// ✅ O livro sai com os links de cada foto, a navegação interna, os
    /// marcadores e os dois botões do fim.
    #[test]
    fn gera_o_livro_com_links_navegacao_e_marcadores() {
        let fotos: Vec<_> = (0..7)
            .map(|i| {
                foto(
                    i % 2 == 0,
                    Some(&format!("https://site/meus-ensaios/g?foto={i}")),
                )
            })
            .collect();
        let capa = Capa {
            titulo: "Ensaio da Maria".into(),
            galeria: Some("https://site/meus-ensaios/g".into()),
            site: "site".into(),
            lugar_e_data: "Gramado · 9 de outubro de 2026".into(),
            agendar: Some("https://site/agendar".into()),
        };
        let bytes = gerar(&capa, &fotos).expect("o PDF");
        assert!(bytes.starts_with(b"%PDF"));
        let texto = String::from_utf8_lossy(&bytes);
        for i in 0..7 {
            assert!(
                texto.contains(&format!("https://site/meus-ensaios/g?foto={i}")),
                "faltou o link da foto {i}"
            );
        }
        assert!(texto.contains("https://site/meus-ensaios/g?acao=baixar-todas"));
        assert!(texto.contains("https://site/meus-ensaios/g?acao=comprar-todas"));
        assert!(
            texto.contains("https://site/agendar"),
            "faltou o convite a voltar"
        );
        assert!(texto.contains("/GoTo"), "faltou a navegação interna");
        assert!(texto.contains("/Outlines"), "faltaram os marcadores");
    }

    #[test]
    fn sem_foto_nao_gera() {
        assert!(gerar(&Capa::default(), &[]).is_err());
    }
}
