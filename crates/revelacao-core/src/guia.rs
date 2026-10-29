//! A guia: a foto reduzida e desfocada que a Claridade, a Textura e o Remover
//! névoa leem.
//!
//! 🔑 **Os três controles do Lightroom que olham a vizinhança larga.** O shader
//! revela um pixel por vez e só alcança a vizinhança 5×5 (ruído e nitidez); o
//! contraste local da Claridade mede dezenas de pixels, e o Remover névoa
//! precisa do "canal escuro" de uma janela inteira e da luz do céu da foto. Os
//! dois saem daqui, calculados uma vez por foto na CPU.
//!
//! ## Os quatro canais
//!
//! | canal | o quê | quem lê |
//! |---|---|---|
//! | `r` | a luminância desfocada larga (σ = 1,2 % do lado maior) | Claridade |
//! | `g` | a luminância desfocada média (σ = 0,25 %) | Textura |
//! | `b` | o canal escuro (mínimo dos três canais numa janela de 0,8 %), suavizado | Remover névoa |
//! | `a` | a luminância reduzida, sem desfoque | — (reserva) |
//!
//! 🔑 **Os raios são frações do lado maior, e não pixels.** A Revelação mostra
//! uma cópia de 2560 px e a exportação revela os 6000 do arquivo: com raio em
//! pixels a Claridade da tela seria outra que a do JPEG. Pela fração, a guia de
//! uma e de outra cobre a mesma parte da cena.
//!
//! ⚠️ **A guia sai dos pixels de entrada, e não do retoque.** Um carimbo ou um
//! band-aid mudam um ponto da foto que a guia ainda vê como era; com o desfoque
//! largo da Claridade a diferença não aparece, e na Textura ela fica no tamanho
//! do retoque. Refazer a guia a cada retoque custaria a CPU inteira de novo.

use crate::transformacao::por_linhas;

/// O lado maior da guia, em pixels. A Textura (σ = 0,25 %) ainda tem 2,5
/// pixels de raio nesse tamanho — abaixo disso o desfoque vira serrilhado.
pub const LADO_DA_GUIA: u32 = 1024;

/// σ do desfoque da Claridade, em fração do lado maior.
pub const SIGMA_DA_CLARIDADE: f32 = 0.012;
/// σ do desfoque da Textura, em fração do lado maior.
pub const SIGMA_DA_TEXTURA: f32 = 0.0025;
/// Raio da janela do canal escuro, em fração do lado maior.
pub const JANELA_DA_NEVOA: f32 = 0.008;

/// A guia pronta para subir: `rgba` em `f32`, linha a linha.
#[derive(Debug, Clone)]
pub struct Guia {
    /// Largura da guia, em texels: a da foto dividida pelo fator inteiro de
    /// redução, com o lado maior até [`LADO_DA_GUIA`].
    pub largura: u32,
    /// Altura da guia, em texels, no mesmo fator da largura.
    pub altura: u32,
    /// Os texels, quatro `f32` cada (`r`, `g`, `b`, `a` da tabela do módulo,
    /// todos em 0–1), linha a linha de cima para baixo:
    /// `largura × altura × 4` valores.
    pub texels: Vec<f32>,
    /// A "luz atmosférica" do Remover névoa, em 0–1: a luminância média do
    /// 0,1 % de pontos mais enevoados (os de canal escuro mais alto).
    pub luz_do_ceu: f32,
}

/// Calcula a guia de uma foto RGBA de 8 bits.
pub fn calcular(pixels: &[u8], largura: u32, altura: u32) -> Guia {
    let (w, h) = (largura.max(1) as usize, altura.max(1) as usize);
    let maior = w.max(h);
    // Redução por média de área com fator inteiro: cada texel da guia é a
    // média de um bloco `f×f` (o último bloco de cada eixo pode ser menor).
    let f = maior.div_ceil(LADO_DA_GUIA as usize).max(1);
    let (gw, gh) = (w.div_ceil(f), h.div_ceil(f));
    // 🔑 Uma linha da guia por vez, com as linhas divididas entre os núcleos:
    // cada texel soma os pixels do seu bloco na ordem de sempre (linha a
    // linha, da esquerda para a direita) — o mesmo `f32` —, sem a divisão
    // `x / f` a cada pixel. A luminância e o mínimo andam juntos.
    let mut reduzida = vec![[0f32; 2]; gw * gh];
    por_linhas(&mut reduzida, gw, |gy, somas| {
        let (ya, yb) = (gy as usize * f, ((gy as usize + 1) * f).min(h));
        for y in ya..yb {
            let linha = &pixels[y * w * 4..(y + 1) * w * 4];
            for (soma, bloco) in somas.iter_mut().zip(linha.chunks(f * 4)) {
                for p in bloco.as_chunks::<4>().0 {
                    let (r, g, b) = (p[0] as f32, p[1] as f32, p[2] as f32);
                    soma[0] += r + g + b;
                    soma[1] += r.min(g).min(b);
                }
            }
        }
        for (gx, soma) in somas.iter_mut().enumerate() {
            let conta = ((yb - ya) * f.min(w - gx * f)) as u32;
            let n = conta.max(1) as f32;
            *soma = [soma[0] / (n * 3.0 * 255.0), soma[1] / (n * 255.0)];
        }
    });
    let lum: Vec<f32> = reduzida.iter().map(|t| t[0]).collect();
    let minimo: Vec<f32> = reduzida.iter().map(|t| t[1]).collect();

    let lado = gw.max(gh) as f32;
    let largo = desfocar(&lum, gw, gh, SIGMA_DA_CLARIDADE * lado);
    let medio = desfocar(&lum, gw, gh, SIGMA_DA_TEXTURA * lado);

    // O canal escuro: o mínimo numa janela quadrada, e depois uma caixa do
    // mesmo raio para tirar os degraus da janela (o "refinamento" barato do
    // He et al.; o filtro guiado inteiro não paga o que custa aqui).
    let raio = ((JANELA_DA_NEVOA * lado).round() as usize).max(1);
    let escuro = minimo_em_janela(&minimo, gw, gh, raio);
    let escuro = caixa(&caixa(&escuro, gw, gh, raio, true), gw, gh, raio, false);

    let luz_do_ceu = luz_atmosferica(&escuro, &lum);

    let mut texels = Vec::with_capacity(gw * gh * 4);
    for i in 0..gw * gh {
        texels.extend_from_slice(&[largo[i], medio[i], escuro[i], lum[i]]);
    }
    Guia {
        largura: gw as u32,
        altura: gh as u32,
        texels,
        luz_do_ceu,
    }
}

/// A luminância média dos pontos de canal escuro mais alto (o 0,1 % de cima,
/// e no mínimo um), presa em 0,5–1: abaixo disso a foto não tem névoa que
/// valha estimar, e a conta do Remover névoa explodiria nas sombras.
fn luz_atmosferica(escuro: &[f32], lum: &[f32]) -> f32 {
    let mut ordem: Vec<usize> = (0..escuro.len()).collect();
    let quantos = (escuro.len() / 1000).max(1);
    ordem.select_nth_unstable_by(quantos - 1, |a, b| escuro[*b].total_cmp(&escuro[*a]));
    let soma: f32 = ordem[..quantos].iter().map(|i| lum[*i]).sum();
    (soma / quantos as f32).clamp(0.5, 1.0)
}

/// Gaussiana aproximada por três caixas (o desvio de três caixas de largura
/// `2r+1` é `sqrt(r(r+1))`).
fn desfocar(dados: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    // Pelo menos um texel: numa foto minúscula o σ arredonda para zero, e a
    // "média da vizinhança" seria o próprio pixel — contraste local nenhum.
    let r = ((((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round() as usize).max(1);
    let mut saida = dados.to_vec();
    for _ in 0..3 {
        saida = caixa(&saida, w, h, r, true);
        saida = caixa(&saida, w, h, r, false);
    }
    saida
}

/// Média numa janela `2r+1` ao longo de um eixo, com a borda repetida.
///
/// 🔑 Cada linha (ou coluna) é a soma corrida de sempre, na mesma ordem — o
/// mesmo `f32`. Na horizontal, as linhas vão para os núcleos; na vertical, as
/// colunas andam juntas, linha a linha, com uma soma para cada: a memória é
/// lida em ordem, e não saltando uma linha inteira a cada valor.
fn caixa(dados: &[f32], w: usize, h: usize, r: usize, horizontal: bool) -> Vec<f32> {
    let mut saida = vec![0f32; dados.len()];
    let largura = (2 * r + 1) as f32;
    let janela = -(r as isize)..=r as isize;
    if horizontal {
        por_linhas(&mut saida, w, |y, saida| {
            let linha = &dados[y as usize * w..(y as usize + 1) * w];
            let ler = |i: isize| linha[i.clamp(0, w as isize - 1) as usize];
            let mut soma: f32 = janela.clone().map(ler).sum();
            for (i, s) in saida.iter_mut().enumerate() {
                *s = soma / largura;
                soma += ler(i as isize + r as isize + 1) - ler(i as isize - r as isize);
            }
        });
        return saida;
    }
    let linha = |i: isize| {
        let i = i.clamp(0, h as isize - 1) as usize;
        &dados[i * w..(i + 1) * w]
    };
    let mut somas: Vec<f32> = (0..w)
        .map(|x| janela.clone().map(|i| linha(i)[x]).sum())
        .collect();
    for (i, saida) in saida.chunks_exact_mut(w).enumerate() {
        let (entra, sai) = (
            linha(i as isize + r as isize + 1),
            linha(i as isize - r as isize),
        );
        for ((soma, s), (e, a)) in somas.iter_mut().zip(saida).zip(entra.iter().zip(sai)) {
            *s = *soma / largura;
            *soma += e - a;
        }
    }
    saida
}

/// O mínimo numa janela quadrada `2r+1` — separável, um eixo de cada vez, as
/// linhas divididas entre os núcleos. Na vertical a janela é de linhas
/// inteiras: o mesmo mínimo de cada coluna, lido em ordem.
fn minimo_em_janela(dados: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut horizontal = vec![0f32; dados.len()];
    por_linhas(&mut horizontal, w, |y, saida| {
        let linha = &dados[y as usize * w..(y as usize + 1) * w];
        for (x, s) in saida.iter_mut().enumerate() {
            let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
            *s = linha[a..=b].iter().copied().fold(f32::MAX, f32::min);
        }
    });
    let mut saida = vec![0f32; dados.len()];
    por_linhas(&mut saida, w, |y, saida| {
        let y = y as usize;
        let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
        saida.fill(f32::MAX);
        for yy in a..=b {
            for (s, v) in saida.iter_mut().zip(&horizontal[yy * w..(yy + 1) * w]) {
                *s = s.min(*v);
            }
        }
    });
    saida
}

#[cfg(test)]
mod testes {
    use super::*;

    /// A guia de antes de 2026-10-22: a redução numa passada só, com `x / f`
    /// por pixel, e a caixa e o mínimo verticais coluna a coluna.
    mod antes {
        use super::super::{
            luz_atmosferica, Guia, JANELA_DA_NEVOA, LADO_DA_GUIA, SIGMA_DA_CLARIDADE,
            SIGMA_DA_TEXTURA,
        };

        pub fn calcular(pixels: &[u8], largura: u32, altura: u32) -> Guia {
            let (w, h) = (largura.max(1) as usize, altura.max(1) as usize);
            let maior = w.max(h);
            let f = maior.div_ceil(LADO_DA_GUIA as usize).max(1);
            let (gw, gh) = (w.div_ceil(f), h.div_ceil(f));
            let mut soma_l = vec![0f32; gw * gh];
            let mut soma_min = vec![0f32; gw * gh];
            let mut conta = vec![0u32; gw * gh];
            for y in 0..h {
                let gy = y / f;
                let linha = &pixels[y * w * 4..(y + 1) * w * 4];
                for x in 0..w {
                    let p = &linha[x * 4..x * 4 + 4];
                    let (r, g, b) = (p[0] as f32, p[1] as f32, p[2] as f32);
                    let i = gy * gw + x / f;
                    soma_l[i] += r + g + b;
                    soma_min[i] += r.min(g).min(b);
                    conta[i] += 1;
                }
            }
            let mut lum = vec![0f32; gw * gh];
            let mut minimo = vec![0f32; gw * gh];
            for i in 0..gw * gh {
                let n = conta[i].max(1) as f32;
                lum[i] = soma_l[i] / (n * 3.0 * 255.0);
                minimo[i] = soma_min[i] / (n * 255.0);
            }
            let lado = gw.max(gh) as f32;
            let largo = desfocar(&lum, gw, gh, SIGMA_DA_CLARIDADE * lado);
            let medio = desfocar(&lum, gw, gh, SIGMA_DA_TEXTURA * lado);
            let raio = ((JANELA_DA_NEVOA * lado).round() as usize).max(1);
            let escuro = minimo_em_janela(&minimo, gw, gh, raio);
            let escuro = caixa(&caixa(&escuro, gw, gh, raio, true), gw, gh, raio, false);
            let luz_do_ceu = luz_atmosferica(&escuro, &lum);
            let mut texels = Vec::with_capacity(gw * gh * 4);
            for i in 0..gw * gh {
                texels.extend_from_slice(&[largo[i], medio[i], escuro[i], lum[i]]);
            }
            Guia {
                largura: gw as u32,
                altura: gh as u32,
                texels,
                luz_do_ceu,
            }
        }

        fn desfocar(dados: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
            let r =
                ((((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round() as usize).max(1);
            let mut saida = dados.to_vec();
            for _ in 0..3 {
                saida = caixa(&saida, w, h, r, true);
                saida = caixa(&saida, w, h, r, false);
            }
            saida
        }

        pub fn caixa(dados: &[f32], w: usize, h: usize, r: usize, horizontal: bool) -> Vec<f32> {
            let (n, linhas) = if horizontal { (w, h) } else { (h, w) };
            let indice = |linha: usize, i: usize| {
                if horizontal {
                    linha * w + i
                } else {
                    i * w + linha
                }
            };
            let mut saida = vec![0f32; dados.len()];
            let largura = (2 * r + 1) as f32;
            for linha in 0..linhas {
                let ler = |i: isize| dados[indice(linha, i.clamp(0, n as isize - 1) as usize)];
                let mut soma: f32 = (-(r as isize)..=r as isize).map(ler).sum();
                for i in 0..n {
                    saida[indice(linha, i)] = soma / largura;
                    soma += ler(i as isize + r as isize + 1) - ler(i as isize - r as isize);
                }
            }
            saida
        }

        pub fn minimo_em_janela(dados: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
            let mut horizontal = vec![0f32; dados.len()];
            for y in 0..h {
                for x in 0..w {
                    let (a, b) = (x.saturating_sub(r), (x + r).min(w - 1));
                    horizontal[y * w + x] = dados[y * w + a..=y * w + b]
                        .iter()
                        .copied()
                        .fold(f32::MAX, f32::min);
                }
            }
            let mut saida = vec![0f32; dados.len()];
            for x in 0..w {
                for y in 0..h {
                    let (a, b) = (y.saturating_sub(r), (y + r).min(h - 1));
                    saida[y * w + x] = (a..=b)
                        .map(|yy| horizontal[yy * w + x])
                        .fold(f32::MAX, f32::min);
                }
            }
            saida
        }
    }

    /// Bytes que variam sem padrão (xorshift), para as somas arredondarem de
    /// todo jeito.
    fn ruido(n: usize, mut estado: u32) -> Vec<u8> {
        (0..n)
            .map(|_| {
                estado ^= estado << 13;
                estado ^= estado >> 17;
                estado ^= estado << 5;
                (estado >> 24) as u8
            })
            .collect()
    }

    fn bits(v: &[f32]) -> Vec<u32> {
        v.iter().map(|x| x.to_bits()).collect()
    }

    /// 🚨 **A guia é a de antes, bit a bit**: sem redução, com fator 2 e 3 e
    /// o último bloco menor nos dois eixos, em pé e deitada, minúscula e de um
    /// pixel — a textura, a luminância, o canal escuro e a luz do céu.
    #[test]
    fn a_guia_em_faixas_e_a_de_antes() {
        for (k, (w, h)) in [
            (300u32, 200u32),
            (1025, 3),
            (3, 1025),
            (2049, 700),
            (640, 2051),
            (7, 5),
            (1, 1),
        ]
        .into_iter()
        .enumerate()
        {
            let pixels = ruido((w * h * 4) as usize, 0x9e37_79b9 ^ k as u32);
            let novo = calcular(&pixels, w, h);
            let antes = antes::calcular(&pixels, w, h);
            assert_eq!((novo.largura, novo.altura), (antes.largura, antes.altura));
            assert!(bits(&novo.texels) == bits(&antes.texels), "{w}×{h}");
            assert_eq!(novo.luz_do_ceu.to_bits(), antes.luz_do_ceu.to_bits());
        }
    }

    /// A caixa e o mínimo, cada um sozinho, com raios que passam do tamanho
    /// (a borda repetida dos dois lados) e linhas e colunas de um valor só.
    #[test]
    fn a_caixa_e_o_minimo_sao_os_de_antes() {
        for (w, h) in [(1usize, 1usize), (1, 9), (9, 1), (7, 5), (64, 33)] {
            let dados: Vec<f32> = ruido(w * h, (w * 31 + h) as u32)
                .into_iter()
                .map(|v| v as f32 / 255.0)
                .collect();
            for r in [1, 2, 4, 10, 70] {
                for horizontal in [true, false] {
                    assert!(
                        bits(&caixa(&dados, w, h, r, horizontal))
                            == bits(&antes::caixa(&dados, w, h, r, horizontal)),
                        "caixa {w}×{h} r {r} {horizontal}"
                    );
                }
                assert!(
                    bits(&minimo_em_janela(&dados, w, h, r))
                        == bits(&antes::minimo_em_janela(&dados, w, h, r)),
                    "mínimo {w}×{h} r {r}"
                );
            }
        }
    }

    fn liso(w: u32, h: u32, v: u8) -> Vec<u8> {
        [v, v, v, 255].repeat((w * h) as usize)
    }

    /// Foto lisa: a guia é a própria cor, sem borda nem degrau — e a Claridade
    /// e a Textura, que somam `pixel - guia`, não mexem em nada.
    #[test]
    fn foto_lisa_da_guia_lisa() {
        let g = calcular(&liso(300, 200, 128), 300, 200);
        assert_eq!((g.largura, g.altura), (300, 200));
        for t in g.texels.chunks(4) {
            for c in t {
                assert!((c - 128.0 / 255.0).abs() < 1e-4, "{c}");
            }
        }
    }

    /// A guia não passa de `LADO_DA_GUIA`, e guarda a proporção.
    #[test]
    fn a_guia_reduz_pelo_lado_maior() {
        let g = calcular(&liso(3000, 2000, 10), 3000, 2000);
        assert_eq!((g.largura, g.altura), (1000, 667));
        assert_eq!(g.texels.len(), 1000 * 667 * 4);
    }

    /// O canal escuro de uma foto colorida é o canal mais fraco: um vermelho
    /// puro não tem névoa nenhuma.
    #[test]
    fn o_canal_escuro_e_o_minimo_dos_tres() {
        let px = [250u8, 40, 30, 255].repeat(100 * 100);
        let g = calcular(&px, 100, 100);
        let b = g.texels[2];
        assert!((b - 30.0 / 255.0).abs() < 1e-3, "{b}");
    }

    /// A luz do céu sai dos pontos mais enevoados, e fica entre 0,5 e 1.
    #[test]
    fn a_luz_do_ceu_vem_da_regiao_mais_clara_e_lavada() {
        let (w, h) = (200u32, 100u32);
        let mut px = liso(w, h, 40);
        // Metade de cima: céu branco-acinzentado.
        for y in 0..50 {
            for x in 0..w as usize {
                let i = (y * w as usize + x) * 4;
                px[i..i + 3].copy_from_slice(&[220, 225, 230]);
            }
        }
        let g = calcular(&px, w, h);
        assert!(
            (g.luz_do_ceu - 225.0 / 255.0).abs() < 0.01,
            "{}",
            g.luz_do_ceu
        );
        let escura = calcular(&liso(w, h, 20), w, h);
        assert_eq!(escura.luz_do_ceu, 0.5);
    }
}
