//! O relógio da bancada: antes e depois alternados, a mediana de cada um.

use std::time::Instant;

/// Roda `vezes` rodadas de **antes e depois alternados** — a máquina que
/// esquenta ou se ocupa no meio pesa nos dois lados por igual — e devolve a
/// mediana de cada um, em milissegundos, com a saída da última rodada.
///
/// `preparo_*` roda fora do relógio: é o que o caminho de verdade já teria na
/// mão (a revelada que a GPU acabou de devolver, por exemplo).
pub fn comparar<PA, SA, PB, SB>(
    vezes: usize,
    mut preparo_antes: impl FnMut() -> PA,
    mut antes: impl FnMut(PA) -> SA,
    mut preparo_depois: impl FnMut() -> PB,
    mut depois: impl FnMut(PB) -> SB,
) -> (f64, f64, SA, SB) {
    let vezes = vezes.max(1);
    let mut tempos_antes = Vec::with_capacity(vezes);
    let mut tempos_depois = Vec::with_capacity(vezes);
    let mut saida_antes = None;
    let mut saida_depois = None;
    for _ in 0..vezes {
        let p = preparo_antes();
        let comeco = Instant::now();
        let s = antes(p);
        tempos_antes.push(comeco.elapsed().as_secs_f64() * 1000.0);
        saida_antes = Some(s);

        let p = preparo_depois();
        let comeco = Instant::now();
        let s = depois(p);
        tempos_depois.push(comeco.elapsed().as_secs_f64() * 1000.0);
        saida_depois = Some(s);
    }
    (
        mediana(tempos_antes),
        mediana(tempos_depois),
        saida_antes.expect("rodou pelo menos uma vez"),
        saida_depois.expect("rodou pelo menos uma vez"),
    )
}

/// A mediana; com um número par de valores, o de cima do meio.
fn mediana(mut valores: Vec<f64>) -> f64 {
    valores.sort_by(f64::total_cmp);
    valores[valores.len() / 2]
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn alterna_e_devolve_a_ultima_saida_de_cada_lado() {
        let mut ordem = Vec::new();
        let ordem_ref = std::cell::RefCell::new(&mut ordem);
        let mut k = 0;
        let (_, _, a, b) = comparar(
            3,
            || (),
            |_| {
                ordem_ref.borrow_mut().push('a');
                k += 1;
                k
            },
            || (),
            |_| {
                ordem_ref.borrow_mut().push('d');
                "depois"
            },
        );
        assert_eq!(a, 3);
        assert_eq!(b, "depois");
        assert_eq!(ordem.iter().collect::<String>(), "adadad");
    }

    #[test]
    fn a_mediana_ignora_o_ponto_fora() {
        assert_eq!(mediana(vec![5.0, 1.0, 900.0]), 5.0);
        assert_eq!(mediana(vec![2.0]), 2.0);
    }
}
