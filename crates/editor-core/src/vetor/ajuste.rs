//! 〰️ O traço à mão vira curva: o ajuste de Béziers cúbicas a pontos
//! digitalizados de Philip J. Schneider ("An Algorithm for Automatically
//! Fitting Digitized Curves", *Graphics Gems*, 1990) — a Caneta de forma
//! livre do Photoshop, com a opção "Ajuste da curva" (o erro máximo).
//!
//! 1. Os pontos repetidos saem e o traço é suavizado de leve (média de três),
//!    para o tremor da mão não virar âncora.
//! 2. Os **cantos** (onde a direção vira mais que [`CANTO_EM_GRAUS`]) partem o
//!    traço: ali a âncora é de canto.
//! 3. Cada trecho recebe uma cúbica por mínimos quadrados com as tangentes
//!    das pontas, parametrizada pelo comprimento; se o erro passa da
//!    tolerância, o parâmetro é refinado por Newton e, não bastando, o trecho
//!    se parte no ponto de maior erro, com a tangente do meio — a emenda fica
//!    lisa (âncora suave).

use super::{Ancora, Ligacao, Ponto};

/// A virada (graus) que faz um canto no traço à mão.
pub const CANTO_EM_GRAUS: f64 = 70.0;

type V = (f64, f64);

fn sub(a: Ponto, b: Ponto) -> V {
    (a.x - b.x, a.y - b.y)
}

fn norm(v: V) -> V {
    let l = v.0.hypot(v.1);
    if l < 1e-12 {
        (0.0, 0.0)
    } else {
        (v.0 / l, v.1 / l)
    }
}

fn dot(a: V, b: V) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

fn bez(p: &[Ponto; 4], t: f64) -> Ponto {
    super::geometria::avaliar(p, t)
}

/// As cúbicas ajustadas aos `pontos` (em pixels do documento), com erro
/// máximo `tolerancia`. Devolve as âncoras do subcaminho (ids 0 — quem chama
/// numera), e se o traço fechou (a ponta voltou ao começo).
pub fn ajustar(pontos: &[Ponto], tolerancia: f64) -> (Vec<Ancora>, bool) {
    let tolerancia = tolerancia.max(0.05);
    // 1. Limpa e suaviza.
    let mut p: Vec<Ponto> = Vec::with_capacity(pontos.len());
    for q in pontos {
        if p.last().is_none_or(|u: &Ponto| u.distancia(*q) > 0.25) {
            p.push(*q);
        }
    }
    if p.len() < 2 {
        let a = p
            .first()
            .map(|q| vec![Ancora::de_canto(0, *q)])
            .unwrap_or_default();
        return (a, false);
    }
    let fechado = p.len() > 3
        && p[0].distancia(*p.last().unwrap()) <= tolerancia.max(3.0)
        && comprimento(&p) > 6.0 * tolerancia.max(3.0);
    if fechado {
        p.pop();
        let primeiro = p[0];
        p.push(primeiro);
    }
    let suave: Vec<Ponto> = (0..p.len())
        .map(|i| {
            if i == 0 || i + 1 == p.len() {
                p[i]
            } else {
                Ponto::novo(
                    (p[i - 1].x + 2.0 * p[i].x + p[i + 1].x) / 4.0,
                    (p[i - 1].y + 2.0 * p[i].y + p[i + 1].y) / 4.0,
                )
            }
        })
        .collect();
    // 2. Os cantos.
    let mut cortes = vec![0usize];
    let janela = 3usize;
    let limite = CANTO_EM_GRAUS.to_radians().cos();
    let mut i = janela;
    while i + janela < suave.len() {
        let a = norm(sub(suave[i], suave[i - janela]));
        let b = norm(sub(suave[i + janela], suave[i]));
        if dot(a, b) < limite {
            // O vértice mais agudo da vizinhança.
            let (mut melhor, mut menor) = (i, f64::MAX);
            for k in i.saturating_sub(janela - 1)..(i + janela).min(suave.len() - janela) {
                if k < janela {
                    continue;
                }
                let d = dot(
                    norm(sub(suave[k], suave[k - 1])),
                    norm(sub(suave[k + 1], suave[k])),
                );
                if d < menor {
                    menor = d;
                    melhor = k;
                }
            }
            if melhor > *cortes.last().unwrap() + 1 {
                cortes.push(melhor);
            }
            i = melhor + janela;
        } else {
            i += 1;
        }
    }
    cortes.push(suave.len() - 1);
    cortes.dedup();
    // 3. Uma sequência de cúbicas por trecho.
    let mut curvas: Vec<([Ponto; 4], bool)> = Vec::new(); // (cúbica, começa num canto)
    for par in cortes.windows(2) {
        let trecho = &suave[par[0]..=par[1]];
        if trecho.len() < 2 {
            continue;
        }
        let t0 = tangente_inicial(trecho);
        let t1 = tangente_final(trecho);
        let mut desta = Vec::new();
        ajustar_trecho(trecho, t0, t1, tolerancia, &mut desta, 0);
        for (k, c) in desta.into_iter().enumerate() {
            curvas.push((c, k == 0));
        }
    }
    // As âncoras: a ponta de cada cúbica, com as alças.
    let mut ancoras: Vec<Ancora> = Vec::new();
    for (k, (c, canto_no_comeco)) in curvas.iter().enumerate() {
        if k == 0 {
            ancoras.push(Ancora {
                id: 0,
                ponto: c[0],
                entrada: None,
                saida: Some(c[1]),
                ligacao: Ligacao::Canto,
                automatica: false,
            });
        } else {
            let a = ancoras.last_mut().unwrap();
            a.saida = Some(c[1]);
            a.ligacao = if *canto_no_comeco {
                Ligacao::Canto
            } else {
                Ligacao::Suave
            };
        }
        ancoras.push(Ancora {
            id: 0,
            ponto: c[3],
            entrada: Some(c[2]),
            saida: None,
            ligacao: Ligacao::Canto,
            automatica: false,
        });
    }
    if fechado && ancoras.len() > 2 {
        // A última âncora é a primeira de novo: a entrada dela vai para a
        // primeira, e o canto do começo/fim decide se a emenda é lisa.
        let ultima = ancoras.pop().unwrap();
        let primeira = &mut ancoras[0];
        primeira.entrada = ultima.entrada;
        let lisa = match (primeira.entrada, primeira.saida) {
            (Some(e), Some(s)) => {
                dot(norm(sub(primeira.ponto, e)), norm(sub(s, primeira.ponto))) > limite
            }
            _ => false,
        };
        primeira.ligacao = if lisa { Ligacao::Suave } else { Ligacao::Canto };
    }
    for a in &mut ancoras {
        a.normalizar();
    }
    (ancoras, fechado)
}

fn comprimento(p: &[Ponto]) -> f64 {
    p.windows(2).map(|w| w[0].distancia(w[1])).sum()
}

fn tangente_inicial(p: &[Ponto]) -> V {
    let k = (p.len() - 1).min(3);
    norm(sub(p[k], p[0]))
}

fn tangente_final(p: &[Ponto]) -> V {
    let n = p.len() - 1;
    let k = n.saturating_sub(3);
    norm(sub(p[k], p[n]))
}

fn ajustar_trecho(
    p: &[Ponto],
    t0: V,
    t1: V,
    tolerancia: f64,
    saida: &mut Vec<[Ponto; 4]>,
    profundidade: u32,
) {
    let n = p.len();
    if n == 2 || profundidade > 24 {
        let d = p[0].distancia(p[n - 1]) / 3.0;
        saida.push([
            p[0],
            p[0].mais((t0.0 * d, t0.1 * d)),
            p[n - 1].mais((t1.0 * d, t1.1 * d)),
            p[n - 1],
        ]);
        return;
    }
    let mut u = parametrizar(p);
    let mut c = gerar(p, &u, t0, t1);
    let (mut erro, mut onde) = maior_erro(p, &c, &u);
    if erro < tolerancia {
        saida.push(c);
        return;
    }
    if erro < tolerancia * 4.0 {
        for _ in 0..8 {
            u = reparametrizar(p, &u, &c);
            c = gerar(p, &u, t0, t1);
            let (e, o) = maior_erro(p, &c, &u);
            erro = e;
            onde = o;
            if erro < tolerancia {
                saida.push(c);
                return;
            }
        }
    }
    let onde = onde.clamp(1, n - 2);
    let centro = norm(sub(p[onde - 1], p[onde + 1]));
    ajustar_trecho(&p[..=onde], t0, centro, tolerancia, saida, profundidade + 1);
    ajustar_trecho(
        &p[onde..],
        (-centro.0, -centro.1),
        t1,
        tolerancia,
        saida,
        profundidade + 1,
    );
}

fn parametrizar(p: &[Ponto]) -> Vec<f64> {
    let mut u = vec![0.0; p.len()];
    for i in 1..p.len() {
        u[i] = u[i - 1] + p[i].distancia(p[i - 1]);
    }
    let total = *u.last().unwrap();
    if total > 0.0 {
        for v in &mut u {
            *v /= total;
        }
    }
    u
}

/// A cúbica por mínimos quadrados com as tangentes das pontas (só os
/// comprimentos das alças são incógnitas).
fn gerar(p: &[Ponto], u: &[f64], t0: V, t1: V) -> [Ponto; 4] {
    let (primeiro, ultimo) = (p[0], p[p.len() - 1]);
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (q, &t) in p.iter().zip(u) {
        let s = 1.0 - t;
        let (b0, b1, b2, b3) = (s * s * s, 3.0 * t * s * s, 3.0 * t * t * s, t * t * t);
        let a0 = (t0.0 * b1, t0.1 * b1);
        let a1 = (t1.0 * b2, t1.1 * b2);
        c00 += dot(a0, a0);
        c01 += dot(a0, a1);
        c11 += dot(a1, a1);
        let tmp = (
            q.x - (primeiro.x * (b0 + b1) + ultimo.x * (b2 + b3)),
            q.y - (primeiro.y * (b0 + b1) + ultimo.y * (b2 + b3)),
        );
        x0 += dot(a0, tmp);
        x1 += dot(a1, tmp);
    }
    let det = c00 * c11 - c01 * c01;
    let (mut alfa0, mut alfa1) = if det.abs() > 1e-12 {
        ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det)
    } else {
        (0.0, 0.0)
    };
    let dist = primeiro.distancia(ultimo);
    let epsilon = 1e-6 * dist;
    if alfa0 < epsilon || alfa1 < epsilon {
        alfa0 = dist / 3.0;
        alfa1 = dist / 3.0;
    }
    [
        primeiro,
        primeiro.mais((t0.0 * alfa0, t0.1 * alfa0)),
        ultimo.mais((t1.0 * alfa1, t1.1 * alfa1)),
        ultimo,
    ]
}

fn maior_erro(p: &[Ponto], c: &[Ponto; 4], u: &[f64]) -> (f64, usize) {
    let mut maior = 0.0;
    let mut onde = p.len() / 2;
    for i in 1..p.len() - 1 {
        let d = bez(c, u[i]).distancia(p[i]);
        if d > maior {
            maior = d;
            onde = i;
        }
    }
    (maior, onde)
}

/// Um passo de Newton em cada parâmetro: o ponto da curva mais perto do
/// ponto digitalizado.
fn reparametrizar(p: &[Ponto], u: &[f64], c: &[Ponto; 4]) -> Vec<f64> {
    let d1 = [
        Ponto::novo(3.0 * (c[1].x - c[0].x), 3.0 * (c[1].y - c[0].y)),
        Ponto::novo(3.0 * (c[2].x - c[1].x), 3.0 * (c[2].y - c[1].y)),
        Ponto::novo(3.0 * (c[3].x - c[2].x), 3.0 * (c[3].y - c[2].y)),
    ];
    let d2 = [
        Ponto::novo(2.0 * (d1[1].x - d1[0].x), 2.0 * (d1[1].y - d1[0].y)),
        Ponto::novo(2.0 * (d1[2].x - d1[1].x), 2.0 * (d1[2].y - d1[1].y)),
    ];
    let quad = |q: &[Ponto; 3], t: f64| {
        let s = 1.0 - t;
        Ponto::novo(
            s * s * q[0].x + 2.0 * s * t * q[1].x + t * t * q[2].x,
            s * s * q[0].y + 2.0 * s * t * q[1].y + t * t * q[2].y,
        )
    };
    let lin = |q: &[Ponto; 2], t: f64| {
        Ponto::novo(
            q[0].x + (q[1].x - q[0].x) * t,
            q[0].y + (q[1].y - q[0].y) * t,
        )
    };
    p.iter()
        .zip(u)
        .map(|(q, &t)| {
            let b = bez(c, t);
            let b1 = quad(&d1, t);
            let b2 = lin(&d2, t);
            let num = (b.x - q.x) * b1.x + (b.y - q.y) * b1.y;
            let den = b1.x * b1.x + b1.y * b1.y + (b.x - q.x) * b2.x + (b.y - q.y) * b2.y;
            if den.abs() < 1e-12 {
                t
            } else {
                (t - num / den).clamp(0.0, 1.0)
            }
        })
        .collect()
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::vetor::{OperacaoDoComponente, Subcaminho};

    fn subcaminho(ancoras: Vec<Ancora>, fechado: bool) -> Subcaminho {
        let mut s = Subcaminho::novo(1, OperacaoDoComponente::Somar);
        for (i, mut a) in ancoras.into_iter().enumerate() {
            a.id = i as u64 + 1;
            s.ancoras.push(a);
        }
        s.fechado = fechado;
        s
    }

    fn distancia_ao_subcaminho(s: &Subcaminho, q: Ponto) -> f64 {
        s.segmentos()
            .map(|g| crate::vetor::geometria::mais_proximo(&g, q).1)
            .fold(f64::MAX, f64::min)
    }

    #[test]
    fn um_arco_a_mao_vira_poucas_curvas_dentro_da_tolerancia() {
        // Meio círculo de raio 100, com tremor de ±0,4 px.
        let pontos: Vec<Ponto> = (0..=200)
            .map(|i| {
                let a = std::f64::consts::PI * i as f64 / 200.0;
                let tremor = if i % 2 == 0 { 0.4 } else { -0.4 };
                Ponto::novo(
                    200.0 + (100.0 + tremor) * a.cos(),
                    200.0 - (100.0 + tremor) * a.sin(),
                )
            })
            .collect();
        let (ancoras, fechado) = ajustar(&pontos, 2.0);
        assert!(!fechado);
        assert!(ancoras.len() <= 4, "{} âncoras", ancoras.len());
        let s = subcaminho(ancoras, false);
        for q in pontos.iter().step_by(5) {
            assert!(distancia_ao_subcaminho(&s, *q) < 2.5, "{q:?}");
        }
        // As emendas do meio são lisas.
        for a in &s.ancoras[1..s.ancoras.len() - 1] {
            assert_eq!(a.ligacao, Ligacao::Suave);
            assert!(a.alcas_colineares());
        }
    }

    #[test]
    fn o_canto_do_traco_vira_ancora_de_canto() {
        let mut pontos = Vec::new();
        for i in 0..=50 {
            pontos.push(Ponto::novo(i as f64 * 2.0, 0.0));
        }
        for i in 1..=50 {
            pontos.push(Ponto::novo(100.0, i as f64 * 2.0));
        }
        let (ancoras, _) = ajustar(&pontos, 1.0);
        let canto = ancoras
            .iter()
            .find(|a| a.ponto.distancia(Ponto::novo(100.0, 0.0)) < 3.0)
            .expect("uma âncora no canto");
        assert_eq!(canto.ligacao, Ligacao::Canto);
        let s = subcaminho(ancoras.clone(), false);
        assert!(distancia_ao_subcaminho(&s, Ponto::novo(50.0, 0.0)) < 1.0);
        assert!(distancia_ao_subcaminho(&s, Ponto::novo(100.0, 50.0)) < 1.0);
    }

    #[test]
    fn voltar_ao_comeco_fecha() {
        let pontos: Vec<Ponto> = (0..=120)
            .map(|i| {
                let a = 2.0 * std::f64::consts::PI * i as f64 / 120.0;
                Ponto::novo(100.0 + 50.0 * a.cos(), 100.0 + 50.0 * a.sin())
            })
            .collect();
        let (ancoras, fechado) = ajustar(&pontos, 1.0);
        assert!(fechado);
        let s = subcaminho(ancoras, true);
        assert!(distancia_ao_subcaminho(&s, Ponto::novo(150.0, 100.0)) < 1.5);
        assert!(distancia_ao_subcaminho(&s, Ponto::novo(100.0, 150.0)) < 1.5);
    }
}
