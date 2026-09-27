//! O nível: quanto girar a foto para que as retas dela fiquem retas.
//!
//! Duas perguntas, as duas do Enquadrar do Lightroom:
//!
//! - **"Auto"** ([`angulo_automatico`]): acha as retas da própria foto —
//!   batentes de janela, rodapé, quina de parede, horizonte — e devolve o
//!   ângulo que as deixa na horizontal ou na vertical.
//! - **A régua** ([`angulo_da_reta`]): o operador traça uma reta sobre algo
//!   que devia ser reto, e a foto gira até ela ficar reta.
//!
//! ## 🔑 A resposta vem da imagem, e não do arquivo
//!
//! Foi a primeira pergunta do dono (2026-09-27): a câmera não guarda o nível?
//! As do estúdio (D3100, D3200, D7200, D750), não. As duas de entrada não têm
//! horizonte virtual; a D750 e a D7200 têm, mas **só no visor** — o `ShotInfo`
//! delas, decifrado pelo exiftool, não traz rolagem nem arfagem. O arquivo só
//! diz a orientação de 90°, que o app já usa. O Lightroom também procura as
//! retas na foto.
//!
//! ## Como acha as retas
//!
//! 1. A foto encolhe para [`LADO_DA_ANALISE`] e vira luminância, com um leve
//!    desfoque que tira o ruído do sensor sem apagar as bordas.
//! 2. Sobel dá a direção de cada borda; a supressão de não-máximos afina a
//!    borda a um pixel, senão uma borda grossa vota como três retas.
//! 3. Uma transformada de Hough **restrita à direção**: cada pixel de borda só
//!    vota nos ângulos perto do da sua borda, e só perto da horizontal ou da
//!    vertical ([`DESVIO_MAXIMO`]). É o que separa a reta de verdade da estampa
//!    do papel de parede, cujas bordas apontam para todo lado.
//! 4. Cada pico é conferido contra os pixels: vira reta só o **trecho
//!    contínuo** e longo, com o ângulo ajustado pelos pixels dele, e cada pixel
//!    fica com uma reta só.
//! 5. As verticais decidem, e o giro sai de um ajuste que separa giro de
//!    convergência (ver [`decidir`]). Sem apoio bastante ([`APOIO_MINIMO`]), o
//!    Auto não responde — não achar é melhor que entortar.
//!
//! ## O sentido do ângulo
//!
//! O de [`crate::transformacao`]: **positivo gira a foto no sentido horário** na
//! tela. Uma reta que desce para a direita (`dy > 0` com `y` para baixo) está
//! girada no sentido horário, e quem a endireita é o ângulo negativo.

use image::DynamicImage;

/// O lado maior da imagem analisada. 1000 px bastam para uma reta de janela ter
/// centenas de pixels, e cabem num quadro: a análise leva dezenas de
/// milissegundos em `release`.
pub const LADO_DA_ANALISE: u32 = 1000;

/// O maior giro que o Auto procura, para cada lado. Uma foto torta mais do que
/// isso foi tirada torta de propósito — e, acima disso, as diagonais de
/// perspectiva (piso, teto, mesa vista de cima) começam a se passar por
/// horizontais.
pub const DESVIO_MAXIMO: f32 = 15.0;

/// A resolução do ângulo: 0,1°, o passo do slider Endireitar.
const PASSO: f32 = 0.1;
/// Quanto a direção de um pixel de borda pode diferir da reta em que ele vota.
const JANELA: f32 = 4.0;
/// A menor reta que conta, em fração do lado menor da imagem analisada.
const MENOR_RETA: f32 = 0.08;
/// A borda mais fraca que conta, em luminância 0–1 depois do Sobel.
const BORDA_MINIMA: f32 = 0.12;
/// Quantas retas entram na conta, das mais longas para as mais curtas.
const RETAS: usize = 40;
/// Quantos máximos do acumulador são conferidos contra os pixels.
const CANDIDATOS: usize = 4000;
/// O maior buraco dentro de uma reta, em pixels da imagem analisada.
const LACUNA: f32 = 6.0;
/// A meia largura da faixa de uma reta, em pixels da imagem analisada. A lente
/// grande-angular curva o batente perto da borda da foto (18 mm no estúdio), e
/// numa faixa estreita a reta entrava e saía — virava pedaços curtos.
const FAIXA: f32 = 2.0;
/// Quão longe do modelo uma reta ainda vota (o corte de Tukey), em graus.
const VIZINHANCA: f32 = 1.0;
/// Quanto de vertical, somado, dispensa as horizontais — em lados menores.
const VERTICAIS_BASTAM: f32 = 0.6;
/// O preço da convergência no ajuste, em fração do peso total. Pequeno: numa
/// foto do estúdio com a câmera 6° para baixo, as verticais iam de −2,3° a
/// −3,8° da esquerda para a direita, e um freio maior (0,05) achatava essa
/// inclinação — o giro virava a média do lado que o recorte deixou.
const FREIO: f32 = 0.003;
/// O menor apoio para o Auto responder, em lados menores de reta que concorda.
/// Medido em 27/09/2026 com 48 fotos do estúdio giradas de −4° e +2°: acima
/// dele, 92 de 96 respostas ficam a 0,2° da certa e a pior a 0,4°; abaixo, o
/// Auto chutava até 4° errado em foto de pouca reta. Não achar é melhor que
/// entortar.
const APOIO_MINIMO: f32 = 0.8;

/// Uma reta achada, em pixels da imagem de origem.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reta {
    /// O giro que a deixaria reta, em graus.
    pub correcao: f32,
    /// É uma vertical (senão, horizontal)?
    pub vertical: bool,
    /// Quantos pixels de borda caem nela, na escala de origem.
    pub comprimento: f32,
    /// As pontas, em pixels da imagem de origem.
    pub de: (f32, f32),
    pub ate: (f32, f32),
}

/// O resultado do Auto.
#[derive(Debug, Clone, PartialEq)]
pub struct Nivel {
    /// O ângulo do Endireitar, arredondado a 0,1°.
    pub angulo: f32,
    /// As retas que decidiram, as mais longas primeiro.
    pub retas: Vec<Reta>,
    /// Quanto de reta concorda com o ângulo, em lados menores da foto.
    pub apoio: f32,
}

/// O giro que endireita um traço de `(dx, dy)` — a régua.
///
/// A reta vai para a horizontal ou para a vertical, **a que estiver mais perto**:
/// traçar ao longo de um batente de porta endireita a porta, e ao longo do
/// rodapé, o rodapé. Traço de comprimento zero não gira nada.
pub fn angulo_da_reta(dx: f32, dy: f32) -> f32 {
    if dx == 0.0 && dy == 0.0 {
        return 0.0;
    }
    -desvio_do_reto(dy.atan2(dx).to_degrees())
}

/// Quanto uma direção (em graus) está fora da horizontal ou da vertical mais
/// próxima, em `(-45, 45]`.
fn desvio_do_reto(graus: f32) -> f32 {
    let mut d = graus.rem_euclid(90.0);
    if d > 45.0 {
        d -= 90.0;
    }
    d
}

/// O ângulo do Endireitar que deixa as retas da foto retas, ou `None` quando
/// ela não tem reta em que confiar (um retrato contra fundo liso, um céu).
///
/// `imagem` é a foto **antes** do endireitamento — a de origem ou já espelhada e
/// girada em 90°, tanto faz: girar 90° não muda o desvio. Espelhar muda o
/// sinal, e é quem chama que sabe se espelhou ([`angulo_automatico_do_corte`]).
pub fn angulo_automatico(imagem: &DynamicImage) -> Option<Nivel> {
    let (luma, largura, altura, escala) = luminancia(imagem);
    if largura < 16 || altura < 16 {
        return None;
    }
    let bordas = bordas(&luma, largura, altura);
    let retas = hough(&bordas, largura, altura, escala);
    decidir(retas, largura as f32 * escala, altura as f32 * escala)
}

/// O Auto já no sentido do [`crate::Corte`]: com um espelho só, a foto na tela
/// é a de origem ao contrário, e o giro também.
pub fn angulo_automatico_do_corte(imagem: &DynamicImage, corte: &crate::Corte) -> Option<Nivel> {
    let mut nivel = angulo_automatico(imagem)?;
    if corte.espelho_h() != corte.espelho_v() {
        nivel.angulo = arredondar(-nivel.angulo);
        for reta in &mut nivel.retas {
            reta.correcao = -reta.correcao;
        }
    }
    Some(nivel)
}

fn arredondar(graus: f32) -> f32 {
    let a = (graus / PASSO).round() * PASSO;
    // `-0.0` escreveria "-0.0°" no rótulo.
    if a == 0.0 {
        0.0
    } else {
        (a * 10.0).round() / 10.0
    }
}

/// A luminância (0–1) da foto encolhida, com o desfoque binomial 5×5. Devolve
/// também quantos pixels de origem cada pixel analisado vale.
fn luminancia(imagem: &DynamicImage) -> (Vec<f32>, usize, usize, f32) {
    let (w0, h0) = (imagem.width(), imagem.height());
    let pequena = if w0.max(h0) > LADO_DA_ANALISE {
        imagem.thumbnail(LADO_DA_ANALISE, LADO_DA_ANALISE)
    } else {
        imagem.clone()
    };
    let rgb = pequena.to_rgb8();
    let (w, h) = (rgb.width() as usize, rgb.height() as usize);
    let escala = w0 as f32 / w.max(1) as f32;
    let mut luma: Vec<f32> = rgb
        .pixels()
        .map(|p| (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0)
        .collect();

    const NUCLEO: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];
    let mut meio = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for (k, peso) in NUCLEO.iter().enumerate() {
                let xx = (x as isize + k as isize - 2).clamp(0, w as isize - 1) as usize;
                s += peso * luma[y * w + xx];
            }
            meio[y * w + x] = s;
        }
    }
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for (k, peso) in NUCLEO.iter().enumerate() {
                let yy = (y as isize + k as isize - 2).clamp(0, h as isize - 1) as usize;
                s += peso * meio[yy * w + x];
            }
            luma[y * w + x] = s;
        }
    }
    (luma, w, h, escala)
}

/// Um pixel de borda: onde está e para onde aponta a normal dela (graus, em
/// `[0, 180)`).
struct Borda {
    x: f32,
    y: f32,
    normal: f32,
}

/// Sobel e supressão de não-máximos. A borda de 1 px de margem fica de fora: o
/// Sobel ali mede a moldura, e não a foto.
fn bordas(luma: &[f32], w: usize, h: usize) -> Vec<Borda> {
    let em = |x: usize, y: usize| luma[y * w + x];
    let mut gx = vec![0.0f32; w * h];
    let mut gy = vec![0.0f32; w * h];
    let mut modulo = vec![0.0f32; w * h];
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let a = em(x + 1, y - 1) + 2.0 * em(x + 1, y) + em(x + 1, y + 1)
                - em(x - 1, y - 1)
                - 2.0 * em(x - 1, y)
                - em(x - 1, y + 1);
            let b = em(x - 1, y + 1) + 2.0 * em(x, y + 1) + em(x + 1, y + 1)
                - em(x - 1, y - 1)
                - 2.0 * em(x, y - 1)
                - em(x + 1, y - 1);
            let i = y * w + x;
            gx[i] = a;
            gy[i] = b;
            modulo[i] = (a * a + b * b).sqrt();
        }
    }

    let mut saida = Vec::new();
    for y in 2..h - 2 {
        for x in 2..w - 2 {
            let i = y * w + x;
            let m = modulo[i];
            if m < BORDA_MINIMA {
                continue;
            }
            let normal = gy[i].atan2(gx[i]).to_degrees().rem_euclid(180.0);
            // Os dois vizinhos ao longo da normal, com ela arredondada a 45°.
            let (dx, dy): (isize, isize) = match ((normal + 22.5) / 45.0) as u32 % 4 {
                0 => (1, 0),
                1 => (1, 1),
                2 => (0, 1),
                _ => (-1, 1),
            };
            let antes = modulo[((y as isize - dy) as usize) * w + (x as isize - dx) as usize];
            let depois = modulo[((y as isize + dy) as usize) * w + (x as isize + dx) as usize];
            if m >= antes && m > depois {
                saida.push(Borda {
                    x: x as f32,
                    y: y as f32,
                    normal,
                });
            }
        }
    }
    saida
}

/// A transformada de Hough restrita à direção, e os picos dela.
///
/// Duas famílias, cada uma com o desvio de `-DESVIO_MAXIMO` a `+DESVIO_MAXIMO`
/// em passos de [`PASSO`]: horizontais (normal perto de 90°) e verticais
/// (normal perto de 0°). `ρ` é medido a partir do centro da imagem.
fn hough(bordas: &[Borda], w: usize, h: usize, escala: f32) -> Vec<Reta> {
    let meio = (DESVIO_MAXIMO / PASSO).round() as isize;
    let angulos = (2 * meio + 1) as usize;
    let janela = (JANELA / PASSO).round() as isize;
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let raio = ((w * w + h * h) as f32).sqrt() / 2.0;
    let rhos = (2.0 * raio).ceil() as usize + 3;
    let deslocamento = raio.ceil() + 1.0;

    // [família][ângulo] → (cos, sen) da normal.
    let tabela: Vec<[(f32, f32); 2]> = (0..angulos)
        .map(|k| {
            let desvio = (k as isize - meio) as f32 * PASSO;
            let h = (90.0 + desvio).to_radians();
            let v = desvio.to_radians();
            [(h.cos(), h.sin()), (v.cos(), v.sin())]
        })
        .collect();

    let mut acumulador = vec![0u32; 2 * angulos * rhos];
    for b in bordas {
        let (px, py) = (b.x - cx, b.y - cy);
        // O desvio da borda em cada família: horizontal tem a normal em 90°.
        let desvios = [
            b.normal - 90.0,
            if b.normal >= 90.0 {
                b.normal - 180.0
            } else {
                b.normal
            },
        ];
        for (familia, desvio) in desvios.into_iter().enumerate() {
            if desvio.abs() > DESVIO_MAXIMO + JANELA {
                continue;
            }
            let centro = (desvio / PASSO).round() as isize;
            let de = (centro - janela).max(-meio);
            let ate = (centro + janela).min(meio);
            for k in de..=ate {
                let indice = (k + meio) as usize;
                let (cos, sen) = tabela[indice][familia];
                let rho = (px * cos + py * sen + deslocamento).round() as usize;
                acumulador[(familia * angulos + indice) * rhos + rho] += 1;
            }
        }
    }

    let menor = (MENOR_RETA * w.min(h) as f32).max(20.0) as u32;
    let mut picos = Vec::new();
    // O pico é o maior num raio de 0,5° e 3 px — duas bordas da mesma moldura
    // a 3 px uma da outra são uma reta só.
    const VIZ_ANGULO: isize = 5;
    const VIZ_RHO: isize = 3;
    for familia in 0..2 {
        let base = familia * angulos;
        for k in 0..angulos {
            for r in 0..rhos {
                let v = acumulador[(base + k) * rhos + r];
                if v < menor {
                    continue;
                }
                let mut maximo = true;
                'viz: for dk in -VIZ_ANGULO..=VIZ_ANGULO {
                    let kk = k as isize + dk;
                    if kk < 0 || kk >= angulos as isize {
                        continue;
                    }
                    for dr in -VIZ_RHO..=VIZ_RHO {
                        let rr = r as isize + dr;
                        if rr < 0 || rr >= rhos as isize || (dk == 0 && dr == 0) {
                            continue;
                        }
                        let o = acumulador[(base + kk as usize) * rhos + rr as usize];
                        // Empate: fica o primeiro, senão um platô vira vários picos.
                        if o > v || (o == v && (dk, dr) < (0, 0)) {
                            maximo = false;
                            break 'viz;
                        }
                    }
                }
                if maximo {
                    picos.push((v, familia, k, r));
                }
            }
        }
    }

    // 🔑 **Cada pixel de borda pertence a uma reta só.** Um batente curto
    // deixa ecos no acumulador a ±1° dele (o "leque" de Hough de um segmento
    // finito), e os ecos são máximos locais. Confirmando as retas da mais
    // votada para a menos, cada uma leva os pixels dela, e o eco fica sem voto.
    //
    // 🔑 **E a reta é um trecho contínuo.** O acumulador soma todo pixel que
    // cai na faixa, de ponta a ponta da foto: pedaços da estampa, a dobra de um
    // vestido e a perna da mesa, alinhados por acaso, somavam uma "vertical"
    // que atravessava a família inteira (fotos do estúdio, 2026-09-27). A faixa
    // é partida onde há buraco maior que [`LACUNA`], e só o trecho longo conta.
    picos.sort_by_key(|p| std::cmp::Reverse(p.0));
    picos.truncate(CANDIDATOS);
    // Onde há borda, e qual: a conferência anda ao longo da reta em vez de
    // varrer todas as bordas a cada candidato.
    let mut mapa = vec![u32::MAX; w * h];
    for (i, b) in bordas.iter().enumerate() {
        mapa[b.y as usize * w + b.x as usize] = i as u32;
    }
    let mut tomado = vec![false; bordas.len()];
    let mut retas = Vec::new();
    for (_, familia, k, r) in picos {
        let desvio = (k as isize - meio) as f32 * PASSO;
        let (cos, sen) = tabela[k][familia];
        let rho = r as f32 - deslocamento;
        let normal = if familia == 0 {
            90.0 + desvio
        } else {
            desvio.rem_euclid(180.0)
        };
        let (dx, dy) = (-sen, cos);
        let mut faixa: Vec<(f32, usize)> = Vec::new();
        let (bx, by) = (cx + rho * cos, cy + rho * sen);
        let mut t = -raio;
        while t <= raio {
            for o in [-2.0f32, -1.0, 0.0, 1.0, 2.0] {
                let x = (bx + t * dx + o * cos).round();
                let y = (by + t * dy + o * sen).round();
                if x < 0.0 || y < 0.0 || x >= w as f32 || y >= h as f32 {
                    continue;
                }
                let i = mapa[y as usize * w + x as usize];
                if i == u32::MAX || tomado[i as usize] {
                    continue;
                }
                let b = &bordas[i as usize];
                if ((b.x - cx) * cos + (b.y - cy) * sen - rho).abs() <= FAIXA
                    && diferenca_de_direcao(b.normal, normal) <= JANELA
                {
                    faixa.push(((b.x - cx) * dx + (b.y - cy) * dy, i as usize));
                }
            }
            t += 1.0;
        }
        faixa.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        faixa.dedup_by_key(|p| p.1);
        if (faixa.len() as u32) < menor {
            continue;
        }
        let mut inicio = 0;
        for fim in 1..=faixa.len() {
            if fim < faixa.len() && faixa[fim].0 - faixa[fim - 1].0 <= LACUNA {
                continue;
            }
            let trecho = &faixa[inicio..fim];
            inicio = fim;
            let extensao = trecho[trecho.len() - 1].0 - trecho[0].0;
            if (trecho.len() as u32) < menor || extensao < menor as f32 {
                continue;
            }
            for (_, i) in trecho {
                tomado[*i] = true;
            }
            if let Some(reta) = ajustar(
                trecho.iter().map(|(_, i)| &bordas[*i]),
                familia == 1,
                escala,
            ) {
                retas.push(reta);
            }
        }
    }
    retas.sort_by(|a, b| b.comprimento.total_cmp(&a.comprimento));
    retas.truncate(RETAS);
    retas
}

/// A reta que melhor passa pelos pixels de um trecho — mínimos quadrados
/// totais, pelo eixo principal. O ângulo sai dos pixels, e não da grade de 0,1°
/// do acumulador.
fn ajustar<'a>(
    pixels: impl Iterator<Item = &'a Borda>,
    vertical: bool,
    escala: f32,
) -> Option<Reta> {
    let pontos: Vec<(f32, f32)> = pixels.map(|b| (b.x, b.y)).collect();
    let n = pontos.len() as f32;
    if n < 2.0 {
        return None;
    }
    let (mx, my) = pontos
        .iter()
        .fold((0.0, 0.0), |(a, b), p| (a + p.0 / n, b + p.1 / n));
    let (mut sxx, mut syy, mut sxy) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in &pontos {
        sxx += (x - mx) * (x - mx);
        syy += (y - my) * (y - my);
        sxy += (x - mx) * (y - my);
    }
    let direcao = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let (vx, vy) = (direcao.cos(), direcao.sin());
    let (mut t0, mut t1) = (f32::MAX, f32::MIN);
    for (x, y) in &pontos {
        let t = (x - mx) * vx + (y - my) * vy;
        t0 = t0.min(t);
        t1 = t1.max(t);
    }
    Some(Reta {
        correcao: -desvio_do_reto(direcao.to_degrees()),
        vertical,
        comprimento: (t1 - t0) * escala,
        de: ((mx + t0 * vx) * escala, (my + t0 * vy) * escala),
        ate: ((mx + t1 * vx) * escala, (my + t1 * vy) * escala),
    })
}

/// A diferença entre duas direções de reta, em graus (`0..=90`): 179° e 1° estão
/// a 2° uma da outra.
fn diferenca_de_direcao(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(180.0);
    d.min(180.0 - d)
}

/// O giro da foto, separado da perspectiva.
///
/// 🔑 **Nem toda reta torta é giro.** Com a câmera apontada um pouco para cima
/// ou para baixo, as verticais convergem: a da esquerda pende para um lado e a
/// da direita para o outro, e só a do meio mostra o giro. Com a câmera de
/// lado, o mesmo acontece com as horizontais. No estúdio isso é a regra — a
/// foto de família é tirada de pé, de cima para baixo.
///
/// Por isso cada reta é lida como `correção = giro + b·x` (verticais, `x` a
/// distância ao centro) ou `giro + c·y` (horizontais). O giro é o mesmo para
/// todas, e é ele que sai. O ajuste é por mínimos quadrados ponderados pelo
/// comprimento, com peso de Tukey para a reta que não concorda (a borda de um
/// móvel que não é reto, a diagonal de um tapete), e começando do giro mais
/// votado.
fn decidir(retas: Vec<Reta>, largura: f32, altura: f32) -> Option<Nivel> {
    // 🔑 **As verticais decidem.** Uma vertical do mundo — batente, perna de
    // mesa, quina — aponta para a gravidade em qualquer parede, e na foto só a
    // inclinação da câmera a desvia (o que o modelo abaixo separa). Uma
    // horizontal só sai reta na parede de frente para a câmera: o cenário tem
    // quina e a mesa é redonda, e numa foto do estúdio as verticais pediam
    // +1,1° enquanto as horizontais pediam −0,9°. As horizontais entram só
    // quando não há vertical bastante.
    let verticais: f32 = retas
        .iter()
        .filter(|r| r.vertical)
        .map(|r| r.comprimento)
        .sum();
    let bastante = largura.min(altura) * VERTICAIS_BASTAM;
    let retas: Vec<Reta> = if verticais >= bastante {
        retas.into_iter().filter(|r| r.vertical).collect()
    } else {
        retas
    };
    if retas.is_empty() {
        return None;
    }
    let raio = (largura * largura + altura * altura).sqrt() / 2.0;
    let posicao = |r: &Reta| {
        let (mx, my) = ((r.de.0 + r.ate.0) / 2.0, (r.de.1 + r.ate.1) / 2.0);
        if r.vertical {
            (mx - largura / 2.0) / raio
        } else {
            (my - altura / 2.0) / raio
        }
    };

    // O ponto de partida: o giro mais votado, com um núcleo de 0,4°.
    const SIGMA: f32 = 0.4;
    let mut melhor = (0.0f32, f32::MIN);
    let mut a = -DESVIO_MAXIMO;
    while a <= DESVIO_MAXIMO + 1e-3 {
        let d: f32 = retas
            .iter()
            .map(|r| r.comprimento * (-((r.correcao - a) / SIGMA).powi(2) / 2.0).exp())
            .sum();
        if d > melhor.1 {
            melhor = (a, d);
        }
        a += PASSO / 2.0;
    }

    let total: f32 = retas.iter().map(|r| r.comprimento).sum();
    // A convergência custa: sem retas dos dois lados para prová-la, ela fica
    // perto de zero e o giro não escorrega para dentro dela.
    let freio = FREIO * total;
    let (mut giro, mut b, mut c) = (melhor.0, 0.0f32, 0.0f32);
    let residuo = |r: &Reta, giro: f32, b: f32, c: f32| {
        r.correcao - giro - if r.vertical { b } else { c } * posicao(r)
    };
    let mut pesos = vec![0.0f32; retas.len()];
    for volta in 0..12 {
        // Tukey: largo na primeira volta, que parte do giro mais votado, e
        // 1° depois — a reta mais longe que isso do modelo não vota.
        let corte = if volta == 0 { 2.0 } else { VIZINHANCA };
        for (p, r) in pesos.iter_mut().zip(&retas) {
            let e = residuo(r, giro, b, c) / corte;
            *p = if e.abs() < 1.0 {
                r.comprimento * (1.0 - e * e).powi(2)
            } else {
                0.0
            };
        }
        // Equações normais de [giro, b, c], com o freio em b e c.
        let mut m = [[0.0f32; 3]; 3];
        let mut v = [0.0f32; 3];
        for (p, r) in pesos.iter().zip(&retas) {
            let u = posicao(r);
            let f = if r.vertical {
                [1.0, u, 0.0]
            } else {
                [1.0, 0.0, u]
            };
            for i in 0..3 {
                v[i] += p * f[i] * r.correcao;
                for j in 0..3 {
                    m[i][j] += p * f[i] * f[j];
                }
            }
        }
        m[1][1] += freio;
        m[2][2] += freio;
        let Some([g, nb, nc]) = resolver(m, v) else {
            break;
        };
        let parado = (g - giro).abs() < 1e-3;
        (giro, b, c) = (g, nb, nc);
        if parado {
            break;
        }
    }

    let usadas: Vec<Reta> = retas
        .iter()
        .zip(&pesos)
        .filter(|(_, p)| **p > 0.0)
        .map(|(r, _)| *r)
        .collect();
    let apoio = pesos.iter().sum::<f32>() / largura.min(altura);
    if usadas.len() < 2 || apoio < APOIO_MINIMO || !giro.is_finite() || giro.abs() > DESVIO_MAXIMO {
        return None;
    }
    Some(Nivel {
        angulo: arredondar(giro),
        retas: usadas,
        apoio,
    })
}

/// Cramer, para o sistema 3×3 do ajuste.
fn resolver(m: [[f32; 3]; 3], v: [f32; 3]) -> Option<[f32; 3]> {
    let det = |m: [[f32; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d = det(m);
    if d.abs() < 1e-9 {
        return None;
    }
    let mut x = [0.0; 3];
    for (k, xk) in x.iter_mut().enumerate() {
        let mut mk = m;
        for i in 0..3 {
            mk[i][k] = v[i];
        }
        *xk = det(mk) / d;
    }
    Some(x)
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::transformacao::inclinar_inteira;
    use image::{Rgba, RgbaImage};

    #[test]
    fn a_regua_leva_a_reta_para_o_reto_mais_perto() {
        // Descendo para a direita = girada no horário: endireita no anti-horário.
        assert!((angulo_da_reta(100.0, 5.0) - -2.862).abs() < 0.01);
        assert!((angulo_da_reta(100.0, -5.0) - 2.862).abs() < 0.01);
        // Traçada da direita para a esquerda, a mesma reta.
        assert!((angulo_da_reta(-100.0, -5.0) - -2.862).abs() < 0.01);
        // Quase vertical vai para a vertical: o topo à direita está girado no
        // horário.
        assert!((angulo_da_reta(5.0, -100.0) - -2.862).abs() < 0.01);
        assert!((angulo_da_reta(-5.0, 100.0) - -2.862).abs() < 0.01);
        assert_eq!(angulo_da_reta(0.0, 0.0), 0.0);
        assert_eq!(angulo_da_reta(100.0, 0.0), 0.0);
    }

    /// Uma sala de mentira: parede com janelas e batentes, piso escuro, e ruído
    /// que imita a estampa do papel de parede.
    fn sala(largura: u32, altura: u32) -> DynamicImage {
        let mut img = RgbaImage::from_pixel(largura, altura, Rgba([170, 150, 140, 255]));
        let mut semente = 12345u32;
        for (x, y, p) in img.enumerate_pixels_mut() {
            semente = semente.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let ruido = (semente >> 16) % 30;
            let v = if y > altura * 7 / 10 {
                60
            } else {
                150 + ruido as u8
            };
            *p = Rgba([v, v - 10, v - 20, 255]);
            // Duas janelas brancas com moldura.
            for jx in [largura / 5, largura * 3 / 5] {
                let dentro = x > jx && x < jx + largura / 5 && y > altura / 8 && y < altura / 2;
                if dentro {
                    *p = Rgba([235, 235, 230, 255]);
                    if (x - jx) % (largura / 15) < 3 {
                        *p = Rgba([200, 200, 195, 255]);
                    }
                }
            }
        }
        DynamicImage::ImageRgba8(img)
    }

    /// O miolo, para as bordas transparentes do giro não virarem retas.
    fn miolo(img: &DynamicImage) -> DynamicImage {
        let (w, h) = (img.width(), img.height());
        img.crop_imm(w / 6, h / 6, w * 2 / 3, h * 2 / 3)
    }

    #[test]
    fn a_foto_reta_fica_reta() {
        let nivel = angulo_automatico(&miolo(&sala(1500, 1000))).expect("tem retas");
        assert_eq!(nivel.angulo, 0.0, "{nivel:?}");
    }

    #[test]
    fn a_foto_torta_volta_ao_reto() {
        for torto in [-6.0f32, -2.5, -0.8, 0.8, 3.0, 9.0] {
            let girada = miolo(&inclinar_inteira(&sala(1500, 1000), torto));
            let nivel = angulo_automatico(&girada).expect("tem retas");
            assert!(
                (nivel.angulo + torto).abs() <= 0.15,
                "girada {torto}°, o Auto pediu {}°",
                nivel.angulo
            );
        }
    }

    /// Uma parede com batentes que convergem para um ponto de fuga bem acima da
    /// foto — a câmera apontada para baixo, como na foto de família.
    fn parede_de_cima(largura: u32, altura: u32, primeiro: u32) -> DynamicImage {
        let (cx, fuga) = (largura as f32 / 2.0, -8.0 * altura as f32);
        let mut semente = 99u32;
        let img = RgbaImage::from_fn(largura, altura, |x, y| {
            semente = semente.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let v = 150 + ((semente >> 16) % 25) as u8;
            // Um batente a cada oitavo da largura, medido no pé da foto.
            let batente = (primeiro..8).any(|k| {
                let pe = largura as f32 * k as f32 / 8.0;
                let t = (y as f32 - altura as f32) / (fuga - altura as f32);
                (x as f32 - (pe + (cx - pe) * t)).abs() < 3.0
            });
            if batente {
                Rgba([240, 240, 235, 255])
            } else {
                Rgba([v, v - 10, v - 20, 255])
            }
        });
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn a_convergencia_nao_vira_giro() {
        let reta = miolo(&parede_de_cima(1500, 1000, 1));
        assert_eq!(angulo_automatico(&reta).expect("tem retas").angulo, 0.0);
        for torto in [-2.0f32, 1.5] {
            let girada = inclinar_inteira(&parede_de_cima(1500, 1000, 1), torto);
            let nivel = angulo_automatico(&miolo(&girada)).expect("tem retas");
            assert!(
                (nivel.angulo + torto).abs() <= 0.15,
                "girada {torto}°: {}°",
                nivel.angulo
            );
            // Batentes só na metade da direita: pendem todos para o mesmo
            // lado, e o giro ainda é o do centro da foto.
            let girada = inclinar_inteira(&parede_de_cima(1500, 1000, 5), torto);
            let nivel = angulo_automatico(&miolo(&girada)).expect("tem retas");
            assert!(
                (nivel.angulo + torto).abs() <= 0.3,
                "metade, girada {torto}°: {}°",
                nivel.angulo
            );
        }
    }

    #[test]
    fn sem_reta_nao_inventa() {
        let mut semente = 7u32;
        let img = RgbaImage::from_fn(800, 600, |_, _| {
            semente = semente.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let v = 120 + ((semente >> 16) % 20) as u8;
            Rgba([v, v, v, 255])
        });
        assert_eq!(angulo_automatico(&DynamicImage::ImageRgba8(img)), None);
    }

    #[test]
    fn com_um_espelho_o_giro_troca_de_sinal() {
        let girada = miolo(&inclinar_inteira(&sala(1500, 1000), 3.0));
        let sem = angulo_automatico(&girada).unwrap().angulo;
        let espelho = crate::Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, true, false);
        let com = angulo_automatico_do_corte(&girada, &espelho)
            .unwrap()
            .angulo;
        assert_eq!(com, -sem);
        let dois = crate::Corte::novo(0.0, 0.0, 1.0, 1.0, 0, 0.0, true, true);
        assert_eq!(
            angulo_automatico_do_corte(&girada, &dois).unwrap().angulo,
            sem
        );
    }
}
