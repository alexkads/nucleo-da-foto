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

use revelacao_core::{Ajustes, Corte, Entrada};
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

/// Quantos números o enquadramento carrega.
const CAMPOS_DO_CORTE: usize = 8;

/// O enquadramento vindo do JavaScript: `[x, y, largura, altura, giro_90,
/// angulo, espelho_h, espelho_v]`, na ordem de `Corte::novo`.
///
/// Os dois espelhos viajam como 0 ou 1 e o giro como inteiro num `f32`: um
/// vetor só, do mesmo tipo do dos ajustes, é o que o `wasm-bindgen` passa sem
/// custo. `Corte::novo` limita tudo, então valor fora da faixa entra corrigido
/// em vez de virar erro — o mesmo tratamento que o desktop dá.
fn corte_de_vetor(v: &[f32]) -> Result<Corte, JsValue> {
    if v.len() != CAMPOS_DO_CORTE {
        return Err(erro(format!(
            "esperava {CAMPOS_DO_CORTE} campos de corte, recebi {}",
            v.len()
        )));
    }
    Ok(Corte::novo(
        v[0],
        v[1],
        v[2],
        v[3],
        v[4] as i32,
        v[5],
        v[6] != 0.0,
        v[7] != 0.0,
    ))
}

/// O enquadramento neutro: a foto inteira, sem giro, ângulo ou espelho.
#[wasm_bindgen]
pub fn corte_inteiro() -> Vec<f32> {
    vec![0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0]
}

/// A geometria do enquadramento, para a tela desenhá-lo — **calculada aqui**.
///
/// 🚨 O editor não recalcula nada: ele desenha o retângulo com estes números e
/// anuncia o tamanho de saída com estes números. Dois arredondamentos
/// diferentes fariam o operador enquadrar uma coisa na tela e o cliente receber
/// outra, sem erro em lugar nenhum. Ver `Corte::retangulo` e o teste
/// `as_dimensoes_de_saida_sao_as_do_arquivo`.
///
/// Devolve, nesta ordem: largura e altura **do espaço girado** (onde o
/// retângulo mora), o retângulo (`x, y, w, h`) nesse espaço, e as dimensões do
/// arquivo que vai sair.
#[wasm_bindgen]
pub fn enquadramento(corte: &[f32], largura: u32, altura: u32) -> Result<Vec<f32>, JsValue> {
    let corte = corte_de_vetor(corte)?;
    let (lg, ag) = corte.dimensoes_giradas(largura, altura);
    let (x, y, w, h) = corte.retangulo(lg, ag);
    let (sw, sh) = corte.dimensoes_de_saida(largura, altura);
    Ok(vec![
        lg as f32, ag as f32, x as f32, y as f32, w as f32, h as f32, sw as f32, sh as f32,
    ])
}

/// Codifica pixels RGBA em JPEG — **sem GPU e sem canvas**.
///
/// É o caminho da *importação* do pós-venda, que não revela nada: o operador
/// escolhe 30 originais de câmera e o navegador recodifica cada um antes de
/// subir, para a API não pagar a decodificação de 30 MB por foto (decisão do
/// dono, 2026-09-05).
///
/// 🔑 **Livre de GPU de propósito.** [`Motor::exportar_jpeg`] precisa de um
/// `<canvas>`, que não existe dentro de um Worker — e é dentro de um Worker
/// que a importação roda, para a barra de progresso não travar com a aba. Como
/// não há ajuste nenhum a aplicar, a passagem pelo shader seria uma cópia cara
/// de ida e volta pela GPU.
///
/// 🔑 **O codificador é o mesmo** de [`Motor::exportar_jpeg`] e do desktop
/// (`revelacao_core::jpeg`): a foto importada e a foto revelada saem do mesmo
/// lugar, e "recomprimir no navegador" não vira um segundo formato de arquivo
/// para o mesmo produto.
///
/// Quem reduz o tamanho é o JavaScript, antes de chamar aqui: `createImageBitmap`
/// decodifica com o decodificador nativo e o `drawImage` reamostra — os dois
/// muito mais rápidos que os equivalentes em wasm sem SIMD. Ver `imagem.ts`.
#[wasm_bindgen]
pub fn comprimir_jpeg(
    largura: u32,
    altura: u32,
    rgba: &[u8],
    qualidade: u8,
) -> Result<Vec<u8>, JsValue> {
    console_error_panic_hook::set_once();

    let esperado = (largura as usize)
        .checked_mul(altura as usize)
        .and_then(|p| p.checked_mul(4))
        .ok_or_else(|| erro(format!("{largura}×{altura} não cabe na memória")))?;
    if rgba.len() != esperado {
        return Err(erro(format!(
            "os bytes não batem com largura × altura × 4: {} para {esperado}",
            rgba.len()
        )));
    }

    let buffer = image::RgbaImage::from_raw(largura, altura, rgba.to_vec())
        .ok_or_else(|| erro("os pixels não formam uma imagem"))?;

    revelacao_core::jpeg::codificar(
        &image::DynamicImage::ImageRgba8(buffer),
        qualidade.clamp(1, 100),
    )
    .map_err(|e| erro(format!("o JPEG não codificou: {e}")))
}

/// Abre o motor sobre o canvas: WebGPU se houver, senão WebGL2.
///
/// # 🚨 Por que o backend é escolhido **antes** de tocar no canvas
///
/// Um canvas só aceita **um** tipo de contexto: pedir `webgpu` nele o impede
/// de dar `webgl2` depois. Uma instância com os dois backends resolve a
/// surface pelo primeiro que responder — e num navegador onde `navigator.gpu`
/// **existe mas não devolve adaptador** (Chrome sem GPU compatível, WebGPU
/// desligado por política, headless) o canvas era consumido pela tentativa
/// WebGPU e o WebGL2 já não podia mais entrar. O editor dizia "este navegador
/// não tem GPU" numa máquina com WebGL2 perfeito.
///
/// 🔑 Isso **não aparece em HTTP**: `navigator.gpu` só existe em contexto
/// seguro, então em `http://` o caminho WebGPU nem é tentado. Foi encontrado
/// em produção, em 2026-09-04, com a pilha local passando.
///
/// O conserto é perguntar primeiro: `request_adapter` **sem**
/// `compatible_surface` não toca no canvas. Só depois de saber quem responde é
/// que a surface é criada, com uma instância de um backend só.
#[wasm_bindgen]
pub async fn abrir(canvas: web_sys::HtmlCanvasElement) -> Result<Motor, JsValue> {
    console_error_panic_hook::set_once();

    let sem_superficie = wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    };

    // 1. WebGPU: a pergunta **não** toca no canvas, e por isso pode vir antes.
    let instancia = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        ..Default::default()
    });
    let webgpu = instancia.request_adapter(&sem_superficie).await;

    let (adaptador, superficie, backend) = match webgpu {
        Some(adaptador) => {
            let superficie = instancia
                .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
                .map_err(|e| erro(format!("o canvas não virou superfície (webgpu): {e}")))?;
            (adaptador, superficie, "webgpu")
        }
        // 2. Não respondeu: instância nova, só GL — e aqui a surface vem
        //    **antes**, porque o backend WebGL2 nasce de um canvas: sem ele
        //    não há contexto, e `request_adapter` devolveria `None` mesmo num
        //    navegador que tem WebGL2 de sobra.
        None => {
            let instancia = wgpu::Instance::new(wgpu::InstanceDescriptor {
                backends: wgpu::Backends::GL,
                ..Default::default()
            });
            let superficie = instancia
                .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
                .map_err(|e| erro(format!("o canvas não virou superfície (webgl): {e}")))?;
            let adaptador = instancia
                .request_adapter(&wgpu::RequestAdapterOptions {
                    compatible_surface: Some(&superficie),
                    ..sem_superficie
                })
                .await
                .ok_or_else(|| erro("nenhum adaptador de GPU: nem WebGPU nem WebGL2"))?;
            (adaptador, superficie, "webgl")
        }
    };

    let info = adaptador.get_info();
    web_sys::console::log_1(&JsValue::from_str(&format!(
        "[Revelação] adaptador: {} ({backend}, {:?})",
        info.name, info.device_type
    )));

    // 🔑 **Os limites pedidos são exatamente os do adaptador.** `Limits::default()`
    // é o piso de uma GPU de verdade e passa do que um adaptador de software
    // (SwiftShader, o headless do Chrome) oferece — e `request_device` recusa
    // qualquer limite acima do suportado. O que o adaptador diz que tem, ele
    // dá; o que interessa ao motor é `max_texture_dimension_2d`, que vem junto.
    let limites = adaptador.limits();

    let motor = revelacao_core::Motor::abrir_com(&adaptador, Entrada::Fragmento, limites)
        .await
        .map_err(|e| {
            erro(format!(
                "o adaptador respondeu, mas o dispositivo não abriu: {e}"
            ))
        })?;

    // O formato do canvas é o primeiro que **não** é sRGB (Bgra8Unorm no
    // WebGPU, Rgba8Unorm no WebGL2): o byte na tela é o byte do arquivo.
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

    /// Revela uma imagem **inteira** (não a cópia de trabalho), aplica o
    /// enquadramento e devolve o JPEG.
    ///
    /// É a exportação: o site manda a foto na resolução de saída, os mesmos 46
    /// ajustes, o enquadramento e a qualidade (1–100). Lê de volta da GPU e
    /// codifica com o mesmo codificador do desktop.
    ///
    /// 🔑 **A ordem é a da tela**: o shader devolve a foto inteira e o
    /// enquadramento vem **depois** — é o que `image_exporter.rs` faz no
    /// desktop. Inverter daria uma vinheta centrada no quadro cortado em vez de
    /// no original.
    pub async fn exportar_jpeg(
        &mut self,
        largura: u32,
        altura: u32,
        rgba: &[u8],
        ajustes: &[f32],
        corte: &[f32],
        qualidade: u8,
    ) -> Result<Vec<u8>, JsValue> {
        let corte = corte_de_vetor(corte)?;
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

        let enquadrada = revelacao_core::transformacao::aplicar(&revelada, &corte, true);
        drop(revelada);

        revelacao_core::jpeg::codificar(&enquadrada, qualidade.clamp(1, 100))
            .map_err(|e| erro(format!("o JPEG não codificou: {e}")))
    }
}

impl Motor {
    fn motor_dispositivo(&self) -> &wgpu::Device {
        self.motor.dispositivo()
    }
}
