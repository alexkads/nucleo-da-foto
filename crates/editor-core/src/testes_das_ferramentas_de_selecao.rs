//! ⬚ As opções das ferramentas de seleção na sessão: os quatro modos, o
//! acabamento (difusão e antisserrilhado) que só vale para a próxima forma, a
//! varinha com a fonte separada (camada atual × todas), o laço poligonal num
//! passo e o "Transformar seleção" — e o contrato da etapa 13 de pé: seleção
//! no desfazer, sem pedir para salvar e sem ir para o projeto.

use std::sync::Arc;

use image::RgbImage;

use crate::documento::{BaseRef, Documento};
use crate::historico::Historico;
use crate::retangulo::Retangulo;
use crate::selecao::{Acabamento, Forma, Operacao};
use crate::sessao::{AmostraDaVarinha, OpcoesDaVarinha, Sessao, VarinhaRecusada};
use crate::transformar::Transformacao;
use crate::Ajuste;

/// 600×400, a base é um degradê cinza com um quadrado vermelho em
/// (100..200, 100..200).
fn sessao() -> Sessao {
    let base = Arc::new(RgbImage::from_fn(600, 400, |x, y| {
        if (100..200).contains(&x) && (100..200).contains(&y) {
            image::Rgb([220, 20, 20])
        } else {
            image::Rgb([(x / 3) as u8, (x / 3) as u8, (x / 3) as u8])
        }
    }));
    let doc = Documento::novo(BaseRef::da_imagem(&base));
    let mut s = Sessao::nova(base, doc, Historico::novo(), 300);
    s.pincel.suavizacao = 0.0;
    s
}

fn retangulo(x: u32, y: u32, l: u32, a: u32) -> Forma {
    Forma::Retangulo(Retangulo::novo(x, y, l, a))
}

fn valor(s: &Sessao, x: u32, y: u32) -> u8 {
    s.selecao().map_or(0, |sel| sel.valor(x, y))
}

const SECO: Acabamento = Acabamento {
    suavizar: false,
    difusao: 0,
};

#[test]
fn os_quatro_modos_cada_um_um_passo_sem_pedir_para_salvar() {
    let mut s = sessao();
    s.selecionar_com(&retangulo(0, 0, 200, 200), Operacao::Nova, SECO);
    s.selecionar_com(&retangulo(300, 0, 100, 100), Operacao::Somar, SECO);
    assert_eq!((valor(&s, 10, 10), valor(&s, 350, 50)), (255, 255));
    s.selecionar_com(&retangulo(0, 0, 100, 400), Operacao::Subtrair, SECO);
    assert_eq!((valor(&s, 10, 10), valor(&s, 150, 10)), (0, 255));
    s.selecionar_com(&retangulo(120, 0, 300, 50), Operacao::Intersecao, SECO);
    assert_eq!(
        (valor(&s, 150, 10), valor(&s, 150, 100), valor(&s, 350, 20)),
        (255, 0, 255)
    );
    assert_eq!(s.historico().passos().len(), 4);
    assert!(!s.alterado(), "seleção não pede para salvar");
    // A seleção desfaz passo a passo.
    assert!(s.desfazer());
    assert_eq!(valor(&s, 150, 100), 255);
    // Nova troca tudo.
    s.selecionar_com(&retangulo(500, 300, 10, 10), Operacao::Nova, SECO);
    assert_eq!((valor(&s, 150, 10), valor(&s, 505, 305)), (0, 255));
}

#[test]
fn a_difusao_da_ferramenta_so_vale_para_a_forma_nova() {
    let mut s = sessao();
    s.selecionar_com(&retangulo(0, 0, 100, 100), Operacao::Nova, SECO);
    let dura = Acabamento {
        suavizar: true,
        difusao: 20,
    };
    s.selecionar_com(&retangulo(300, 100, 100, 100), Operacao::Somar, dura);
    // A seleção de antes continua com a borda exata…
    assert_eq!((valor(&s, 99, 50), valor(&s, 100, 50)), (255, 0));
    // …e só a forma nova é difusa.
    let borda = valor(&s, 300, 150);
    assert!((60..200).contains(&borda), "{borda}");
}

#[test]
fn a_varinha_na_camada_atual_nao_ve_a_base_e_todas_ve() {
    let mut s = sessao();
    // A camada 1 (transparente) com um borrão azul em (400..450, 50..100).
    s.pincel.cor = [0, 0, 255];
    s.pincel.dureza = 1.0;
    s.pincel.raio = 30.0;
    s.apertar(425.0, 75.0);
    assert!(s.soltar());
    let camada = OpcoesDaVarinha {
        tolerancia: 32,
        contigua: true,
        suavizar: false,
        amostra: AmostraDaVarinha::CamadaAtual,
    };
    // Clicar no quadrado vermelho da base, na camada atual: ali a camada é
    // transparente — vem todo o transparente, e não o vermelho.
    s.varinha_com(150.0, 150.0, camada, Operacao::Nova).unwrap();
    assert_eq!(valor(&s, 150, 150), 255);
    assert_eq!(
        valor(&s, 10, 10),
        255,
        "o cinza da base também é transparente na camada"
    );
    assert_eq!(valor(&s, 425, 75), 0, "o azul pintado não");
    // Todas as camadas: o vermelho da base, só ele.
    let todas = OpcoesDaVarinha {
        amostra: AmostraDaVarinha::Todas,
        ..camada
    };
    s.varinha_com(150.0, 150.0, todas, Operacao::Nova).unwrap();
    assert_eq!((valor(&s, 150, 150), valor(&s, 10, 10)), (255, 0));
    // Clicar no azul, na camada: o borrão.
    s.varinha_com(425.0, 75.0, camada, Operacao::Nova).unwrap();
    assert_eq!((valor(&s, 425, 75), valor(&s, 150, 150)), (255, 0));
    assert_eq!(s.historico().passos().len(), 4, "o traço e três varinhas");
}

#[test]
fn a_varinha_recusa_camada_escondida_ou_sem_pixels() {
    let mut s = sessao();
    let camada = OpcoesDaVarinha {
        amostra: AmostraDaVarinha::CamadaAtual,
        ..OpcoesDaVarinha::default()
    };
    s.alternar_visibilidade();
    assert_eq!(
        s.varinha_com(10.0, 10.0, camada, Operacao::Nova),
        Err(VarinhaRecusada::CamadaEscondida)
    );
    s.alternar_visibilidade();
    s.nova_camada_de_ajuste(Ajuste::Inverter);
    // A camada de ajuste nasce com a máscara escolhida: lê a máscara (branca).
    assert!(s.na_mascara());
    assert!(s.varinha_com(10.0, 10.0, camada, Operacao::Nova).is_ok());
    assert_eq!(valor(&s, 599, 399), 255);
    assert_eq!(
        s.varinha_com(-1.0, 10.0, camada, Operacao::Nova),
        Err(VarinhaRecusada::ForaDaFoto)
    );
}

#[test]
fn o_laco_poligonal_concluido_e_um_passo() {
    let mut s = sessao();
    s.selecionar_com(&retangulo(0, 0, 50, 50), Operacao::Nova, SECO);
    let antes = s.historico().passos().len();
    s.selecionar_poligono(
        vec![
            (100.0, 100.0),
            (300.0, 100.0),
            (300.0, 300.0),
            (100.0, 300.0),
        ],
        Operacao::Somar,
        Acabamento::default(),
    );
    assert_eq!(s.historico().passos().len(), antes + 1);
    assert_eq!((valor(&s, 10, 10), valor(&s, 200, 200)), (255, 255));
    assert_eq!(
        s.historico()
            .a_desfazer()
            .map(|p| p.descricao(s.documento())),
        Some("Laço poligonal".to_string())
    );
    assert!(s.desfazer());
    assert_eq!((valor(&s, 10, 10), valor(&s, 200, 200)), (255, 0));
}

#[test]
fn transformar_a_selecao_nao_mexe_em_pixel_e_e_um_passo() {
    let mut s = sessao();
    // Algo pintado, para provar que os pixels não andam.
    s.pincel.cor = [0, 255, 0];
    s.apertar(150.0, 150.0);
    assert!(s.soltar());
    let pixels_antes = s.documento().camadas[0].pixels.clone();
    s.selecionar_com(&retangulo(100, 100, 100, 100), Operacao::Nova, SECO);
    let passos = s.historico().passos().len();

    assert!(s.comecar_a_transformar_a_selecao());
    assert!(s.transformando() && s.transformando_a_selecao());
    let (caixa, _) = s.transformacao().unwrap();
    assert_eq!(caixa, crate::transformar::Caixa::nova(100, 100, 100, 100));
    s.definir_transformacao(Transformacao::deslocamento(200.0, 50.0));
    assert_eq!((valor(&s, 350, 200), valor(&s, 150, 150)), (255, 0));
    // Esc: volta a de antes, sem passo.
    s.cancelar_transformacao();
    assert_eq!((valor(&s, 150, 150), valor(&s, 350, 200)), (255, 0));
    assert_eq!(s.historico().passos().len(), passos);

    // De novo, o dobro do tamanho em volta do centro, e Enter.
    assert!(s.comecar_a_transformar_a_selecao());
    s.definir_transformacao(Transformacao {
        escala_x: 2.0,
        escala_y: 2.0,
        ..Transformacao::default()
    });
    assert!(s.aplicar_transformacao());
    assert_eq!((valor(&s, 60, 60), valor(&s, 40, 40)), (255, 0));
    assert_eq!(s.historico().passos().len(), passos + 1);
    assert_eq!(
        s.historico()
            .a_desfazer()
            .map(|p| p.descricao(s.documento())),
        Some("Transformar seleção".to_string())
    );
    assert_eq!(
        s.documento().camadas[0].pixels,
        pixels_antes,
        "nenhum pixel mudou"
    );
    assert!(!s.transformando());
    // O passo de seleção não deixa o projeto pendente além do traço.
    let (_, hist) = s.instantaneo();
    s.salvo(&hist);
    assert!(!s.alterado());
    assert!(s.desfazer());
    assert!(!s.alterado(), "desfazer só a seleção não pede para salvar");
    assert_eq!((valor(&s, 150, 150), valor(&s, 60, 60)), (255, 0));
}

#[test]
fn transformar_a_selecao_sem_selecao_nao_abre() {
    let mut s = sessao();
    assert!(!s.comecar_a_transformar_a_selecao());
    assert!(!s.transformando());
}
