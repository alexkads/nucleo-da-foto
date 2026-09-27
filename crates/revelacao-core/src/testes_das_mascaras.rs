//! As máscaras na GPU contra a referência em CPU, nas duas entradas do motor.
//!
//! ⚠️ **Falham, e não pulam, sem GPU** — a mesma regra de `motor::testes`.

use std::sync::Arc;

use image::{DynamicImage, GenericImageView};

use crate::locais::*;
use crate::transformacao::{self, Corte};
use crate::{Ajustes, Entrada, Motor};

fn motor(entrada: Entrada) -> Motor {
    Motor::abrir_por(entrada).expect("nenhum adaptador de GPU — o motor de revelação não roda aqui")
}

/// Uma foto com textura: um degradê em dois eixos com um xadrez fino por cima.
fn foto(largura: u32, altura: u32) -> Arc<Vec<u8>> {
    Arc::new(
        (0..altura)
            .flat_map(|y| {
                (0..largura).flat_map(move |x| {
                    let xadrez = if (x / 3 + y / 3) % 2 == 0 { 12 } else { 0 };
                    let r = (40 + x * 150 / largura + xadrez) as u8;
                    let g = (60 + y * 120 / altura) as u8;
                    let b = (90 + (x + y) * 60 / (largura + altura)) as u8;
                    [r, g, b, 255]
                })
            })
            .collect(),
    )
}

fn pincel(
    pontos: Vec<[f32; 3]>,
    raio: f32,
    feather: f32,
    opacidade: f32,
    modo: Modo,
) -> Componente {
    Componente {
        modo,
        forma: Forma::Pincel(BrushStroke {
            raio,
            feather,
            opacidade,
            pontos,
        }),
    }
}

/// Uma camada que usa as três formas, os dois modos, pressão, feather e
/// stroke que volta sobre si mesmo.
fn camada_completa(ev: f32) -> Camada {
    Camada {
        ajustes: AjustesLocais { exposicao_ev: ev },
        componentes: vec![
            pincel(
                vec![
                    [0.1, 0.2, 0.3],
                    [0.5, 0.25, 1.0],
                    [0.2, 0.3, 0.6],
                    [0.6, 0.35, 1.0],
                ],
                0.06,
                0.5,
                0.8,
                Modo::Somar,
            ),
            Componente {
                modo: Modo::Somar,
                forma: Forma::Linear(GradienteLinear {
                    inicio: [0.5, 1.0],
                    fim: [0.5, 0.6],
                }),
            },
            Componente {
                modo: Modo::Somar,
                forma: Forma::Radial(GradienteRadial {
                    centro: [0.75, 0.5],
                    raio_x: 0.15,
                    raio_y: 0.08,
                    angulo: 30.0,
                    feather: 0.4,
                    fora: false,
                }),
            },
            pincel(vec![[0.4, 0.3, 1.0]], 0.05, 0.2, 1.0, Modo::Subtrair),
            // Um laço em estrela (côncavo), com feather, que se cruza com o resto.
            Componente {
                modo: Modo::Somar,
                forma: Forma::Laco(Laco {
                    pontos: (0..10)
                        .map(|k| {
                            let a = k as f32 / 10.0 * std::f32::consts::TAU;
                            let r = if k % 2 == 0 { 0.2 } else { 0.08 };
                            [0.3 + r * a.cos(), 0.65 + r * a.sin()]
                        })
                        .collect(),
                    feather: 0.03,
                }),
            },
            // E um laço que apaga.
            Componente {
                modo: Modo::Subtrair,
                forma: Forma::Laco(Laco {
                    pontos: vec![[0.7, 0.4], [0.85, 0.45], [0.8, 0.6]],
                    feather: 0.0,
                }),
            },
            // Um clique na borda, meio fora da foto.
            pincel(vec![[0.0, 0.0, 1.0]], 0.08, 0.0, 0.5, Modo::Somar),
        ],
        invertida: false,
        ..Default::default()
    }
}

fn receita(camadas: Vec<Camada>) -> ReceitaLocal {
    ReceitaLocal {
        camadas,
        ..Default::default()
    }
}

fn revelar(motor: &mut Motor, pixels: &Arc<Vec<u8>>, w: u32, h: u32, ajustes: &Ajustes) -> Vec<u8> {
    motor
        .revelar(pixels, w, h, ajustes)
        .expect("o motor não devolveu imagem")
        .into_rgba8()
        .into_raw()
}

fn maior_diferenca(a: &[u8], b: &[u8]) -> u8 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

/// 🚨 Receita vazia é a foto de antes, byte a byte — e voltar a ela depois de
/// ter usado máscara também.
#[test]
fn receita_vazia_sai_bit_a_bit_a_de_antes() {
    let (w, h) = (97, 61);
    let pixels = foto(w, h);
    let ajustes = Ajustes {
        exposure: 0.4,
        contrast: 1.2,
        saturation: 20.0,
        ..Default::default()
    };
    for entrada in [Entrada::Compute, Entrada::Fragmento] {
        let mut m = motor(entrada);
        let antes = revelar(&mut m, &pixels, w, h, &ajustes);

        m.definir_locais(&ReceitaLocal::default()).unwrap();
        assert_eq!(
            revelar(&mut m, &pixels, w, h, &ajustes),
            antes,
            "{entrada:?}"
        );

        // Máscara com exposição zero: desenhada, mas não muda nada.
        m.definir_locais(&receita(vec![camada_completa(0.0)]))
            .unwrap();
        assert_eq!(
            revelar(&mut m, &pixels, w, h, &ajustes),
            antes,
            "{entrada:?} ev 0"
        );

        m.definir_locais(&receita(vec![camada_completa(1.0)]))
            .unwrap();
        assert_ne!(revelar(&mut m, &pixels, w, h, &ajustes), antes);

        m.definir_locais(&ReceitaLocal::default()).unwrap();
        assert_eq!(
            revelar(&mut m, &pixels, w, h, &ajustes),
            antes,
            "{entrada:?} de volta"
        );
    }
}

/// A máscara da GPU é a da referência em CPU, a menos da quantização de 8 bits
/// (o stroke passa pelo rascunho `R8` antes de entrar na camada).
#[test]
fn a_mascara_da_gpu_e_a_da_referencia_em_cpu() {
    let (w, h) = (97, 61);
    let pixels = foto(w, h);
    let camada = camada_completa(1.0);
    let referencia: Vec<u8> = mascara_em_cpu(&camada, w, h)
        .iter()
        .map(|v| (v * 255.0).round() as u8)
        .collect();
    for entrada in [Entrada::Compute, Entrada::Fragmento] {
        let mut m = motor(entrada);
        m.definir_locais(&receita(vec![camada.clone()])).unwrap();
        revelar(&mut m, &pixels, w, h, &Ajustes::default());
        let gpu = m.ler_camada_da_mascara(w, h, 0);
        let pior = maior_diferenca(&gpu, &referencia);
        assert!(pior <= 3, "{entrada:?}: diferença de {pior} níveis");
        assert!(gpu.iter().any(|&v| v > 200) && gpu.contains(&0));
    }
}

/// 🚨 O requisito do pincel, na GPU: passar e voltar no mesmo gesto não passa
/// da opacidade.
#[test]
fn na_gpu_o_stroke_que_volta_nao_passa_da_opacidade() {
    let (w, h) = (64, 64);
    let pontos = (0..60)
        .map(|i| {
            let t = (i % 20) as f32 / 19.0;
            let x = if (i / 20) % 2 == 0 { t } else { 1.0 - t };
            [0.2 + 0.6 * x, 0.5, 1.0]
        })
        .collect();
    let mut m = motor(Entrada::Compute);
    m.definir_locais(&receita(vec![Camada {
        ajustes: AjustesLocais { exposicao_ev: 1.0 },
        componentes: vec![pincel(pontos, 0.1, 0.5, 0.5, Modo::Somar)],
        invertida: false,
        ..Default::default()
    }]))
    .unwrap();
    revelar(&mut m, &foto(w, h), w, h, &Ajustes::default());
    let pico = *m.ler_camada_da_mascara(w, h, 0).iter().max().unwrap();
    assert!(
        (127..=128).contains(&pico),
        "pico {pico}, esperava 0,5 × 255"
    );
}

/// As duas entradas do motor revelam o mesmo pixel com máscara — e a de
/// fragmento com os limites do WebGL2, que é o que o navegador sem WebGPU tem.
#[test]
fn compute_fragmento_e_limites_do_webgl2_revelam_o_mesmo_com_mascara() {
    let (w, h) = (97, 61);
    let pixels = foto(w, h);
    let locais = receita(vec![camada_completa(1.5), {
        let mut c = camada_completa(-1.0);
        c.invertida = true;
        c
    }]);
    let ajustes = Ajustes {
        contrast: 1.1,
        ..Default::default()
    };
    let mut compute = motor(Entrada::Compute);
    compute.definir_locais(&locais).unwrap();
    let a = revelar(&mut compute, &pixels, w, h, &ajustes);

    let mut fragmento = motor(Entrada::Fragmento);
    fragmento.definir_locais(&locais).unwrap();
    let b = revelar(&mut fragmento, &pixels, w, h, &ajustes);
    assert!(maior_diferenca(&a, &b) <= 1);

    let instancia = wgpu::Instance::default();
    let adaptador =
        pollster::block_on(instancia.request_adapter(&Default::default())).expect("adaptador");
    let limites = wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adaptador.limits());
    let mut webgl2 = pollster::block_on(Motor::abrir_com(&adaptador, Entrada::Fragmento, limites))
        .expect("o motor abre com os limites do WebGL2");
    webgl2.definir_locais(&locais).unwrap();
    let c = revelar(&mut webgl2, &pixels, w, h, &ajustes);
    assert!(maior_diferenca(&a, &c) <= 1);
}

/// +1 EV com máscara cheia dobra a luz **linear** — não o valor sRGB.
#[test]
fn a_exposicao_local_e_em_rgb_linear() {
    let (w, h) = (16, 16);
    let cinza = Arc::new([100u8, 100, 100, 255].repeat((w * h) as usize));
    let mut m = motor(Entrada::Compute);
    m.definir_locais(&receita(vec![Camada {
        ajustes: AjustesLocais { exposicao_ev: 1.0 },
        componentes: vec![pincel(vec![[0.5, 0.5, 1.0]], 1.0, 0.0, 1.0, Modo::Somar)],
        invertida: false,
        ..Default::default()
    }]))
    .unwrap();
    let saida = revelar(&mut m, &cinza, w, h, &Ajustes::default());
    let lin = |v: f32| {
        let x = v / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    let srgb = |v: f32| {
        if v <= 0.0031308 {
            v * 12.92 * 255.0
        } else {
            (1.055 * v.powf(1.0 / 2.4) - 0.055) * 255.0
        }
    };
    let esperado = srgb(lin(100.0) * 2.0).round() as i32;
    assert_eq!(esperado, 138, "a conta de referência");
    assert!(
        (saida[0] as i32 - esperado).abs() <= 1,
        "saiu {}, esperava {esperado} (em sRGB seria 200)",
        saida[0]
    );
}

/// 🚨 Preview e exportação: a máscara é refeita dos parâmetros em cada
/// resolução, e a do arquivo reduzida é a do preview — não o preview ampliado.
#[test]
fn a_mascara_do_preview_e_a_da_exportacao_reduzida() {
    let (w, h) = (300, 198);
    let (pw, ph) = (100, 66);
    let camada = camada_completa(1.0);
    let mut m = motor(Entrada::Compute);
    m.definir_locais(&receita(vec![camada])).unwrap();
    revelar(&mut m, &foto(w, h), w, h, &Ajustes::default());
    let cheia = m.ler_camada_da_mascara(w, h, 0);
    revelar(&mut m, &foto(pw, ph), pw, ph, &Ajustes::default());
    let preview = m.ler_camada_da_mascara(pw, ph, 0);

    let mut soma = 0u64;
    let mut pior = 0u8;
    for y in 0..ph {
        for x in 0..pw {
            let mut media = 0u32;
            for dy in 0..3 {
                for dx in 0..3 {
                    media += cheia[((y * 3 + dy) * w + x * 3 + dx) as usize] as u32;
                }
            }
            let reduzida = ((media + 4) / 9) as u8;
            let d = reduzida.abs_diff(preview[(y * pw + x) as usize]);
            soma += d as u64;
            pior = pior.max(d);
        }
    }
    let media = soma as f32 / (pw * ph) as f32;
    assert!(media < 1.5, "diferença média de {media} níveis");
    // 🔑 Só as bordas duras (o clique no canto e o laço que apaga, ambos com
    // feather 0) divergem mais: num pixel de borda dura o preview amostra o
    // centro (0 ou 1) e o arquivo reduzido é a média de 9 — até ~meia escala.
    // A média acima é o que mede a paridade.
    assert!(pior <= 140, "pior {pior}");
}

/// A máscara é da foto, não do quadro: com giro e espelho, o ponto pintado
/// vai para onde o `transformacao::aplicar` leva aquele pixel da foto.
#[test]
fn a_mascara_acompanha_a_foto_no_corte_e_no_giro() {
    let (w, h) = (80, 50);
    let preto = Arc::new([0u8, 0, 0, 255].repeat((w * h) as usize));
    let mut claro = preto.as_ref().clone();
    let (px, py) = (20u32, 12u32);
    for c in 0..3 {
        claro[((py * w + px) * 4 + c) as usize] = 255;
    }
    let corte = Corte::novo(0.1, 0.0, 0.8, 0.9, 1, 0.0, true, false);

    let mut m = motor(Entrada::Compute);
    m.definir_corte(&corte);
    m.definir_locais(&receita(vec![Camada {
        ajustes: AjustesLocais { exposicao_ev: 5.0 },
        componentes: vec![pincel(
            vec![[
                (px as f32 + 0.5) / w as f32,
                (py as f32 + 0.5) / h as f32,
                1.0,
            ]],
            0.4 / w as f32,
            0.0,
            1.0,
            Modo::Somar,
        )],
        invertida: false,
        ..Default::default()
    }]))
    .unwrap();
    // Um cinza escuro, para a exposição ter o que clarear.
    let cinza = Arc::new([30u8, 30, 30, 255].repeat((w * h) as usize));
    let revelada = m.revelar(&cinza, w, h, &Ajustes::default()).unwrap();
    let marcador = DynamicImage::ImageRgba8(image::RgbaImage::from_raw(w, h, claro).unwrap());

    let onde = |img: &DynamicImage| {
        let img = transformacao::aplicar(img, &corte, true);
        let mut melhor = (0, 0, 0u8);
        for (x, y, p) in img.pixels() {
            if p[0] > melhor.2 {
                melhor = (x, y, p[0]);
            }
        }
        (melhor.0, melhor.1)
    };
    assert_eq!(onde(&revelada), onde(&marcador));
}

/// Só o que mudou é desenhado: stroke novo no fim desenha só ele, e mudar uma
/// camada não refaz a outra.
#[test]
fn so_o_que_mudou_e_rasterizado() {
    let (w, h) = (64, 64);
    let pixels = foto(w, h);
    let traco = |x| pincel(vec![[x, 0.5, 1.0]], 0.05, 0.3, 1.0, Modo::Somar);
    let camada = |componentes| Camada {
        ajustes: AjustesLocais { exposicao_ev: 1.0 },
        componentes,
        invertida: false,
        ..Default::default()
    };
    let mut m = motor(Entrada::Compute);
    let passo = |m: &mut Motor, r: ReceitaLocal| {
        m.definir_locais(&r).unwrap();
        revelar(m, &pixels, w, h, &Ajustes::default());
        let md = m.medidas_dos_locais();
        (md.camadas_refeitas, md.componentes_desenhados)
    };

    let fixa = camada(vec![traco(0.8)]);
    assert_eq!(
        passo(
            &mut m,
            receita(vec![camada(vec![traco(0.2)]), fixa.clone()])
        ),
        (2, 2)
    );
    // Stroke novo no fim da primeira: só ele.
    assert_eq!(
        passo(
            &mut m,
            receita(vec![camada(vec![traco(0.2), traco(0.4)]), fixa.clone()])
        ),
        (0, 1)
    );
    // Nada mudou: nada é desenhado.
    assert_eq!(
        passo(
            &mut m,
            receita(vec![camada(vec![traco(0.2), traco(0.4)]), fixa.clone()])
        ),
        (0, 0)
    );
    // Desfazer o primeiro stroke refaz só a primeira camada.
    assert_eq!(
        passo(
            &mut m,
            receita(vec![camada(vec![traco(0.4)]), fixa.clone()])
        ),
        (1, 1)
    );

    // E a máscara incremental é a mesma que a feita de uma vez.
    let incremental = m.ler_camada_da_mascara(w, h, 0);
    let mut limpo = motor(Entrada::Compute);
    passo(&mut limpo, receita(vec![camada(vec![traco(0.4)]), fixa]));
    assert_eq!(limpo.ler_camada_da_mascara(w, h, 0), incremental);
}

#[test]
fn a_gpu_desta_maquina_desenha_mascara() {
    assert!(motor(Entrada::Compute).mascaras_suportadas());
}

// --------------------------------------------------------------- retoques

/// Uma foto com textura forte (xadrez de 2 px) sobre um degradê de luz: a
/// esquerda escura, a direita clara.
fn foto_com_luz(largura: u32, altura: u32) -> Arc<Vec<u8>> {
    Arc::new(
        (0..altura)
            .flat_map(|y| {
                (0..largura).flat_map(move |x| {
                    let luz = 40 + x * 160 / largura;
                    let v = (luz + if (x / 2 + y / 2) % 2 == 0 { 20 } else { 0 }) as u8;
                    [v, v, v, 255]
                })
            })
            .collect(),
    )
}

fn carimbo(origem: [f32; 2], destino: [f32; 2], raio: f32, feather: f32) -> Carimbo {
    Carimbo {
        origem,
        destino_inicial: destino,
        caminho: vec![destino],
        raio,
        feather,
        opacidade: 1.0,
    }
}

fn com_retoques(retoques: Vec<Retoque>) -> ReceitaLocal {
    ReceitaLocal {
        retoques,
        ..Default::default()
    }
}

/// A média de um canal num quadrado de lado `2·meio` em volta de `(cx, cy)`.
fn media(img: &[u8], largura: u32, cx: u32, cy: u32, meio: u32) -> f32 {
    let mut soma = 0.0;
    let mut n = 0.0;
    for y in cy - meio..cy + meio {
        for x in cx - meio..cx + meio {
            soma += img[((y * largura + x) * 4) as usize] as f32;
            n += 1.0;
        }
    }
    soma / n
}

/// O Clone copia: o centro do destino é o pixel da origem.
#[test]
fn o_clone_copia_a_origem_para_o_destino() {
    let (w, h) = (160, 100);
    let pixels = foto_com_luz(w, h);
    let mut m = motor(Entrada::Compute);
    m.definir_locais(&com_retoques(vec![Retoque::Clone(carimbo(
        [0.25, 0.5],
        [0.75, 0.5],
        0.08,
        0.0,
    ))]))
    .unwrap();
    let saida = revelar(&mut m, &pixels, w, h, &Ajustes::default());
    for (dx, dy) in [(0i32, 0i32), (3, 1), (-4, 2)] {
        let destino = ((50 + dy) * w as i32 + (120 + dx)) as usize * 4;
        let origem = ((50 + dy) * w as i32 + (40 + dx)) as usize * 4;
        assert_eq!(saida[destino], pixels[origem], "({dx},{dy})");
    }
    // Fora do raio, nada muda.
    assert_eq!(
        saida[(50 * w + 150) as usize * 4],
        pixels[(50 * w + 150) as usize * 4]
    );
    assert_eq!(
        pixels.len(),
        (w * h * 4) as usize,
        "a origem continua a mesma"
    );
}

/// 🚨 Política de fonte fora da foto: não pinta — o destino fica como estava.
#[test]
fn fonte_fora_da_foto_nao_pinta() {
    let (w, h) = (160, 100);
    let pixels = foto_com_luz(w, h);
    let mut m = motor(Entrada::Compute);
    m.definir_locais(&ReceitaLocal::default()).unwrap();
    let antes = revelar(&mut m, &pixels, w, h, &Ajustes::default());
    // A origem 30 px à esquerda da borda: metade do carimbo tem fonte fora.
    m.definir_locais(&com_retoques(vec![Retoque::Clone(carimbo(
        [-0.05, 0.5],
        [0.1, 0.5],
        0.1,
        0.0,
    ))]))
    .unwrap();
    let depois = revelar(&mut m, &pixels, w, h, &Ajustes::default());
    // Pixel do destino cuja fonte cai fora (x=4 → fonte x=-20): intacto.
    let i = (50 * w + 4) as usize * 4;
    assert_eq!(depois[i], antes[i]);
    // Pixel do destino com fonte dentro (x=28 → fonte x=4): copiado.
    let j = (50 * w + 28) as usize * 4;
    assert_eq!(depois[j], pixels[(50 * w + 4) as usize * 4]);
}

/// 🚨 O Heal não é carimbo: numa área de outra luz, ele leva a luz do destino
/// e mantém a textura da fonte. O Clone leva a luz da fonte junto.
#[test]
fn o_heal_adapta_a_luz_ao_destino_e_o_clone_nao() {
    let (w, h) = (200, 100);
    let pixels = foto_com_luz(w, h);
    // A fonte escura (x = 40) vai para o destino claro (x = 160).
    let c = carimbo([0.2, 0.5], [0.8, 0.5], 0.06, 0.3);
    let mut m = motor(Entrada::Compute);
    m.definir_locais(&ReceitaLocal::default()).unwrap();
    let original = revelar(&mut m, &pixels, w, h, &Ajustes::default());
    m.definir_locais(&com_retoques(vec![Retoque::Clone(c.clone())]))
        .unwrap();
    let clone = revelar(&mut m, &pixels, w, h, &Ajustes::default());
    m.definir_locais(&com_retoques(vec![Retoque::Heal(c)]))
        .unwrap();
    let heal = revelar(&mut m, &pixels, w, h, &Ajustes::default());

    let alvo = media(&original, w, 160, 50, 4);
    let (mc, mh) = (media(&clone, w, 160, 50, 4), media(&heal, w, 160, 50, 4));
    assert!(
        (mc - alvo).abs() > 40.0,
        "o clone leva a luz da fonte: {mc} vs {alvo}"
    );
    assert!(
        (mh - alvo).abs() < 8.0,
        "o heal fica com a luz do destino: {mh} vs {alvo}"
    );

    // E a textura (o xadrez) é a da fonte: a diferença entre vizinhos continua.
    let contraste = |img: &[u8]| {
        (150..170)
            .map(|x| {
                img[((50 * w + x) * 4) as usize].abs_diff(img[((50 * w + x + 2) * 4) as usize])
                    as f32
            })
            .sum::<f32>()
            / 20.0
    };
    assert!(
        contraste(&heal) > 8.0,
        "o remendo tem textura, não é mancha"
    );
}

/// A cadeia: o segundo retoque lê o resultado do primeiro, e a cadeia pronta
/// não é refeita — retoque novo no fim roda só ele.
#[test]
fn a_cadeia_de_retoques_e_reaproveitada() {
    let (w, h) = (160, 100);
    let pixels = foto_com_luz(w, h);
    let um = Retoque::Clone(carimbo([0.2, 0.3], [0.5, 0.3], 0.05, 0.2));
    let dois = Retoque::Heal(carimbo([0.2, 0.7], [0.5, 0.7], 0.05, 0.2));
    let mut m = motor(Entrada::Compute);
    let passo = |m: &mut Motor, r: Vec<Retoque>| {
        m.definir_locais(&com_retoques(r)).unwrap();
        let s = revelar(m, &pixels, w, h, &Ajustes::default());
        (m.medidas_dos_locais().retoques_aplicados, s)
    };
    assert_eq!(passo(&mut m, vec![um.clone()]).0, 1);
    assert_eq!(passo(&mut m, vec![um.clone()]).0, 0, "nada mudou");
    let (n, incremental) = passo(&mut m, vec![um.clone(), dois.clone()]);
    assert_eq!(n, 1, "só o novo");
    let mut limpo = motor(Entrada::Compute);
    let (n, de_uma_vez) = passo(&mut limpo, vec![um.clone(), dois]);
    assert_eq!(n, 2);
    assert_eq!(incremental, de_uma_vez);
    // Desfazer o segundo volta ao resultado de só o primeiro.
    let (_, desfeito) = passo(&mut m, vec![um.clone()]);
    assert_eq!(desfeito, passo(&mut limpo, vec![um]).1);
}

/// Compute, fragmento e os limites do WebGL2 retocam o mesmo pixel.
#[test]
fn retoques_iguais_nas_duas_entradas_e_no_webgl2() {
    let (w, h) = (97, 61);
    let pixels = foto_com_luz(w, h);
    let locais = ReceitaLocal {
        camadas: vec![camada_completa(1.0)],
        retoques: vec![
            Retoque::Clone(carimbo([0.2, 0.3], [0.6, 0.3], 0.05, 0.3)),
            Retoque::Heal(Carimbo {
                caminho: vec![[0.5, 0.7], [0.7, 0.75]],
                ..carimbo([0.2, 0.7], [0.5, 0.7], 0.04, 0.4)
            }),
        ],
        ..Default::default()
    };
    let mut a = motor(Entrada::Compute);
    a.definir_locais(&locais).unwrap();
    let x = revelar(&mut a, &pixels, w, h, &Ajustes::default());
    let mut b = motor(Entrada::Fragmento);
    b.definir_locais(&locais).unwrap();
    assert!(maior_diferenca(&x, &revelar(&mut b, &pixels, w, h, &Ajustes::default())) <= 1);

    let instancia = wgpu::Instance::default();
    let adaptador = pollster::block_on(instancia.request_adapter(&Default::default())).unwrap();
    let limites = wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adaptador.limits());
    let mut c =
        pollster::block_on(Motor::abrir_com(&adaptador, Entrada::Fragmento, limites)).unwrap();
    c.definir_locais(&locais).unwrap();
    assert!(maior_diferenca(&x, &revelar(&mut c, &pixels, w, h, &Ajustes::default())) <= 1);
}

/// 🚨 Content-Aware pelo motor: some a mancha, fica a textura, e sai igual nas
/// duas entradas e com os limites do WebGL2 (a síntese é Rust determinístico,
/// a mistura é o mesmo shader).
#[test]
fn o_content_aware_tira_a_mancha_e_mantem_a_textura() {
    let (w, h) = (160, 120);
    let mut img: Vec<u8> = (0..h)
        .flat_map(|y| {
            (0..w).flat_map(move |x| {
                let v = if (x / 4) % 2 == 0 { 60 } else { 180 } + ((x * 7 + y * 13) % 9) as u8;
                [v, v, v, 255]
            })
        })
        .collect();
    // Uma mancha vermelha no meio.
    for y in 55..65 {
        for x in 75..85 {
            let i = ((y * w + x) * 4) as usize;
            img[i..i + 3].copy_from_slice(&[250, 20, 20]);
        }
    }
    let pixels = Arc::new(img);
    let locais = com_retoques(vec![Retoque::Preencher(Preenchimento {
        caminho: vec![[0.5, 0.5]],
        raio: 0.06,
        feather: 0.2,
        opacidade: 1.0,
        laco: Vec::new(),
    })]);
    let mut a = motor(Entrada::Compute);
    a.definir_locais(&locais).unwrap();
    let saida = revelar(&mut a, &pixels, w, h, &Ajustes::default());
    assert!(a.medidas_dos_locais().sintese_ms > 0.0);

    let vermelhos = (55..65)
        .flat_map(|y| (75..85).map(move |x| ((y * w + x) * 4) as usize))
        .filter(|&i| saida[i] > 200 && saida[i + 1] < 60)
        .count();
    assert_eq!(vermelhos, 0, "a mancha sumiu");
    let linha: Vec<u8> = (70..90)
        .map(|x| saida[((60 * w + x) * 4) as usize])
        .collect();
    assert!(
        linha.iter().any(|&v| v < 100) && linha.iter().any(|&v| v > 150),
        "as listras atravessam o remendo: {linha:?}"
    );

    // Revelar de novo não sintetiza de novo.
    revelar(&mut a, &pixels, w, h, &Ajustes::default());
    assert_eq!(a.medidas_dos_locais().sintese_ms, 0.0);

    let mut b = motor(Entrada::Fragmento);
    b.definir_locais(&locais).unwrap();
    assert!(maior_diferenca(&saida, &revelar(&mut b, &pixels, w, h, &Ajustes::default())) <= 1);
}

/// Content-Aware por laço: cercar a mancha basta — sem raio, sem pincel.
#[test]
fn o_content_aware_por_laco_tira_o_que_foi_cercado() {
    let (w, h) = (160, 120);
    let mut img: Vec<u8> = (0..h)
        .flat_map(|y| {
            (0..w).flat_map(move |x| {
                let v = if (x / 4) % 2 == 0 { 60 } else { 180 } + ((x * 7 + y * 13) % 9) as u8;
                [v, v, v, 255]
            })
        })
        .collect();
    // Uma mancha comprida e torta, que um círculo cobriria mal.
    for y in 40..80u32 {
        let x0 = 60 + (y - 40) / 2;
        for x in x0..x0 + 8 {
            let i = ((y * w + x) * 4) as usize;
            img[i..i + 3].copy_from_slice(&[250, 20, 20]);
        }
    }
    let pixels = Arc::new(img);
    let (fw, fh) = (w as f32, h as f32);
    let laco = vec![
        [56.0 / fw, 36.0 / fh],
        [72.0 / fw, 36.0 / fh],
        [92.0 / fw, 84.0 / fh],
        [76.0 / fw, 84.0 / fh],
    ];
    let locais = com_retoques(vec![Retoque::Preencher(Preenchimento {
        caminho: Vec::new(),
        raio: 0.01,
        feather: 0.01,
        opacidade: 1.0,
        laco,
    })]);
    let mut a = motor(Entrada::Compute);
    a.definir_locais(&locais).unwrap();
    let saida = revelar(&mut a, &pixels, w, h, &Ajustes::default());
    let vermelhos = saida.chunks(4).filter(|p| p[0] > 200 && p[1] < 60).count();
    assert_eq!(vermelhos, 0, "nada da mancha sobrou");
    // Fora do laço, a foto é a mesma.
    assert_eq!(
        saida[((10 * w + 10) * 4) as usize],
        pixels[((10 * w + 10) * 4) as usize]
    );

    let mut b = motor(Entrada::Fragmento);
    b.definir_locais(&locais).unwrap();
    assert!(maior_diferenca(&saida, &revelar(&mut b, &pixels, w, h, &Ajustes::default())) <= 1);
}

#[test]
fn sonda_faixa_do_band_aid() {
    let (w, h) = (1200, 800);
    let pixels = foto_com_luz(w, h);
    let mut m = motor(Entrada::Compute);
    m.definir_locais(&ReceitaLocal::default()).unwrap();
    let original = revelar(&mut m, &pixels, w, h, &Ajustes::default());
    let c = Carimbo {
        caminho: vec![[0.5, 0.4], [0.505, 0.4]],
        ..carimbo([0.45, 0.35], [0.5, 0.4], 0.03, 0.5)
    };
    m.definir_locais(&com_retoques(vec![Retoque::Heal(c)])).unwrap();
    let heal = revelar(&mut m, &pixels, w, h, &Ajustes::default());
    let mut colunas = std::collections::BTreeMap::new();
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            if heal[i] != original[i] {
                let e = colunas.entry(x).or_insert((y, y, 0));
                e.1 = y;
                e.2 += 1;
            }
        }
    }
    for (x, (a, b, n)) in &colunas {
        if b - a > 100 { eprintln!("coluna {x}: y {a}..{b} ({n})"); }
    }
    eprintln!("colunas mudadas: {}", colunas.len());
}
