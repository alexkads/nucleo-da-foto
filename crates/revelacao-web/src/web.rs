//! O motor de revelação no navegador.
//!
//! É a mesma matemática do desktop — o [`revelacao_core`] inteiro — exposta ao
//! JavaScript por `wasm-bindgen`. O site (`recordarfotos-e-commerce`) carrega
//! o `.wasm` no painel do pós-venda e revela a foto do cliente sem sair do
//! navegador (decisão do dono, 2026-09-04).
//!
//! ## O que fica de cada lado
//!
//! | lado | faz |
//! |---|---|
//! | JavaScript | decodifica o JPEG (`createImageBitmap`, 5–10× mais rápido que decodificar em wasm), desenha os sliders, manda os 46 valores |
//! | aqui | sobe os pixels para a GPU, aplica os ajustes, desenha no canvas, lê de volta e codifica o JPEG na exportação |
//!
//! 🔑 **O JPEG é codificado aqui, e não por `canvas.toBlob`.** O codificador do
//! navegador não é o do `image`, e o arquivo deixaria de ser o mesmo que o
//! desktop gravaria para a mesma revelação.
//!
//! ## WebGPU primeiro, WebGL2 depois
//!
//! [`abrir`] pede um adaptador WebGPU; se o navegador não tem, pede um WebGL2.
//! Nos dois o shader entra por fragmento ([`revelacao_core::Entrada::Fragmento`]),
//! porque o WebGL2 não tem compute. Os limites vêm do adaptador que respondeu:
//! o padrão do WebGL2 no wgpu declara 2048 px de textura, e uma foto de 24 MP
//! tem 6000.

use std::sync::Arc;

use revelacao_core::{Ajustes, Entrada};
use wasm_bindgen::prelude::*;

/// O motor aberto sobre um `<canvas>`.
///
/// Guarda a cópia de trabalho (os pixels que os sliders animam) e a superfície
/// do canvas, redimensionada para o tamanho exato dessa cópia — quem escala para
/// a tela é o CSS, e um canvas maior que a imagem reamostraria a foto duas
/// vezes.
#[wasm_bindgen]
pub struct Motor {
    motor: revelacao_core::Motor,
    superficie: wgpu::Surface<'static>,
    formato: wgpu::TextureFormat,
    canvas: web_sys::HtmlCanvasElement,
    /// A cópia de trabalho, por `Arc`: o core sobe a textura só quando o
    /// ponteiro muda, e um arrasto de slider não sobe nada.
    trabalho: Option<(Arc<Vec<u8>>, u32, u32)>,
    backend: &'static str,
}

fn erro(mensagem: impl Into<String>) -> JsValue {
    JsValue::from_str(&mensagem.into())
}

/// Abre o motor sobre o canvas: WebGPU se houver, senão WebGL2.
///
/// Rejeita quando nenhum dos dois dá adaptador — a tela do site mostra "este
/// navegador não tem GPU disponível".
#[wasm_bindgen]
pub async fn abrir(canvas: web_sys::HtmlCanvasElement) -> Result<Motor, JsValue> {
    console_error_panic_hook::set_once();

    let instancia = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
        ..Default::default()
    });
    let superficie = instancia
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(|e| erro(format!("o canvas não virou superfície: {e}")))?;

    let adaptador = instancia
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&superficie),
            force_fallback_adapter: false,
        })
        .await
        .ok_or_else(|| erro("nenhum adaptador de GPU: nem WebGPU nem WebGL2"))?;

    let (backend, limites) = match adaptador.get_info().backend {
        wgpu::Backend::Gl => (
            "webgl",
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adaptador.limits()),
        ),
        _ => (
            "webgpu",
            wgpu::Limits::default().using_resolution(adaptador.limits()),
        ),
    };

    let motor = revelacao_core::Motor::abrir_com(&adaptador, Entrada::Fragmento, limites)
        .await
        .ok_or_else(|| erro("o adaptador respondeu, mas o dispositivo não abriu"))?;

    // O formato do canvas — o primeiro que **não** é sRGB, para o byte na tela
    // ser o byte do desktop e do arquivo. `Bgra8Unorm` no WebGPU, `Rgba8Unorm`
    // no WebGL2.
    let capacidades = superficie.get_capabilities(&adaptador);
    let formato = capacidades
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .ok_or_else(|| erro("a superfície só oferece formatos sRGB"))?;

    Ok(Motor {
        motor,
        superficie,
        formato,
        canvas,
        trabalho: None,
        backend,
    })
}

#[wasm_bindgen]
impl Motor {
    /// `"webgpu"` ou `"webgl"` — o que respondeu.
    pub fn backend(&self) -> String {
        self.backend.to_string()
    }

    /// O maior lado de textura que este dispositivo aceita, em pixels.
    ///
    /// Uma foto maior que isso não sobe inteira; o site reduz a exportação a
    /// este limite e diz isso na tela.
    pub fn limite_de_textura(&self) -> u32 {
        self.motor.limites().max_texture_dimension_2d
    }

    /// Os 46 valores do neutro, na ordem do `uniform`.
    pub fn ajustes_padrao() -> Vec<f32> {
        Ajustes::default().como_vetor().to_vec()
    }

    /// Os 46 nomes, na ordem do `uniform`, como JSON.
    pub fn nomes_dos_ajustes() -> String {
        serde_json::to_string(&Ajustes::NOMES[..]).expect("46 strings viram JSON")
    }

    /// Sobe a cópia de trabalho (RGBA, 4 bytes por pixel) e ajusta o canvas ao
    /// tamanho dela.
    pub fn carregar(&mut self, largura: u32, altura: u32, rgba: &[u8]) -> Result<(), JsValue> {
        if largura == 0 || altura == 0 {
            return Err(erro("imagem sem tamanho"));
        }
        if rgba.len() != (largura as usize) * (altura as usize) * 4 {
            return Err(erro(format!(
                "esperava {} bytes RGBA para {largura}×{altura}, recebi {}",
                largura as usize * altura as usize * 4,
                rgba.len()
            )));
        }
        let limite = self.limite_de_textura();
        if largura > limite || altura > limite {
            return Err(erro(format!(
                "{largura}×{altura} passa do limite de textura deste dispositivo ({limite} px)"
            )));
        }

        self.canvas.set_width(largura);
        self.canvas.set_height(altura);
        self.superficie.configure(
            self.motor_dispositivo(),
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: self.formato,
                width: largura,
                height: altura,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        );
        self.trabalho = Some((Arc::new(rgba.to_vec()), largura, altura));
        Ok(())
    }

    /// Aplica os 46 ajustes à cópia de trabalho e desenha no canvas.
    pub fn aplicar(&mut self, ajustes: &[f32]) -> Result<(), JsValue> {
        let ajustes = Ajustes::de_vetor(ajustes)
            .ok_or_else(|| erro(format!("esperava 46 ajustes, recebi {}", ajustes.len())))?;
        let (pixels, largura, altura) = self
            .trabalho
            .clone()
            .ok_or_else(|| erro("nenhuma imagem carregada"))?;

        let quadro = self
            .superficie
            .get_current_texture()
            .map_err(|e| erro(format!("a superfície não deu um quadro: {e:?}")))?;
        let vista = quadro
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.motor
            .desenhar(&pixels, largura, altura, &ajustes, &vista, self.formato)
            .ok_or_else(|| erro("o motor não desenhou"))?;
        quadro.present();
        Ok(())
    }

    /// Revela uma imagem **inteira** (não a cópia de trabalho) e devolve o JPEG.
    ///
    /// É a exportação: o site manda a foto na resolução de saída, os mesmos 46
    /// ajustes e a qualidade (1–100). Lê de volta da GPU e codifica com o mesmo
    /// codificador do desktop.
    pub async fn exportar_jpeg(
        &mut self,
        largura: u32,
        altura: u32,
        rgba: &[u8],
        ajustes: &[f32],
        qualidade: u8,
    ) -> Result<Vec<u8>, JsValue> {
        let ajustes = Ajustes::de_vetor(ajustes)
            .ok_or_else(|| erro(format!("esperava 46 ajustes, recebi {}", ajustes.len())))?;
        if rgba.len() != (largura as usize) * (altura as usize) * 4 {
            return Err(erro("os bytes não batem com largura × altura × 4"));
        }
        let limite = self.limite_de_textura();
        if largura > limite || altura > limite {
            return Err(erro(format!(
                "{largura}×{altura} passa do limite de textura deste dispositivo ({limite} px)"
            )));
        }

        let pixels = Arc::new(rgba.to_vec());
        let revelada = self
            .motor
            .revelar_async(&pixels, largura, altura, &ajustes)
            .await
            .ok_or_else(|| erro("a GPU não devolveu a imagem revelada"))?;
        drop(pixels);

        revelacao_core::jpeg::codificar(&revelada, qualidade.clamp(1, 100))
            .map_err(|e| erro(format!("o JPEG não codificou: {e}")))
    }
}

impl Motor {
    fn motor_dispositivo(&self) -> &wgpu::Device {
        self.motor.dispositivo()
    }
}
