//! O caminho vira cobertura: quanto de cada pixel do documento fica dentro
//! da área dele, de 0 a 255 — a máscara de onde saem a seleção ("Fazer
//! seleção", ⌘↵), a máscara vetorial e o preenchimento do caminho.
//!
//! 🔑 **Por componente, depois as operações.** Cada subcaminho é achatado em
//! retas (tolerância de exportação, bem abaixo de um pixel), fechado
//! virtualmente se estiver aberto (o caminho não muda), e rasterizado
//! **sozinho** com a regra de preenchimento do caminho — é ali que a
//! autointerseção se resolve. Os componentes então se juntam na ordem, cada
//! um pela operação dele (`OperacaoDoComponente`, onde a ordem de avaliação
//! está escrita). Regra de preenchimento e operação são coisas diferentes: a
//! regra diz o que é dentro **de um** componente; a operação, como ele entra
//! no resultado.
//!
//! 🔑 **Antisserrilhado de verdade**: quatro linhas de amostra por pixel e,
//! em cada uma, a fração exata de cada pixel coberta na horizontal — a mesma
//! régua do laço antisserrilhado da seleção. Sem antisserrilhado, o pixel é
//! dentro ou fora pelo centro.
//!
//! O resultado é uma [`Selecao`] (esparsa, em tiles): independente do
//! caminho — editar o caminho depois não muda o que já foi rasterizado.

use crate::retangulo::Retangulo;
use crate::selecao::Selecao;

use super::geometria::achatar;
use super::{Caminho, OperacaoDoComponente, Ponto, RegraDePreenchimento, Subcaminho};

/// Como rasterizar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opcoes {
    pub suavizar: bool,
    /// O erro máximo do achatamento das curvas, em pixels do documento.
    pub tolerancia: f64,
}

impl Opcoes {
    /// A da máscara vetorial e da exportação: antisserrilhada, um vigésimo de
    /// pixel de erro.
    pub fn da_mascara() -> Self {
        Self {
            suavizar: true,
            tolerancia: 0.05,
        }
    }

    pub fn da_selecao(suavizar: bool) -> Self {
        Self {
            suavizar,
            tolerancia: 0.05,
        }
    }
}

/// O caminho inteiro em cobertura, na foto `largura × altura`.
pub fn rasterizar(caminho: &Caminho, largura: u32, altura: u32, opcoes: &Opcoes) -> Selecao {
    let mut acumulado: Option<Selecao> = None;
    for s in caminho.subcaminhos.iter().filter(|s| s.ancoras.len() >= 2) {
        let componente = do_componente(s, caminho.regra, largura, altura, opcoes);
        acumulado = Some(match acumulado {
            None => match s.operacao {
                OperacaoDoComponente::Subtrair => {
                    let mut tudo = Selecao::tudo(largura, altura);
                    tudo.combinar_com(&componente, |a, b| {
                        OperacaoDoComponente::Subtrair.juntar(a, b)
                    });
                    tudo
                }
                _ => componente,
            },
            Some(mut a) => {
                let op = s.operacao;
                a.combinar_com(&componente, |x, y| op.juntar(x, y));
                a
            }
        });
    }
    acumulado.unwrap_or_else(|| Selecao::vazia(largura, altura))
}

/// A miniatura do caminho (o painel Caminhos): a foto `largura × altura`
/// reduzida a `l × a`, um byte de cobertura por pixel (fechamento virtual,
/// operações e regra valendo, como na seleção).
pub fn miniatura(c: &Caminho, largura: u32, altura: u32, l: u32, a: u32) -> Vec<u8> {
    let mut reduzido = c.clone();
    let (fx, fy) = (
        l as f64 / largura.max(1) as f64,
        a as f64 / altura.max(1) as f64,
    );
    super::edicao::transformar(&mut reduzido, [fx, 0.0, 0.0, 0.0, fy, 0.0]);
    let s = rasterizar(&reduzido, l, a, &Opcoes::da_mascara());
    let mut v = Vec::with_capacity((l * a) as usize);
    for y in 0..a {
        for x in 0..l {
            v.push(s.valor(x, y));
        }
    }
    v
}

/// Um componente sozinho, com a regra.
pub fn do_componente(
    s: &Subcaminho,
    regra: RegraDePreenchimento,
    largura: u32,
    altura: u32,
    opcoes: &Opcoes,
) -> Selecao {
    let poligono = achatar(s, opcoes.tolerancia, true);
    let (caixa, densa) = cobrir(&poligono, regra, largura, altura, opcoes.suavizar);
    if caixa.vazio() {
        return Selecao::vazia(largura, altura);
    }
    Selecao::do_recorte(largura, altura, &caixa, &densa)
}

/// Quantas linhas de amostra por pixel com antisserrilhado.
const AMOSTRAS: usize = 4;

/// Uma aresta do polígono, de cima para baixo (`y0 < y1`), com o sentido
/// original (`+1` descendo, `−1` subindo) — é ele que conta as voltas.
struct Aresta {
    y0: f64,
    y1: f64,
    x0: f64,
    dxdy: f64,
    sentido: i32,
}

/// O polígono rasterizado: a caixa (dentro da foto) e um byte por pixel dela.
pub fn cobrir(
    poligono: &[Ponto],
    regra: RegraDePreenchimento,
    largura: u32,
    altura: u32,
    suavizar: bool,
) -> (Retangulo, Vec<u8>) {
    let n = poligono.len();
    if n < 3 {
        return (Retangulo::default(), Vec::new());
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in poligono {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let cx0 = x0.floor().max(0.0);
    let cy0 = y0.floor().max(0.0);
    let cx1 = x1.ceil().min(largura as f64);
    let cy1 = y1.ceil().min(altura as f64);
    if cx1 <= cx0 || cy1 <= cy0 {
        return (Retangulo::default(), Vec::new());
    }
    let caixa = Retangulo::novo(
        cx0 as u32,
        cy0 as u32,
        (cx1 - cx0) as u32,
        (cy1 - cy0) as u32,
    );
    let mut arestas: Vec<Aresta> = (0..n)
        .filter_map(|i| {
            let (a, b) = (poligono[i], poligono[(i + 1) % n]);
            if a.y == b.y || !a.y.is_finite() || !b.y.is_finite() {
                return None;
            }
            let (de, ate, sentido) = if a.y < b.y { (a, b, 1) } else { (b, a, -1) };
            Some(Aresta {
                y0: de.y,
                y1: ate.y,
                x0: de.x,
                dxdy: (ate.x - de.x) / (ate.y - de.y),
                sentido,
            })
        })
        .collect();
    arestas.sort_by(|a, b| a.y0.total_cmp(&b.y0));

    let (l, a) = (caixa.largura as usize, caixa.altura as usize);
    let mut saida = vec![0u8; l * a];
    let xa = caixa.x as f64;
    let xb = caixa.direita() as f64;
    let amostras = if suavizar { AMOSTRAS } else { 1 };
    let peso = 1.0 / amostras as f32;
    let mut parcial = vec![0f32; l + 1];
    let mut delta = vec![0f32; l + 2];
    let mut ativas: Vec<usize> = Vec::new();
    let mut proxima = 0usize;
    let mut cortes: Vec<(f64, i32)> = Vec::new();
    let dentro = |voltas: i32| match regra {
        RegraDePreenchimento::NaoZero => voltas != 0,
        RegraDePreenchimento::ParImpar => voltas % 2 != 0,
    };
    for linha in 0..a {
        parcial.iter_mut().for_each(|v| *v = 0.0);
        delta.iter_mut().for_each(|v| *v = 0.0);
        let y = caixa.y as f64 + linha as f64;
        for k in 0..amostras {
            let ys = if suavizar {
                y + (k as f64 + 0.5) / amostras as f64
            } else {
                y + 0.5
            };
            while proxima < arestas.len() && arestas[proxima].y0 <= ys {
                ativas.push(proxima);
                proxima += 1;
            }
            ativas.retain(|&e| arestas[e].y1 > ys);
            cortes.clear();
            for &e in &ativas {
                let r = &arestas[e];
                if r.y0 <= ys {
                    cortes.push((r.x0 + (ys - r.y0) * r.dxdy, r.sentido));
                }
            }
            cortes.sort_by(|p, q| p.0.total_cmp(&q.0));
            let mut voltas = 0;
            let mut inicio: Option<f64> = None;
            for &(x, sentido) in &cortes {
                let antes = dentro(voltas);
                voltas += sentido;
                let depois = dentro(voltas);
                match (antes, depois) {
                    (false, true) => inicio = Some(x),
                    (true, false) => {
                        if let Some(de) = inicio.take() {
                            somar_trecho(
                                de,
                                x,
                                xa,
                                xb,
                                l,
                                peso,
                                suavizar,
                                &mut parcial,
                                &mut delta,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut corrente = 0f32;
        let fora = &mut saida[linha * l..(linha + 1) * l];
        for (i, v) in fora.iter_mut().enumerate() {
            corrente += delta[i];
            let c = (parcial[i] + corrente).clamp(0.0, 1.0);
            *v = (c * 255.0).round() as u8;
        }
    }
    (caixa, saida)
}

/// Soma o trecho `[de, ate)` (pixels da foto) à linha: a fração exata de cada
/// pixel das pontas e o peso inteiro no meio (pela diferença, de uma vez).
#[allow(clippy::too_many_arguments)]
fn somar_trecho(
    de: f64,
    ate: f64,
    xa: f64,
    xb: f64,
    l: usize,
    peso: f32,
    suavizar: bool,
    parcial: &mut [f32],
    delta: &mut [f32],
) {
    if !suavizar {
        // Pelo centro: os pixels com `x + 0.5` dentro de `[de, ate)`.
        let i0 = ((de - xa - 0.5).ceil().max(0.0)) as usize;
        let i1 = ((ate - xa - 0.5).ceil().max(0.0) as usize).min(l);
        if i1 > i0 {
            delta[i0] += peso;
            delta[i1] -= peso;
        }
        return;
    }
    let (de, ate) = (de.max(xa), ate.min(xb));
    if ate <= de {
        return;
    }
    let i0 = ((de - xa).floor() as usize).min(l - 1);
    let i1 = ((ate - xa).ceil() as usize).min(l);
    if i1 <= i0 + 1 {
        parcial[i0] += (ate - de) as f32 * peso;
        return;
    }
    parcial[i0] += ((xa + i0 as f64 + 1.0) - de) as f32 * peso;
    parcial[i1 - 1] += (ate - (xa + (i1 - 1) as f64)) as f32 * peso;
    if i1 - 1 > i0 + 1 {
        delta[i0 + 1] += peso;
        delta[i1 - 1] -= peso;
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::vetor::edicao::{poligono_em_caminho, retangulo_em_caminho};

    fn soma(s: &Selecao, ret: &Retangulo) -> u64 {
        let mut t = 0;
        for y in ret.y..ret.baixo() {
            for x in ret.x..ret.direita() {
                t += s.valor(x, y) as u64;
            }
        }
        t
    }

    #[test]
    fn o_quadrado_inteiro_cobre_exato() {
        let c = retangulo_em_caminho(10.0, 10.0, 30.0, 20.0);
        let s = rasterizar(&c, 64, 64, &Opcoes::da_mascara());
        assert_eq!(s.valor(10, 10), 255);
        assert_eq!(s.valor(39, 29), 255);
        assert_eq!(s.valor(9, 10), 0);
        assert_eq!(s.valor(40, 10), 0);
        assert_eq!(soma(&s, &Retangulo::inteiro(64, 64)), 30 * 20 * 255);
    }

    #[test]
    fn a_borda_fracionaria_vira_meio_pixel() {
        let c = retangulo_em_caminho(10.5, 10.0, 10.0, 10.0);
        let s = rasterizar(&c, 64, 64, &Opcoes::da_mascara());
        assert!(
            (126..=129).contains(&s.valor(10, 12)),
            "{}",
            s.valor(10, 12)
        );
        assert!((126..=129).contains(&s.valor(20, 12)));
        let sem = rasterizar(&c, 64, 64, &Opcoes::da_selecao(false));
        assert!(matches!(sem.valor(10, 12), 0 | 255));
    }

    #[test]
    fn a_estrela_cheia_ou_vazada_pela_regra() {
        // A estrela de cinco pontas desenhada de uma vez: o miolo dá duas
        // voltas.
        let pontos: Vec<(f64, f64)> = (0..5)
            .map(|k| {
                let a = -std::f64::consts::FRAC_PI_2 + k as f64 * 4.0 * std::f64::consts::PI / 5.0;
                (50.0 + 40.0 * a.cos(), 50.0 + 40.0 * a.sin())
            })
            .collect();
        let mut c = poligono_em_caminho(&pontos);
        let cheia = rasterizar(&c, 100, 100, &Opcoes::da_mascara());
        assert_eq!(cheia.valor(50, 52), 255, "não-zero: o miolo é dentro");
        c.regra = RegraDePreenchimento::ParImpar;
        let vazada = rasterizar(&c, 100, 100, &Opcoes::da_mascara());
        assert_eq!(vazada.valor(50, 52), 0, "par-ímpar: o miolo é fora");
        assert_eq!(vazada.valor(50, 18), 255, "a ponta continua dentro");
    }

    #[test]
    fn as_operacoes_dos_componentes_na_ordem() {
        // Um anel: o de fora soma, o de dentro subtrai.
        let mut c = retangulo_em_caminho(10.0, 10.0, 40.0, 40.0);
        let dentro = retangulo_em_caminho(20.0, 20.0, 20.0, 20.0);
        let mut furo = dentro.subcaminhos[0].clone();
        furo.id = 99;
        furo.operacao = OperacaoDoComponente::Subtrair;
        c.subcaminhos.push(furo.clone());
        let anel = rasterizar(&c, 64, 64, &Opcoes::da_mascara());
        assert_eq!(anel.valor(15, 15), 255);
        assert_eq!(anel.valor(30, 30), 0, "o furo");

        // Intersectar: só o miolo.
        c.subcaminhos[1].operacao = OperacaoDoComponente::Intersectar;
        let meio = rasterizar(&c, 64, 64, &Opcoes::da_mascara());
        assert_eq!(meio.valor(15, 15), 0);
        assert_eq!(meio.valor(30, 30), 255);

        // Excluir sobreposição: o anel de novo, por outra conta.
        c.subcaminhos[1].operacao = OperacaoDoComponente::Excluir;
        let ex = rasterizar(&c, 64, 64, &Opcoes::da_mascara());
        assert_eq!(ex.valor(15, 15), 255);
        assert_eq!(ex.valor(30, 30), 0);

        // Subtrair como primeiro componente parte da foto cheia.
        let mut so_furo = retangulo_em_caminho(20.0, 20.0, 20.0, 20.0);
        so_furo.subcaminhos[0].operacao = OperacaoDoComponente::Subtrair;
        let invertida = rasterizar(&so_furo, 64, 64, &Opcoes::da_mascara());
        assert_eq!(invertida.valor(5, 5), 255);
        assert_eq!(invertida.valor(30, 30), 0);
    }

    #[test]
    fn o_aberto_preenche_com_o_fechamento_virtual_sem_mudar() {
        let mut c = poligono_em_caminho(&[(10.0, 10.0), (50.0, 10.0), (50.0, 50.0)]);
        c.subcaminhos[0].fechado = false;
        let antes = c.clone();
        let s = rasterizar(&c, 64, 64, &Opcoes::da_mascara());
        assert_eq!(s.valor(45, 20), 255, "dentro do triângulo virtual");
        assert_eq!(s.valor(15, 40), 0);
        assert_eq!(c, antes, "o caminho continua aberto");
    }

    #[test]
    fn a_curva_fica_dentro_da_tolerancia() {
        // Um círculo de raio 30 feito de 4 cúbicas: a área rasterizada é a do
        // círculo (com o erro da aproximação cúbica, ~0,03%).
        let c = crate::vetor::edicao::elipse_em_caminho(50.0, 50.0, 30.0, 30.0);
        let s = rasterizar(&c, 100, 100, &Opcoes::da_mascara());
        let area = soma(&s, &Retangulo::inteiro(100, 100)) as f64 / 255.0;
        let esperada = std::f64::consts::PI * 30.0 * 30.0;
        assert!(
            (area - esperada).abs() / esperada < 0.002,
            "{area} × {esperada}"
        );
    }

    #[test]
    fn fora_da_foto_corta_sem_achatar() {
        let c = retangulo_em_caminho(-20.0, -20.0, 40.0, 40.0);
        let s = rasterizar(&c, 64, 64, &Opcoes::da_mascara());
        assert_eq!(s.valor(0, 0), 255);
        assert_eq!(s.valor(19, 19), 255);
        assert_eq!(s.valor(20, 20), 0);
    }
}
