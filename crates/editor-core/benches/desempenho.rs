//! Quanto custam os caminhos quentes do editor — `cargo bench -p editor-core`.
//!
//! Imprime a mediana de cada caminho, para comparar antes e depois de uma
//! mudança na mesma máquina. Não é teste: nada aqui falha.

#![allow(clippy::print_stdout)]

use std::hint::black_box;

use editor_core::carimbo::Fonte;
use editor_core::composicao;
use editor_core::deformar::{self, Malha};
use editor_core::misturador::{OpcoesDoMisturador, Tinta};
use editor_core::operacoes;
use editor_core::pincel::{Ferramenta, Traco};
use editor_core::selecao::{Forma, Molde, Selecao};
use editor_core::tiles::CamadaDePixels;
use editor_core::transformar::{self, Conteudo, Transformacao};
use editor_core::vista::Vista;
use editor_core::{Ajuste, BaseRef, Camada, Documento, Mascara, Pincel, Retangulo};
use image::RgbImage;

/// Roda `f` uma vez para aquecer, depois `vezes` vezes, e imprime a mediana.
///
/// Sem dependência de propósito (o `criterion` traria dezenas de crates ao
/// `cargo deny`): a mediana de poucas rodadas basta para comparar antes e
/// depois de uma mudança na mesma máquina.
fn medir(nome: &str, vezes: usize, mut f: impl FnMut()) {
    f();
    let mut tempos: Vec<std::time::Duration> = (0..vezes)
        .map(|_| {
            let comeco = std::time::Instant::now();
            f();
            comeco.elapsed()
        })
        .collect();
    tempos.sort();
    let mediana = tempos[tempos.len() / 2].as_secs_f64() * 1000.0;
    println!("{nome:<56} {mediana:>9.2} ms  (mediana de {vezes})");
}

fn main() {
    // Uma foto de 12 MP com uma camada de pixels cheia e opaca e uma de
    // Níveis por cima — o arrasto do slider dos Níveis.
    let (w, h) = (4000u32, 3000u32);
    let base = RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x * 7 + y) as u8, ((y * 3) ^ x) as u8, 90])
    });
    let mut doc = Documento::novo(BaseRef::da_imagem(&base));
    let mut pintada = Camada::nova("Pintada", w, h);
    pintada.pixels = CamadaDePixels::da_imagem(&RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x / 3) as u8, (y / 5) as u8, 40])
    }));
    doc.camadas.push(pintada);
    doc.camadas.push(Camada::de_ajuste(
        "Níveis",
        Ajuste::Niveis {
            preto: 10.0,
            gama: 1.3,
            branco: 240.0,
        },
        w,
        h,
    ));
    let tudo = Retangulo::inteiro(w, h);

    let mut vista = Vista::nova(&base, &doc, 1600);
    medir(
        "vista 1600 px de 12 MP: rascunho (arrasto de slider)",
        10,
        || {
            vista.rascunhar(&base, &doc, &tudo);
        },
    );
    medir("vista 1600 px de 12 MP: refazer", 3, || {
        vista.refazer(&base, &doc, &tudo);
    });
    medir("compor 12 MP inteira (salvar, varinha)", 3, || {
        black_box(composicao::compor(&base, &doc));
    });
    // A camada pintada a 70 %: cada pixel passa pela mescla, e não pelo
    // atalho do Normal opaco.
    let mut a_70 = doc.clone();
    a_70.camadas[1].opacidade = 0.7;
    medir("compor 12 MP inteira com a camada a 70 %", 3, || {
        black_box(composicao::compor(&base, &a_70));
    });
    // Uma máscara pintada com difusão de 20 px: cada pixel lê o mapa
    // desfocado (a bilinear da difusão); o mapa é feito no aquecimento.
    let mut difusa = doc.clone();
    let mut mascara = Mascara::nova(255, w, h);
    mascara.pixels = CamadaDePixels::da_imagem(&RgbImage::from_fn(w, h, |x, y| {
        let v = if (x / 500 + y / 400) % 2 == 0 { 255 } else { 0 };
        image::Rgb([v, v, v])
    }));
    mascara.difusao = 20.0;
    difusa.camadas[1].mascara = Some(mascara);
    medir("compor 12 MP inteira com uma máscara difusa", 3, || {
        black_box(composicao::compor(&base, &difusa));
    });
    // O pincel: um traço de um lado a outro da foto, com um evento do
    // ponteiro a cada 8 px, na camada cheia — a ponta padrão (raio 40) e uma
    // grande e macia (raio 200, dureza 0, a de pintar máscara).
    for (nome, pincel) in [
        (
            "pincel: traço de 3600 px, raio 40 (o padrão)",
            Pincel::default(),
        ),
        (
            "pincel: traço de 3600 px, raio 200 e dureza 0",
            Pincel {
                raio: 200.0,
                dureza: 0.0,
                ..Pincel::default()
            },
        ),
    ] {
        let mut pintada = doc.camadas[1].pixels.clone();
        medir(nome, 3, || {
            let mut traco = Traco::novo(pincel);
            let mut x = 200.0f32;
            while x < 3800.0 {
                traco.ate(&mut pintada, x, 1500.0 + (x * 0.01).sin() * 300.0);
                x += 8.0;
            }
            black_box(traco.terminar(&mut pintada));
        });
    }
    // O Carimbo (copiando 300 px para a esquerda e 200 para cima, da foto
    // composta até a camada) e o Misturador, com a ponta de raio 100.
    let base_compartilhada = std::sync::Arc::new(base.clone());
    let traco_de = |pincel: Pincel| {
        let mut traco = Traco::novo(pincel);
        match pincel.ferramenta {
            Ferramenta::Carimbo => {
                traco = traco.copiando_de(Fonte::nova(
                    base_compartilhada.clone(),
                    &doc,
                    1,
                    (-300.0, -200.0),
                ));
            }
            Ferramenta::Misturador => {
                let mut tinta = Tinta::default();
                tinta.carregar([200, 40, 90]);
                traco = traco.misturando(tinta, OpcoesDoMisturador::default());
            }
            _ => {}
        }
        traco
    };
    for (nome, ferramenta) in [
        ("carimbo: traço de 3600 px, raio 100", Ferramenta::Carimbo),
        (
            "misturador: traço de 3600 px, raio 100",
            Ferramenta::Misturador,
        ),
    ] {
        let pincel = Pincel {
            ferramenta,
            raio: 100.0,
            ..Pincel::default()
        };
        let mut pintada = doc.camadas[1].pixels.clone();
        medir(nome, 3, || {
            let mut traco = traco_de(pincel);
            let mut x = 400.0f32;
            while x < 3800.0 {
                traco.ate(&mut pintada, x, 1500.0 + (x * 0.01).sin() * 300.0);
                x += 8.0;
            }
            black_box(traco.terminar(&mut pintada));
        });
    }
    let camada = &doc.camadas[1].pixels;
    medir("mover: deslocar a camada de 12 MP por (0, 0)", 5, || {
        black_box(operacoes::deslocada(camada, 0, 0));
    });
    medir("mover: deslocar a camada de 12 MP por (13, 7)", 5, || {
        black_box(operacoes::deslocada(camada, 13, 7));
    });

    // ⌘T: o conteúdo da camada inteira, tirado no começo do gesto e
    // desenhado a cada evento do arrasto.
    let elipse = Selecao::da_forma(w, h, &Forma::Elipse(Retangulo::novo(200, 150, 3600, 2700)));
    medir("⌘T: começar (o conteúdo da camada de 12 MP)", 3, || {
        black_box(Conteudo::da_camada(camada, None));
    });
    medir(
        "mover com seleção: começar (a elipse de 7,6 MP)",
        3,
        || {
            black_box(Conteudo::da_camada(camada, Some(&elipse)));
        },
    );
    let conteudo = Conteudo::da_camada(camada, None).expect("a camada tem pixels");
    let girando = Transformacao {
        dx: 13.0,
        dy: 7.0,
        escala_x: 1.05,
        escala_y: 1.05,
        angulo: 0.035,
    };
    medir(
        "⌘T: desenhar 12 MP com escala 1,05 e giro de 2°",
        3,
        || {
            black_box(transformar::desenhar(&conteudo, &girando, w, h));
        },
    );
    // O Deformar: a malha com os pontos de baixo levantados (o retoque do
    // queixo), cada evento do arrasto.
    let mut malha = Malha::da_caixa(&conteudo.caixa);
    malha.mover_ponto(2, 1, 0.0, -60.0);
    malha.mover_ponto(2, 2, 0.0, -60.0);
    malha.mover_ponto(3, 1, 0.0, -25.0);
    malha.mover_ponto(3, 2, 0.0, -25.0);
    medir("Deformar: desenhar 12 MP com a malha puxada", 3, || {
        black_box(deformar::desenhar(&conteudo, &malha, w, h));
    });
    let quebrado = Transformacao::deslocamento(13.5, 7.25);
    medir("⌘T: desenhar 12 MP deslocando (13,5, 7,25)", 3, || {
        black_box(transformar::desenhar(&conteudo, &quebrado, w, h));
    });
    // O Mover com seleção e o colar: deslocamento inteiro.
    let andando = Transformacao::deslocamento(13.0, 7.0);
    medir("⌘T: desenhar 12 MP deslocando (13, 7)", 3, || {
        black_box(transformar::desenhar(&conteudo, &andando, w, h));
    });
    let desenhado = transformar::desenhar(&conteudo, &andando, w, h);
    medir(
        "⌘T com seleção: o desenhado sobre o fundo (12 MP)",
        3,
        || {
            black_box(transformar::sobre(camada, &desenhado));
        },
    );

    // O Mover com a seleção: a seleção anda junto a cada evento.
    medir(
        "mover a seleção (elipse de 7,6 MP) por (13, 7)",
        5,
        || {
            black_box(elipse.deslocada(13, 7));
        },
    );

    // Transformar seleção: o molde tirado no começo, amostrado a cada evento.
    medir(
        "transformar a seleção: começar (elipse de 7,6 MP)",
        3,
        || {
            black_box(Molde::de(&elipse));
        },
    );
    let molde = Molde::de(&elipse);
    let caixa: transformar::Caixa = molde.caixa().into();
    let c = girando.inversa(&caixa, 0.0, 0.0);
    let (ex, ey) = (
        girando.inversa(&caixa, 1.0, 0.0),
        girando.inversa(&caixa, 0.0, 1.0),
    );
    medir(
        "transformar a seleção (elipse de 7,6 MP), escala e giro",
        3,
        || {
            black_box(molde.transformado(
                &tudo,
                c,
                (ex.0 - c.0, ex.1 - c.1),
                (ey.0 - c.0, ey.1 - c.1),
            ));
        },
    );
}
