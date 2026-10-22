//! Os casos da bancada: cada caminho que a fase de desempenho mexeu, rodado
//! na tag (`*_antes`) e nesta pasta, com a mesma foto.

use image::{DynamicImage, RgbImage, RgbaImage};

use crate::foto::Foto;
use crate::medir::comparar;

/// O resultado de um caso.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Medida {
    /// A mediana do caminho da tag, em milissegundos.
    pub antes_ms: f64,
    /// A mediana do caminho desta pasta, em milissegundos.
    pub depois_ms: f64,
    /// As duas saídas são iguais byte a byte.
    pub mesma_saida: bool,
}

impl Medida {
    /// Quantas vezes mais rápido (acima de 1) ou mais lento (abaixo de 1).
    pub fn ganho(&self) -> f64 {
        self.antes_ms / self.depois_ms.max(1e-6)
    }
}

/// Um caminho a comparar.
#[derive(Clone, Copy)]
pub struct Caso {
    /// O grupo na tabela: Abrir, Revelar, Exportar, Enquadrar, Editor,
    /// Arrastar, Retoque.
    pub grupo: &'static str,
    /// O que o caso faz, como o usuário diz.
    pub nome: &'static str,
    /// Só faz sentido com um arquivo RAW aberto.
    pub so_raw: bool,
    /// Roda os dois lados e devolve a medida.
    pub rodar: fn(&Foto) -> Medida,
}

/// Os casos que valem para esta foto, na ordem da tabela.
pub fn casos_para(foto: &Foto) -> Vec<Caso> {
    TODOS
        .iter()
        .copied()
        .filter(|c| !c.so_raw || (foto.raw && foto.bytes.is_some()))
        .collect()
}

const TODOS: &[Caso] = &[
    Caso {
        grupo: "Abrir",
        nome: "Revelar o RAW (demosaico, cor, de pé)",
        so_raw: true,
        rodar: abrir_raw,
    },
    Caso {
        grupo: "Abrir",
        nome: "Procurar o XMP do Lightroom no arquivo",
        so_raw: true,
        rodar: procurar_xmp,
    },
    Caso {
        grupo: "Abrir",
        nome: "Adobe RGB → sRGB numa miniatura de 256 px",
        so_raw: false,
        rodar: converter_cor,
    },
    Caso {
        grupo: "Revelar",
        nome: "Guia da Claridade, Textura e Névoa (1º uso numa foto)",
        so_raw: false,
        rodar: guia,
    },
    Caso {
        grupo: "Exportar",
        nome: "JPEG q90 da foto inteira",
        so_raw: false,
        rodar: jpeg,
    },
    Caso {
        grupo: "Exportar",
        nome: "JPEG q90 dos pixels do navegador (wasm)",
        so_raw: false,
        rodar: jpeg_do_navegador,
    },
    Caso {
        grupo: "Enquadrar",
        nome: "Sem corte (cada quadro do slider, a exportação)",
        so_raw: false,
        rodar: enquadrar_sem_corte,
    },
    Caso {
        grupo: "Enquadrar",
        nome: "Recorte reto",
        so_raw: false,
        rodar: enquadrar_reto,
    },
    Caso {
        grupo: "Enquadrar",
        nome: "Girar 90° e recortar",
        so_raw: false,
        rodar: enquadrar_girado,
    },
    Caso {
        grupo: "Enquadrar",
        nome: "Endireitar 3° e recortar",
        so_raw: false,
        rodar: enquadrar_inclinado,
    },
    Caso {
        grupo: "Enquadrar",
        nome: "Endireitar 3° na tela do Enquadrar (foto inteira)",
        so_raw: false,
        rodar: enquadrar_na_tela,
    },
    Caso {
        grupo: "Editor",
        nome: "Arrastar o slider dos Níveis (vista de 1600 px)",
        so_raw: false,
        rodar: vista_rascunho,
    },
    Caso {
        grupo: "Editor",
        nome: "Soltar o slider (vista de 1600 px refeita)",
        so_raw: false,
        rodar: vista_refazer,
    },
    Caso {
        grupo: "Editor",
        nome: "Compor a foto inteira (salvar, varinha mágica)",
        so_raw: false,
        rodar: compor_inteira,
    },
    Caso {
        grupo: "Editor",
        nome: "Mover a camada: 1º evento do arrasto (0, 0)",
        so_raw: false,
        rodar: mover_sem_sair_do_lugar,
    },
    Caso {
        grupo: "Editor",
        nome: "Mover a camada por (13, 7)",
        so_raw: false,
        rodar: mover_um_pouco,
    },
    Caso {
        grupo: "Editor",
        nome: "Começar o Mover com seleção (o conteúdo de uma elipse)",
        so_raw: false,
        rodar: tirar_o_conteudo,
    },
    Caso {
        grupo: "Arrastar",
        nome: "Mover a camada, por evento do ponteiro",
        so_raw: false,
        rodar: arrastar_mover,
    },
    Caso {
        grupo: "Arrastar",
        nome: "⌘T deslocando, por evento do ponteiro",
        so_raw: false,
        rodar: arrastar_transformar,
    },
    Caso {
        grupo: "Arrastar",
        nome: "Mover com seleção, por evento do ponteiro",
        so_raw: false,
        rodar: arrastar_com_selecao,
    },
    Caso {
        grupo: "Arrastar",
        nome: "Mover só o contorno da seleção, por evento",
        so_raw: false,
        rodar: arrastar_o_contorno,
    },
    Caso {
        grupo: "Arrastar",
        nome: "Prévia de filtro, por valor do slider",
        so_raw: false,
        rodar: arrastar_filtro,
    },
    Caso {
        grupo: "Arrastar",
        nome: "Opacidade da camada, por valor do slider",
        so_raw: false,
        rodar: arrastar_opacidade,
    },
    Caso {
        grupo: "Retoque",
        nome: "Content-Aware: buraco de 80 px no meio da foto",
        so_raw: false,
        rodar: content_aware,
    },
];

/// Rodadas por caso: menos quando a foto é grande, para a tabela não demorar.
fn vezes(foto: &Foto, leves: usize) -> usize {
    let mp = foto.rgba.width() as f64 * foto.rgba.height() as f64 / 1e6;
    if mp > 16.0 {
        (leves / 2).max(1)
    } else {
        leves
    }
}

fn bytes_do_arquivo(foto: &Foto) -> &[u8] {
    foto.bytes.as_deref().unwrap_or_default()
}

fn abrir_raw(foto: &Foto) -> Medida {
    let bytes = bytes_do_arquivo(foto);
    let (a, d, sa, sd) = comparar(
        vezes(foto, 3),
        || (),
        |_| raw_codec_antes::decodificar(bytes),
        || (),
        |_| raw_codec::decodificar(bytes),
    );
    let mesma = match (&sa, &sd) {
        (Ok(x), Ok(y)) => x.as_raw() == y.as_raw() && x.dimensions() == y.dimensions(),
        (Err(x), Err(y)) => x == y,
        _ => false,
    };
    Medida {
        antes_ms: a,
        depois_ms: d,
        mesma_saida: mesma,
    }
}

fn procurar_xmp(foto: &Foto) -> Medida {
    let bytes = bytes_do_arquivo(foto);
    let (a, d, sa, sd) = comparar(
        vezes(foto, 7),
        || (),
        |_| raw_codec_antes::xmp(bytes),
        || (),
        |_| raw_codec::xmp(bytes),
    );
    Medida {
        antes_ms: a,
        depois_ms: d,
        mesma_saida: sa == sd,
    }
}

/// O canto de cima à esquerda, até `lado × lado`.
fn recorte(foto: &Foto, lado: u32) -> RgbaImage {
    let (w, h) = foto.rgba.dimensions();
    image::imageops::crop_imm(&foto.rgba, 0, 0, lado.min(w), lado.min(h)).to_image()
}

fn converter_cor(foto: &Foto) -> Medida {
    let miniatura = recorte(foto, 256).into_raw();
    let (a, d, sa, sd) = comparar(
        20,
        || miniatura.clone(),
        |mut p| {
            foto_codec_antes::espaco_de_cor::rgba_para_srgb(
                &mut p,
                foto_codec_antes::espaco_de_cor::EspacoDeCor::AdobeRgb,
            );
            p
        },
        || miniatura.clone(),
        |mut p| {
            foto_codec::espaco_de_cor::rgba_para_srgb(
                &mut p,
                foto_codec::espaco_de_cor::EspacoDeCor::AdobeRgb,
            );
            p
        },
    );
    Medida {
        antes_ms: a,
        depois_ms: d,
        mesma_saida: sa == sd,
    }
}

fn jpeg(foto: &Foto) -> Medida {
    let imagem = DynamicImage::ImageRgba8(foto.rgba.clone());
    let (a, d, sa, sd) = comparar(
        vezes(foto, 3),
        || (),
        |_| foto_codec_antes::codificar(&imagem, 90).ok(),
        || (),
        |_| foto_codec::codificar(&imagem, 90).ok(),
    );
    Medida {
        antes_ms: a,
        depois_ms: d,
        mesma_saida: sa.is_some() && sa == sd,
    }
}

/// O `comprimir_jpeg` do `revelacao-web`: os pixels chegam emprestados do
/// JavaScript e vão direto ao codificador (`codificar_rgba`, desde a 0.1.2).
fn jpeg_do_navegador(foto: &Foto) -> Medida {
    let (w, h) = foto.rgba.dimensions();
    let crus: &[u8] = foto.rgba.as_raw();
    let (a, d, sa, sd) = comparar(
        vezes(foto, 3),
        || (),
        |_| foto_codec_antes::codificar_rgba(crus, w, h, 90).ok(),
        || (),
        |_| foto_codec::codificar_rgba(crus, w, h, 90).ok(),
    );
    Medida {
        antes_ms: a,
        depois_ms: d,
        mesma_saida: sa.is_some() && sa == sd,
    }
}

/// Os pixels de uma imagem, para comparar as saídas — **fora do relógio**: a
/// cópia da conferência não é parte do caminho medido.
fn pixels(imagem: &DynamicImage) -> (u32, u32, Vec<u8>) {
    (
        imagem.width(),
        imagem.height(),
        imagem.to_rgba8().into_raw(),
    )
}

/// O caminho do resultado da GPU: quem chama já é dono da revelada e a toma
/// (`aplicar_dono`, desde a 0.1.2). O clone que dá a cada lado a sua revelada
/// fica fora do relógio.
fn enquadrar_sem_corte(foto: &Foto) -> Medida {
    use revelacao_core::transformacao as depois;
    use revelacao_core_antes::transformacao as antes;
    let revelada = DynamicImage::ImageRgba8(foto.rgba.clone());
    let corte_antes = antes::Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, false, false);
    let corte_depois = depois::Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, false, false);
    let (a, d, sa, sd) = comparar(
        vezes(foto, 9),
        || revelada.clone(),
        |r| antes::aplicar_dono(r, &corte_antes, true),
        || revelada.clone(),
        |r| depois::aplicar_dono(r, &corte_depois, true),
    );
    Medida {
        antes_ms: a,
        depois_ms: d,
        mesma_saida: pixels(&sa) == pixels(&sd),
    }
}

/// Um enquadramento com corte, na tag e aqui: `(x, y, w, h, giro, ângulo)`.
fn enquadrar(foto: &Foto, corte: (f32, f32, f32, f32, i32, f32)) -> Medida {
    enquadrar_com(foto, corte, true)
}

/// [`enquadrar`]; `recortar` falso é a tela do Enquadrar (a foto inteira,
/// girada e endireitada, com o retângulo desenhado por cima).
fn enquadrar_com(
    foto: &Foto,
    (x, y, w, h, giro, angulo): (f32, f32, f32, f32, i32, f32),
    recortar: bool,
) -> Medida {
    use revelacao_core::transformacao as depois;
    use revelacao_core_antes::transformacao as antes;
    let revelada = DynamicImage::ImageRgba8(foto.rgba.clone());
    let corte_antes = antes::Corte::novo(x, y, w, h, giro, angulo, false, false);
    let corte_depois = depois::Corte::novo(x, y, w, h, giro, angulo, false, false);
    let (a, d, sa, sd) = comparar(
        vezes(foto, 5),
        || (),
        |_| antes::aplicar(&revelada, &corte_antes, recortar),
        || (),
        |_| depois::aplicar(&revelada, &corte_depois, recortar),
    );
    Medida {
        antes_ms: a,
        depois_ms: d,
        mesma_saida: pixels(&sa) == pixels(&sd),
    }
}

fn enquadrar_na_tela(foto: &Foto) -> Medida {
    enquadrar_com(foto, (0.1, 0.1, 0.8, 0.8, 0, 3.0), false)
}

fn enquadrar_reto(foto: &Foto) -> Medida {
    enquadrar(foto, (0.1, 0.1, 0.8, 0.8, 0, 0.0))
}

fn enquadrar_girado(foto: &Foto) -> Medida {
    enquadrar(foto, (0.1, 0.1, 0.8, 0.8, 1, 0.0))
}

fn enquadrar_inclinado(foto: &Foto) -> Medida {
    enquadrar(foto, (0.1, 0.1, 0.8, 0.8, 0, 3.0))
}

/// A base RGB do editor, e a mesma foto um pouco mais escura numa camada de
/// pixels opaca por cima — o "pintado" que a composição mescla.
fn base_e_camada(foto: &Foto) -> (RgbImage, RgbImage) {
    let base = DynamicImage::ImageRgba8(foto.rgba.clone()).into_rgb8();
    let mut camada = base.clone();
    for p in camada.pixels_mut() {
        p.0 = p.0.map(|v| (v as u16 * 7 / 8) as u8);
    }
    (base, camada)
}

/// O documento do editor nas duas versões: a camada pintada e uma de Níveis.
macro_rules! documento {
    ($ec:ident, $base:expr, $camada:expr) => {{
        let (w, h) = $base.dimensions();
        let mut doc = $ec::Documento::novo($ec::BaseRef::da_imagem($base));
        let mut pintada = $ec::Camada::nova("Pintada", w, h);
        pintada.pixels = $ec::tiles::CamadaDePixels::da_imagem($camada);
        doc.camadas.push(pintada);
        doc.camadas.push($ec::Camada::de_ajuste(
            "Níveis",
            $ec::Ajuste::Niveis {
                preto: 10.0,
                gama: 1.3,
                branco: 240.0,
            },
            w,
            h,
        ));
        doc
    }};
}

/// A vista de 1600 px, rascunhada (`rascunho`) ou refeita, nas duas versões.
fn vista(foto: &Foto, rascunho: bool) -> Medida {
    let (base, camada) = base_e_camada(foto);
    let doc_antes = documento!(editor_core_antes, &base, &camada);
    let doc_depois = documento!(editor_core, &base, &camada);
    let (w, h) = base.dimensions();
    let mut vista_antes = editor_core_antes::vista::Vista::nova(&base, &doc_antes, 1600);
    let mut vista_depois = editor_core::vista::Vista::nova(&base, &doc_depois, 1600);
    let tudo_antes = editor_core_antes::Retangulo::inteiro(w, h);
    let tudo_depois = editor_core::Retangulo::inteiro(w, h);
    let (a, d, (), ()) = comparar(
        vezes(foto, if rascunho { 7 } else { 3 }),
        || (),
        |_| {
            if rascunho {
                vista_antes.rascunhar(&base, &doc_antes, &tudo_antes);
            } else {
                vista_antes.refazer(&base, &doc_antes, &tudo_antes);
            }
        },
        || (),
        |_| {
            if rascunho {
                vista_depois.rascunhar(&base, &doc_depois, &tudo_depois);
            } else {
                vista_depois.refazer(&base, &doc_depois, &tudo_depois);
            }
        },
    );
    Medida {
        antes_ms: a,
        depois_ms: d,
        mesma_saida: vista_antes.imagem().as_raw() == vista_depois.imagem().as_raw(),
    }
}

fn vista_rascunho(foto: &Foto) -> Medida {
    vista(foto, true)
}

fn vista_refazer(foto: &Foto) -> Medida {
    vista(foto, false)
}

fn compor_inteira(foto: &Foto) -> Medida {
    let (base, camada) = base_e_camada(foto);
    let doc_antes = documento!(editor_core_antes, &base, &camada);
    let doc_depois = documento!(editor_core, &base, &camada);
    let (a, d, sa, sd) = comparar(
        vezes(foto, 3),
        || (),
        |_| editor_core_antes::composicao::compor(&base, &doc_antes).into_raw(),
        || (),
        |_| editor_core::composicao::compor(&base, &doc_depois).into_raw(),
    );
    Medida {
        antes_ms: a,
        depois_ms: d,
        mesma_saida: sa == sd,
    }
}

/// Os tiles de uma camada, para comparar as duas versões.
macro_rules! tiles {
    ($camada:expr) => {
        $camada
            .todos()
            .map(|(p, t)| (*p, t.as_slice().to_vec()))
            .collect::<Vec<_>>()
    };
}

fn mover(foto: &Foto, dx: i64, dy: i64) -> Medida {
    let (_, camada) = base_e_camada(foto);
    let antes = editor_core_antes::tiles::CamadaDePixels::da_imagem(&camada);
    let depois = editor_core::tiles::CamadaDePixels::da_imagem(&camada);
    let (a, d, sa, sd) = comparar(
        vezes(foto, 7),
        || (),
        |_| editor_core_antes::operacoes::deslocada(&antes, dx, dy),
        || (),
        |_| editor_core::operacoes::deslocada(&depois, dx, dy),
    );
    Medida {
        antes_ms: a,
        depois_ms: d,
        mesma_saida: tiles!(sa) == tiles!(sd),
    }
}

fn mover_sem_sair_do_lugar(foto: &Foto) -> Medida {
    mover(foto, 0, 0)
}

fn mover_um_pouco(foto: &Foto) -> Medida {
    mover(foto, 13, 7)
}

/// Um buraco de 80 px no meio de um pedaço de 400 × 300 do centro da foto.
fn content_aware(foto: &Foto) -> Medida {
    let (fw, fh) = foto.rgba.dimensions();
    let (w, h) = (400.min(fw), 300.min(fh));
    let pedaco = image::imageops::crop_imm(&foto.rgba, (fw - w) / 2, (fh - h) / 2, w, h).to_image();
    let lado = 80.min(w / 3).min(h / 3).max(1);
    let (x0, y0) = ((w - lado) / 2, (h - lado) / 2);
    let caixa = (x0, y0, x0 + lado, y0 + lado);
    let no_buraco = |x: f32, y: f32| {
        (x0 as f32..(x0 + lado) as f32).contains(&x) && (y0 as f32..(y0 + lado) as f32).contains(&y)
    };
    let crus = pedaco.as_raw();
    let (a, d, sa, sd) = comparar(
        vezes(foto, 3),
        || (),
        |_| {
            revelacao_core_antes::preenchimento::preencher_buraco(crus, w, h, caixa, &no_buraco, 7)
                .map(|r| (r.x0, r.y0, r.largura, r.altura, r.rgba))
        },
        || (),
        |_| {
            revelacao_core::preenchimento::preencher_buraco(crus, w, h, caixa, &no_buraco, 7)
                .map(|r| (r.x0, r.y0, r.largura, r.altura, r.rgba))
        },
    );
    Medida {
        antes_ms: a,
        depois_ms: d,
        mesma_saida: sa == sd,
    }
}

/// A vista do app: o lado maior que a janela mostra.
const LADO_DA_VISTA: u32 = 1600;

/// Uma sessão do editor nas duas versões: a foto como base e uma camada
/// pintada por cima de tudo, a 60% — o que ela faz aparece na foto inteira.
macro_rules! sessao {
    ($ec:ident, $foto:expr) => {{
        let base = std::sync::Arc::new(DynamicImage::ImageRgba8($foto.rgba.clone()).into_rgb8());
        let (w, h) = base.dimensions();
        let doc = $ec::Documento::novo($ec::BaseRef::da_imagem(&base));
        let mut s = $ec::sessao::Sessao::nova(base, doc, $ec::Historico::novo(), LADO_DA_VISTA);
        s.nova_camada();
        s.selecionar(
            &$ec::selecao::Forma::Retangulo($ec::Retangulo::inteiro(w, h)),
            $ec::selecao::Operacao::Nova,
        );
        s.pincel.cor = [200, 40, 90];
        s.preencher_selecao();
        s.desmarcar();
        s.mover_opacidade(0.6);
        s.confirmar_opacidade();
        s
    }};
}

/// Um gesto de arrasto nas duas versões: `comecar` abre o gesto, `evento`
/// é um passo do arrasto (o `k`-ésimo), `fim` fecha. Dentro deles, `ec` é o
/// crate do editor da versão daquele lado. O tempo é o de **um
/// evento**; a foto composta e a vista depois do fim têm de ser as mesmas —
/// no meio do arrasto a vista é rascunho de propósito, e não entra na conta.
macro_rules! arrasto {
    ($foto:expr, $vezes:expr, |$s:ident| $comecar:expr, |$s2:ident, $k:ident| $evento:expr, |$s3:ident| $fim:expr) => {{
        let mut antes = sessao!(editor_core_antes, $foto);
        let mut depois = sessao!(editor_core, $foto);
        {
            #[allow(unused_imports)] // só os começos que nomeiam um tipo usam
            use editor_core_antes as ec;
            let $s = &mut antes;
            $comecar;
        }
        {
            #[allow(unused_imports)] // só os começos que nomeiam um tipo usam
            use editor_core as ec;
            let $s = &mut depois;
            $comecar;
        }
        let (mut ka, mut kd) = (0usize, 0usize);
        let (a, d, (), ()) = comparar(
            $vezes,
            || (),
            |_| {
                #[allow(unused_imports)] // só os eventos que nomeiam um tipo usam
                use editor_core_antes as ec;
                ka += 1;
                let ($s2, $k) = (&mut antes, ka);
                $evento;
            },
            || (),
            |_| {
                #[allow(unused_imports)] // só os eventos que nomeiam um tipo usam
                use editor_core as ec;
                kd += 1;
                let ($s2, $k) = (&mut depois, kd);
                $evento;
            },
        );
        {
            let $s3 = &mut antes;
            $fim;
        }
        {
            let $s3 = &mut depois;
            $fim;
        }
        Medida {
            antes_ms: a,
            depois_ms: d,
            mesma_saida: antes.vista().imagem().as_raw() == depois.vista().imagem().as_raw()
                && antes.compor().as_raw() == depois.compor().as_raw()
                && selecao!(antes) == selecao!(depois),
        }
    }};
}

/// A seleção da sessão, tile a tile (a máscara, ou o valor do tile que não
/// existe), para comparar as duas versões.
macro_rules! selecao {
    ($sessao:expr) => {
        $sessao.selecao().map(|s| {
            let lado = editor_core::tiles::LADO_DO_TILE;
            (0..s.altura().div_ceil(lado) as i32)
                .flat_map(|ty| (0..s.largura().div_ceil(lado) as i32).map(move |tx| (tx, ty)))
                .map(|p| s.do_tile(p).map(<[u8]>::to_vec))
                .collect::<Vec<_>>()
        })
    };
}

/// Uma elipse que pega quase a foto toda — a seleção dos casos que andam
/// com ela.
macro_rules! elipse {
    ($ec:ident, $w:expr, $h:expr) => {
        $ec::selecao::Forma::Elipse($ec::Retangulo::novo(
            $w / 20,
            $h / 20,
            $w * 9 / 10,
            $h * 9 / 10,
        ))
    };
}

fn arrastar_mover(foto: &Foto) -> Medida {
    arrasto!(
        foto,
        vezes(foto, 7),
        |s| s.comecar_a_mover(),
        |s, k| s.mover_por(5 * k as i64, 3 * k as i64),
        |s| s.terminar_de_mover()
    )
}

fn arrastar_transformar(foto: &Foto) -> Medida {
    arrasto!(
        foto,
        vezes(foto, 5),
        |s| s.comecar_a_transformar(),
        |s, k| s.definir_transformacao(ec::transformar::Transformacao::deslocamento(
            4.0 * k as f32,
            2.0 * k as f32
        )),
        |s| s.aplicar_transformacao()
    )
}

fn arrastar_com_selecao(foto: &Foto) -> Medida {
    let (w, h) = foto.rgba.dimensions();
    arrasto!(
        foto,
        vezes(foto, 5),
        |s| {
            s.selecionar(&elipse!(ec, w, h), ec::selecao::Operacao::Nova);
            s.comecar_a_mover()
        },
        |s, k| s.mover_por(5 * k as i64, 3 * k as i64),
        |s| s.terminar_de_mover()
    )
}

fn arrastar_o_contorno(foto: &Foto) -> Medida {
    let (w, h) = foto.rgba.dimensions();
    arrasto!(
        foto,
        vezes(foto, 7),
        |s| {
            s.selecionar(&elipse!(ec, w, h), ec::selecao::Operacao::Nova);
            s.comecar_a_mover_o_contorno()
        },
        |s, k| s.mover_o_contorno_por(5 * k as i64, 3 * k as i64),
        |s| s.terminar_de_mover_o_contorno()
    )
}

/// O começo do ⌘T e do Mover com seleção: o conteúdo da camada vezes a
/// seleção, tirado dela.
fn tirar_o_conteudo(foto: &Foto) -> Medida {
    let (_, camada) = base_e_camada(foto);
    let (w, h) = camada.dimensions();
    let camada_antes = editor_core_antes::tiles::CamadaDePixels::da_imagem(&camada);
    let camada_depois = editor_core::tiles::CamadaDePixels::da_imagem(&camada);
    let sel_antes =
        editor_core_antes::selecao::Selecao::da_forma(w, h, &elipse!(editor_core_antes, w, h));
    let sel_depois = editor_core::selecao::Selecao::da_forma(w, h, &elipse!(editor_core, w, h));
    let (a, d, sa, sd) = comparar(
        vezes(foto, 5),
        || (),
        |_| {
            editor_core_antes::transformar::Conteudo::da_camada(&camada_antes, Some(&sel_antes))
                .map(|c| ((c.caixa.x, c.caixa.y), c.imagem().into_raw()))
        },
        || (),
        |_| {
            editor_core::transformar::Conteudo::da_camada(&camada_depois, Some(&sel_depois))
                .map(|c| ((c.caixa.x, c.caixa.y), c.imagem().into_raw()))
        },
    );
    Medida {
        antes_ms: a,
        depois_ms: d,
        mesma_saida: sa == sd,
    }
}

/// A guia que a Claridade, a Textura e o Remover névoa leem: calculada uma
/// vez por foto, no primeiro uso de um dos três (e na exportação).
fn guia(foto: &Foto) -> Medida {
    let (w, h) = foto.rgba.dimensions();
    let pixels = foto.rgba.as_raw();
    let (a, d, sa, sd) = comparar(
        vezes(foto, 5),
        || (),
        |_| revelacao_core_antes::guia::calcular(pixels, w, h),
        || (),
        |_| revelacao_core::guia::calcular(pixels, w, h),
    );
    let bits = |t: &[f32]| t.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
    Medida {
        antes_ms: a,
        depois_ms: d,
        mesma_saida: (sa.largura, sa.altura, sa.luz_do_ceu.to_bits())
            == (sd.largura, sd.altura, sd.luz_do_ceu.to_bits())
            && bits(&sa.texels) == bits(&sd.texels),
    }
}

fn arrastar_filtro(foto: &Foto) -> Medida {
    // A prévia que o filtro calculou fora da thread da tela: a camada com
    // outra cor a cada valor do slider. Montá-la entra no relógio dos dois
    // lados, com a mesma conta — pesa igual nos dois.
    let (w, h) = foto.rgba.dimensions();
    let previa = |k: usize| {
        RgbImage::from_fn(w, h, move |x, y| {
            image::Rgb([(x as usize + k * 9) as u8, (y as usize + k * 5) as u8, 60])
        })
    };
    arrasto!(
        foto,
        vezes(foto, 5),
        |s| s.comecar_filtro(),
        |s, k| s.mostrar_filtro(ec::tiles::CamadaDePixels::da_imagem(&previa(k))),
        |s| s.aplicar_filtro("Prévia")
    )
}

fn arrastar_opacidade(foto: &Foto) -> Medida {
    arrasto!(
        foto,
        vezes(foto, 7),
        // Não há o que abrir: o slider começa no primeiro valor.
        |s| s.soltar(),
        |s, k| s.mover_opacidade(0.2 + (k % 7) as f32 / 10.0),
        |s| s.confirmar_opacidade()
    )
}

#[cfg(test)]
mod testes {
    use super::*;

    /// 🚨 **A regra da bancada é a regra da casa: a foto não muda.** Numa foto
    /// pequena (para o teste ser rápido), todo caso que não pede RAW tem de
    /// sair igual na tag e aqui.
    #[test]
    fn todo_caso_sai_igual_na_tag_e_aqui() {
        let foto = Foto::de_exemplo(320, 240);
        let casos = casos_para(&foto);
        assert!(!casos.is_empty());
        for caso in casos {
            let m = (caso.rodar)(&foto);
            assert!(
                m.mesma_saida,
                "{} › {}: a saída mudou",
                caso.grupo, caso.nome
            );
            assert!(m.antes_ms >= 0.0 && m.depois_ms >= 0.0);
        }
    }

    #[test]
    fn os_casos_de_raw_so_aparecem_com_um_raw_aberto() {
        let exemplo = Foto::de_exemplo(8, 8);
        assert!(casos_para(&exemplo).iter().all(|c| !c.so_raw));
        let raw = Foto {
            raw: true,
            bytes: Some(vec![0; 16]),
            ..Foto::de_exemplo(8, 8)
        };
        assert!(casos_para(&raw).iter().any(|c| c.so_raw));
        assert_eq!(casos_para(&raw).len(), TODOS.len());
    }

    #[test]
    fn o_ganho_e_antes_sobre_depois() {
        let m = Medida {
            antes_ms: 30.0,
            depois_ms: 10.0,
            mesma_saida: true,
        };
        assert_eq!(m.ganho(), 3.0);
    }
}
