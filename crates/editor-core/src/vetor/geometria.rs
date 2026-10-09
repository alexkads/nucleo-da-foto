//! A conta das curvas: avaliar, derivar, dividir (de Casteljau), achar o
//! ponto mais próximo, aproximar por retas e prender ângulos.
//!
//! O ponto mais próximo e a aproximação adaptativa vêm do `kurbo` (MIT/Apache,
//! já no `Cargo.lock` por causa do `usvg`); a divisão é de Casteljau escrita
//! aqui, porque é ela que garante que inserir uma âncora não muda a curva.

use kurbo::{ParamCurve as _, ParamCurveDeriv as _, ParamCurveNearest as _};

use super::{Caminho, Ponto, RefAncora, Segmento, Subcaminho};

/// O ponto da cúbica em `t`.
pub fn avaliar(p: &[Ponto; 4], t: f64) -> Ponto {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Ponto::novo(
        a * p[0].x + b * p[1].x + c * p[2].x + d * p[3].x,
        a * p[0].y + b * p[1].y + c * p[2].y + d * p[3].y,
    )
}

/// A derivada da cúbica em `t` (a direção da tangente).
pub fn derivada(p: &[Ponto; 4], t: f64) -> (f64, f64) {
    let c = kurbo::CubicBez::new(p[0].kurbo(), p[1].kurbo(), p[2].kurbo(), p[3].kurbo());
    let d = c.deriv().eval(t);
    (d.x, d.y)
}

fn lerp(a: Ponto, b: Ponto, t: f64) -> Ponto {
    Ponto::novo(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

/// De Casteljau: a cúbica dividida em `t` nas duas metades exatas — a
/// primeira de `P0` ao ponto em `t`, a segunda dele a `P3`. Juntas, são a
/// mesma curva.
pub fn dividir(p: &[Ponto; 4], t: f64) -> ([Ponto; 4], [Ponto; 4]) {
    let p01 = lerp(p[0], p[1], t);
    let p12 = lerp(p[1], p[2], t);
    let p23 = lerp(p[2], p[3], t);
    let p012 = lerp(p01, p12, t);
    let p123 = lerp(p12, p23, t);
    let meio = lerp(p012, p123, t);
    ([p[0], p01, p012, meio], [meio, p123, p23, p[3]])
}

/// O ponto mais próximo de `q` no segmento: `(t, distância)`.
pub fn mais_proximo(s: &Segmento, q: Ponto) -> (f64, f64) {
    if s.reto() {
        let (a, b) = (s.p[0], s.p[3]);
        let (dx, dy) = b.menos(a);
        let l2 = dx * dx + dy * dy;
        let t = if l2 <= 0.0 {
            0.0
        } else {
            (((q.x - a.x) * dx + (q.y - a.y) * dy) / l2).clamp(0.0, 1.0)
        };
        return (t, avaliar(&s.p, t).distancia(q));
    }
    let n = s.kurbo().nearest(q.kurbo(), 1e-9);
    (n.t, n.distance_sq.sqrt())
}

/// A curva aproximada por retas, com erro máximo `tolerancia` (na unidade
/// dos pontos): poucas retas onde ela é quase reta, muitas onde dobra. Com
/// `fechar`, o aberto ganha o fechamento virtual. Cada polígono é um
/// subcaminho (o primeiro ponto não se repete no fim).
pub fn achatar(sub: &Subcaminho, tolerancia: f64, fechar: bool) -> Vec<Ponto> {
    let mut pontos = Vec::new();
    if sub.ancoras.is_empty() {
        return pontos;
    }
    let curva = sub.bez_path(fechar);
    kurbo::flatten(curva.iter(), tolerancia.max(1e-4), |el| match el {
        kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) => pontos.push(Ponto::de_kurbo(p)),
        _ => {}
    });
    // O `close_path` repete o primeiro ponto às vezes; não precisa.
    if pontos.len() > 1 && pontos.first() == pontos.last() {
        pontos.pop();
    }
    pontos
}

/// `alvo` preso a múltiplos de 45° em volta de `origem` (o ⇧ da Caneta), à
/// mesma distância projetada.
pub fn em_45(origem: Ponto, alvo: Ponto) -> Ponto {
    let (dx, dy) = alvo.menos(origem);
    let r = dx.hypot(dy);
    if r == 0.0 {
        return alvo;
    }
    let passo = std::f64::consts::FRAC_PI_4;
    let angulo = (dy.atan2(dx) / passo).round() * passo;
    // A projeção na direção presa, como o Photoshop: a âncora fica na reta
    // de 45° mais próxima do ponteiro.
    let (ux, uy) = (angulo.cos(), angulo.sin());
    let l = dx * ux + dy * uy;
    Ponto::novo(origem.x + ux * l, origem.y + uy * l)
}

/// O que está sob o ponteiro, numa consulta de clique.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alvo {
    /// Uma alça (ponto de direção) de uma âncora.
    Alca(RefAncora, super::Lado),
    /// Uma âncora.
    Ancora(RefAncora),
    /// Um segmento: o subcaminho, o índice do segmento e o `t` mais próximo.
    Segmento { sub: u64, indice: usize, t: f64 },
    /// Dentro da área preenchida de um componente (a Seleção de caminho).
    Area { sub: u64 },
}

/// Que alças aparecem (e podem ser pegas): as das âncoras escolhidas e as
/// vizinhas delas, como no Photoshop.
pub struct Visiveis<'a> {
    pub alcas_de: &'a dyn Fn(RefAncora) -> bool,
}

/// O que há em `q`, com a tolerância (em pixels do documento — a da tela
/// dividida pela escala). A prioridade é a das ferramentas de edição:
/// **alças visíveis, depois âncoras, depois segmentos**; a área preenchida
/// só com `area` e só se nada de antes estiver ao alcance — a área nunca
/// esconde um controle.
pub fn alvo_em(
    caminho: &Caminho,
    q: Ponto,
    tolerancia: f64,
    visiveis: &Visiveis<'_>,
    area: bool,
) -> Option<Alvo> {
    // As alças: a mais próxima ao alcance.
    let mut melhor: Option<(f64, Alvo)> = None;
    let considerar = |d: f64, alvo: Alvo, melhor: &mut Option<(f64, Alvo)>| {
        if d <= tolerancia && melhor.as_ref().is_none_or(|(m, _)| d < *m) {
            *melhor = Some((d, alvo));
        }
    };
    for s in &caminho.subcaminhos {
        for a in &s.ancoras {
            let r = RefAncora {
                sub: s.id,
                ancora: a.id,
            };
            if !(visiveis.alcas_de)(r) {
                continue;
            }
            for lado in [super::Lado::Entrada, super::Lado::Saida] {
                if let Some(h) = a.alca(lado) {
                    considerar(h.distancia(q), Alvo::Alca(r, lado), &mut melhor);
                }
            }
        }
    }
    if let Some((_, alvo)) = melhor {
        return Some(alvo);
    }
    for s in &caminho.subcaminhos {
        for a in &s.ancoras {
            let r = RefAncora {
                sub: s.id,
                ancora: a.id,
            };
            considerar(a.ponto.distancia(q), Alvo::Ancora(r), &mut melhor);
        }
    }
    if let Some((_, alvo)) = melhor {
        return Some(alvo);
    }
    for s in &caminho.subcaminhos {
        for (i, seg) in s.segmentos().enumerate() {
            let (t, d) = mais_proximo(&seg, q);
            considerar(
                d,
                Alvo::Segmento {
                    sub: s.id,
                    indice: i,
                    t,
                },
                &mut melhor,
            );
        }
    }
    if let Some((_, alvo)) = melhor {
        return Some(alvo);
    }
    if area {
        // De cima para baixo: o componente desenhado por último fica na
        // frente.
        for s in caminho.subcaminhos.iter().rev() {
            if s.ancoras.len() >= 3 && dentro(s, q, caminho.regra) {
                return Some(Alvo::Area { sub: s.id });
            }
        }
    }
    None
}

/// `q` está dentro da área do componente (sozinho, com a regra), com o
/// fechamento virtual no aberto.
pub fn dentro(s: &Subcaminho, q: Ponto, regra: super::RegraDePreenchimento) -> bool {
    let poligono = achatar(s, 0.1, true);
    let mut voltas = 0i32;
    let n = poligono.len();
    if n < 3 {
        return false;
    }
    for i in 0..n {
        let (a, b) = (poligono[i], poligono[(i + 1) % n]);
        if (a.y <= q.y) != (b.y <= q.y) {
            let x = a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if x > q.x {
                voltas += if b.y > a.y { 1 } else { -1 };
            }
        }
    }
    match regra {
        super::RegraDePreenchimento::NaoZero => voltas != 0,
        super::RegraDePreenchimento::ParImpar => voltas % 2 != 0,
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::vetor::{Ancora, Ligacao, OperacaoDoComponente};

    fn curva() -> [Ponto; 4] {
        [
            Ponto::novo(10.0, 80.0),
            Ponto::novo(30.0, 10.0),
            Ponto::novo(90.0, 0.0),
            Ponto::novo(120.0, 70.0),
        ]
    }

    #[test]
    fn de_casteljau_divide_sem_mudar_a_curva() {
        let p = curva();
        for t in [0.1, 0.37, 0.5, 0.83] {
            let (a, b) = dividir(&p, t);
            assert_eq!(a[3], b[0]);
            assert!(a[3].distancia(avaliar(&p, t)) < 1e-9);
            // Cada ponto das metades está na curva original, no `t` certo.
            for k in 0..=20 {
                let s = k as f64 / 20.0;
                let na_primeira = avaliar(&a, s);
                assert!(na_primeira.distancia(avaliar(&p, s * t)) < 1e-9);
                let na_segunda = avaliar(&b, s);
                assert!(na_segunda.distancia(avaliar(&p, t + s * (1.0 - t))) < 1e-9);
            }
        }
    }

    #[test]
    fn a_derivada_aponta_para_a_alca_nas_pontas() {
        let p = curva();
        let (dx, dy) = derivada(&p, 0.0);
        assert!((dx - 3.0 * (p[1].x - p[0].x)).abs() < 1e-9);
        assert!((dy - 3.0 * (p[1].y - p[0].y)).abs() < 1e-9);
    }

    #[test]
    fn o_ponto_mais_proximo_esta_na_curva() {
        let mut s = Subcaminho::novo(1, OperacaoDoComponente::Somar);
        let p = curva();
        s.ancoras.push(Ancora {
            id: 1,
            ponto: p[0],
            entrada: None,
            saida: Some(p[1]),
            ligacao: Ligacao::Canto,
        });
        s.ancoras.push(Ancora {
            id: 2,
            ponto: p[3],
            entrada: Some(p[2]),
            saida: None,
            ligacao: Ligacao::Canto,
        });
        let seg = s.segmento(0).unwrap();
        let alvo = avaliar(&p, 0.42);
        let (t, d) = mais_proximo(&seg, alvo.mais((0.0, -0.0)));
        assert!((t - 0.42).abs() < 1e-4, "t = {t}");
        assert!(d < 1e-6);
    }

    #[test]
    fn o_45_prende_a_diagonal_e_a_horizontal() {
        let o = Ponto::novo(0.0, 0.0);
        let p = em_45(o, Ponto::novo(10.0, 9.0));
        assert!((p.x - p.y).abs() < 1e-9);
        let h = em_45(o, Ponto::novo(10.0, 1.0));
        assert!(h.y.abs() < 1e-9 && (h.x - 10.0).abs() < 1e-9);
    }
}
