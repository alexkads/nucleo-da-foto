//! 🌫️ O menu Filtro do Photoshop: **Desfoque gaussiano** e **Máscara de
//! nitidez**, na camada escolhida (ou na máscara dela), dosados pela seleção.
//!
//! ```text
//! camada (RGBA, alfa reto)  →  pré-multiplicada em planos f32  →  3 caixas por eixo (≈ gaussiana de σ = raio)
//!                           →  de volta ao alfa reto            →  mistura com a original pela seleção
//! ```
//!
//! - **Pré-multiplicado**: o transparente em volta de um traço não escurece a
//!   borda do desfoque, e numa máscara (pixels sobre o `fundo`) o desfoque se
//!   mistura com o fundo — é a difusão que o Photoshop dá ao desfocar a máscara;
//! - o raio é o σ, em pixels da foto, como o "Raio" do Photoshop; as três
//!   caixas seguem o arranjo clássico de Kutskir (larguras ímpares w e w + 2);
//! - fora da foto a borda se repete (a foto não escurece nos cantos);
//! - só a região que pode mudar é calculada: o conteúdo (mais a margem de 3σ
//!   do desfoque), cortado pela caixa da seleção;
//! - a nitidez soma `quantidade × (original − desfocada)` em cada canal onde a
//!   diferença passa do limiar; o alfa fica.
//!
//! Funções puras: a janela calcula em segundo plano e a [`crate::Sessao`] só
//! troca os pixels (prévia) e grava um passo (OK).

use std::sync::Arc;

use crate::retangulo::Retangulo;
use crate::selecao::Selecao;
use crate::tiles::{indice, CamadaDePixels, BYTES_DO_TILE, LADO_DO_TILE};

/// O raio (σ) máximo, em pixels — o do Photoshop é 1000; 250 já desfaz uma
/// foto de 24 MP e mantém a conta em segundos.
pub const RAIO_MAXIMO: f32 = 250.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Filtro {
    /// "Desfoque gaussiano…": o raio é o σ, em pixels.
    DesfoqueGaussiano { raio: f32 },
    /// "Máscara de nitidez…": quantidade (1 = 100%), raio (σ) e limiar (0–255).
    MascaraDeNitidez {
        quantidade: f32,
        raio: f32,
        limiar: u8,
    },
    /// "Desfoque de superfície…": alisa o que é parecido e guarda as bordas
    /// (a pele sem perder olho e boca). Raio em px, limiar em níveis.
    DesfoqueDeSuperficie { raio: f32, limiar: u8 },
    /// "Alta frequência…" (Outros): o detalhe menor que o raio, sobre 50%
    /// cinza — a camada de cima da separação de frequências.
    AltaFrequencia { raio: f32 },
    /// "Mediana…" (Ruído): cada pixel vira a mediana do quadrado de lado
    /// 2 × raio + 1 — tira pontinhos e poeira.
    Mediana { raio: u32 },
    /// "Adicionar ruído…" (Ruído): quantidade (1 = 100%), gaussiano ou
    /// uniforme, colorido ou monocromático. O ruído sai da posição do pixel —
    /// a prévia e o OK dão o mesmo grão.
    AdicionarRuido {
        quantidade: f32,
        gaussiano: bool,
        monocromatico: bool,
    },
    /// "Suavizar tons…" (Tratamento de pele): o desfoque gaussiano **pesado
    /// pela seleção** — só os pixels selecionados entram na média, então o
    /// cabelo, a sobrancelha ou o fundo que ficaram fora da seleção não
    /// escorrem para a pele (o gaussiano comum os puxaria pela borda). Para a
    /// baixa frequência: alisa manchas e transições de cor e guarda o volume
    /// que passa do raio.
    SuavizarTons { raio: f32 },
}

/// O raio máximo da Mediana, em px.
pub const MEDIANA_MAXIMA: u32 = 20;
/// O raio máximo do Desfoque de superfície, em px (o do Photoshop é 100).
pub const SUPERFICIE_MAXIMA: f32 = 100.0;

impl Filtro {
    /// O nome no Histórico e no menu.
    pub fn nome(&self) -> &'static str {
        match self {
            Filtro::DesfoqueGaussiano { .. } => "Desfoque gaussiano",
            Filtro::MascaraDeNitidez { .. } => "Máscara de nitidez",
            Filtro::DesfoqueDeSuperficie { .. } => "Desfoque de superfície",
            Filtro::AltaFrequencia { .. } => "Alta frequência",
            Filtro::Mediana { .. } => "Mediana",
            Filtro::AdicionarRuido { .. } => "Adicionar ruído",
            Filtro::SuavizarTons { .. } => "Suavizar tons",
        }
    }

    fn raio(&self) -> f32 {
        match *self {
            Filtro::DesfoqueGaussiano { raio }
            | Filtro::MascaraDeNitidez { raio, .. }
            | Filtro::AltaFrequencia { raio }
            | Filtro::SuavizarTons { raio } => raio.clamp(0.1, RAIO_MAXIMO),
            Filtro::DesfoqueDeSuperficie { raio, .. } => raio.round().clamp(1.0, SUPERFICIE_MAXIMA),
            Filtro::Mediana { raio } => raio.clamp(1, MEDIANA_MAXIMA) as f32,
            Filtro::AdicionarRuido { .. } => 0.0,
        }
    }

    /// Quantos pixels em volta a conta lê.
    fn margem(&self) -> u32 {
        match self {
            Filtro::DesfoqueGaussiano { .. }
            | Filtro::MascaraDeNitidez { .. }
            | Filtro::AltaFrequencia { .. }
            | Filtro::SuavizarTons { .. } => (self.raio() * 3.0).ceil() as u32 + 1,
            Filtro::DesfoqueDeSuperficie { .. } => 2 * self.raio() as u32 + 1,
            Filtro::Mediana { .. } => self.raio() as u32,
            Filtro::AdicionarRuido { .. } => 0,
        }
    }

    /// Não muda nada (raio pequeno demais, nitidez sem quantidade).
    pub fn neutro(&self) -> bool {
        match *self {
            Filtro::DesfoqueGaussiano { raio } | Filtro::SuavizarTons { raio } => raio < 0.1,
            Filtro::MascaraDeNitidez { quantidade, .. }
            | Filtro::AdicionarRuido { quantidade, .. } => quantidade <= 0.0,
            Filtro::DesfoqueDeSuperficie { .. }
            | Filtro::AltaFrequencia { .. }
            | Filtro::Mediana { .. } => false,
        }
    }
}

/// As larguras das três caixas que, passadas em sequência, dão a gaussiana
/// de `sigma` (ímpares; as primeiras `m` com `w`, as outras com `w + 2`).
pub fn caixas_da_gaussiana(sigma: f32) -> [usize; 3] {
    let n = 3.0f32;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wl = wl.max(1);
    let wu = wl + 2;
    let wlf = wl as f32;
    let m = ((12.0 * sigma * sigma - n * wlf * wlf - 4.0 * n * wlf - 3.0 * n) / (-4.0 * wlf - 4.0))
        .round() as i64;
    [0, 1, 2].map(|i| if i < m { wl as usize } else { wu as usize })
}

/// A camada com o filtro: os tiles fora da região que muda são os mesmos
/// `Arc` da original (copiar é barato e a diferença sai pequena).
pub fn filtrada(
    original: &CamadaDePixels,
    filtro: Filtro,
    selecao: Option<&Selecao>,
) -> CamadaDePixels {
    filtrada_com(original, filtro, selecao, 1.0)
}

/// [`filtrada`] com a **intensidade** (0..=1, o "Atenuar" do Photoshop): o
/// resultado entra na proporção dela sobre a original, junto da seleção.
/// Sempre a partir da original — mexer no controle não acumula desfoque.
pub fn filtrada_com(
    original: &CamadaDePixels,
    filtro: Filtro,
    selecao: Option<&Selecao>,
    intensidade: f32,
) -> CamadaDePixels {
    let mut saida = original.clone();
    let intensidade = intensidade.clamp(0.0, 1.0);
    if filtro.neutro() || intensidade <= 0.0 {
        return saida;
    }
    let (largura, altura) = (original.largura(), original.altura());
    let sigma = filtro.raio();
    let margem = filtro.margem();
    let conteudo = original
        .existentes()
        .fold(Retangulo::default(), |a, (p, _)| {
            a.uniao(&crate::tiles::retangulo_do_tile(*p, largura, altura))
        });
    if conteudo.vazio() {
        return saida;
    }
    // O que pode mudar: o desfoque espalha o conteúdo pela margem; os outros
    // não saem dele.
    let mut fora = match filtro {
        Filtro::DesfoqueGaussiano { .. } => expandido(&conteudo, margem, largura, altura),
        _ => conteudo,
    };
    if let Some(s) = selecao {
        fora = intersecao(&fora, &s.caixa_justa());
    }
    if fora.vazio() {
        return saida;
    }
    let dentro = expandido(&fora, margem, largura, altura);
    let rgba = ler(original, &dentro);
    let (l, a) = (dentro.largura, dentro.altura);
    let resultado = match filtro {
        Filtro::DesfoqueGaussiano { .. } => desfocar_rgba(&rgba, l, a, sigma),
        Filtro::MascaraDeNitidez {
            quantidade, limiar, ..
        } => {
            let desfocada = desfocar_rgba(&rgba, l, a, sigma);
            nitidez(&rgba, &desfocada, quantidade.max(0.0), limiar)
        }
        Filtro::AltaFrequencia { .. } => {
            let desfocada = desfocar_rgba(&rgba, l, a, sigma);
            alta_frequencia(&rgba, &desfocada)
        }
        Filtro::DesfoqueDeSuperficie { limiar, .. } => {
            superficie(&rgba, l as usize, a as usize, sigma as usize, limiar)
        }
        Filtro::Mediana { .. } => mediana(&rgba, l as usize, a as usize, sigma as usize),
        Filtro::AdicionarRuido {
            quantidade,
            gaussiano,
            monocromatico,
        } => ruido(&rgba, &dentro, quantidade, gaussiano, monocromatico),
        Filtro::SuavizarTons { .. } => {
            let peso: Vec<f32> = (0..(l * a) as usize)
                .map(|k| {
                    let (x, y) = (dentro.x + k as u32 % l, dentro.y + k as u32 / l);
                    selecao.map_or(1.0, |s| s.valor(x, y) as f32 / 255.0)
                })
                .collect();
            suavizar_pesado(&rgba, &peso, l as usize, a as usize, sigma)
        }
    };
    escrever(
        &mut saida,
        original,
        &dentro,
        &fora,
        &resultado,
        selecao,
        intensidade,
    );
    saida
}

/// O desfoque com peso: `Σ g·α·p·cor / Σ g·α·p`, com `p` o peso de cada
/// pixel (a seleção). Onde nada pesa em volta, a cor fica. O alfa fica.
fn suavizar_pesado(rgba: &[u8], peso: &[f32], l: usize, a: usize, sigma: f32) -> Vec<u8> {
    let n = l * a;
    let caixas = caixas_da_gaussiana(sigma);
    let base: Vec<f32> = (0..n)
        .map(|k| rgba[k * 4 + 3] as f32 / 255.0 * peso[k])
        .collect();
    let mut w = base.clone();
    desfocar_plano(&mut w, l, a, &caixas);
    let mut saida = rgba.to_vec();
    for c in 0..3 {
        let mut plano: Vec<f32> = (0..n).map(|k| rgba[k * 4 + c] as f32 * base[k]).collect();
        desfocar_plano(&mut plano, l, a, &caixas);
        for k in 0..n {
            if rgba[k * 4 + 3] == 0 || w[k] <= 1e-4 {
                continue;
            }
            saida[k * 4 + c] = (plano[k] / w[k]).round().clamp(0.0, 255.0) as u8;
        }
    }
    saida
}

pub(crate) fn expandido(r: &Retangulo, margem: u32, largura: u32, altura: u32) -> Retangulo {
    let x0 = r.x.saturating_sub(margem);
    let y0 = r.y.saturating_sub(margem);
    let x1 = (r.direita() + margem).min(largura);
    let y1 = (r.baixo() + margem).min(altura);
    Retangulo::novo(x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
}

fn intersecao(a: &Retangulo, b: &Retangulo) -> Retangulo {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.direita().min(b.direita());
    let y1 = a.baixo().min(b.baixo());
    if x1 <= x0 || y1 <= y0 {
        return Retangulo::default();
    }
    Retangulo::novo(x0, y0, x1 - x0, y1 - y0)
}

/// Os pixels RGBA de `r`, linha a linha (o transparente onde não há tile).
pub(crate) fn ler(camada: &CamadaDePixels, r: &Retangulo) -> Vec<u8> {
    let (l, a) = (r.largura as usize, r.altura as usize);
    let mut saida = vec![0u8; l * a * 4];
    for posicao in camada.tiles_do_retangulo(r) {
        let Some(tile) = camada.tile(posicao) else {
            continue;
        };
        let (ox, oy) = crate::tiles::origem_do_tile(posicao);
        let x0 = (ox.max(r.x as i64)) as u32;
        let y0 = (oy.max(r.y as i64)) as u32;
        let x1 = ((ox + LADO_DO_TILE as i64).min(r.direita() as i64)) as u32;
        let y1 = ((oy + LADO_DO_TILE as i64).min(r.baixo() as i64)) as u32;
        for y in y0..y1 {
            let de = indice(x0 - ox as u32, y - oy as u32);
            let n = (x1 - x0) as usize * 4;
            let para = ((y - r.y) as usize * l + (x0 - r.x) as usize) * 4;
            saida[para..para + n].copy_from_slice(&tile[de..de + n]);
        }
    }
    saida
}

/// O desfoque de uma imagem RGBA de alfa reto, devolvido em alfa reto.
fn desfocar_rgba(rgba: &[u8], largura: u32, altura: u32, sigma: f32) -> Vec<u8> {
    let (l, a) = (largura as usize, altura as usize);
    let n = l * a;
    let caixas = caixas_da_gaussiana(sigma);
    let mut alfa: Vec<f32> = (0..n).map(|i| rgba[i * 4 + 3] as f32).collect();
    desfocar_plano(&mut alfa, l, a, &caixas);
    let mut saida = vec![0u8; n * 4];
    for c in 0..3 {
        let mut plano: Vec<f32> = (0..n)
            .map(|i| rgba[i * 4 + c] as f32 * rgba[i * 4 + 3] as f32 / 255.0)
            .collect();
        desfocar_plano(&mut plano, l, a, &caixas);
        for i in 0..n {
            let al = alfa[i];
            saida[i * 4 + c] = if al > 0.01 {
                (plano[i] * 255.0 / al).round().clamp(0.0, 255.0) as u8
            } else {
                0
            };
        }
    }
    for i in 0..n {
        saida[i * 4 + 3] = alfa[i].round().clamp(0.0, 255.0) as u8;
    }
    saida
}

/// Original + quantidade × (original − desfocada), canal a canal, onde a
/// diferença passa do limiar. O alfa é o da original.
fn nitidez(original: &[u8], desfocada: &[u8], quantidade: f32, limiar: u8) -> Vec<u8> {
    let mut saida = original.to_vec();
    for (px, (o, d)) in saida.as_chunks_mut::<4>().0.iter_mut().zip(
        original
            .as_chunks::<4>()
            .0
            .iter()
            .zip(desfocada.as_chunks::<4>().0),
    ) {
        if o[3] == 0 {
            continue;
        }
        for c in 0..3 {
            let dif = o[c] as f32 - d[c] as f32;
            if dif.abs() < limiar as f32 {
                continue;
            }
            px[c] = (o[c] as f32 + quantidade * dif).round().clamp(0.0, 255.0) as u8;
        }
    }
    saida
}

/// O detalhe sobre 50% cinza: original − desfocada + 128, canal a canal.
fn alta_frequencia(original: &[u8], desfocada: &[u8]) -> Vec<u8> {
    let mut saida = original.to_vec();
    for (px, (o, d)) in saida.as_chunks_mut::<4>().0.iter_mut().zip(
        original
            .as_chunks::<4>()
            .0
            .iter()
            .zip(desfocada.as_chunks::<4>().0),
    ) {
        if o[3] == 0 {
            continue;
        }
        for c in 0..3 {
            px[c] = (o[c] as i32 - d[c] as i32 + 128).clamp(0, 255) as u8;
        }
    }
    saida
}

/// O Desfoque de superfície pelo filtro guiado (He, Sun e Tang): em cada
/// janela de lado 2r + 1, a cor vira `a·I + b`, com `a = var / (var + ε)` —
/// onde a janela varia pouco (pele), `a` → 0 e sai a média; numa borda
/// (variância grande), `a` → 1 e a cor fica. ε vem do limiar: (limiar/255)².
/// As médias são pesadas pelo alfa, para o transparente não entrar como preto.
/// O custo por pixel não depende do raio.
fn superficie(rgba: &[u8], l: usize, a: usize, r: usize, limiar: u8) -> Vec<u8> {
    let n = l * a;
    let eps = (limiar.max(1) as f32 / 255.0).powi(2);
    let caixa = [2 * r + 1];
    let mut peso: Vec<f32> = (0..n).map(|i| rgba[i * 4 + 3] as f32 / 255.0).collect();
    desfocar_plano(&mut peso, l, a, &caixa);
    let mut saida = rgba.to_vec();
    for c in 0..3 {
        let cor = |i: usize| rgba[i * 4 + c] as f32 / 255.0;
        let alfa = |i: usize| rgba[i * 4 + 3] as f32 / 255.0;
        let mut media: Vec<f32> = (0..n).map(|i| alfa(i) * cor(i)).collect();
        let mut quadrado: Vec<f32> = (0..n).map(|i| alfa(i) * cor(i) * cor(i)).collect();
        desfocar_plano(&mut media, l, a, &caixa);
        desfocar_plano(&mut quadrado, l, a, &caixa);
        // `media` vira b e `quadrado` vira a, no lugar.
        for i in 0..n {
            let w = peso[i];
            if w <= 1e-6 {
                quadrado[i] = 0.0;
                media[i] = 0.0;
                continue;
            }
            let m = media[i] / w;
            let var = (quadrado[i] / w - m * m).max(0.0);
            let k = var / (var + eps);
            quadrado[i] = k;
            media[i] = m - k * m;
        }
        desfocar_plano(&mut quadrado, l, a, &caixa);
        desfocar_plano(&mut media, l, a, &caixa);
        for i in 0..n {
            if rgba[i * 4 + 3] == 0 {
                continue;
            }
            let q = quadrado[i] * cor(i) + media[i];
            saida[i * 4 + c] = (q * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    saida
}

/// A mediana do quadrado de lado 2r + 1, por canal, com o histograma que
/// anda pela linha (16 caixas grossas + 256 finas: achar a mediana custa ~32
/// passos). Linhas em threads.
fn mediana(rgba: &[u8], l: usize, a: usize, r: usize) -> Vec<u8> {
    let mut saida = rgba.to_vec();
    if l == 0 || a == 0 || r == 0 {
        return saida;
    }
    let total = ((2 * r + 1) * (2 * r + 1)) as u32;
    let meio = total / 2 + 1;
    let em = |x: i64, y: i64, c: usize| {
        let x = x.clamp(0, l as i64 - 1) as usize;
        let y = y.clamp(0, a as i64 - 1) as usize;
        rgba[(y * l + x) * 4 + c]
    };
    let partes = linhas_de_trabalho();
    let por_faixa = a.div_ceil(partes).max(1);
    std::thread::scope(|escopo| {
        for (k, faixa) in saida.chunks_mut(por_faixa * l * 4).enumerate() {
            escopo.spawn(move || {
                let y0 = k * por_faixa;
                for (dy, linha) in faixa.chunks_exact_mut(l * 4).enumerate() {
                    let y = (y0 + dy) as i64;
                    for c in 0..3 {
                        let mut fino = [0u32; 256];
                        let mut grosso = [0u32; 16];
                        for yy in y - r as i64..=y + r as i64 {
                            for xx in -(r as i64)..=r as i64 {
                                por(&mut fino, &mut grosso, em(xx, yy, c), 1);
                            }
                        }
                        for x in 0..l {
                            if x > 0 {
                                let sai = x as i64 - r as i64 - 1;
                                let entra = x as i64 + r as i64;
                                for yy in y - r as i64..=y + r as i64 {
                                    por(&mut fino, &mut grosso, em(sai, yy, c), -1);
                                    por(&mut fino, &mut grosso, em(entra, yy, c), 1);
                                }
                            }
                            if linha[x * 4 + 3] == 0 {
                                continue;
                            }
                            let mut conta = 0;
                            let mut g = 0;
                            while conta + grosso[g] < meio {
                                conta += grosso[g];
                                g += 1;
                            }
                            let mut v = g * 16;
                            while conta + fino[v] < meio {
                                conta += fino[v];
                                v += 1;
                            }
                            linha[x * 4 + c] = v as u8;
                        }
                    }
                }
            });
        }
    });
    saida
}

/// Põe (`d` = 1) ou tira (−1) o valor `v` dos dois histogramas.
fn por(fino: &mut [u32; 256], grosso: &mut [u32; 16], v: u8, d: i32) {
    fino[v as usize] = fino[v as usize].wrapping_add_signed(d);
    grosso[(v >> 4) as usize] = grosso[(v >> 4) as usize].wrapping_add_signed(d);
}

/// Um número de 0 a 1 que só depende de (x, y, canal) — o mesmo grão na
/// prévia e no OK, e sem emenda entre as regiões calculadas.
fn sorteio(x: u32, y: u32, c: u32) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ (c as u64).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    h = h.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    h ^= h >> 33;
    ((h >> 40) as f32 + 0.5) / (1u64 << 24) as f32
}

/// Adicionar ruído: uniforme em ±quantidade × 255, ou gaussiano com desvio
/// de metade disso (Box–Muller).
fn ruido(rgba: &[u8], r: &Retangulo, quantidade: f32, gaussiano: bool, mono: bool) -> Vec<u8> {
    let mut saida = rgba.to_vec();
    let forca = quantidade.max(0.0) * 255.0;
    let l = r.largura as usize;
    if l == 0 {
        return saida;
    }
    let valor = move |x: u32, y: u32, c: u32| {
        if gaussiano {
            let (u1, u2) = (sorteio(x, y, 2 * c), sorteio(x, y, 2 * c + 1));
            (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos() * forca * 0.5
        } else {
            (sorteio(x, y, c) * 2.0 - 1.0) * forca
        }
    };
    let por_faixa = (r.altura as usize).div_ceil(linhas_de_trabalho()).max(1);
    std::thread::scope(|escopo| {
        for (k, faixa) in saida.chunks_mut(por_faixa * l * 4).enumerate() {
            escopo.spawn(move || {
                for (i, px) in faixa.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    if px[3] == 0 {
                        continue;
                    }
                    let (x, y) = (r.x + (i % l) as u32, r.y + (k * por_faixa + i / l) as u32);
                    let mono_valor = mono.then(|| valor(x, y, 7));
                    for (c, canal) in px.iter_mut().take(3).enumerate() {
                        let d = mono_valor.unwrap_or_else(|| valor(x, y, c as u32));
                        *canal = (*canal as f32 + d).round().clamp(0.0, 255.0) as u8;
                    }
                }
            });
        }
    });
    saida
}

fn linhas_de_trabalho() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .clamp(1, 16)
}

/// As três caixas, nas linhas e depois nas colunas, com a borda repetida.
pub(crate) fn desfocar_plano(plano: &mut [f32], l: usize, a: usize, caixas: &[usize]) {
    if l == 0 || a == 0 {
        return;
    }
    let partes = linhas_de_trabalho();
    // Nas linhas: cada faixa de linhas numa thread, no lugar.
    let por_faixa = a.div_ceil(partes).max(1);
    std::thread::scope(|escopo| {
        for faixa in plano.chunks_mut(por_faixa * l) {
            escopo.spawn(move || {
                let mut aux = vec![0f32; l];
                for linha in faixa.chunks_exact_mut(l) {
                    for &w in caixas {
                        caixa_na_linha(linha, &mut aux, w / 2);
                        linha.copy_from_slice(&aux);
                    }
                }
            });
        }
    });
    // Nas colunas: cada faixa de colunas numa thread, com a soma corrida por
    // coluna descendo as linhas (lê o plano de forma contígua).
    let por_faixa = l.div_ceil(partes).max(1);
    let fonte: &[f32] = plano;
    let faixas: Vec<(usize, Vec<f32>)> = std::thread::scope(|escopo| {
        let tarefas: Vec<_> = (0..l)
            .step_by(por_faixa)
            .map(|x0| {
                let x1 = (x0 + por_faixa).min(l);
                escopo.spawn(move || {
                    let lf = x1 - x0;
                    let mut atual: Vec<f32> = (0..a)
                        .flat_map(|y| fonte[y * l + x0..y * l + x1].iter().copied())
                        .collect();
                    let mut aux = vec![0f32; lf * a];
                    for &w in caixas {
                        caixa_nas_colunas(&atual, &mut aux, lf, a, w / 2);
                        std::mem::swap(&mut atual, &mut aux);
                    }
                    (x0, atual)
                })
            })
            .collect();
        tarefas
            .into_iter()
            .map(|t| t.join().expect("faixa do desfoque"))
            .collect()
    });
    for (x0, faixa) in faixas {
        let lf = faixa.len() / a;
        for y in 0..a {
            plano[y * l + x0..y * l + x0 + lf].copy_from_slice(&faixa[y * lf..(y + 1) * lf]);
        }
    }
}

/// A média de 2r + 1 vizinhos em cada ponto, repetindo as pontas.
fn caixa_na_linha(linha: &[f32], saida: &mut [f32], r: usize) {
    let n = linha.len();
    if r == 0 {
        saida.copy_from_slice(linha);
        return;
    }
    let em = |i: i64| linha[i.clamp(0, n as i64 - 1) as usize];
    let mut soma: f64 = (-(r as i64)..=r as i64).map(|i| em(i) as f64).sum();
    let div = (2 * r + 1) as f64;
    for (i, s) in saida.iter_mut().enumerate() {
        *s = (soma / div) as f32;
        let i = i as i64;
        soma += em(i + r as i64 + 1) as f64 - em(i - r as i64) as f64;
    }
}

/// A mesma caixa, na vertical, para `lf` colunas lado a lado.
fn caixa_nas_colunas(fonte: &[f32], saida: &mut [f32], lf: usize, a: usize, r: usize) {
    if r == 0 {
        saida.copy_from_slice(fonte);
        return;
    }
    let linha = |y: i64| {
        let y = y.clamp(0, a as i64 - 1) as usize;
        &fonte[y * lf..(y + 1) * lf]
    };
    let mut soma = vec![0f64; lf];
    for y in -(r as i64)..=r as i64 {
        for (s, v) in soma.iter_mut().zip(linha(y)) {
            *s += *v as f64;
        }
    }
    let div = (2 * r + 1) as f64;
    for y in 0..a {
        for (o, s) in saida[y * lf..(y + 1) * lf].iter_mut().zip(&soma) {
            *o = (*s / div) as f32;
        }
        let (entra, sai) = (linha(y as i64 + r as i64 + 1), linha(y as i64 - r as i64));
        for ((s, e), t) in soma.iter_mut().zip(entra).zip(sai) {
            *s += *e as f64 - *t as f64;
        }
    }
}

/// O resultado (calculado em `dentro`) volta aos tiles só em `fora`,
/// misturado com a original pela seleção.
fn escrever(
    saida: &mut CamadaDePixels,
    original: &CamadaDePixels,
    dentro: &Retangulo,
    fora: &Retangulo,
    resultado: &[u8],
    selecao: Option<&Selecao>,
    intensidade: f32,
) {
    let l = dentro.largura as usize;
    for posicao in original.tiles_do_retangulo(fora) {
        let (ox, oy) = crate::tiles::origem_do_tile(posicao);
        let velho = original.tile(posicao).cloned();
        let mut novo: Vec<u8> = velho
            .as_ref()
            .map_or_else(|| vec![0; BYTES_DO_TILE], |t| t.as_ref().clone());
        let m = selecao.map_or(Err(255), |s| s.do_tile(posicao));
        let x0 = (ox.max(fora.x as i64)) as u32;
        let y0 = (oy.max(fora.y as i64)) as u32;
        let x1 = ((ox + LADO_DO_TILE as i64).min(fora.direita() as i64)) as u32;
        let y1 = ((oy + LADO_DO_TILE as i64).min(fora.baixo() as i64)) as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let (lx, ly) = (x - ox as u32, y - oy as u32);
                let v = match &m {
                    Ok(t) => t[(ly * LADO_DO_TILE + lx) as usize],
                    Err(v) => *v,
                } as u32;
                let v = (v as f32 * intensidade).round() as u32;
                if v == 0 {
                    continue;
                }
                let i = indice(lx, ly);
                let j = ((y - dentro.y) as usize * l + (x - dentro.x) as usize) * 4;
                for c in 0..4 {
                    let (o, f) = (novo[i + c] as u32, resultado[j + c] as u32);
                    novo[i + c] = ((o * (255 - v) + f * v + 127) / 255) as u8;
                }
            }
        }
        let tem_algo = novo.iter().skip(3).step_by(4).any(|a| *a != 0);
        let tile = tem_algo.then(|| Arc::new(novo));
        if tile.as_ref() != velho.as_ref() {
            saida.definir(posicao, tile);
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::selecao::Forma;

    fn camada_com_quadrado() -> CamadaDePixels {
        let mut c = CamadaDePixels::nova(600, 400);
        for y in 100..300 {
            for x in 200..400 {
                let (ox, oy) = (x / LADO_DO_TILE, y / LADO_DO_TILE);
                let t = c.tile_mut((ox as i32, oy as i32));
                let i = indice(x % LADO_DO_TILE, y % LADO_DO_TILE);
                t[i..i + 4].copy_from_slice(&[200, 40, 40, 255]);
            }
        }
        c
    }

    #[test]
    fn as_caixas_dao_a_variancia_da_gaussiana() {
        for sigma in [0.8f32, 2.0, 5.0, 17.3, 60.0] {
            let caixas = caixas_da_gaussiana(sigma);
            // A variância de uma caixa de largura w é (w² − 1)/12.
            let var: f32 = caixas.iter().map(|&w| ((w * w) as f32 - 1.0) / 12.0).sum();
            assert!(
                (var.sqrt() - sigma).abs() < 0.6,
                "σ {sigma}: caixas {caixas:?} dão {}",
                var.sqrt()
            );
            assert!(caixas.iter().all(|w| w % 2 == 1));
        }
    }

    #[test]
    fn o_desfoque_espalha_a_borda_e_conserva_o_meio_e_a_cor() {
        let c = camada_com_quadrado();
        let d = filtrada(&c, Filtro::DesfoqueGaussiano { raio: 6.0 }, None);
        // O meio do quadrado fica igual.
        assert_eq!(d.pixel(300, 200), [200, 40, 40, 255]);
        // Na borda, meio alfa, e a cor **não** escurece (pré-multiplicado).
        let borda = d.pixel(200, 200);
        assert!((110..=145).contains(&borda[3]), "alfa na borda {borda:?}");
        assert!(borda[0] >= 198 && borda[1] <= 42, "cor na borda {borda:?}");
        // Fora, a 10 px, já tem um pouco; a 40 px, nada.
        assert!(d.pixel(190, 200)[3] > 0);
        assert_eq!(d.pixel(150, 200)[3], 0);
        // A original não mudou.
        assert_eq!(c.pixel(200, 200)[3], 255);
    }

    #[test]
    fn a_selecao_dosa_o_desfoque() {
        let c = camada_com_quadrado();
        let s = Selecao::da_forma(600, 400, &Forma::Retangulo(Retangulo::novo(0, 0, 300, 400)));
        let d = filtrada(&c, Filtro::DesfoqueGaussiano { raio: 6.0 }, Some(&s));
        // A borda esquerda (dentro da seleção) desfoca; a direita fica dura.
        assert!(d.pixel(200, 200)[3] < 200);
        assert_eq!(d.pixel(399, 200), [200, 40, 40, 255]);
        assert_eq!(d.pixel(400, 200)[3], 0);
    }

    #[test]
    fn a_nitidez_realca_a_borda_e_respeita_o_limiar() {
        let mut c = CamadaDePixels::nova(300, 100);
        for y in 0..100 {
            for x in 0..300 {
                let v = if x < 150 { 100 } else { 150 };
                let t = c.tile_mut(((x / LADO_DO_TILE) as i32, (y / LADO_DO_TILE) as i32));
                let i = indice(x % LADO_DO_TILE, y % LADO_DO_TILE);
                t[i..i + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let n = filtrada(
            &c,
            Filtro::MascaraDeNitidez {
                quantidade: 1.0,
                raio: 3.0,
                limiar: 0,
            },
            None,
        );
        // O lado escuro escurece junto da divisa, o claro clareia; longe dela
        // nada muda; o alfa fica.
        assert!(n.pixel(148, 50)[0] < 100);
        assert!(n.pixel(151, 50)[0] > 150);
        assert_eq!(n.pixel(20, 50), [100, 100, 100, 255]);
        // Com limiar acima da diferença, nada muda.
        let alto = filtrada(
            &c,
            Filtro::MascaraDeNitidez {
                quantidade: 1.0,
                raio: 3.0,
                limiar: 60,
            },
            None,
        );
        assert_eq!(alto.pixel(148, 50), [100, 100, 100, 255]);
    }

    /// Pele lisa com uma borda forte: o Desfoque de superfície tira o grão
    /// e guarda a borda; o gaussiano do mesmo raio a espalha.
    #[test]
    fn a_superficie_alisa_e_guarda_a_borda() {
        let mut c = CamadaDePixels::nova(200, 100);
        for y in 0..100u32 {
            for x in 0..200u32 {
                let grao = (sorteio(x, y, 0) * 12.0) as u8;
                let v = if x < 100 { 60 + grao } else { 200 + grao };
                let t = c.tile_mut(((x / LADO_DO_TILE) as i32, (y / LADO_DO_TILE) as i32));
                let i = indice(x % LADO_DO_TILE, y % LADO_DO_TILE);
                t[i..i + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let s = filtrada(
            &c,
            Filtro::DesfoqueDeSuperficie {
                raio: 5.0,
                limiar: 20,
            },
            None,
        );
        let desvio = |cam: &CamadaDePixels| {
            let v: Vec<f32> = (20..80).map(|x| cam.pixel(x, 50)[0] as f32).collect();
            let m = v.iter().sum::<f32>() / v.len() as f32;
            (v.iter().map(|x| (x - m).powi(2)).sum::<f32>() / v.len() as f32).sqrt()
        };
        assert!(
            desvio(&s) < desvio(&c) / 2.0,
            "{} → {}",
            desvio(&c),
            desvio(&s)
        );
        // A borda: dois pixels de cada lado continuam longe um do outro.
        assert!(s.pixel(101, 50)[0] as i32 - s.pixel(98, 50)[0] as i32 > 120);
        let g = filtrada(&c, Filtro::DesfoqueGaussiano { raio: 5.0 }, None);
        assert!((g.pixel(101, 50)[0] as i32 - g.pixel(98, 50)[0] as i32) < 60);
    }

    #[test]
    fn a_alta_frequencia_e_cinza_no_liso_e_marca_a_borda() {
        let mut c = camada_com_quadrado();
        // Metade do quadrado mais clara: uma borda de cor em x = 300.
        for y in 100..300u32 {
            for x in 300..400u32 {
                let t = c.tile_mut(((x / LADO_DO_TILE) as i32, (y / LADO_DO_TILE) as i32));
                let i = indice(x % LADO_DO_TILE, y % LADO_DO_TILE);
                t[i..i + 4].copy_from_slice(&[250, 90, 90, 255]);
            }
        }
        let h = filtrada(&c, Filtro::AltaFrequencia { raio: 4.0 }, None);
        assert_eq!(h.pixel(250, 200), [128, 128, 128, 255], "liso = 50% cinza");
        assert!(
            h.pixel(298, 200)[0] < 128 && h.pixel(301, 200)[0] > 128,
            "a borda aparece"
        );
        assert_eq!(h.pixel(100, 50)[3], 0, "o transparente fica");
    }

    #[test]
    fn a_mediana_tira_o_ponto_e_guarda_o_quadrado() {
        let mut c = camada_com_quadrado();
        let i = indice(300 % LADO_DO_TILE, 200 % LADO_DO_TILE);
        c.tile_mut((1, 0))[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
        let m = filtrada(&c, Filtro::Mediana { raio: 2 }, None);
        assert_eq!(m.pixel(300, 200), [200, 40, 40, 255], "o ponto sumiu");
        assert_eq!(m.pixel(205, 205), [200, 40, 40, 255]);
    }

    #[test]
    fn o_ruido_e_o_mesmo_em_toda_conta_e_tem_a_forca_pedida() {
        let c = camada_com_quadrado();
        let f = Filtro::AdicionarRuido {
            quantidade: 0.1,
            gaussiano: false,
            monocromatico: true,
        };
        let a = filtrada(&c, f, None);
        let b = filtrada(&c, f, None);
        assert!(
            crate::operacoes::diferenca(&a, &b).is_none(),
            "o mesmo grão"
        );
        let p = a.pixel(250, 150);
        assert_eq!(p[0] as i32 - 200, p[1] as i32 - 40, "monocromático");
        let mudou = (200..400)
            .filter(|x| a.pixel(*x, 150) != [200, 40, 40, 255])
            .count();
        assert!(mudou > 150, "quase todo pixel muda: {mudou}");
        assert!((200..400).all(|x| (a.pixel(x, 150)[0] as i32 - 200).abs() <= 26));
        assert_eq!(a.pixel(100, 50)[3], 0, "o transparente fica");
    }

    #[test]
    fn camada_vazia_ou_filtro_neutro_nao_mudam() {
        let vazia = CamadaDePixels::nova(300, 200);
        let d = filtrada(&vazia, Filtro::DesfoqueGaussiano { raio: 10.0 }, None);
        assert!(d.vazia());
        let c = camada_com_quadrado();
        let n = filtrada(
            &c,
            Filtro::MascaraDeNitidez {
                quantidade: 0.0,
                raio: 3.0,
                limiar: 0,
            },
            None,
        );
        assert!(crate::operacoes::diferenca(&c, &n).is_none());
    }
}

/// Medida: `cargo test --release -p editor-core medir_os_filtros -- --ignored --nocapture`.
#[cfg(test)]
#[test]
#[ignore]
fn medir_os_filtros_em_24_mp() {
    let foto = image::RgbImage::from_fn(6000, 4000, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x ^ y) % 256) as u8])
    });
    let camada = CamadaDePixels::da_imagem(&foto);
    for filtro in [
        Filtro::DesfoqueGaussiano { raio: 4.0 },
        Filtro::DesfoqueGaussiano { raio: 60.0 },
        Filtro::MascaraDeNitidez {
            quantidade: 1.0,
            raio: 2.0,
            limiar: 0,
        },
        Filtro::DesfoqueDeSuperficie {
            raio: 8.0,
            limiar: 15,
        },
        Filtro::AltaFrequencia { raio: 6.0 },
        Filtro::Mediana { raio: 3 },
        Filtro::AdicionarRuido {
            quantidade: 0.1,
            gaussiano: true,
            monocromatico: false,
        },
    ] {
        let t = std::time::Instant::now();
        let f = filtrada(&camada, filtro, None);
        eprintln!("{filtro:?}: {:?} ({} tiles)", t.elapsed(), f.quantos());
    }
}
