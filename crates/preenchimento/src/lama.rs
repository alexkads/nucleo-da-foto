//! 🤖 LaMa local: a rede de Suvorov et al. (2021, "Resolution-robust Large
//! Mask Inpainting with Fourier Convolutions"), em ONNX, rodando no computador
//! do balcão pelo `ia-local`.
//!
//! ## O modelo (verificado em 06/out/2026)
//!
//! `lama_fp32.onnx` de huggingface.co/Carve/LaMa-ONNX (commit `c3c0c9e`),
//! 208 044 816 bytes, SHA-256 `1faef530…68d6`. Apache-2.0 nos pesos, no port
//! exportável (Carve-Photos/lama) e no original (advimman/lama).
//!
//! - entradas `image` `[1, 3, 512, 512]` e `mask` `[1, 1, 512, 512]`, `f32`;
//!   a imagem em 0..1, a máscara 1 onde reconstruir;
//! - saída `output` `[1, 3, 512, 512]`, já em 0..255;
//! - 🚨 **os canais são BGR**: com RGB (como faz a demonstração do autor), o
//!   miolo sai azulado. Achado no primeiro teste real e conferido trocando a
//!   ordem.
//!
//! ## A resolução
//!
//! A entrada é **fixa em 512 × 512**. O recorte em volta do destino (com a
//! margem de contexto) entra na resolução da foto quando cabe — o resto é
//! preenchido por espelho, como no pré-processamento original —, e é
//! **reduzido** quando passa disso; o resultado é então ampliado de volta, e a
//! tela avisa a redução (não é reconstrução nativa em alta resolução).
//!
//! ## O backend (medido num Mac com Apple Silicon)
//!
//! CPU: 2,0 s por inferência, 1,1 GB. CoreML: 12 a 42 s, 2,1 GB — as FFTs do
//! modelo voltam para a CPU e o vaivém custa mais do que ganha. O automático é
//! a CPU; CoreML, DirectML e CUDA ficam na escolha manual.

use std::path::PathBuf;

use ia_local::execucao::{self, Backend, ErroDeExecucao, Tensor32};
use ia_local::modelos::{self, Estado, Modelo};

use crate::{caixa_do_destino, Controle, Entrada, Erro, Metodo, Motor, Progresso, Saida};

pub const MODELO: Modelo = Modelo {
    id: "lama-fp32",
    nome: "LaMa (big-lama, ONNX fp32)",
    arquivo: "lama_fp32.onnx",
    versao: "Carve/LaMa-ONNX @ c3c0c9e4",
    url: "https://huggingface.co/Carve/LaMa-ONNX/resolve/c3c0c9e468934d62e79c329e35d82dd09ff8c444/lama_fp32.onnx",
    bytes: 208_044_816,
    sha256: "1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6",
    licenca: "Apache-2.0",
    origem: "advimman/lama (Samsung AI Center), exportado para ONNX por Carve-Photos/lama",
};

/// O lado da entrada do modelo.
pub const LADO: u32 = 512;

/// O backend do automático: a CPU (a medida está no topo do módulo).
pub const BACKEND_AUTOMATICO: Backend = Backend::Cpu;

pub struct LaMa {
    pub pasta: PathBuf,
    pub backend: Backend,
}

/// O recorte de contexto `(x, y, largura, altura)` na região: um quadrado em
/// volta da caixa do destino, `contexto` (fração do lado do destino) de cada
/// lado, preso à região.
pub fn recorte_de_contexto(
    caixa: (u32, u32, u32, u32),
    largura: u32,
    altura: u32,
    contexto: f32,
) -> (u32, u32, u32, u32) {
    let (x0, y0, x1, y1) = caixa;
    let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
    let maior = bw.max(bh) as f32;
    let lado = (maior * (1.0 + 2.0 * contexto.clamp(0.0, 3.0)))
        .max(maior + 16.0)
        .round() as u32;
    let (cx, cy) = (x0 as f32 + bw as f32 / 2.0, y0 as f32 + bh as f32 / 2.0);
    let eixo = |centro: f32, total: u32| -> (u32, u32) {
        let l = lado.min(total);
        let ini = (centro - l as f32 / 2.0).round().max(0.0) as u32;
        (ini.min(total - l), l)
    };
    let (rx, rw) = eixo(cx, largura);
    let (ry, rh) = eixo(cy, altura);
    (rx, ry, rw, rh)
}

/// A entrada do modelo: a imagem BGR em 0..1 e a máscara, 512 × 512, com o
/// recorte reduzido por `escala` (média da área) e espelhado no que sobra.
pub struct Preparado {
    pub imagem: Vec<f32>,
    pub mascara: Vec<f32>,
    /// O tamanho útil (sem o espelho) dentro dos 512.
    pub util: (u32, u32),
    /// 512 ÷ maior lado do recorte, até 1 (sem ampliar).
    pub escala: f32,
}

pub fn preparar(e: &Entrada, recorte: (u32, u32, u32, u32)) -> Preparado {
    let (rx, ry, rw, rh) = recorte;
    let escala = (LADO as f32 / rw.max(rh) as f32).min(1.0);
    let (uw, uh) = (
        ((rw as f32 * escala).round() as u32).clamp(1, LADO),
        ((rh as f32 * escala).round() as u32).clamp(1, LADO),
    );
    let n = (LADO * LADO) as usize;
    let mut imagem = vec![0f32; 3 * n];
    let mut mascara = vec![0f32; n];
    // A área de cada pixel útil no recorte.
    let (fx, fy) = (rw as f32 / uw as f32, rh as f32 / uh as f32);
    let mut util_bgr = vec![[0f32; 3]; (uw * uh) as usize];
    for y in 0..uh {
        let (sy0, sy1) = (
            (y as f32 * fy) as u32,
            (((y + 1) as f32 * fy).ceil() as u32).min(rh).max(1),
        );
        for x in 0..uw {
            let (sx0, sx1) = (
                (x as f32 * fx) as u32,
                (((x + 1) as f32 * fx).ceil() as u32).min(rw).max(1),
            );
            let mut soma = [0f32; 3];
            let mut conta = 0f32;
            let mut buraco = false;
            for sy in sy0..sy1.max(sy0 + 1) {
                for sx in sx0..sx1.max(sx0 + 1) {
                    let i = ((ry + sy) * e.largura + rx + sx) as usize;
                    // BGR (ver o topo do módulo).
                    soma[0] += e.rgba[i * 4 + 2] as f32;
                    soma[1] += e.rgba[i * 4 + 1] as f32;
                    soma[2] += e.rgba[i * 4] as f32;
                    conta += 1.0;
                    buraco |= e.destino[i];
                }
            }
            util_bgr[(y * uw + x) as usize] = soma.map(|v| v / conta / 255.0);
            if buraco {
                mascara[(y * LADO + x) as usize] = 1.0;
            }
        }
    }
    // O espelho (simétrico, como o `pad_img_to_modulo` do LaMa).
    let espelho = |v: u32, n: u32| -> u32 {
        let periodo = 2 * n;
        let m = v % periodo;
        if m < n {
            m
        } else {
            periodo - 1 - m
        }
    };
    for y in 0..LADO {
        for x in 0..LADO {
            let p = util_bgr[(espelho(y, uh) * uw + espelho(x, uw)) as usize];
            for c in 0..3 {
                imagem[c * n + (y * LADO + x) as usize] = p[c];
            }
        }
    }
    Preparado {
        imagem,
        mascara,
        util: (uw, uh),
        escala,
    }
}

/// O remendo da caixa do destino, em RGBA, a partir da saída do modelo
/// (BGR em 0..255, 512 × 512): dentro do destino, a saída amostrada de volta
/// (bilinear) nas coordenadas da região; fora dele, a própria entrada.
pub fn reconstruir(
    e: &Entrada,
    caixa: (u32, u32, u32, u32),
    recorte: (u32, u32, u32, u32),
    p: &Preparado,
    saida: &[f32],
) -> Saida {
    let (x0, y0, x1, y1) = caixa;
    let (rx, ry, rw, rh) = recorte;
    let (uw, uh) = p.util;
    let n = (LADO * LADO) as usize;
    let (fx, fy) = (uw as f32 / rw as f32, uh as f32 / rh as f32);
    let amostra = |c: usize, x: f32, y: f32| -> f32 {
        let (x, y) = (x.clamp(0.0, uw as f32 - 1.0), y.clamp(0.0, uh as f32 - 1.0));
        let (xa, ya) = (x.floor() as u32, y.floor() as u32);
        let (xb, yb) = ((xa + 1).min(uw - 1), (ya + 1).min(uh - 1));
        let (tx, ty) = (x - xa as f32, y - ya as f32);
        let v = |a: u32, b: u32| saida[c * n + (b * LADO + a) as usize];
        (v(xa, ya) * (1.0 - tx) + v(xb, ya) * tx) * (1.0 - ty)
            + (v(xa, yb) * (1.0 - tx) + v(xb, yb) * tx) * ty
    };
    let (largura, altura) = (x1 - x0 + 1, y1 - y0 + 1);
    let mut rgba = Vec::with_capacity((largura * altura * 4) as usize);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let i = (y * e.largura + x) as usize;
            if e.destino[i] {
                let (ux, uy) = ((x - rx) as f32 + 0.5, (y - ry) as f32 + 0.5);
                let (sx, sy) = (ux * fx - 0.5, uy * fy - 0.5);
                let canal = |c: usize| amostra(c, sx, sy).round().clamp(0.0, 255.0) as u8;
                rgba.extend_from_slice(&[canal(2), canal(1), canal(0), 255]);
            } else {
                rgba.extend_from_slice(&[e.rgba[i * 4], e.rgba[i * 4 + 1], e.rgba[i * 4 + 2], 255]);
            }
        }
    }
    Saida {
        x0,
        y0,
        largura,
        altura,
        rgba,
        reducao: 1.0 / p.escala,
        executado_em: String::new(),
    }
}

/// A assinatura que um arquivo importado tem de ter para ser aceito no lugar
/// do publicado.
pub fn conferir_assinatura(caminho: &std::path::Path) -> Result<(), String> {
    let (entradas, saidas) = execucao::assinatura(caminho)?;
    let tem = |lista: &[(String, Vec<i64>)], nome: &str, canais: i64| {
        lista.iter().any(|(n, f)| {
            n == nome
                && f.len() == 4
                && f[1] == canais
                && f[2] == LADO as i64
                && f[3] == LADO as i64
        })
    };
    if !tem(&entradas, "image", 3) || !tem(&entradas, "mask", 1) {
        return Err("as entradas não são image [·,3,512,512] e mask [·,1,512,512]".into());
    }
    if !saidas
        .iter()
        .any(|(_, f)| f.len() == 4 && f[1] == 3 && f[2] == LADO as i64 && f[3] == LADO as i64)
    {
        return Err("a saída não é uma imagem [·,3,512,512]".into());
    }
    Ok(())
}

impl Motor for LaMa {
    fn metodo(&self) -> Metodo {
        Metodo::LaMa
    }

    fn preencher(&self, e: &Entrada, controle: &Controle) -> Result<Saida, Erro> {
        let caminho = match modelos::estado(&self.pasta, &MODELO) {
            Estado::Instalado { caminho, .. } => caminho,
            Estado::Ausente => return Err(Erro::ModeloAusente),
            Estado::Corrompido { .. } => {
                return Err(Erro::ModeloInvalido(
                    "o arquivo instalado está incompleto — remova e baixe de novo".into(),
                ))
            }
        };
        let caixa = caixa_do_destino(e.destino, e.largura).ok_or(Erro::SemDestino)?;
        let indeterminado = |etapa: &str| {
            (controle.progresso)(Progresso {
                fracao: None,
                etapa: etapa.into(),
            })
        };
        indeterminado("Carregando o modelo");
        let sessao =
            execucao::sessao(&caminho, self.backend, BACKEND_AUTOMATICO).map_err(|e| match e {
                ErroDeExecucao::ModeloInvalido(m) => Erro::ModeloInvalido(m),
                ErroDeExecucao::Cancelado => Erro::Cancelado,
                ErroDeExecucao::Processamento(m) => Erro::Processamento(m),
            })?;
        if controle.foi_cancelado() {
            return Err(Erro::Cancelado);
        }
        let recorte = recorte_de_contexto(caixa, e.largura, e.altura, e.contexto);
        let p = preparar(e, recorte);
        indeterminado("Reconstruindo com a IA");
        let lado = LADO as i64;
        let saidas = sessao
            .rodar(
                vec![
                    (
                        "image",
                        Tensor32 {
                            forma: vec![1, 3, lado, lado],
                            dados: p.imagem.clone(),
                        },
                    ),
                    (
                        "mask",
                        Tensor32 {
                            forma: vec![1, 1, lado, lado],
                            dados: p.mascara.clone(),
                        },
                    ),
                ],
                controle.cancelado,
            )
            .map_err(|e| match e {
                ErroDeExecucao::Cancelado => Erro::Cancelado,
                ErroDeExecucao::ModeloInvalido(m) | ErroDeExecucao::Processamento(m) => {
                    Erro::Processamento(m)
                }
            })?;
        let saida = saidas
            .into_iter()
            .find(|t| t.forma == [1, 3, lado, lado])
            .ok_or_else(|| {
                Erro::Processamento("a saída do modelo não tem a forma esperada".into())
            })?;
        let mut r = reconstruir(e, caixa, recorte, &p, &saida.dados);
        r.executado_em = match &sessao.aviso {
            Some(aviso) => format!("{} — {aviso}", sessao.backend.nome()),
            None => sessao.backend.nome().to_string(),
        };
        Ok(r)
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn entrada<'a>(rgba: &'a [u8], destino: &'a [bool], w: u32, h: u32) -> Entrada<'a> {
        Entrada {
            rgba,
            largura: w,
            altura: h,
            destino,
            amostragem: None,
            semente: 1,
            contexto: 0.5,
        }
    }

    fn cena(w: u32, h: u32) -> Vec<u8> {
        (0..h)
            .flat_map(|y| {
                (0..w).flat_map(move |x| {
                    [(x % 256) as u8, (y % 256) as u8, ((x + y) % 97) as u8, 255]
                })
            })
            .collect()
    }

    /// Um "modelo" identidade: devolve a imagem de entrada (BGR) em 0..255.
    fn identidade(p: &Preparado) -> Vec<f32> {
        p.imagem.iter().map(|v| v * 255.0).collect()
    }

    #[test]
    fn o_recorte_de_contexto_cerca_o_destino_e_respeita_a_borda() {
        // Destino 40×20 no meio de uma região 1000×800, contexto 0,5: 80 de lado.
        assert_eq!(
            recorte_de_contexto((480, 390, 519, 409), 1000, 800, 0.5),
            (460, 360, 80, 80)
        );
        // Na quina: o quadrado é empurrado para dentro.
        assert_eq!(
            recorte_de_contexto((0, 0, 39, 19), 1000, 800, 0.5),
            (0, 0, 80, 80)
        );
        // Maior que a região: a região inteira.
        assert_eq!(
            recorte_de_contexto((10, 10, 300, 200), 320, 240, 1.0),
            (0, 0, 320, 240)
        );
    }

    #[test]
    fn na_resolucao_nativa_ida_e_volta_preserva_os_pixels_e_as_coordenadas() {
        let (w, h) = (400u32, 300u32);
        let rgba = cena(w, h);
        let destino: Vec<bool> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                (150..230).contains(&x) && (100..160).contains(&y)
            })
            .collect();
        let e = entrada(&rgba, &destino, w, h);
        let caixa = caixa_do_destino(&destino, w).unwrap();
        assert_eq!(caixa, (150, 100, 229, 159));
        let recorte = recorte_de_contexto(caixa, w, h, 0.5);
        let p = preparar(&e, recorte);
        assert_eq!(p.escala, 1.0, "cabe em 512: sem redução");
        assert_eq!(p.util, (recorte.2, recorte.3));
        // A máscara cai no lugar certo dentro dos 512.
        assert_eq!(
            p.mascara[((100 - recorte.1) * LADO + (150 - recorte.0)) as usize],
            1.0
        );
        assert_eq!(
            p.mascara[((99 - recorte.1) * LADO + (150 - recorte.0)) as usize],
            0.0
        );
        // BGR: o canal 0 é o azul.
        let k = (5 * LADO + 5) as usize;
        let i = ((recorte.1 + 5) * w + recorte.0 + 5) as usize;
        assert_eq!((p.imagem[k] * 255.0).round() as u8, rgba[i * 4 + 2]);
        // A volta com o modelo identidade devolve a foto exata.
        let r = reconstruir(&e, caixa, recorte, &p, &identidade(&p));
        assert_eq!((r.x0, r.y0, r.largura, r.altura), (150, 100, 80, 60));
        assert_eq!(r.reducao, 1.0);
        for y in 0..r.altura {
            for x in 0..r.largura {
                let k = ((y * r.largura + x) * 4) as usize;
                let i = (((r.y0 + y) * w + r.x0 + x) * 4) as usize;
                assert_eq!(r.rgba[k..k + 3], rgba[i..i + 3], "({x}, {y})");
            }
        }
    }

    #[test]
    fn acima_de_512_reduz_amplia_e_avisa_a_reducao() {
        let (w, h) = (1600u32, 1200u32);
        // Um degradê suave: a ida e volta reduzida erra pouco.
        let rgba: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| [(x / 8) as u8, (y / 8) as u8, 100, 255]))
            .collect();
        let destino: Vec<bool> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                (500..900).contains(&x) && (400..700).contains(&y)
            })
            .collect();
        let e = entrada(&rgba, &destino, w, h);
        let caixa = caixa_do_destino(&destino, w).unwrap();
        let recorte = recorte_de_contexto(caixa, w, h, 0.5);
        let p = preparar(&e, recorte);
        assert!(p.escala < 1.0);
        let r = reconstruir(&e, caixa, recorte, &p, &identidade(&p));
        assert!(r.reducao > 1.5, "avisa a redução ({})", r.reducao);
        let mut pior = 0i32;
        for y in 0..r.altura {
            for x in 0..r.largura {
                let k = ((y * r.largura + x) * 4) as usize;
                let i = (((r.y0 + y) * w + r.x0 + x) * 4) as usize;
                for c in 0..3 {
                    pior = pior.max((r.rgba[k + c] as i32 - rgba[i + c] as i32).abs());
                }
            }
        }
        assert!(pior <= 2, "o degradê volta no lugar ({pior})");
    }

    #[test]
    fn sem_modelo_o_erro_e_modelo_ausente() {
        let dir = tempfile::tempdir().unwrap();
        let rgba = cena(64, 64);
        let destino = vec![true; 64 * 64];
        let motor = LaMa {
            pasta: dir.path().to_path_buf(),
            backend: Backend::Automatico,
        };
        let cancelado = std::sync::atomic::AtomicBool::new(false);
        let r = motor.preencher(
            &entrada(&rgba, &destino, 64, 64),
            &Controle {
                cancelado: &cancelado,
                progresso: &|_| {},
            },
        );
        assert_eq!(r, Err(Erro::ModeloAusente));
    }

    /// 🧪 **Com o modelo de verdade** (não roda sem ele): `VLB_MODELOS`
    /// apontando a pasta com o `lama_fp32.onnx` instalado.
    ///
    /// ```text
    /// VLB_MODELOS=… cargo test -p preenchimento --release -- --ignored lama_de_verdade
    /// ```
    #[test]
    #[ignore]
    fn lama_de_verdade_remove_o_quadrado_e_nao_mexe_fora() {
        let pasta = modelos::pasta_padrao();
        let (w, h) = (300u32, 240u32);
        let mut rgba = cena(w, h);
        let destino: Vec<bool> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                (120..180).contains(&x) && (90..150).contains(&y)
            })
            .collect();
        for (i, d) in destino.iter().enumerate() {
            if *d {
                rgba[i * 4..i * 4 + 3].copy_from_slice(&[255, 0, 255]);
            }
        }
        let motor = LaMa {
            pasta,
            backend: Backend::Automatico,
        };
        let cancelado = std::sync::atomic::AtomicBool::new(false);
        let inicio = std::time::Instant::now();
        let r = motor
            .preencher(
                &entrada(&rgba, &destino, w, h),
                &Controle {
                    cancelado: &cancelado,
                    progresso: &|_| {},
                },
            )
            .expect("com o modelo instalado");
        eprintln!("LaMa: {:?} em {}", inicio.elapsed(), r.executado_em);
        let magenta = r
            .rgba
            .chunks(4)
            .filter(|p| p[0] > 200 && p[1] < 60 && p[2] > 200)
            .count();
        assert_eq!(magenta, 0, "nada do quadrado");
        // Cancelar antes de rodar devolve Cancelado (e a sessão continua boa).
        cancelado.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            motor.preencher(
                &entrada(&rgba, &destino, w, h),
                &Controle {
                    cancelado: &cancelado,
                    progresso: &|_| {}
                }
            ),
            Err(Erro::Cancelado)
        );
    }
}
