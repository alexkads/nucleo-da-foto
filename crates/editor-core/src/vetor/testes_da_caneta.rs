//! A Caneta como o operador a usa: cliques, arrastos, modificadores e teclas,
//! com o ponteiro já em pixels do documento.

use super::caneta::{Acao, Caneta, Estado, FerramentaVetorial, Medida, Modificadores, Resultado};
use super::edicao::Extremo;
use super::{Caminho, Lado, Ligacao, Ponto, RefAncora};

const M: Medida = Medida { por_ponto: 1.0 };
const NADA: Modificadores = Modificadores {
    shift: false,
    alt: false,
    comando: false,
};
const ALT: Modificadores = Modificadores {
    shift: false,
    alt: true,
    comando: false,
};
const SHIFT: Modificadores = Modificadores {
    shift: true,
    alt: false,
    comando: false,
};
const CMD: Modificadores = Modificadores {
    shift: false,
    alt: false,
    comando: true,
};

fn p(x: f64, y: f64) -> Ponto {
    Ponto::novo(x, y)
}

/// Um clique (sem arrastar) — devolve o resultado do soltar.
fn clicar(k: &mut Caneta, c: &mut Caminho, q: Ponto, m: Modificadores) -> Resultado {
    let r = k.apertar(c, q, m, M);
    if matches!(r, Resultado::Passo(_)) {
        return r;
    }
    k.soltar(c, M)
}

/// Apertar em `de`, arrastar até `ate` em passos, soltar.
fn arrastar(k: &mut Caneta, c: &mut Caminho, de: Ponto, ate: Ponto, m: Modificadores) -> Resultado {
    k.apertar(c, de, m, M);
    for i in 1..=5 {
        let t = i as f64 / 5.0;
        k.arrastar(
            c,
            p(de.x + (ate.x - de.x) * t, de.y + (ate.y - de.y) * t),
            m,
            M,
        );
    }
    k.soltar(c, M)
}

fn novo() -> (Caneta, Caminho) {
    (Caneta::nova(), Caminho::novo(1, "Caminho de trabalho"))
}

#[test]
fn cliques_fazem_cantos_e_arrasto_faz_curva_no_mesmo_caminho() {
    let (mut k, mut c) = novo();
    assert_eq!(
        clicar(&mut k, &mut c, p(10.0, 10.0), NADA),
        Resultado::Passo("Ponto de ancoragem")
    );
    assert!(matches!(
        k.estado(),
        Estado::Construindo {
            extremo: Extremo::Fim,
            ..
        }
    ));
    clicar(&mut k, &mut c, p(100.0, 10.0), NADA);
    // Arrasto: uma âncora com alças, suave, simétrica no gesto.
    let r = arrastar(&mut k, &mut c, p(100.0, 100.0), p(130.0, 100.0), NADA);
    assert_eq!(r, Resultado::Passo("Ponto de ancoragem"));
    let s = &c.subcaminhos[0];
    assert_eq!(s.ancoras.len(), 3);
    assert!(s.ancoras[0].entrada.is_none() && s.ancoras[0].saida.is_none());
    let a = &s.ancoras[2];
    assert_eq!(a.ligacao, Ligacao::Suave);
    assert_eq!(a.saida, Some(p(130.0, 100.0)));
    assert_eq!(a.entrada, Some(p(70.0, 100.0)));
    assert!(s.segmento(0).unwrap().reto());
    assert!(!s.segmento(1).unwrap().reto());
}

#[test]
fn shift_prende_a_proxima_ancora_em_45_graus() {
    let (mut k, mut c) = novo();
    clicar(&mut k, &mut c, p(0.0, 0.0), NADA);
    clicar(&mut k, &mut c, p(50.0, 47.0), SHIFT);
    let a = c.subcaminhos[0].ancoras[1].ponto;
    assert!((a.x - a.y).abs() < 1e-9, "{a:?}");
}

#[test]
fn clicar_na_primeira_fecha_e_arrastar_nela_ajusta_as_alcas() {
    let (mut k, mut c) = novo();
    for q in [p(0.0, 0.0), p(100.0, 0.0), p(100.0, 100.0)] {
        clicar(&mut k, &mut c, q, NADA);
    }
    // O cursor anuncia o fechamento.
    assert!(matches!(
        k.decidir(&c, p(1.0, 1.0), NADA, M),
        Acao::Fechar { .. }
    ));
    assert_eq!(
        clicar(&mut k, &mut c, p(1.0, 1.0), NADA),
        Resultado::Passo("Fechar caminho")
    );
    assert!(c.subcaminhos[0].fechado);
    assert_eq!(k.estado(), &Estado::Ocioso);

    // De novo, fechando com arrasto.
    let (mut k, mut c) = novo();
    for q in [p(0.0, 0.0), p(100.0, 0.0), p(100.0, 100.0)] {
        clicar(&mut k, &mut c, q, NADA);
    }
    arrastar(&mut k, &mut c, p(0.0, 0.0), p(0.0, -20.0), NADA);
    let s = &c.subcaminhos[0];
    assert!(s.fechado);
    assert_eq!(s.ancoras.len(), 3, "fechar não cria âncora");
    let primeira = &s.ancoras[0];
    assert_eq!(primeira.saida, Some(p(0.0, -20.0)));
    assert_eq!(
        primeira.entrada,
        Some(p(0.0, 20.0)),
        "o segmento de fechamento curva"
    );
}

#[test]
fn enter_e_esc_deixam_aberto_sem_apagar() {
    let (mut k, mut c) = novo();
    clicar(&mut k, &mut c, p(0.0, 0.0), NADA);
    clicar(&mut k, &mut c, p(50.0, 0.0), NADA);
    assert!(k.encerrar());
    assert_eq!(k.estado(), &Estado::Ocioso);
    assert_eq!(c.subcaminhos[0].ancoras.len(), 2);
    assert!(!c.subcaminhos[0].fechado);
    // Um clique novo no vazio começa outro componente — o primeiro fica.
    clicar(&mut k, &mut c, p(0.0, 80.0), NADA);
    assert_eq!(c.subcaminhos.len(), 2);
    assert_eq!(k.esc(&mut c), Resultado::Nada);
    assert_eq!(k.estado(), &Estado::Ocioso);
    assert_eq!(c.subcaminhos.len(), 2, "o Esc não apaga");
}

#[test]
fn comando_mais_clique_fora_encerra_e_comando_vira_selecao_direta() {
    let (mut k, mut c) = novo();
    clicar(&mut k, &mut c, p(0.0, 0.0), NADA);
    clicar(&mut k, &mut c, p(50.0, 0.0), NADA);
    assert_eq!(k.ferramenta_efetiva(CMD), FerramentaVetorial::SelecaoDireta);
    assert_eq!(
        clicar(&mut k, &mut c, p(300.0, 300.0), CMD),
        Resultado::Nada
    );
    assert_eq!(k.estado(), &Estado::Ocioso);
    // ⌘ + arrasto numa âncora move só ela, sem trocar a ferramenta.
    let r = arrastar(&mut k, &mut c, p(50.0, 0.0), p(60.0, 10.0), CMD);
    assert_eq!(r, Resultado::Passo("Mover pontos"));
    assert_eq!(c.subcaminhos[0].ancoras[1].ponto, p(60.0, 10.0));
    assert_eq!(k.ferramenta, FerramentaVetorial::Caneta);
}

#[test]
fn retomar_pelo_fim_e_pelo_inicio() {
    let (mut k, mut c) = novo();
    clicar(&mut k, &mut c, p(0.0, 0.0), NADA);
    clicar(&mut k, &mut c, p(50.0, 0.0), NADA);
    k.encerrar();
    // Pelo fim.
    assert!(matches!(
        k.decidir(&c, p(50.0, 0.0), NADA, M),
        Acao::Retomar {
            extremo: Extremo::Fim,
            ..
        }
    ));
    clicar(&mut k, &mut c, p(50.0, 0.0), NADA);
    clicar(&mut k, &mut c, p(50.0, 50.0), NADA);
    assert_eq!(c.subcaminhos[0].ancoras.len(), 3);
    k.encerrar();
    // Pelo início: as novas entram antes da primeira.
    clicar(&mut k, &mut c, p(0.0, 0.0), NADA);
    assert!(matches!(
        k.estado(),
        Estado::Construindo {
            extremo: Extremo::Inicio,
            ..
        }
    ));
    arrastar(&mut k, &mut c, p(-50.0, 0.0), p(-50.0, 30.0), NADA);
    let s = &c.subcaminhos[0];
    assert_eq!(s.ancoras.len(), 4);
    assert_eq!(s.ancoras[0].ponto, p(-50.0, 0.0));
    // A alça arrastada aponta para a frente do desenho — a entrada, no início.
    assert_eq!(s.ancoras[0].entrada, Some(p(-50.0, 30.0)));
    assert_eq!(s.ancoras[0].saida, Some(p(-50.0, -30.0)));
    // Fechar a partir do início: clicar na última.
    assert_eq!(
        clicar(&mut k, &mut c, p(50.0, 50.0), NADA),
        Resultado::Passo("Fechar caminho")
    );
    assert!(c.subcaminhos[0].fechado);
}

#[test]
fn esc_no_meio_do_arrasto_volta_a_geometria_e_o_estado() {
    let (mut k, mut c) = novo();
    clicar(&mut k, &mut c, p(0.0, 0.0), NADA);
    let antes = c.clone();
    k.apertar(&mut c, p(100.0, 0.0), NADA, M);
    k.arrastar(&mut c, p(130.0, 30.0), NADA, M);
    assert_eq!(c.subcaminhos[0].ancoras.len(), 2);
    assert_eq!(k.esc(&mut c), Resultado::Cancelado);
    assert_eq!(c, antes);
    assert!(matches!(k.estado(), Estado::Construindo { .. }));
    // E o soltar que vem depois não faz nada.
    assert_eq!(k.soltar(&mut c, M), Resultado::Nada);

    // Mover pontos cancelado.
    k.encerrar();
    let antes = c.clone();
    k.apertar(&mut c, p(0.0, 0.0), CMD, M);
    k.arrastar(&mut c, p(40.0, 40.0), CMD, M);
    assert_ne!(c, antes);
    assert_eq!(k.esc(&mut c), Resultado::Cancelado);
    assert_eq!(c, antes);
}

#[test]
fn alt_na_ultima_tira_a_alca_da_frente_e_continua_com_reta() {
    let (mut k, mut c) = novo();
    clicar(&mut k, &mut c, p(0.0, 0.0), NADA);
    arrastar(&mut k, &mut c, p(100.0, 0.0), p(130.0, 0.0), NADA);
    let entrada = c.subcaminhos[0].ancoras[1].entrada;
    assert!(c.subcaminhos[0].ancoras[1].saida.is_some());
    assert_eq!(
        clicar(&mut k, &mut c, p(100.0, 0.0), ALT),
        Resultado::Passo("Converter ponto")
    );
    let a = &c.subcaminhos[0].ancoras[1];
    assert!(a.saida.is_none(), "a alça da frente saiu");
    assert_eq!(a.entrada, entrada, "o segmento de entrada fica como estava");
    assert!(matches!(k.estado(), Estado::Construindo { .. }));
    clicar(&mut k, &mut c, p(200.0, 0.0), NADA);
    assert!(c.subcaminhos[0].segmento(1).unwrap().reto());
}

#[test]
fn alt_no_meio_do_arrasto_quebra_as_alcas() {
    let (mut k, mut c) = novo();
    clicar(&mut k, &mut c, p(0.0, 0.0), NADA);
    k.apertar(&mut c, p(100.0, 0.0), NADA, M);
    k.arrastar(&mut c, p(130.0, 0.0), NADA, M);
    k.arrastar(&mut c, p(100.0, 40.0), ALT, M);
    k.soltar(&mut c, M);
    let a = &c.subcaminhos[0].ancoras[1];
    assert_eq!(a.entrada, Some(p(70.0, 0.0)), "a de trás ficou onde estava");
    assert_eq!(a.saida, Some(p(100.0, 40.0)));
    assert_eq!(a.ligacao, Ligacao::Canto);
}

#[test]
fn adicionar_e_excluir_automaticamente_e_desligado() {
    let mut c = super::edicao::retangulo_em_caminho(0.0, 0.0, 100.0, 100.0);
    let mut k = Caneta::nova();
    assert!(matches!(
        k.decidir(&c, p(50.0, 1.0), NADA, M),
        Acao::Adicionar { .. }
    ));
    assert_eq!(
        clicar(&mut k, &mut c, p(50.0, 1.0), NADA),
        Resultado::Passo("Adicionar ponto de ancoragem")
    );
    assert_eq!(c.subcaminhos[0].ancoras.len(), 5);
    // A âncora de um fechado (não é ponta): excluir.
    assert!(matches!(
        k.decidir(&c, p(100.0, 100.0), NADA, M),
        Acao::Excluir(_)
    ));
    assert_eq!(
        clicar(&mut k, &mut c, p(100.0, 100.0), NADA),
        Resultado::Passo("Excluir ponto de ancoragem")
    );
    assert_eq!(c.subcaminhos[0].ancoras.len(), 4);
    // Desligado: nada implícito — o clique começa um componente novo.
    k.opcoes.auto_adicionar_excluir = false;
    assert_eq!(k.decidir(&c, p(50.0, 1.0), NADA, M), Acao::NovoComponente);
    assert_eq!(k.decidir(&c, p(0.0, 0.0), NADA, M), Acao::NovoComponente);
}

#[test]
fn selecao_direta_retangulo_shift_e_setas() {
    let mut c = super::edicao::retangulo_em_caminho(0.0, 0.0, 100.0, 100.0);
    let mut k = Caneta::nova();
    k.usar(FerramentaVetorial::SelecaoDireta);
    // Retângulo pegando as duas de cima.
    arrastar(&mut k, &mut c, p(-10.0, -10.0), p(110.0, 10.0), NADA);
    assert_eq!(k.pontos_escolhidos().len(), 2);
    // Shift + clique numa terceira soma; de novo, tira.
    clicar(&mut k, &mut c, p(100.0, 100.0), SHIFT);
    assert_eq!(k.pontos_escolhidos().len(), 3);
    clicar(&mut k, &mut c, p(100.0, 100.0), SHIFT);
    assert_eq!(k.pontos_escolhidos().len(), 2);
    // As setas levam as escolhidas.
    assert_eq!(
        k.empurrar(&mut c, (0.0, -10.0)),
        Resultado::Passo("Mover pontos")
    );
    let s = &c.subcaminhos[0];
    assert_eq!(s.ancoras[0].ponto, p(0.0, -10.0));
    assert_eq!(s.ancoras[1].ponto, p(100.0, -10.0));
    assert_eq!(s.ancoras[2].ponto, p(100.0, 100.0));
    // Um arrasto leva as duas juntas.
    arrastar(&mut k, &mut c, p(0.0, -10.0), p(5.0, -5.0), NADA);
    assert_eq!(c.subcaminhos[0].ancoras[1].ponto, p(105.0, -5.0));
    // Delete abre o fechado onde elas estavam.
    assert_eq!(
        k.excluir(&mut c),
        Resultado::Passo("Excluir pontos de ancoragem")
    );
    assert!(!c.subcaminhos[0].fechado);
    assert_eq!(c.subcaminhos[0].ancoras.len(), 2);
}

#[test]
fn selecao_direta_mexe_na_alca_com_a_curva_e_alt_solta() {
    let (mut k, mut c) = novo();
    clicar(&mut k, &mut c, p(0.0, 0.0), NADA);
    arrastar(&mut k, &mut c, p(100.0, 0.0), p(130.0, 0.0), NADA);
    clicar(&mut k, &mut c, p(200.0, 0.0), NADA);
    k.encerrar();
    k.usar(FerramentaVetorial::SelecaoDireta);
    // Escolhe a do meio: as alças aparecem.
    clicar(&mut k, &mut c, p(100.0, 0.0), NADA);
    let meio = RefAncora {
        sub: c.subcaminhos[0].id,
        ancora: c.subcaminhos[0].ancoras[1].id,
    };
    assert!(k.alcas_a_mostrar(&c).contains(&meio));
    // Arrastar a saída para cima: a entrada gira junto (suave), com o tamanho dela.
    k.apertar(&mut c, p(130.0, 0.0), NADA, M);
    let r = k.arrastar(&mut c, p(100.0, -50.0), NADA, M);
    assert_eq!(r, Resultado::AoVivo, "a curva muda durante o arrasto");
    assert_eq!(k.soltar(&mut c, M), Resultado::Passo("Mover alça"));
    let a = c.ancora(meio).unwrap();
    assert_eq!(a.saida, Some(p(100.0, -50.0)));
    assert!((a.entrada.unwrap().y - 30.0).abs() < 1e-9);
    // ⌥ + arrasto da alça: só ela, e a âncora vira canto.
    arrastar(&mut k, &mut c, p(100.0, -50.0), p(150.0, -50.0), ALT);
    let a = c.ancora(meio).unwrap();
    assert_eq!(a.ligacao, Ligacao::Canto);
    assert!((a.entrada.unwrap().y - 30.0).abs() < 1e-9);
}

#[test]
fn converter_ponto_puxa_e_tira_alcas() {
    let mut c = super::edicao::retangulo_em_caminho(0.0, 0.0, 100.0, 100.0);
    let mut k = Caneta::nova();
    k.usar(FerramentaVetorial::ConverterPonto);
    let r = arrastar(&mut k, &mut c, p(100.0, 0.0), p(130.0, 20.0), NADA);
    assert_eq!(r, Resultado::Passo("Converter ponto"));
    let a = &c.subcaminhos[0].ancoras[1];
    assert_eq!(a.ligacao, Ligacao::Suave);
    assert_eq!(a.saida, Some(p(130.0, 20.0)));
    assert_eq!(
        clicar(&mut k, &mut c, p(100.0, 0.0), NADA),
        Resultado::Passo("Converter ponto")
    );
    let a = &c.subcaminhos[0].ancoras[1];
    assert!(a.saida.is_none() && a.entrada.is_none());
    // Na Caneta, ⌥ sobre uma âncora converte também.
    k.usar(FerramentaVetorial::Caneta);
    assert!(matches!(
        k.decidir(&c, p(0.0, 100.0), ALT, M),
        Acao::Converter(_)
    ));
}

#[test]
fn selecao_de_caminho_move_duplica_e_exclui_componentes() {
    let mut c = super::edicao::retangulo_em_caminho(0.0, 0.0, 50.0, 50.0);
    let mut k = Caneta::nova();
    k.usar(FerramentaVetorial::SelecaoDeCaminho);
    // Clique dentro da área pega o componente.
    let r = arrastar(&mut k, &mut c, p(25.0, 25.0), p(35.0, 25.0), NADA);
    assert_eq!(r, Resultado::Passo("Mover componente"));
    assert_eq!(c.subcaminhos[0].ancoras[0].ponto, p(10.0, 0.0));
    // ⌥ + arrasto duplica; o original fica.
    let r = arrastar(&mut k, &mut c, p(35.0, 25.0), p(35.0, 125.0), ALT);
    assert_eq!(r, Resultado::Passo("Duplicar componente"));
    assert_eq!(c.subcaminhos.len(), 2);
    assert_eq!(c.subcaminhos[0].ancoras[0].ponto, p(10.0, 0.0));
    assert_eq!(c.subcaminhos[1].ancoras[0].ponto, p(10.0, 100.0));
    assert_eq!(k.componentes_escolhidos().len(), 1);
    assert!(k.componentes_escolhidos().contains(&c.subcaminhos[1].id));
    // Esc no meio de um ⌥ + arrasto não deixa a cópia.
    let antes = c.clone();
    k.apertar(&mut c, p(35.0, 125.0), ALT, M);
    k.arrastar(&mut c, p(80.0, 125.0), ALT, M);
    assert_eq!(c.subcaminhos.len(), 3);
    k.esc(&mut c);
    assert_eq!(c, antes);
    // Delete exclui o escolhido.
    assert_eq!(k.excluir(&mut c), Resultado::Passo("Excluir componente"));
    assert_eq!(c.subcaminhos.len(), 1);
}

#[test]
fn delete_desenhando_tira_a_ultima_ancora() {
    let (mut k, mut c) = novo();
    for q in [p(0.0, 0.0), p(10.0, 0.0), p(20.0, 0.0)] {
        clicar(&mut k, &mut c, q, NADA);
    }
    assert_eq!(
        k.excluir(&mut c),
        Resultado::Passo("Excluir ponto de ancoragem")
    );
    assert_eq!(c.subcaminhos[0].ancoras.len(), 2);
    assert!(matches!(k.estado(), Estado::Construindo { .. }));
    clicar(&mut k, &mut c, p(10.0, 50.0), NADA);
    assert_eq!(c.subcaminhos[0].ancoras[2].ponto, p(10.0, 50.0));
}

#[test]
fn a_faixa_elastica_vai_da_alca_da_ponta_ate_o_ponteiro() {
    let (mut k, mut c) = novo();
    clicar(&mut k, &mut c, p(0.0, 0.0), NADA);
    arrastar(&mut k, &mut c, p(100.0, 0.0), p(120.0, 0.0), NADA);
    k.arrastar(&mut c, p(200.0, 50.0), NADA, M);
    assert_eq!(k.previa(&c, false), None, "desligada por padrão");
    k.opcoes.previa = true;
    let s = k.previa(&c, false).unwrap();
    assert_eq!(
        s,
        [p(100.0, 0.0), p(120.0, 0.0), p(200.0, 50.0), p(200.0, 50.0)]
    );
}

#[test]
fn a_tolerancia_segue_a_escala_da_tela() {
    let c = super::edicao::retangulo_em_caminho(0.0, 0.0, 100.0, 100.0);
    let k = Caneta::nova();
    // Ampliado 4×: um ponto da tela é ¼ de pixel; 2 px do documento (8
    // pontos da tela) já está longe.
    let perto = Medida { por_ponto: 0.25 };
    assert_eq!(
        k.decidir(&c, p(2.0, 2.0), NADA, perto),
        Acao::NovoComponente
    );
    // Reduzido a ¼: 2 px do documento é meio ponto da tela — pega a âncora.
    let longe = Medida { por_ponto: 4.0 };
    assert!(matches!(
        k.decidir(&c, p(2.0, 2.0), NADA, longe),
        Acao::Excluir(_)
    ));
}

#[test]
fn trocar_de_ferramenta_preserva_a_geometria() {
    let (mut k, mut c) = novo();
    clicar(&mut k, &mut c, p(0.0, 0.0), NADA);
    clicar(&mut k, &mut c, p(10.0, 0.0), NADA);
    let antes = c.clone();
    k.usar(FerramentaVetorial::SelecaoDeCaminho);
    assert_eq!(c, antes);
    assert_eq!(k.estado(), &Estado::Ocioso);
    assert_eq!(k.componentes_escolhidos().len(), 1);
    // A Caneta de volta retoma pela ponta.
    k.usar(FerramentaVetorial::Caneta);
    assert!(matches!(
        k.decidir(&c, p(10.0, 0.0), NADA, M),
        Acao::Retomar { .. }
    ));
    let _ = Lado::Saida;
}
