//! ✒️ A Caneta na sessão: um passo por gesto, desfazer e refazer, o caminho
//! de trabalho e os nomeados, a gravação, "Fazer seleção" e a máscara
//! vetorial — com a foto e o projeto de verdade.

use std::sync::Arc;

use image::RgbImage;

use crate::composicao;
use crate::documento::{BaseRef, Documento};
use crate::historico::Historico;
use crate::projeto::{DiscoReal, Projeto};
use crate::retangulo::Retangulo;
use crate::selecao::{Forma, Operacao};
use crate::sessao::{OpcoesDaSelecaoDoCaminho, Sessao};
use crate::vetor::caneta::{FerramentaVetorial, Medida, Modificadores};
use crate::vetor::{LugarDoCaminho, OperacaoDoComponente, Ponto};

const M: Medida = Medida { por_ponto: 1.0 };
const NADA: Modificadores = Modificadores {
    shift: false,
    alt: false,
    comando: false,
    duplo: false,
};

fn base() -> Arc<RgbImage> {
    Arc::new(RgbImage::from_fn(600, 400, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 200) as u8])
    }))
}

fn sessao() -> Sessao {
    let base = base();
    let doc = Documento::novo(BaseRef::da_imagem(&base));
    Sessao::nova(base, doc, Historico::novo(), 300)
}

fn p(x: f64, y: f64) -> Ponto {
    Ponto::novo(x, y)
}

fn clicar(s: &mut Sessao, q: Ponto) {
    s.caneta_apertar(q, NADA, M);
    s.caneta_soltar(M);
}

fn arrastar(s: &mut Sessao, de: Ponto, ate: Ponto) {
    s.caneta_apertar(de, NADA, M);
    for k in 1..=4 {
        let t = k as f64 / 4.0;
        s.caneta_arrastar(
            p(de.x + (ate.x - de.x) * t, de.y + (ate.y - de.y) * t),
            NADA,
            M,
        );
    }
    s.caneta_soltar(M);
}

/// Um quadrado fechado de (100, 100) a (300, 300) no caminho de trabalho.
fn quadrado(s: &mut Sessao) {
    for q in [
        p(100.0, 100.0),
        p(300.0, 100.0),
        p(300.0, 300.0),
        p(100.0, 300.0),
    ] {
        clicar(s, q);
    }
    clicar(s, p(100.0, 100.0));
}

fn nome_do_ultimo(s: &Sessao) -> String {
    s.historico().a_desfazer().unwrap().descricao(s.documento())
}

#[test]
fn cada_ancora_e_um_passo_e_o_desfazer_volta_por_gesto() {
    let mut s = sessao();
    clicar(&mut s, p(100.0, 100.0));
    assert_eq!(s.alvo_vetorial(), Some(LugarDoCaminho::Trabalho));
    assert_eq!(s.historico().posicao(), 1);
    clicar(&mut s, p(300.0, 100.0));
    arrastar(&mut s, p(300.0, 300.0), p(340.0, 300.0));
    assert_eq!(
        s.historico().posicao(),
        3,
        "um passo por âncora, não por evento"
    );
    assert_eq!(nome_do_ultimo(&s), "Ponto de ancoragem");
    let tres = s.documento().caminhos.trabalho.clone().unwrap();
    assert_eq!(tres.subcaminhos[0].ancoras.len(), 3);

    assert!(s.desfazer());
    let dois = s.documento().caminhos.trabalho.clone().unwrap();
    assert_eq!(dois.subcaminhos[0].ancoras.len(), 2);
    assert!(s.refazer());
    assert_eq!(s.documento().caminhos.trabalho.as_ref(), Some(&tres));
    // O desenho continua de onde parou depois do refazer? A Caneta foi
    // conferida: o subcaminho ainda está aberto, então retoma pela ponta.
    assert!(s.historico().alterado(), "caminho é alteração do projeto");
    // Desfazer tudo: nem caminho de trabalho.
    while s.desfazer() {}
    assert!(s.documento().caminhos.trabalho.is_none());
}

#[test]
fn esc_no_meio_do_arrasto_nao_deixa_passo() {
    let mut s = sessao();
    clicar(&mut s, p(100.0, 100.0));
    let antes = s.documento().clone();
    let posicao = s.historico().posicao();
    s.caneta_apertar(p(300.0, 100.0), NADA, M);
    s.caneta_arrastar(p(330.0, 140.0), NADA, M);
    assert_ne!(s.documento(), &antes, "a prévia está no documento");
    assert!(s.caneta_esc());
    assert_eq!(s.documento(), &antes);
    assert_eq!(s.historico().posicao(), posicao);
    assert!(!s.caneta_soltar(M));
}

#[test]
fn salvar_o_de_trabalho_e_um_novo_desenho_nao_apaga_o_nomeado() {
    let mut s = sessao();
    quadrado(&mut s);
    assert_eq!(nome_do_ultimo(&s), "Fechar caminho");
    let lugar = s.salvar_caminho_de_trabalho("Contorno").unwrap();
    assert_eq!(s.documento().caminhos.trabalho, None);
    assert_eq!(s.documento().caminhos.nomeados.len(), 1);
    assert_eq!(s.documento().caminhos.nomeados[0].nome, "Contorno");
    assert_eq!(s.alvo_vetorial(), Some(lugar));
    // Ocultar (nenhum escolhido) e desenhar: nasce um de trabalho; o nomeado fica.
    s.escolher_caminho(None);
    clicar(&mut s, p(10.0, 10.0));
    assert!(s.documento().caminhos.trabalho.is_some());
    assert_eq!(s.documento().caminhos.nomeados.len(), 1);
    // De novo sem escolher: o de trabalho é trocado, e o desfazer o devolve.
    let velho = s.documento().caminhos.trabalho.clone();
    s.caneta.encerrar();
    s.escolher_caminho(None);
    clicar(&mut s, p(50.0, 50.0));
    assert_ne!(s.documento().caminhos.trabalho, velho);
    s.desfazer();
    assert_eq!(s.documento().caminhos.trabalho, velho);
    assert_eq!(s.documento().caminhos.nomeados.len(), 1);
    // Renomear, duplicar e excluir o nomeado — cada um um passo.
    assert!(s.renomear_caminho(lugar, "Silhueta"));
    let copia = s.duplicar_caminho(lugar).unwrap();
    assert_eq!(s.documento().caminho(copia).unwrap().nome, "Silhueta cópia");
    assert!(s.excluir_caminho(copia));
    assert!(s.documento().caminho(copia).is_none());
    s.desfazer();
    assert!(s.documento().caminho(copia).is_some());
}

#[test]
fn fazer_selecao_com_difusao_operacoes_e_independente_do_caminho() {
    let mut s = sessao();
    quadrado(&mut s);
    assert!(s.fazer_selecao_do_caminho(OpcoesDaSelecaoDoCaminho::default()));
    assert_eq!(nome_do_ultimo(&s), "Fazer seleção");
    let sel = s.selecao().unwrap();
    assert_eq!(sel.valor(150, 150), 255);
    assert_eq!(sel.valor(99, 150), 0);
    assert_eq!(sel.caixa_justa(), Retangulo::novo(100, 100, 200, 200));

    // Editar o caminho depois não mexe na seleção feita.
    let antes = s.selecao().cloned();
    s.usar_ferramenta_vetorial(FerramentaVetorial::SelecaoDireta);
    s.caneta_apertar(p(300.0, 300.0), NADA, M);
    s.caneta_arrastar(p(400.0, 380.0), NADA, M);
    s.caneta_soltar(M);
    assert_eq!(nome_do_ultimo(&s), "Mover pontos");
    assert_eq!(s.selecao().cloned(), antes);

    // Difusão: a borda vira rampa.
    s.desmarcar();
    let mut o = OpcoesDaSelecaoDoCaminho {
        difusao: 8,
        ..Default::default()
    };
    assert!(s.fazer_selecao_do_caminho(o));
    let sel = s.selecao().unwrap();
    let v = sel.valor(100, 160);
    assert!(v > 40 && v < 215, "rampa na borda: {v}");

    // Subtrair de uma seleção grande.
    s.selecionar(
        &Forma::Retangulo(Retangulo::inteiro(600, 400)),
        Operacao::Nova,
    );
    o.difusao = 0;
    o.operacao = Operacao::Subtrair;
    assert!(s.fazer_selecao_do_caminho(o));
    let sel = s.selecao().unwrap();
    assert_eq!(sel.valor(150, 150), 0);
    assert_eq!(sel.valor(20, 20), 255);
}

#[test]
fn componentes_com_furo_viram_selecao_vazada() {
    let mut s = sessao();
    quadrado(&mut s);
    // Um segundo componente, menor, que subtrai.
    s.caneta.opcoes.operacao = OperacaoDoComponente::Subtrair;
    for q in [
        p(150.0, 150.0),
        p(250.0, 150.0),
        p(250.0, 250.0),
        p(150.0, 250.0),
    ] {
        clicar(&mut s, q);
    }
    clicar(&mut s, p(150.0, 150.0));
    assert_eq!(s.caminho_alvo().unwrap().subcaminhos.len(), 2);
    assert!(s.fazer_selecao_do_caminho(OpcoesDaSelecaoDoCaminho::default()));
    let sel = s.selecao().unwrap();
    assert_eq!(sel.valor(120, 120), 255);
    assert_eq!(sel.valor(200, 200), 0, "o furo");
    // Trocar a operação do furo para somar fecha o furo (um passo).
    assert!(s.editar_caminho_alvo("Combinar formas", |c| {
        let id = c.subcaminhos[1].id;
        crate::vetor::edicao::definir_operacao(c, &[id], OperacaoDoComponente::Somar)
    }));
    s.desmarcar();
    s.fazer_selecao_do_caminho(OpcoesDaSelecaoDoCaminho::default());
    assert_eq!(s.selecao().unwrap().valor(200, 200), 255);
}

/// A camada "Pintura" cheia de vermelho (sem tocar na base).
fn camada_vermelha(s: &mut Sessao) {
    s.selecionar_tudo();
    s.pincel.cor = [250, 0, 0];
    assert!(s.preencher_selecao());
    s.desmarcar();
}

#[test]
fn mascara_vetorial_vezes_a_de_pixels_sem_mudar_pixel_nenhum() {
    let mut s = sessao();
    camada_vermelha(&mut s);
    let pixels_antes = s.documento().camadas[0].pixels.clone();
    let base = s.base().clone();
    quadrado(&mut s);
    assert!(s.criar_mascara_vetorial());
    assert_eq!(s.alvo_vetorial(), Some(LugarDoCaminho::Mascara(0)));
    let foto = s.compor();
    assert_eq!(foto.get_pixel(150, 150).0, [250, 0, 0], "dentro: a camada");
    assert_eq!(
        foto.get_pixel(50, 50).0,
        base.get_pixel(50, 50).0,
        "fora: a base"
    );
    assert_eq!(
        s.documento().camadas[0].pixels,
        pixels_antes,
        "nenhum pixel muda"
    );

    // A máscara de pixels esconde a metade de cima: o produto das duas.
    assert!(s.adicionar_mascara(false));
    s.selecionar(
        &Forma::Retangulo(Retangulo::novo(0, 0, 600, 200)),
        Operacao::Nova,
    );
    s.pincel.cor = [0, 0, 0];
    assert!(s.preencher_selecao());
    s.desmarcar();
    let foto = s.compor();
    // Revelado pela de pixels (y ≥ 200) e pela vetorial (dentro do quadrado).
    assert_eq!(foto.get_pixel(150, 250).0, [250, 0, 0]);
    // Revelado só pela vetorial: escondido.
    assert_eq!(foto.get_pixel(150, 150).0, base.get_pixel(150, 150).0);
    // Revelado só pela de pixels: escondido.
    assert_eq!(foto.get_pixel(50, 250).0, base.get_pixel(50, 250).0);

    // Desligar a vetorial: a camada volta, o caminho continua.
    assert!(s.alternar_mascara_vetorial(0));
    assert_eq!(s.compor().get_pixel(50, 250).0, [250, 0, 0]);
    assert!(s.documento().camadas[0].mascara_vetorial.is_some());
    assert!(s.alternar_mascara_vetorial(0));

    // Editar o caminho da máscara muda a foto ao soltar, num passo.
    s.usar_ferramenta_vetorial(FerramentaVetorial::SelecaoDireta);
    s.escolher_caminho(Some(LugarDoCaminho::Mascara(0)));
    s.caneta_apertar(p(100.0, 300.0), NADA, M);
    s.caneta_arrastar(p(20.0, 380.0), NADA, M);
    s.caneta_soltar(M);
    assert_eq!(nome_do_ultimo(&s), "Mover pontos");
    let foto = s.compor();
    assert_eq!(foto.get_pixel(40, 365).0, [250, 0, 0], "a máscara cresceu");
    // A vista incremental é a mesma refeita do zero.
    let zero = crate::vista::Vista::nova(s.base(), s.documento(), 300);
    assert_eq!(s.vista().imagem().as_raw(), zero.imagem().as_raw());
    // Mesclar para baixo com a máscara vetorial aplica a máscara (o
    // escondido não desce).
    s.nova_camada();
    s.escolher_camada(0);
    let foto_antes = s.compor();
    s.escolher_camada(1);
    s.mover_camada(-1);
    // A de baixo agora é a nova (vazia); a vermelha mascarada vai em cima.
    s.escolher_camada(1);
    assert!(s.documento().camadas[1].mascara_vetorial.is_some());
    assert!(s.mesclar_para_baixo().is_ok());
    assert!(s.documento().camadas[0].mascara_vetorial.is_none());
    assert_eq!(s.compor().as_raw(), foto_antes.as_raw());
}

#[test]
fn a_mascara_vetorial_vazia_revela_tudo_e_o_mover_leva_o_caminho() {
    let mut s = sessao();
    camada_vermelha(&mut s);
    assert!(s.criar_mascara_vetorial());
    assert_eq!(
        s.compor().get_pixel(10, 10).0,
        [250, 0, 0],
        "vazia revela tudo"
    );
    // Desenhar nela recorta.
    quadrado(&mut s);
    assert_eq!(s.alvo_vetorial(), Some(LugarDoCaminho::Mascara(0)));
    let base = s.base().clone();
    assert_eq!(s.compor().get_pixel(10, 10).0, base.get_pixel(10, 10).0);
    // O Mover leva o caminho vinculado junto (um passo "Mover").
    assert!(s.comecar_a_mover());
    s.mover_por(50, 0);
    assert!(s.terminar_de_mover());
    assert_eq!(nome_do_ultimo(&s), "Mover");
    let c = &s.documento().camadas[0]
        .mascara_vetorial
        .as_ref()
        .unwrap()
        .caminho;
    assert_eq!(c.subcaminhos[0].ancoras[0].ponto, p(150.0, 100.0));
    s.desfazer();
    let c = &s.documento().camadas[0]
        .mascara_vetorial
        .as_ref()
        .unwrap()
        .caminho;
    assert_eq!(c.subcaminhos[0].ancoras[0].ponto, p(100.0, 100.0));
}

#[test]
fn preencher_e_contornar_sao_passos_explicitos_na_camada() {
    let mut s = sessao();
    quadrado(&mut s);
    s.pincel.cor = [0, 0, 255];
    // Dentro de uma seleção que pega só a metade da esquerda.
    s.selecionar(
        &Forma::Retangulo(Retangulo::novo(0, 0, 200, 400)),
        Operacao::Nova,
    );
    assert!(s.preencher_caminho(1.0));
    assert_eq!(nome_do_ultimo(&s), "Preencher caminho");
    let c = &s.documento().camadas[0].pixels;
    assert_eq!(c.pixel(150, 150), [0, 0, 255, 255]);
    assert_eq!(c.pixel(250, 150)[3], 0, "fora da seleção não pinta");
    s.desmarcar();
    let antes = s.historico().posicao();
    s.pincel.raio = 3.0;
    assert!(s.contornar_caminho());
    assert_eq!(s.historico().posicao(), antes + 1, "um passo só");
    assert_eq!(nome_do_ultimo(&s), "Contornar caminho");
    let alfa: Vec<u8> = (290..311)
        .map(|x| s.documento().camadas[0].pixels.pixel(x, 200)[3])
        .collect();
    assert!(alfa.iter().any(|a| *a > 200), "{alfa:?} {:?}", s.pincel);
}

#[test]
fn salvar_fechar_e_reabrir_mantem_o_caminho_editavel() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = sessao();
    camada_vermelha(&mut s);
    quadrado(&mut s);
    // Uma curva: arrasta uma âncora nova no meio de um lado.
    s.usar_ferramenta_vetorial(FerramentaVetorial::AdicionarPonto);
    clicar(&mut s, p(200.0, 100.0));
    let lugar = s.salvar_caminho_de_trabalho("Objeto").unwrap();
    assert!(s.criar_mascara_vetorial());
    let (doc, hist) = s.instantaneo();
    let base = s.base().clone();
    let projeto = Projeto::novo(dir.path().join("e1"), Arc::new(DiscoReal));
    let salvo = projeto.salvar("e1", &base, &doc, &hist, 1).unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("e1/projeto.json")).unwrap())
            .unwrap();
    assert_eq!(json["formato"], crate::projeto::FORMATO);
    assert_eq!(json["caminhos"]["nomeados"][0]["nome"], "Objeto");

    let outro = Projeto::novo(dir.path().join("e1"), Arc::new(DiscoReal));
    let aberto = outro.abrir(&base).unwrap().unwrap();
    assert_eq!(
        aberto.documento, doc,
        "ids, nomes, subcaminhos, alças e a máscara"
    );
    assert_eq!(aberto.historico.passos(), hist.para_gravar().0);
    let png =
        crate::projeto::ler_png(&std::fs::read(&salvo.versao.unwrap().arquivo).unwrap()).unwrap();
    assert_eq!(png.as_raw(), composicao::compor(&base, &doc).as_raw());

    // Reaberto, o nomeado continua editável e o desfazer anda para trás.
    let mut s2 = Sessao::nova(base, aberto.documento, aberto.historico, 300);
    s2.escolher_caminho(Some(lugar));
    s2.usar_ferramenta_vetorial(FerramentaVetorial::SelecaoDireta);
    s2.caneta_apertar(p(200.0, 100.0), NADA, M);
    s2.caneta_arrastar(p(200.0, 60.0), NADA, M);
    s2.caneta_soltar(M);
    let c = s2.documento().caminho(lugar).unwrap();
    assert_eq!(c.subcaminhos[0].ancoras[1].ponto, p(200.0, 60.0));
    assert!(s2.desfazer());
    assert!(s2.desfazer());
    assert!(s2.documento().camadas[0].mascara_vetorial.is_none());
}

#[test]
fn o_projeto_do_formato_10_abre_sem_caminhos() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = sessao();
    camada_vermelha(&mut s);
    let (doc, hist) = s.instantaneo();
    let base = s.base().clone();
    let projeto = Projeto::novo(dir.path().join("e1"), Arc::new(DiscoReal));
    projeto.salvar("e1", &base, &doc, &hist, 1).unwrap();
    let caminho = dir.path().join("e1/projeto.json");
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&caminho).unwrap()).unwrap();
    json["formato"] = 10.into();
    std::fs::write(&caminho, serde_json::to_vec(&json).unwrap()).unwrap();
    let aberto = projeto.abrir(&base).unwrap().unwrap();
    assert!(aberto.documento.caminhos.vazio());
    assert!(aberto.documento.camadas[0].mascara_vetorial.is_none());
}

#[test]
fn exportar_nao_leva_o_caminho_nem_muda_a_base() {
    let mut s = sessao();
    let base = s.base().clone();
    quadrado(&mut s);
    // Caminho sem máscara: a imagem editada é a base (o caminho não pinta).
    assert_eq!(s.compor().as_raw(), base.as_raw());
    assert!(s.documento().neutro());
    camada_vermelha(&mut s);
    assert!(s.criar_mascara_vetorial());
    let foto = s.compor();
    // A borda na resolução final: o pixel da aresta é exato (o quadrado
    // cai em pixels inteiros) e o de fora é a base.
    assert_eq!(foto.get_pixel(100, 150).0, [250, 0, 0]);
    assert_eq!(foto.get_pixel(99, 150).0, base.get_pixel(99, 150).0);
    assert_eq!(s.base().as_raw(), base.as_raw());
}

#[test]
fn densidade_e_difusao_da_mascara_vetorial_sem_mexer_no_caminho() {
    let mut s = sessao();
    camada_vermelha(&mut s);
    quadrado(&mut s);
    assert!(s.criar_mascara_vetorial());
    let base = s.base().clone();
    let caminho = s.documento().camadas[0]
        .mascara_vetorial
        .clone()
        .unwrap()
        .caminho;
    // Densidade 50%: fora do quadrado, meia camada.
    s.mover_propriedades_da_mascara_vetorial(0, Some(0.5), None);
    s.mover_propriedades_da_mascara_vetorial(0, Some(0.5), None);
    s.confirmar_mascara_vetorial();
    assert_eq!(
        nome_do_ultimo(&s),
        "Densidade da máscara vetorial de Pintura (50%)"
    );
    let fora = s.compor().get_pixel(20, 20).0;
    let b = base.get_pixel(20, 20).0;
    assert!(
        (fora[0] as i32 - (b[0] as i32 + 250) / 2).abs() <= 2,
        "{fora:?} × {b:?}"
    );
    // A vista incremental acompanhou (a foto inteira mudou).
    let zero = crate::vista::Vista::nova(s.base(), s.documento(), 300);
    assert_eq!(s.vista().imagem().as_raw(), zero.imagem().as_raw());
    s.desfazer();
    assert_eq!(s.compor().get_pixel(20, 20).0, b);
    // Difusão 10 px: a borda vira rampa; o caminho não muda.
    s.mover_propriedades_da_mascara_vetorial(0, None, Some(10.0));
    s.confirmar_mascara_vetorial();
    let borda = s.compor().get_pixel(100, 200).0;
    assert!(
        borda[0] > b[0].min(250) && borda[0] < 250,
        "rampa: {borda:?}"
    );
    assert_eq!(
        s.documento().camadas[0]
            .mascara_vetorial
            .as_ref()
            .unwrap()
            .caminho,
        caminho
    );
    let zero = crate::vista::Vista::nova(s.base(), s.documento(), 300);
    assert_eq!(s.vista().imagem().as_raw(), zero.imagem().as_raw());
}

#[test]
fn transformar_caminho_com_a_caixa_do_cmd_t() {
    use crate::transformar::Transformacao;
    let mut s = sessao();
    quadrado(&mut s);
    let antes = s.caminho_alvo().cloned().unwrap();
    assert!(s.comecar_a_transformar_caminho());
    assert!(s.transformando() && s.transformando_o_caminho());
    let (caixa, _) = s.transformacao().unwrap();
    assert_eq!(
        (caixa.x, caixa.y, caixa.largura, caixa.altura),
        (100, 100, 200, 200)
    );
    // Gira 90° em volta do centro (200, 200) e dobra a largura.
    s.definir_transformacao(Transformacao {
        escala_x: 2.0,
        angulo: std::f32::consts::FRAC_PI_2,
        ..Default::default()
    });
    let a = s.caminho_alvo().unwrap().subcaminhos[0].ancoras[0].ponto;
    // (100,100) → relativo (-100,-100) → escala (-200,-100) → gira 90°: (100,-200).
    assert!(a.distancia(p(300.0, 0.0)) < 1e-3, "{a:?}");
    let posicao = s.historico().posicao();
    assert_eq!(s.historico().posicao(), posicao, "ao vivo, sem passo");
    // Esc volta.
    s.cancelar_transformacao();
    assert_eq!(s.caminho_alvo(), Some(&antes));
    // De novo, só andando: um passo "Mover caminho"; desfazer volta.
    s.comecar_a_transformar_caminho();
    s.definir_transformacao(Transformacao::deslocamento(10.0, 5.0));
    assert!(s.aplicar_transformacao());
    assert_eq!(nome_do_ultimo(&s), "Mover caminho");
    assert_eq!(
        s.caminho_alvo().unwrap().subcaminhos[0].ancoras[0].ponto,
        p(110.0, 105.0)
    );
    s.desfazer();
    assert_eq!(s.caminho_alvo(), Some(&antes));
    // Com um componente escolhido, só ele.
    s.caneta.opcoes.operacao = OperacaoDoComponente::Somar;
    for q in [p(400.0, 50.0), p(450.0, 50.0), p(450.0, 90.0)] {
        clicar(&mut s, q);
    }
    s.caneta.encerrar();
    let segundo = s.caminho_alvo().unwrap().subcaminhos[1].id;
    s.caneta.escolher_componentes([segundo]);
    s.comecar_a_transformar_caminho();
    s.definir_transformacao(Transformacao {
        escala_x: 2.0,
        escala_y: 2.0,
        ..Default::default()
    });
    assert!(s.aplicar_transformacao());
    assert_eq!(nome_do_ultimo(&s), "Transformar caminho");
    let c = s.caminho_alvo().unwrap();
    assert_eq!(
        c.subcaminhos[0], antes.subcaminhos[0],
        "o outro componente fica"
    );
    assert!(c.subcaminhos[1].ancoras[0].ponto.distancia(p(375.0, 30.0)) < 1e-3);
}

#[test]
fn preencher_com_cor_difusao_e_contornar_com_a_borracha() {
    let mut s = sessao();
    camada_vermelha(&mut s);
    quadrado(&mut s);
    // Branco com difusão 8: a borda vira rampa.
    assert!(s.preencher_caminho_com([255, 255, 255], 1.0, 8));
    let c = &s.documento().camadas[0].pixels;
    assert_eq!(c.pixel(200, 200), [255, 255, 255, 255]);
    let borda = c.pixel(100, 200);
    assert!(borda[1] > 20 && borda[1] < 235, "rampa: {borda:?}");
    // Contornar com a borracha apaga ao longo da curva; o pincel volta.
    s.pincel.raio = 4.0;
    s.pincel.ferramenta = crate::pincel::Ferramenta::Pincel;
    assert!(s.contornar_caminho_com(crate::pincel::Ferramenta::Borracha));
    let alfa: Vec<u8> = (290..311)
        .map(|x| s.documento().camadas[0].pixels.pixel(x, 200)[3])
        .collect();
    assert!(alfa[10] < 60, "{alfa:?}");
    assert_eq!(s.pincel.ferramenta, crate::pincel::Ferramenta::Pincel);
    assert_eq!(nome_do_ultimo(&s), "Contornar caminho");
}

#[test]
fn o_modo_forma_cria_a_camada_de_forma_e_shift_soma() {
    use crate::vetor::caneta::ModoDaCaneta;
    let mut s = sessao();
    let base = s.base().clone();
    s.caneta.opcoes.modo = ModoDaCaneta::Forma;
    s.pincel.cor = [0, 0, 255];
    quadrado(&mut s);
    assert_eq!(s.documento().camadas.len(), 2);
    let forma = &s.documento().camadas[1];
    assert_eq!(forma.nome, "Forma 1");
    assert!(s.e_camada_de_forma(1));
    assert_eq!(s.alvo_vetorial(), Some(LugarDoCaminho::Mascara(1)));
    let foto = s.compor();
    assert_eq!(
        foto.get_pixel(200, 200).0,
        [0, 0, 255],
        "a forma pinta a cor"
    );
    assert_eq!(
        foto.get_pixel(20, 20).0,
        base.get_pixel(20, 20).0,
        "fora, a foto"
    );
    // Um clique novo no vazio: outra camada de forma.
    s.caneta.encerrar();
    for q in [p(400.0, 50.0), p(500.0, 50.0), p(450.0, 150.0)] {
        clicar(&mut s, q);
    }
    clicar(&mut s, p(400.0, 50.0));
    assert_eq!(s.documento().camadas.len(), 3);
    assert_eq!(s.documento().camadas[2].nome, "Forma 2");
    // Com ⇧, soma à forma escolhida.
    let shift = Modificadores {
        shift: true,
        ..NADA
    };
    s.caneta_apertar(p(500.0, 300.0), shift, M);
    s.caneta_soltar(M);
    for q in [p(580.0, 300.0), p(540.0, 380.0)] {
        clicar(&mut s, q);
    }
    clicar(&mut s, p(500.0, 300.0));
    assert_eq!(s.documento().camadas.len(), 3);
    assert_eq!(s.caminho_alvo().unwrap().subcaminhos.len(), 2);
    // Esc no meio do primeiro arrasto de uma forma nova: a camada não fica.
    s.caneta.encerrar();
    s.caneta_apertar(p(30.0, 300.0), NADA, M);
    s.caneta_arrastar(p(60.0, 330.0), NADA, M);
    assert_eq!(s.documento().camadas.len(), 4);
    s.caneta_esc();
    assert_eq!(s.documento().camadas.len(), 3);
    // Trocar a cor da forma é um passo de ajuste; o caminho fica.
    s.escolher_camada(1);
    s.mover_ajuste(crate::ajuste::Ajuste::CorSolida { cor: [0, 255, 0] });
    s.confirmar_ajuste();
    assert_eq!(s.compor().get_pixel(200, 200).0, [0, 255, 0]);
}
