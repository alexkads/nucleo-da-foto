//! O Pincel de recuperação com origem manual (o *Healing Brush* do Photoshop):
//! a **textura** vem da origem escolhida com ⌥ + clique, e a **cor e a luz**
//! se adaptam às do destino em volta do traço.
//!
//! ## A conta
//!
//! É a do artigo que descreve a ferramenta da Adobe (T. Georgiev, *Photoshop
//! Healing Brush: a Tool for Seamless Cloning*, 2004), uma variante
//! multiplicativa da edição de Poisson (Pérez, Gangnet e Blake, 2003). Para
//! cada canal, com `S` a origem (o que o carimbo copiaria) e `D` o destino
//! (a foto no próprio lugar, antes do traço):
//!
//! ```text
//! dentro do traço (Ω):   Δh = 0                       (h harmônica)
//! na borda (∂Ω):         h  = (D + ε) / (S + ε)       (suavizado ao longo da borda)
//! resultado:             R  = (S + ε) · h − ε       (ε = 50, h em [¼, 4])
//! ```
//!
//! `h` é o **fator de correção** mais liso que existe (a membrana de menor
//! energia) que leva a origem à cor do destino na borda. Como ele multiplica
//! a origem, o contraste da textura acompanha a luz: um poro tirado de uma
//! pele mais clara fica com a força certa numa pele mais escura — o que uma
//! correção aditiva não faz. `ε` (50 em 255) puxa a conta para o aditivo nos
//! escuros — uma origem escura sobre pele clara não explode em brilho — e o
//! fator fica entre ¼ e 4.
//!
//! **Difusão** (1 a 7, o controle da barra — não é a difusão da seleção): o
//! quanto o fator da borda é suavizado **ao longo dela** (σ de 0 a 4,8 px),
//! só com os pixels de fora do traço. Baixa (1), a cor casa pixel a pixel com
//! a borda (bom para grão e textura fina, mas o ruído da borda entra); alta,
//! casa com a média de cada trecho da borda (transição mais lisa). O miolo —
//! o que está sendo tirado — nunca entra na conta.
//!
//! A equação é resolvida por **SOR** de grosso a fino (pirâmide de 2×): cada
//! nível parte da solução do nível de baixo, então regiões largas convergem
//! sem milhares de iterações.
//!
//! ## Limites conhecidos
//!
//! - A adaptação é a da borda inteira do traço: se a borda cruza uma
//!   aresta forte (a sombra da mandíbula), a cor escura entra no traço como
//!   um degradê — passe o pincel sem tocar a aresta, ou faça uma seleção que
//!   a deixe de fora (a borda da seleção também é borda do traço).
//! - O resultado sai no soltar do botão; durante o traço a tela mostra a
//!   cópia crua da origem (como o Photoshop).
//! - Traços maiores que [`LIMITE_DE_PIXELS`] ficam só com a cópia (a memória da
//!   solução cresce com a área).

/// O maior retângulo (em pixels) que a recuperação resolve num traço.
pub const LIMITE_DE_PIXELS: usize = 8_000_000;

/// O `ε` da conta multiplicativa, em 0..=255. 🔑 Não é só para não dividir
/// por zero: `R = (S + ε)·h − ε` vai do multiplicativo puro (`ε = 0`) ao aditivo
/// (`ε → ∞`). Com 10 (a primeira versão), uma origem escura (sobrancelha,
/// pupila) sobre pele clara dava `h ≈ 7` e um brilho alaranjado no traço
/// (visto na foto real, 07/out/2026); com 50, a textura de pele em luz
/// parecida continua multiplicativa e o escuro deixa de explodir.
const EPSILON: f32 = 50.0;

/// O fator de correção fica em `[1/4, 4]`: uma borda de destino muito
/// diferente da origem não amplifica a textura sem limite.
const FATOR_MAXIMO: f32 = 4.0;

/// A cor adaptada de cada pixel livre do retângulo `w × h`.
///
/// - `origem`: `S`, a cor que o carimbo copiaria em cada pixel;
/// - `destino`: `D`, a foto no lugar antes do traço;
/// - `livre[k]`: o pixel está no traço (Ω). A borda do retângulo nunca é livre.
///
/// Devolve `R` para todos os pixels (fora de Ω, o próprio destino).
pub fn adaptar(
    origem: &[[f32; 3]],
    destino: &[[f32; 3]],
    livre: &[bool],
    w: usize,
    h: usize,
    difusao: u8,
) -> Vec<[f32; 3]> {
    debug_assert_eq!(origem.len(), w * h);
    // O fator exato em cada pixel fixo.
    let razao = |k: usize| -> [f32; 3] {
        let mut f = [1.0; 3];
        for c in 0..3 {
            f[c] = ((destino[k][c] + EPSILON) / (origem[k][c] + EPSILON))
                .clamp(1.0 / FATOR_MAXIMO, FATOR_MAXIMO);
        }
        f
    };
    let mut fator: Vec<[f32; 3]> = (0..w * h).map(razao).collect();
    // 🔑 A difusão suaviza o fator **ao longo da borda**, só com os pixels de
    // fora (o anel de até 2 px em volta do traço): o miolo — a mancha que
    // está saindo — nunca entra na conta da borda. A primeira versão
    // suavizava destino e origem atravessando a borda, e a cor da mancha
    // voltava como emenda no Remendo (borda dura).
    let sigma = (difusao.clamp(1, 7) - 1) as f32 * 0.8;
    if sigma > 0.0 {
        let anel = anel_da_borda(livre, w, h);
        let peso: Vec<[f32; 3]> = anel
            .iter()
            .map(|a| if *a { [1.0; 3] } else { [0.0; 3] })
            .collect();
        let ponderado: Vec<[f32; 3]> = (0..w * h)
            .map(|k| if anel[k] { fator[k] } else { [0.0; 3] })
            .collect();
        let (num, den) = (
            desfocar(&ponderado, w, h, sigma),
            desfocar(&peso, w, h, sigma),
        );
        for k in 0..w * h {
            if anel[k] && den[k][0] > 1e-6 {
                for c in 0..3 {
                    fator[k][c] = num[k][c] / den[k][0];
                }
            }
        }
    }
    resolver(&mut fator, livre, w, h);
    (0..w * h)
        .map(|k| {
            if !livre[k] {
                return destino[k];
            }
            let mut r = [0.0; 3];
            for c in 0..3 {
                r[c] = ((origem[k][c] + EPSILON) * fator[k][c] - EPSILON).clamp(0.0, 255.0);
            }
            r
        })
        .collect()
}

/// Os pixels fixos a até 2 px (em passos de 4 vizinhos) de um pixel livre.
fn anel_da_borda(livre: &[bool], w: usize, h: usize) -> Vec<bool> {
    let mut anel = vec![false; w * h];
    let mut frente: Vec<usize> = (0..w * h).filter(|&k| livre[k]).collect();
    let mut visto = livre.to_vec();
    for _ in 0..2 {
        let mut proxima = Vec::new();
        for k in frente {
            let (x, y) = (k % w, k / w);
            let mut vizinho = |v: usize| {
                if !visto[v] {
                    visto[v] = true;
                    anel[v] = true;
                    proxima.push(v);
                }
            };
            if x > 0 {
                vizinho(k - 1);
            }
            if x + 1 < w {
                vizinho(k + 1);
            }
            if y > 0 {
                vizinho(k - w);
            }
            if y + 1 < h {
                vizinho(k + w);
            }
        }
        frente = proxima;
    }
    anel
}

/// Gaussiano separável de desvio `sigma`, com a borda repetida.
fn desfocar(img: &[[f32; 3]], w: usize, h: usize, sigma: f32) -> Vec<[f32; 3]> {
    let raio = (3.0 * sigma).ceil() as i64;
    let pesos: Vec<f32> = (-raio..=raio)
        .map(|d| (-(d * d) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f32 = pesos.iter().sum();
    let passar = |fonte: &[[f32; 3]], horizontal: bool| -> Vec<[f32; 3]> {
        let mut saida = vec![[0.0; 3]; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut soma = [0.0; 3];
                for (k, p) in pesos.iter().enumerate() {
                    let d = k as i64 - raio;
                    let (qx, qy) = if horizontal {
                        ((x as i64 + d).clamp(0, w as i64 - 1) as usize, y)
                    } else {
                        (x, (y as i64 + d).clamp(0, h as i64 - 1) as usize)
                    };
                    let v = fonte[qy * w + qx];
                    for c in 0..3 {
                        soma[c] += v[c] * p;
                    }
                }
                saida[y * w + x] = soma.map(|v| v / total);
            }
        }
        saida
    };
    passar(&passar(img, true), false)
}

/// Resolve `Δu = 0` nos pixels livres, com os fixos como borda (Dirichlet) e
/// a beira do retângulo sem fluxo (Neumann). `u` chega com os fixos certos e
/// sai com os livres resolvidos.
fn resolver(u: &mut [[f32; 3]], livre: &[bool], w: usize, h: usize) {
    if !livre.iter().any(|l| *l) {
        return;
    }
    // O ponto de partida dos livres: a média dos fixos (ou 1, a identidade).
    let (mut soma, mut n) = ([0.0f32; 3], 0usize);
    for k in 0..w * h {
        if !livre[k] {
            for c in 0..3 {
                soma[c] += u[k][c];
            }
            n += 1;
        }
    }
    let media = if n == 0 {
        [1.0; 3]
    } else {
        soma.map(|v| v / n as f32)
    };
    if w >= 16 && h >= 16 {
        // O nível de baixo: um pixel por 2 × 2. Livre se algum filho é livre;
        // o valor fixo é a média dos filhos fixos.
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let mut cu = vec![media; cw * ch];
        let mut cl = vec![false; cw * ch];
        for cy in 0..ch {
            for cx in 0..cw {
                let (mut s, mut q, mut algum_livre) = ([0.0f32; 3], 0, false);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (x, y) = (2 * cx + dx, 2 * cy + dy);
                    if x >= w || y >= h {
                        continue;
                    }
                    let k = y * w + x;
                    if livre[k] {
                        algum_livre = true;
                    } else {
                        for c in 0..3 {
                            s[c] += u[k][c];
                        }
                        q += 1;
                    }
                }
                let k = cy * cw + cx;
                cl[k] = algum_livre;
                if !algum_livre && q > 0 {
                    cu[k] = s.map(|v| v / q as f32);
                }
            }
        }
        resolver(&mut cu, &cl, cw, ch);
        for y in 0..h {
            for x in 0..w {
                let k = y * w + x;
                if livre[k] {
                    u[k] = cu[(y / 2) * cw + x / 2];
                }
            }
        }
        sor(u, livre, w, h, 60);
    } else {
        for k in 0..w * h {
            if livre[k] {
                u[k] = media;
            }
        }
        sor(u, livre, w, h, 4 * w.max(h) + 50);
    }
}

/// Sobre-relaxação sucessiva (ω = 1,9), em xadrez vermelho/preto.
fn sor(u: &mut [[f32; 3]], livre: &[bool], w: usize, h: usize, iteracoes: usize) {
    const OMEGA: f32 = 1.9;
    for _ in 0..iteracoes {
        for cor in 0..2 {
            for y in 0..h {
                for x in ((y + cor) % 2..w).step_by(2) {
                    let k = y * w + x;
                    if !livre[k] {
                        continue;
                    }
                    let mut soma = [0.0f32; 3];
                    let mut n = 0.0;
                    if x > 0 {
                        add(&mut soma, u[k - 1]);
                        n += 1.0;
                    }
                    if x + 1 < w {
                        add(&mut soma, u[k + 1]);
                        n += 1.0;
                    }
                    if y > 0 {
                        add(&mut soma, u[k - w]);
                        n += 1.0;
                    }
                    if y + 1 < h {
                        add(&mut soma, u[k + w]);
                        n += 1.0;
                    }
                    for c in 0..3 {
                        u[k][c] += OMEGA * (soma[c] / n - u[k][c]);
                    }
                }
            }
        }
    }
}

#[inline]
fn add(a: &mut [f32; 3], b: [f32; 3]) {
    for c in 0..3 {
        a[c] += b[c];
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Um disco livre de raio 20 no meio de 64 × 64.
    fn disco(w: usize, h: usize) -> Vec<bool> {
        (0..w * h)
            .map(|k| {
                let (x, y) = ((k % w) as f32 - 32.0, (k / w) as f32 - 32.0);
                x * x + y * y < 400.0
            })
            .collect()
    }

    #[test]
    fn a_textura_vem_da_origem_e_a_luz_do_destino() {
        let (w, h) = (64, 64);
        // Origem: textura de ±20 sobre 200 (pele clara); destino: liso a 100.
        let textura = |k: usize| {
            if (k % w + k / w).is_multiple_of(2) {
                20.0
            } else {
                -20.0
            }
        };
        let origem: Vec<[f32; 3]> = (0..w * h).map(|k| [200.0 + textura(k); 3]).collect();
        let destino = vec![[100.0f32; 3]; w * h];
        let livre = disco(w, h);
        let r = adaptar(&origem, &destino, &livre, w, h, 5);
        let centro = 32 * w + 32;
        let media = (r[centro][0] + r[centro + 1][0]) / 2.0;
        assert!((media - 100.0).abs() < 4.0, "a luz é a do destino: {media}");
        let amplitude = (r[centro][0] - r[centro + 1][0]).abs() / 2.0;
        // Multiplicativo: ±20 em 210 vira ±~10 em ~110.
        assert!(
            (6.0..14.0).contains(&amplitude),
            "a textura fica, na escala: {amplitude}"
        );
        assert_eq!(r[0], destino[0], "fora do traço, o destino");
    }

    #[test]
    fn a_mancha_do_miolo_nao_entra_na_borda() {
        // Destino liso a 150 com uma mancha escura que enche o miolo até
        // perto da borda; origem lisa a 150. O resultado tem de ser 150 em
        // toda parte — a mancha está saindo, não é referência de cor.
        let (w, h) = (64, 64);
        let livre = disco(w, h);
        let destino: Vec<[f32; 3]> = (0..w * h)
            .map(|k| {
                let (x, y) = ((k % w) as f32 - 32.0, (k / w) as f32 - 32.0);
                if x * x + y * y < 18.0 * 18.0 {
                    [30.0; 3]
                } else {
                    [150.0; 3]
                }
            })
            .collect();
        let origem = vec![[150.0f32; 3]; w * h];
        for difusao in [1, 5, 7] {
            let r = adaptar(&origem, &destino, &livre, w, h, difusao);
            for k in (0..w * h).filter(|&k| livre[k]) {
                assert!(
                    (r[k][0] - 150.0).abs() < 1.0,
                    "difusão {difusao}, pixel {k}: {:?}",
                    r[k]
                );
            }
        }
    }

    #[test]
    fn sem_diferenca_a_origem_passa_igual() {
        let (w, h) = (64, 64);
        let img: Vec<[f32; 3]> = (0..w * h)
            .map(|k| [(k % 97) as f32 + 60.0, 80.0, (k % 13) as f32 * 9.0])
            .collect();
        let livre = disco(w, h);
        // Origem = destino: o fator é 1 em toda parte (a menos da suavização
        // da borda, que é a mesma nos dois).
        let r = adaptar(&img, &img, &livre, w, h, 3);
        for k in 0..w * h {
            for c in 0..3 {
                assert!((r[k][c] - img[k][c]).abs() < 0.5, "{k}");
            }
        }
    }

    #[test]
    fn a_correcao_e_lisa_entre_bordas_diferentes() {
        // Destino escuro à esquerda e claro à direita: o fator sai em rampa,
        // sem degrau no meio.
        let (w, h) = (96, 32);
        let origem = vec![[128.0f32; 3]; w * h];
        let destino: Vec<[f32; 3]> = (0..w * h)
            .map(|k| [if k % w < 48 { 60.0 } else { 180.0 }; 3])
            .collect();
        let livre: Vec<bool> = (0..w * h)
            .map(|k| (k % w) > 10 && (k % w) < 86 && (k / w) > 2 && (k / w) < 29)
            .collect();
        let r = adaptar(&origem, &destino, &livre, w, h, 2);
        let linha = 16 * w;
        let mut anterior = r[linha + 11][0];
        for x in 12..86 {
            let v = r[linha + x][0];
            assert!(v >= anterior - 0.5, "monotônica em x = {x}");
            assert!((v - anterior).abs() < 6.0, "sem degrau em x = {x}");
            anterior = v;
        }
    }
}
