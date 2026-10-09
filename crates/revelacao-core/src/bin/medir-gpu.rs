//! A mesma revelação em **cada API gráfica da máquina**, e o caminho até a janela.
//!
//! # Por que isto existe
//!
//! *"Não seria interessante criar um teste de desempenho de DX11, DX12 e
//! Vulkan?"* — dono, 9/out/2026, sobre a Revelação mais lenta no Windows.
//!
//! O botão Desempenho mede o app aberto, mas só na API em que o motor abriu, e
//! não alcança o GPUI. Este binário responde às duas perguntas que ele deixa:
//!
//! 1. **O motor seria mais rápido em outra API?** A mesma foto, os mesmos
//!    ajustes, em DX12, Vulkan e OpenGL (Windows), Vulkan e OpenGL (Linux) ou
//!    Metal (Mac), em toda placa que cada API enxerga.
//! 2. **Quanto custa a foto chegar à janela?** No Windows a janela é do GPUI
//!    e desenha em **DirectX 11**; o motor, em DX12. As duas não compartilham
//!    memória: cada quadro do slider volta da GPU para a CPU, vira BGRA e sobe
//!    de novo para o DX11. A última seção mede essa subida do jeito que o atlas
//!    do GPUI faz (`gpui-pre-windows/src/directx_atlas.rs`: textura BGRA
//!    `USAGE_DEFAULT` e `UpdateSubresource`), na placa que ele usa (a primeira
//!    da lista do Windows). Somada ao motor, é o custo de um quadro.
//!
//! O DX11 aparece só aí: o wgpu não tem backend DX11, então o motor não roda
//! nele, e quem usa DX11 é a janela.
//!
//! ```bash
//! cargo run --release -p revelacao-core --bin medir-gpu                # foto sintética de 24 MP
//! cargo run --release -p revelacao-core --bin medir-gpu -- foto.jpg    # uma foto de verdade
//! ```
//!
//! 🔑 **Mora no `revelacao-core`, e não no `ui-gpui`, para sair como `.exe`
//! do Mac.** O GPUI do Windows compila os shaders dele com o `fxc` da
//! Microsoft no build, e isso só roda no Windows; este binário não precisa do
//! GPUI, e assim `cargo build --target x86_64-pc-windows-gnu` o entrega pronto
//! para levar ao balcão. O instalador compila só `-p ui-gpui --bin ui-gpui`:
//! nada daqui entra no app.
//!
//! 📄 **O resultado fica num arquivo**, `medir-gpu-<data>.txt`, ao lado do
//! executável (ou na pasta atual, se lá não der para gravar): é o que o
//! operador manda de volta. A janela espera um Enter antes de fechar, para o
//! `.exe` aberto com dois cliques não sumir com o resultado.
//!
//! ⚠️ **Só vale fora do `debug`**, e com o app fechado: duas coisas na mesma
//! GPU dividem o tempo dela.

use std::sync::Arc;
use std::time::Instant;

use revelacao_core::{Ajustes, Entrada, Motor};

/// Tudo o que vai para a tela vai também para o relatório em arquivo.
static RELATORIO: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

macro_rules! dizer {
    () => { dizer!("") };
    ($($arg:tt)*) => {{
        let linha = format!($($arg)*);
        println!("{linha}");
        if let Ok(mut relatorio) = crate::RELATORIO.lock() {
            relatorio.push_str(&linha);
            relatorio.push('\n');
        }
    }};
}

/// O orçamento de um quadro a 60fps.
const QUADRO_MS: f64 = 16.7;

/// Quantas revelações por medida morna (a primeira, fria, fica de fora).
const REPETICOES: usize = 20;

/// Os tamanhos que a Revelação pede ao motor.
///
/// - **arrasto**: o rascunho durante o gesto, do tamanho do palco em pixels
///   do monitor (`tela.rs`, `lado_do_rascunho`) — 1600 é um palco típico;
/// - **soltou**: a prévia grande (`reposicao.rs`, `LADO_DO_PREVIEW`);
/// - **exportação**: a foto inteira.
const TAMANHOS: [(&str, Option<u32>); 3] = [
    ("arrasto (1600)", Some(1600)),
    ("soltou (2560)", Some(2560)),
    ("exportação", None),
];

fn main() {
    if cfg!(debug_assertions) {
        eprintln!("⚠️  perfil `debug`: os números não valem. Use --release.\n");
    }

    dizer!(
        "medir-gpu · VintageLightbox {} · {} {} · {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        agora()
    );
    let (nome, foto) = carregar_foto();
    dizer!("foto: {nome} — {}×{}\n", foto.width(), foto.height());

    let mut linhas = Vec::new();
    for (rotulo, backends) in apis_da_plataforma() {
        let instancia = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });
        let adaptadores = instancia.enumerate_adapters(backends);
        if adaptadores.is_empty() {
            dizer!("— {rotulo}: nenhuma placa responde nesta API\n");
            continue;
        }
        for adaptador in adaptadores {
            let info = adaptador.get_info();
            if info.device_type == wgpu::DeviceType::Cpu {
                // Um rasterizador por software (WARP, llvmpipe) não é o que o
                // balcão usa, e levaria minutos na exportação.
                dizer!("— {rotulo} · {}: emulada na CPU, pulada\n", info.name);
                continue;
            }
            let motor = pollster::block_on(Motor::abrir_com(
                &adaptador,
                Entrada::Compute,
                wgpu::Limits::default(),
            ));
            match motor {
                Ok(mut motor) => {
                    dizer!("═══ {rotulo} · {} ({:?})", info.name, info.device_type);
                    for (tamanho, lado) in TAMANHOS {
                        let (pixels, largura, altura) = reduzida(&foto, lado);
                        if let Some(medida) = medir(&mut motor, &pixels, largura, altura) {
                            medida.imprimir(tamanho, largura, altura);
                            linhas.push((rotulo, info.name.clone(), tamanho, medida));
                        } else {
                            dizer!("  {tamanho}: o motor não devolveu a foto");
                        }
                    }
                    dizer!();
                }
                Err(erro) => dizer!("— {rotulo} · {}: não abriu ({erro})\n", info.name),
            }
        }
    }

    let janela = janela::medir(&foto);

    dizer!("=== resumo: um quadro do slider (arrasto, mediana) ===");
    for (api, placa, tamanho, medida) in &linhas {
        if !tamanho.starts_with("arrasto") {
            continue;
        }
        let ate_a_janela = janela
            .iter()
            .find(|j| j.tamanho.starts_with("arrasto"))
            .map(|j| j.mediana_ms);
        let total = medida.mediana_ms + medida.conversao_ms + ate_a_janela.unwrap_or(0.0);
        let marca = if total <= QUADRO_MS { "✅" } else { "🚨" };
        let inicio = format!(
            "{marca} {api} · {placa}: motor {:.1} + BGRA {:.1}",
            medida.mediana_ms, medida.conversao_ms
        );
        match ate_a_janela {
            Some(ms) => dizer!("{inicio} + subida ao DX11 {ms:.1} = {total:.1} ms"),
            None => dizer!("{inicio} = {total:.1} ms"),
        }
    }
    dizer!("\nO orçamento de um quadro a 60fps é {QUADRO_MS} ms.");
    gravar_relatorio();
}

/// Data e hora em UTC, sem crate: só para nomear e datar o relatório.
fn agora() -> String {
    let segundos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (dias, resto) = ((segundos / 86_400) as i64, segundos % 86_400);
    // Dias desde 1970 → data civil (Howard Hinnant, "chrono-compatible
    // low-level date algorithms").
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = doy - (153 * mp + 2) / 5 + 1;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 };
    let ano = yoe + era * 400 + i64::from(mes <= 2);
    format!(
        "{ano:04}-{mes:02}-{dia:02} {:02}h{:02} UTC",
        resto / 3_600,
        resto % 3_600 / 60
    )
}

/// Grava o relatório ao lado do executável e espera um Enter.
fn gravar_relatorio() {
    let texto = RELATORIO.lock().map(|r| r.clone()).unwrap_or_default();
    let nome = format!("medir-gpu-{}.txt", agora().replace([' ', ':'], "-"));
    let pastas = [
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|p| p.to_path_buf())),
        std::env::current_dir().ok(),
    ];
    let gravado = pastas
        .into_iter()
        .flatten()
        .map(|pasta| pasta.join(&nome))
        .find(|caminho| std::fs::write(caminho, &texto).is_ok());
    match gravado {
        Some(caminho) => println!("\n📄 Relatório gravado em {}", caminho.display()),
        None => println!("\n⚠️  Não consegui gravar o relatório; copie o texto acima."),
    }
    if cfg!(target_os = "windows") {
        println!("Enter para fechar.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}

/// A foto pedida na linha de comando, ou uma sintética de 6000×4000 — o
/// tamanho de uma D750 — com variação suficiente para nenhum driver achar
/// atalho em textura lisa.
fn carregar_foto() -> (String, image::RgbaImage) {
    if let Some(caminho) = std::env::args().nth(1) {
        match image::open(&caminho) {
            Ok(foto) => return (caminho, foto.into_rgba8()),
            Err(erro) => eprintln!("⚠️  {caminho} não abriu ({erro}); usando a sintética\n"),
        }
    }
    let foto = image::RgbaImage::from_fn(6000, 4000, |x, y| {
        let onda = ((x as f32 / 37.0).sin() * (y as f32 / 23.0).cos() * 40.0) as i32;
        image::Rgba([
            ((x * 255 / 6000) as i32 + onda).clamp(0, 255) as u8,
            ((y * 255 / 4000) as i32 - onda).clamp(0, 255) as u8,
            (((x + y) / 40) % 256) as u8,
            255,
        ])
    });
    ("sintética".into(), foto)
}

/// As APIs que fazem sentido em cada sistema, cada uma sozinha.
fn apis_da_plataforma() -> Vec<(&'static str, wgpu::Backends)> {
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
struct Medida {
    fria_ms: f64,
    mediana_ms: f64,
    p95_ms: f64,
    espera_ms: f64,
    leitura_ms: f64,
    gpu_ms: Option<f64>,
    conversao_ms: f64,
}

impl Medida {
    fn imprimir(&self, tamanho: &str, largura: u32, altura: u32) {
        let gpu = self
            .gpu_ms
            .map(|ms| format!("{ms:.1}"))
            .unwrap_or_else(|| "—".into());
        dizer!(
            "  {tamanho:<15} {largura:>5}×{altura:<5} fria {:>7.1} | mediana {:>6.1} p95 {:>6.1} ms \
             | espera pela GPU {:>5.1} · leitura {:>5.1} · shaders {gpu} · BGRA {:.1}",
            self.fria_ms,
            self.mediana_ms,
            self.p95_ms,
            self.espera_ms,
            self.leitura_ms,
            self.conversao_ms,
        );
    }
}

fn medir(motor: &mut Motor, pixels: &Arc<Vec<u8>>, largura: u32, altura: u32) -> Option<Medida> {
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

    // O que `imagem::para_gpui` faz com cada resultado antes de o GPUI vê-lo.
    let comeco = Instant::now();
    let _bgra = para_bgra(ultima?);
    let conversao_ms = comeco.elapsed().as_secs_f64() * 1000.0;

    Some(Medida {
        fria_ms,
        mediana_ms: totais[REPETICOES / 2],
        p95_ms: totais[(REPETICOES * 95 / 100).min(REPETICOES - 1)],
        espera_ms: espera / n,
        leitura_ms: leitura / n,
        gpu_ms: (com_gpu > 0).then(|| gpu / com_gpu as f64),
        conversao_ms,
    })
}

fn para_bgra(imagem: image::DynamicImage) -> Vec<u8> {
    let mut bytes = imagem.into_rgba8().into_raw();
    for pixel in bytes.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    bytes
}

/// Uma medida da subida à janela.
#[allow(dead_code)]
struct Subida {
    tamanho: &'static str,
    mediana_ms: f64,
}

#[cfg(not(target_os = "windows"))]
mod janela {
    pub fn medir(_foto: &image::RgbaImage) -> Vec<super::Subida> {
        dizer!("=== subida à janela ===");
        dizer!(
            "Só no Windows: lá a janela desenha em DirectX 11 e o motor em DX12. \
             Aqui as duas falam a mesma API.\n"
        );
        Vec::new()
    }
}

#[cfg(target_os = "windows")]
mod janela {
    use std::time::Instant;

    use windows::Win32::Foundation::{HMODULE, TRUE};
    use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_QUERY_DESC,
        D3D11_QUERY_EVENT, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Query, ID3D11Texture2D,
    };
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
    use windows::core::BOOL;

    use super::{REPETICOES, Subida, TAMANHOS, reduzida};

    /// Cada quadro do slider é uma `RenderImage` nova no GPUI: textura nova no
    /// atlas e `UpdateSubresource`. Medido até a GPU confirmar (consulta de
    /// evento), e não só até a chamada voltar — o driver pode adiar a cópia.
    pub fn medir(foto: &image::RgbaImage) -> Vec<Subida> {
        dizer!("=== subida à janela (DirectX 11, como o atlas do GPUI) ===");
        let Some((dispositivo, contexto)) = abrir() else {
            dizer!("o DirectX 11 não abriu nesta máquina\n");
            return Vec::new();
        };
        let mut medidas = Vec::new();
        for (tamanho, lado) in TAMANHOS {
            let (pixels, largura, altura) = reduzida(foto, lado);
            let mut tempos = Vec::with_capacity(REPETICOES);
            for _ in 0..=REPETICOES {
                let comeco = Instant::now();
                if !subir(&dispositivo, &contexto, &pixels, largura, altura) {
                    dizer!("  {tamanho}: a textura não foi criada");
                    break;
                }
                tempos.push(comeco.elapsed().as_secs_f64() * 1000.0);
            }
            if tempos.len() <= 1 {
                continue;
            }
            tempos.remove(0);
            tempos.sort_by(f64::total_cmp);
            let mediana_ms = tempos[tempos.len() / 2];
            dizer!(
                "  {tamanho:<15} {largura:>5}×{altura:<5} mediana {mediana_ms:>6.1} ms  p95 {:>6.1} ms",
                tempos[(tempos.len() * 95 / 100).min(tempos.len() - 1)]
            );
            medidas.push(Subida {
                tamanho,
                mediana_ms,
            });
        }
        dizer!();
        medidas
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
    ) -> bool {
        let mut bgra = rgba.to_vec();
        for pixel in bgra.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
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
            if dispositivo
                .CreateTexture2D(&descricao, None, Some(&mut textura))
                .is_err()
            {
                return false;
            }
            let Some(textura) = textura else {
                return false;
            };
            contexto.UpdateSubresource(&textura, 0, None, bgra.as_ptr() as _, 4 * largura, 0);
            let pedido = D3D11_QUERY_DESC {
                Query: D3D11_QUERY_EVENT,
                MiscFlags: 0,
            };
            if dispositivo
                .CreateQuery(&pedido, Some(&mut consulta))
                .is_err()
            {
                return false;
            }
            let Some(consulta) = consulta else {
                return false;
            };
            contexto.End(&consulta);
            contexto.Flush();
            let mut pronto = BOOL(0);
            // Um driver que perdeu o dispositivo nunca responde: 10 s e desiste.
            let limite = Instant::now() + std::time::Duration::from_secs(10);
            while pronto != TRUE {
                if Instant::now() > limite {
                    return false;
                }
                let _ = contexto.GetData(
                    &consulta,
                    Some(&mut pronto as *mut BOOL as *mut _),
                    std::mem::size_of::<BOOL>() as u32,
                    0,
                );
            }
        }
        true
    }
}
