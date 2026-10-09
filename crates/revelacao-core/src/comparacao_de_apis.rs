//! A mesma revelação em **cada API gráfica da máquina**, e o caminho até a janela.
//!
//! # Por que isto existe
//!
//! *"Não seria interessante criar um teste de desempenho de DX11, DX12 e
//! Vulkan?"* — dono, 9/out/2026, sobre a Revelação mais lenta no Windows.
//!
//! A captura do botão Desempenho mede só a API em que o motor abriu, e não
//! alcança o GPUI. Esta comparação responde às duas perguntas que ela deixa:
//!
//! 1. **O motor seria mais rápido em outra API?** A mesma foto, os mesmos
//!    ajustes, em DX12, Vulkan e OpenGL (Windows), Vulkan e OpenGL (Linux) ou
//!    Metal (Mac), em toda placa que cada API enxerga.
//! 2. **Quanto custa a foto chegar à janela?** No Windows a janela é do GPUI e
//!    desenha em **DirectX 11**; o motor, em DX12. As duas não compartilham
//!    memória: cada quadro do slider volta da GPU para a CPU, vira BGRA e sobe
//!    de novo para o DX11. [`dx11`] mede essa subida do jeito que o atlas do
//!    GPUI faz (`gpui-pre-windows/src/directx_atlas.rs`: textura BGRA
//!    `USAGE_DEFAULT` e `UpdateSubresource`), na placa que ele usa (a primeira
//!    da lista do Windows). Somada ao motor, é o custo de um quadro.
//!
//! O DX11 só aparece aí: o wgpu não tem backend DX11, então o motor não roda
//! nele — quem usa DX11 é a janela.
//!
//! # Quem usa
//!
//! - a janela **Desempenho → Comparar APIs gráficas**, num processo filho do
//!   app (`--comparar-apis`): um driver que derruba o processo ao abrir (o
//!   Vulkan da Intel UHD 630 no Windows fez isso com o LightCraft, issue 136)
//!   leva o filho, e não o app. Por isso o resultado é gravado a cada medida:
//!   o que veio antes da queda chega ao banco;
//! - o binário `medir-gpu`, para levar a uma máquina sem o app.
//!
//! ⚠️ A foto padrão é **sintética** ([`foto_sintetica`]): a mesma em toda
//! máquina, para que os números se comparem entre balcões.

use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::{Ajustes, Entrada, Motor};

/// O orçamento de um quadro a 60fps.
pub const QUADRO_MS: f64 = 16.7;

/// Quantas revelações por medida morna (a primeira, fria, fica de fora).
const REPETICOES: usize = 20;

/// Os tamanhos que a Revelação pede ao motor: (id estável, rótulo, lado).
///
/// - **arrasto**: o rascunho durante o gesto, do tamanho do palco em pixels
///   do monitor (`ui-gpui/src/revelacao/tela.rs`, `lado_do_rascunho`) — 1600
///   é um palco típico;
/// - **soltou**: a prévia grande (`reposicao.rs`, `LADO_DO_PREVIEW`);
/// - **exportação**: a foto inteira.
pub const TAMANHOS: [(&str, &str, Option<u32>); 3] = [
    ("arrasto", "arrasto (1600)", Some(1600)),
    ("soltou", "soltou (2560)", Some(2560)),
    ("exportacao", "exportação", None),
];

/// Uma API numa placa, num tamanho.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MedidaDaApi {
    pub api: String,
    pub placa: String,
    /// `integrada`, `dedicada`, `virtual` ou `outra`.
    pub tipo: String,
    pub driver: String,
    /// Um dos ids de [`TAMANHOS`].
    pub tamanho: String,
    pub largura: u32,
    pub altura: u32,
    /// A primeira revelação: a foto sobe para a GPU.
    pub fria_ms: f64,
    pub mediana_ms: f64,
    pub p95_ms: f64,
    /// Médias das etapas que o motor mede ([`crate::TemposDoMotor`]).
    pub espera_ms: f64,
    pub leitura_ms: f64,
    /// Os shaders, por timestamp query — `None` sem suporte.
    pub gpu_ms: Option<f64>,
    /// RGBA → BGRA, o que `ui-gpui/src/imagem.rs` faz antes de o GPUI ver.
    pub bgra_ms: f64,
}

/// A subida de um resultado ao DirectX 11 da janela (só Windows).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubidaAJanela {
    pub tamanho: String,
    pub largura: u32,
    pub altura: u32,
    pub mediana_ms: f64,
    pub p95_ms: f64,
}

/// A comparação inteira.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ComparacaoDeApis {
    pub foto: String,
    pub largura: u32,
    pub altura: u32,
    pub medidas: Vec<MedidaDaApi>,
    /// `None` fora do Windows: lá a janela e o motor falam a mesma API.
    pub subidas: Option<Vec<SubidaAJanela>>,
    /// O que não abriu, não respondeu ou foi pulado, uma frase cada.
    pub ocorrencias: Vec<String>,
    /// `false` enquanto a comparação anda (e para sempre, se o processo caiu).
    pub terminou: bool,
    /// O passo em andamento — é como o app mostra o progresso do processo
    /// filho, e diz onde ele estava se caiu.
    pub passo: String,
}

/// As APIs que fazem sentido em cada sistema, cada uma sozinha.
pub fn apis_da_plataforma() -> Vec<(&'static str, wgpu::Backends)> {
    if cfg!(target_os = "windows") {
        vec![
            ("DX12", wgpu::Backends::DX12),
            ("Vulkan", wgpu::Backends::VULKAN),
            ("OpenGL", wgpu::Backends::GL),
        ]
    } else if cfg!(target_os = "macos") {
        vec![("Metal", wgpu::Backends::METAL)]
    } else {
        vec![
            ("Vulkan", wgpu::Backends::VULKAN),
            ("OpenGL", wgpu::Backends::GL),
        ]
    }
}

/// 6000×4000 — o tamanho de uma D750 — com variação suficiente para nenhum
/// driver achar atalho em textura lisa. Determinística: a mesma em toda
/// máquina.
pub fn foto_sintetica() -> image::RgbaImage {
    image::RgbaImage::from_fn(6000, 4000, |x, y| {
        let onda = ((x as f32 / 37.0).sin() * (y as f32 / 23.0).cos() * 40.0) as i32;
        image::Rgba([
            ((x * 255 / 6000) as i32 + onda).clamp(0, 255) as u8,
            ((y * 255 / 4000) as i32 - onda).clamp(0, 255) as u8,
            (((x + y) / 40) % 256) as u8,
            255,
        ])
    })
}

/// Roda a comparação. `progresso` recebe uma frase antes de cada passo (o
/// processo filho a escreve; o app a mostra), e `parcial` o resultado até
/// ali, depois de cada medida.
pub fn comparar(
    foto: &image::RgbaImage,
    nome_da_foto: &str,
    mut progresso: impl FnMut(&str),
    mut parcial: impl FnMut(&ComparacaoDeApis),
) -> ComparacaoDeApis {
    let mut resultado = ComparacaoDeApis {
        foto: nome_da_foto.into(),
        largura: foto.width(),
        altura: foto.height(),
        ..Default::default()
    };
    let reduzidas: Vec<_> = TAMANHOS
        .iter()
        .map(|(id, rotulo, lado)| (*id, *rotulo, reduzida(foto, *lado)))
        .collect();

    for (api, backends) in apis_da_plataforma() {
        avancar(
            &mut resultado,
            &format!("{api}: procurando placas"),
            &mut progresso,
            &mut parcial,
        );
        let instancia = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });
        let adaptadores = instancia.enumerate_adapters(backends);
        if adaptadores.is_empty() {
            resultado
                .ocorrencias
                .push(format!("{api}: nenhuma placa responde nesta API"));
            parcial(&resultado);
            continue;
        }
        for adaptador in adaptadores {
            let info = crate::motor::InfoDoAdaptador::de_adaptador(&adaptador);
            if info.tipo == "cpu" {
                // Um rasterizador por software (WARP, llvmpipe) não é o que o
                // balcão usa, e levaria minutos na exportação.
                resultado
                    .ocorrencias
                    .push(format!("{api} · {}: emulada na CPU, pulada", info.nome));
                parcial(&resultado);
                continue;
            }
            avancar(
                &mut resultado,
                &format!("{api} · {}: abrindo o motor", info.nome),
                &mut progresso,
                &mut parcial,
            );
            let mut motor = match pollster::block_on(Motor::abrir_com(
                &adaptador,
                Entrada::Compute,
                wgpu::Limits::default(),
            )) {
                Ok(motor) => motor,
                Err(erro) => {
                    resultado
                        .ocorrencias
                        .push(format!("{api} · {}: não abriu ({erro})", info.nome));
                    parcial(&resultado);
                    continue;
                }
            };
            for (id, rotulo, (pixels, largura, altura)) in &reduzidas {
                avancar(
                    &mut resultado,
                    &format!("{api} · {}: {rotulo}", info.nome),
                    &mut progresso,
                    &mut parcial,
                );
                match medir(&mut motor, pixels, *largura, *altura) {
                    Some(mut medida) => {
                        medida.api = api.into();
                        medida.placa = info.nome.clone();
                        medida.tipo = info.tipo.into();
                        medida.driver = [info.driver.as_str(), info.driver_info.as_str()]
                            .into_iter()
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join(" ");
                        medida.tamanho = (*id).into();
                        resultado.medidas.push(medida);
                    }
                    None => resultado.ocorrencias.push(format!(
                        "{api} · {}: {rotulo} — o motor não devolveu a foto",
                        info.nome
                    )),
                }
                parcial(&resultado);
            }
        }
    }

    if cfg!(target_os = "windows") {
        avancar(
            &mut resultado,
            "DirectX 11: subindo à janela como o GPUI",
            &mut progresso,
            &mut parcial,
        );
        let mut subidas = Vec::new();
        for (id, _, (pixels, largura, altura)) in &reduzidas {
            match dx11::medir(pixels, *largura, *altura) {
                Ok((mediana_ms, p95_ms)) => subidas.push(SubidaAJanela {
                    tamanho: (*id).into(),
                    largura: *largura,
                    altura: *altura,
                    mediana_ms,
                    p95_ms,
                }),
                Err(erro) => resultado.ocorrencias.push(format!("DirectX 11: {erro}")),
            }
        }
        resultado.subidas = Some(subidas);
    }
    resultado.terminou = true;
    resultado.passo.clear();
    parcial(&resultado);
    resultado
}

/// Anota o passo no resultado e avisa os dois lados: quem mostra o progresso
/// e quem grava o parcial (o arquivo que o app lê, mesmo se o processo cair).
fn avancar(
    resultado: &mut ComparacaoDeApis,
    passo: &str,
    progresso: &mut impl FnMut(&str),
    parcial: &mut impl FnMut(&ComparacaoDeApis),
) {
    resultado.passo = passo.into();
    progresso(passo);
    parcial(resultado);
}

/// A foto reduzida para o lado maior pedido, como a Revelação manda ao motor.
fn reduzida(foto: &image::RgbaImage, lado: Option<u32>) -> (Arc<Vec<u8>>, u32, u32) {
    let (largura, altura) = foto.dimensions();
    let Some(lado) = lado.filter(|lado| *lado < largura.max(altura)) else {
        return (Arc::new(foto.as_raw().clone()), largura, altura);
    };
    let fator = lado as f32 / largura.max(altura) as f32;
    let (l, a) = (
        ((largura as f32 * fator).round() as u32).max(1),
        ((altura as f32 * fator).round() as u32).max(1),
    );
    let menor = image::imageops::resize(foto, l, a, image::imageops::FilterType::Triangle);
    (Arc::new(menor.into_raw()), l, a)
}

/// Uma revelação fria (a foto sobe para a GPU) e [`REPETICOES`] mornas, cada
/// uma com a exposição diferente, como um slider arrastado.
fn medir(
    motor: &mut Motor,
    pixels: &Arc<Vec<u8>>,
    largura: u32,
    altura: u32,
) -> Option<MedidaDaApi> {
    motor.definir_medicao(true);
    let mut ajustes = Ajustes::default();

    let comeco = Instant::now();
    motor.revelar(pixels, largura, altura, &ajustes)?;
    let fria_ms = comeco.elapsed().as_secs_f64() * 1000.0;

    let mut totais = Vec::with_capacity(REPETICOES);
    let (mut espera, mut leitura, mut gpu, mut com_gpu) = (0.0, 0.0, 0.0, 0usize);
    let mut ultima = None;
    for i in 0..REPETICOES {
        ajustes.exposure = (i as f32 / REPETICOES as f32) - 0.5;
        let comeco = Instant::now();
        ultima = Some(motor.revelar(pixels, largura, altura, &ajustes)?);
        totais.push(comeco.elapsed().as_secs_f64() * 1000.0);
        let tempos = motor.ultimos_tempos();
        espera += f64::from(tempos.espera_ms);
        leitura += f64::from(tempos.leitura_ms);
        if let Some(total) = tempos.gpu.and_then(|g| g.total_ms) {
            gpu += f64::from(total);
            com_gpu += 1;
        }
    }
    totais.sort_by(f64::total_cmp);
    let n = REPETICOES as f64;

    let comeco = Instant::now();
    let _bgra = para_bgra(ultima?.into_rgba8().into_raw());
    let bgra_ms = comeco.elapsed().as_secs_f64() * 1000.0;

    Some(MedidaDaApi {
        largura,
        altura,
        fria_ms,
        mediana_ms: totais[REPETICOES / 2],
        p95_ms: totais[(REPETICOES * 95 / 100).min(REPETICOES - 1)],
        espera_ms: espera / n,
        leitura_ms: leitura / n,
        gpu_ms: (com_gpu > 0).then(|| gpu / com_gpu as f64),
        bgra_ms,
        ..Default::default()
    })
}

fn para_bgra(mut bytes: Vec<u8>) -> Vec<u8> {
    for pixel in bytes.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    bytes
}

fn rotulo_do_tamanho(id: &str) -> &'static str {
    TAMANHOS
        .iter()
        .find(|(i, _, _)| *i == id)
        .map_or("?", |(_, rotulo, _)| rotulo)
}

impl ComparacaoDeApis {
    /// Um quadro do slider (arrasto, mediana) por API e placa: motor + BGRA
    /// (+ a subida ao DX11 no Windows), contra o orçamento de 60fps.
    pub fn resumo(&self) -> Vec<String> {
        let subida = self
            .subidas
            .as_ref()
            .and_then(|s| s.iter().find(|s| s.tamanho == "arrasto"))
            .map(|s| s.mediana_ms);
        self.medidas
            .iter()
            .filter(|m| m.tamanho == "arrasto")
            .map(|m| {
                let total = m.mediana_ms + m.bgra_ms + subida.unwrap_or(0.0);
                let marca = if total <= QUADRO_MS { "✅" } else { "🚨" };
                let mut linha = format!(
                    "{marca} {} · {}: motor {:.1} + BGRA {:.1}",
                    m.api, m.placa, m.mediana_ms, m.bgra_ms
                );
                if let Some(ms) = subida {
                    linha.push_str(&format!(" + subida ao DX11 {ms:.1}"));
                }
                linha.push_str(&format!(" = {total:.1} ms"));
                linha
            })
            .collect()
    }

    /// O relatório para ler, colar numa conversa ou gravar em arquivo.
    pub fn texto(&self) -> String {
        let mut t = String::new();
        let mut linha = |s: String| {
            t.push_str(&s);
            t.push('\n');
        };
        linha(format!(
            "foto: {} — {}×{}",
            self.foto, self.largura, self.altura
        ));
        let mut atual = (String::new(), String::new());
        for m in &self.medidas {
            if (m.api.clone(), m.placa.clone()) != atual {
                atual = (m.api.clone(), m.placa.clone());
                linha(String::new());
                linha(format!(
                    "═══ {} · {} ({}){}",
                    m.api,
                    m.placa,
                    m.tipo,
                    if m.driver.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", m.driver)
                    }
                ));
            }
            linha(format!(
                "  {:<15} {:>5}×{:<5} fria {:>7.1} | mediana {:>6.1} p95 {:>6.1} ms \
                 | espera pela GPU {:>5.1} · leitura {:>5.1} · shaders {} · BGRA {:.1}",
                rotulo_do_tamanho(&m.tamanho),
                m.largura,
                m.altura,
                m.fria_ms,
                m.mediana_ms,
                m.p95_ms,
                m.espera_ms,
                m.leitura_ms,
                m.gpu_ms.map_or_else(|| "—".into(), |ms| format!("{ms:.1}")),
                m.bgra_ms,
            ));
        }
        linha(String::new());
        match &self.subidas {
            Some(subidas) => {
                linha("=== subida à janela (DirectX 11, como o atlas do GPUI) ===".into());
                for s in subidas {
                    linha(format!(
                        "  {:<15} {:>5}×{:<5} mediana {:>6.1} ms  p95 {:>6.1} ms",
                        rotulo_do_tamanho(&s.tamanho),
                        s.largura,
                        s.altura,
                        s.mediana_ms,
                        s.p95_ms
                    ));
                }
            }
            None => linha(
                "=== subida à janela: só no Windows (lá a janela é DirectX 11 e o motor DX12) ==="
                    .into(),
            ),
        }
        if !self.ocorrencias.is_empty() {
            linha(String::new());
            linha("=== ocorrências ===".into());
            for o in &self.ocorrencias {
                linha(format!("  {o}"));
            }
        }
        linha(String::new());
        linha("=== resumo: um quadro do slider (arrasto, mediana) ===".into());
        for r in self.resumo() {
            linha(r);
        }
        if !self.terminou {
            linha(format!(
                "⚠️ a comparação não terminou: o processo caiu em \"{}\"",
                self.passo
            ));
        }
        linha(format!(
            "O orçamento de um quadro a 60fps é {QUADRO_MS} ms."
        ));
        t
    }
}

#[cfg(not(target_os = "windows"))]
mod dx11 {
    pub fn medir(_rgba: &[u8], _largura: u32, _altura: u32) -> Result<(f64, f64), String> {
        Err("só existe no Windows".into())
    }
}

#[cfg(target_os = "windows")]
mod dx11 {
    use std::time::{Duration, Instant};

    use windows::core::BOOL;
    use windows::Win32::Foundation::{HMODULE, TRUE};
    use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Query, ID3D11Texture2D,
        D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_QUERY_DESC,
        D3D11_QUERY_EVENT, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

    use super::REPETICOES;

    /// Cada quadro do slider é uma `RenderImage` nova no GPUI: textura nova no
    /// atlas e `UpdateSubresource`. Medido até a GPU confirmar (consulta de
    /// evento), e não só até a chamada voltar — o driver pode adiar a cópia.
    /// Devolve (mediana, p95).
    pub fn medir(rgba: &[u8], largura: u32, altura: u32) -> Result<(f64, f64), String> {
        let (dispositivo, contexto) = abrir().ok_or("o DirectX 11 não abriu nesta máquina")?;
        let mut tempos = Vec::with_capacity(REPETICOES + 1);
        for _ in 0..=REPETICOES {
            let comeco = Instant::now();
            subir(&dispositivo, &contexto, rgba, largura, altura)?;
            tempos.push(comeco.elapsed().as_secs_f64() * 1000.0);
        }
        tempos.remove(0);
        tempos.sort_by(f64::total_cmp);
        Ok((
            tempos[tempos.len() / 2],
            tempos[(tempos.len() * 95 / 100).min(tempos.len() - 1)],
        ))
    }

    fn abrir() -> Option<(ID3D11Device, ID3D11DeviceContext)> {
        let mut dispositivo = None;
        let mut contexto = None;
        // Sem adaptador: o primeiro da lista do Windows, que é o que o GPUI
        // usa (`EnumAdapters(0)` em `directx_devices.rs`).
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut dispositivo),
                None,
                Some(&mut contexto),
            )
            .ok()?;
        }
        Some((dispositivo?, contexto?))
    }

    fn subir(
        dispositivo: &ID3D11Device,
        contexto: &ID3D11DeviceContext,
        rgba: &[u8],
        largura: u32,
        altura: u32,
    ) -> Result<(), String> {
        let bgra = super::para_bgra(rgba.to_vec());
        let descricao = D3D11_TEXTURE2D_DESC {
            Width: largura,
            Height: altura,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut textura: Option<ID3D11Texture2D> = None;
        let mut consulta: Option<ID3D11Query> = None;
        unsafe {
            dispositivo
                .CreateTexture2D(&descricao, None, Some(&mut textura))
                .map_err(|e| format!("a textura não foi criada ({e})"))?;
            let textura = textura.ok_or("a textura não foi criada")?;
            contexto.UpdateSubresource(&textura, 0, None, bgra.as_ptr() as _, 4 * largura, 0);
            let pedido = D3D11_QUERY_DESC {
                Query: D3D11_QUERY_EVENT,
                MiscFlags: 0,
            };
            dispositivo
                .CreateQuery(&pedido, Some(&mut consulta))
                .map_err(|e| format!("a consulta não foi criada ({e})"))?;
            let consulta = consulta.ok_or("a consulta não foi criada")?;
            contexto.End(&consulta);
            contexto.Flush();
            let mut pronto = BOOL(0);
            // Um driver que perdeu o dispositivo nunca responde: 10 s e desiste.
            let limite = Instant::now() + Duration::from_secs(10);
            while pronto != TRUE {
                if Instant::now() > limite {
                    return Err("a GPU não confirmou a cópia em 10 s".into());
                }
                let _ = contexto.GetData(
                    &consulta,
                    Some(&mut pronto as *mut BOOL as *mut _),
                    std::mem::size_of::<BOOL>() as u32,
                    0,
                );
            }
        }
        Ok(())
    }
}
