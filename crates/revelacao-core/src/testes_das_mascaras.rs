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
            // Um clique na borda, meio fora da foto.
            pincel(vec![[0.0, 0.0, 1.0]], 0.08, 0.0, 0.5, Modo::Somar),
        ],
        invertida: false,
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
    // Só a borda dura do clique no canto (feather 0) pode divergir mais.
    assert!(pior < 70, "pior {pior}");
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
