//! A tela: as camadas, o cruzamento e o desenho.

use std::sync::Arc;

use revelacao_core::{Ajustes, Corte, Entrada};
use wasm_bindgen::prelude::*;

use crate::contrato::{
    self, alfa_da_saida, alfa_e_zoom_da_entrada, camada_a_descartar, escala_do_original,
    fica_no_ar, progresso, CamadaUniforme,
};

/// O formato das texturas em que cada foto é revelada.
///
/// Não-sRGB de propósito, como a superfície: o byte que o shader de revelação
/// escreveu é o byte que vai para a tela, sem uma conversão de gama no meio que
/// o editor não tem.
const FORMATO_DA_CAMADA: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn erro(mensagem: impl Into<String>) -> JsValue {
    JsValue::from_str(&mensagem.into())
}

/// Uma foto no ar — a que entra ou a que sai.
struct Camada {
    /// Os pixels como vieram do JavaScript, por `Arc`: o `revelacao_core` sobe
    /// a textura só quando o ponteiro muda, e mexer num slider não sobe nada.
    pixels: Arc<Vec<u8>>,
    largura: u32,
    altura: u32,
    ajustes: Ajustes,
    corte: Corte,
    /// A razão entre estes pixels e a foto original (ver `Tela::mostrar`).
    escala: f32,
    /// A foto **revelada**, do tamanho dela. É o que o compositor amostra; a
    /// vista é recriada para revelar porque `TextureView` não se clona, e o
    /// empréstimo de `self.camadas[i]` não sobrevive à chamada ao motor.
    textura: wgpu::Texture,
    grupo: wgpu::BindGroup,
    uniforme: wgpu::Buffer,
    /// Os ajustes mudaram e a textura ainda é a de antes.
    suja: bool,
    /// Quando esta camada entrou, no relógio do `quadro`. `None` = ainda não
    /// começou a contar (o primeiro quadro é quem carimba).
    entrou_em: Option<f64>,
    /// Esta camada deixou de ser a de cima e está esmaecendo. Ver [`Saida`].
    saindo: Option<Saida>,
    /// O alfa do último quadro — é dele que a saída parte.
    alfa: f32,
}

/// A despedida de uma camada.
///
/// 🚨 **Ela parte do alfa que a camada tinha, e não de 1.** Sem isto, trocar de
/// foto antes de o cruzamento anterior terminar fazia a foto do meio saltar: ela
/// estava em 30% de opacidade, virava "a que sai", e o cálculo antigo — que
/// media o tempo desde a **entrada** dela — a colocava direto num alfa muito
/// menor. Com o cruzamento em um segundo isso deixou de ser sutil.
struct Saida {
    /// Quando começou a sair; `None` até o primeiro quadro carimbar.
    em: Option<f64>,
    /// De qual opacidade ela parte.
    de: f32,
}

/// A tela do cliente aberta sobre um `<canvas>`.
#[wasm_bindgen]
pub struct Tela {
    motor: revelacao_core::Motor,
    superficie: wgpu::Surface<'static>,
    formato: wgpu::TextureFormat,
    canvas: web_sys::HtmlCanvasElement,
    pipeline: wgpu::RenderPipeline,
    leiaute: wgpu::BindGroupLayout,
    amostrador: wgpu::Sampler,
    /// No máximo duas: a que sai (índice 0) e a que entra.
    camadas: Vec<Camada>,
    largura: u32,
    altura: u32,
    backend: &'static str,
}

/// Abre a tela sobre o canvas: WebGPU se houver, senão WebGL2.
///
/// 🚨 **O backend é escolhido antes de tocar no canvas**, pelo mesmo motivo do
/// editor: um canvas só aceita **um** tipo de contexto, e num navegador onde
/// `navigator.gpu` existe mas não devolve adaptador a tentativa WebGPU consumia
/// o canvas e o WebGL2 já não podia entrar.
#[wasm_bindgen]
pub async fn abrir(canvas: web_sys::HtmlCanvasElement) -> Result<Tela, JsValue> {
    console_error_panic_hook::set_once();

    let sem_superficie = wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    };

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
        "[Tela do cliente] adaptador: {} ({backend}, {:?})",
        info.name, info.device_type
    )));

    // Os limites pedidos são os do adaptador, como no editor: `Limits::default()`
    // passa do que um adaptador de software oferece, e `request_device` recusa.
    let limites = adaptador.limits();
    let motor = revelacao_core::Motor::abrir_com(&adaptador, Entrada::Fragmento, limites)
        .await
        .map_err(|e| erro(format!("o dispositivo não abriu: {e}")))?;

    let capacidades = superficie.get_capabilities(&adaptador);
    let formato = capacidades
        .formats
        .iter()
        .copied()
        .find(|f| !f.is_srgb())
        .ok_or_else(|| erro("a superfície só oferece formatos sRGB"))?;

    let dispositivo = motor.dispositivo();
    let modulo = dispositivo.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Tela do cliente: compositor"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/compositor.wgsl").into()),
    });
    let leiaute = dispositivo.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Tela do cliente: camada"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let pipeline_leiaute = dispositivo.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Tela do cliente"),
        bind_group_layouts: &[&leiaute],
        push_constant_ranges: &[],
    });
    let pipeline = dispositivo.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Tela do cliente"),
        layout: Some(&pipeline_leiaute),
        vertex: wgpu::VertexState {
            module: &modulo,
            entry_point: Some("vs"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &modulo,
            entry_point: Some("fs"),
            // Alfa comum: a que sai esmaece por baixo da que entra.
            targets: &[Some(wgpu::ColorTargetState {
                format: formato,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        multiview: None,
        cache: None,
    });
    let amostrador = dispositivo.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Tela do cliente"),
        // Linear nos dois: a foto é sempre reduzida para caber na janela, e
        // `Nearest` numa redução de 2048 para 1080 serrilha cada borda.
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    Ok(Tela {
        motor,
        superficie,
        formato,
        canvas,
        pipeline,
        leiaute,
        amostrador,
        camadas: Vec::new(),
        largura: 0,
        altura: 0,
        backend,
    })
}

#[wasm_bindgen]
impl Tela {
    /// `"webgpu"` ou `"webgl"` — o que respondeu.
    pub fn backend(&self) -> String {
        self.backend.to_string()
    }

    /// O maior lado de textura que este dispositivo aceita, em pixels.
    pub fn limite_de_textura(&self) -> u32 {
        self.motor.limites().max_texture_dimension_2d
    }

    /// A janela mudou de tamanho: o outro monitor, a tela cheia, o arrasto.
    ///
    /// O canvas fica com o tamanho **em pixels de dispositivo** — quem escala
    /// para CSS é a folha de estilo. Uma janela de 1920 num monitor 2× tem
    /// 3840 px de foto, e é isso que o cliente vê de perto.
    pub fn redimensionar(&mut self, largura: u32, altura: u32) {
        let largura = largura.max(1);
        let altura = altura.max(1);
        if self.largura == largura && self.altura == altura {
            return;
        }
        self.largura = largura;
        self.altura = altura;
        self.canvas.set_width(largura);
        self.canvas.set_height(altura);
        self.superficie.configure(
            self.motor.dispositivo(),
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
    }

    /// Uma foto nova entra, cruzando com a que estiver no ar.
    ///
    /// Os pixels vêm decodificados do JavaScript (RGBA), que é onde o
    /// decodificador nativo está. `ajustes` são os 53 na ordem do `uniform` e
    /// `corte` os 8 do enquadramento — os mesmos vetores do editor.
    /// `lado_original` é o maior lado da foto original; ausente, vale a cópia.
    pub fn mostrar(
        &mut self,
        largura: u32,
        altura: u32,
        rgba: &[u8],
        ajustes: &[f32],
        corte: &[f32],
        lado_original: Option<u32>,
    ) -> Result<(), JsValue> {
        contrato::conferir_foto(largura, altura, rgba.len(), self.limite_de_textura())
            .map_err(erro)?;
        let ajustes = ajustes_de(ajustes)?;
        let corte = corte_de(corte)?;

        let mut camada = self.nova_camada(largura, altura, rgba, ajustes, corte);
        // 🔑 Os módulos locais do darktable medem em pixels da foto original;
        // a tela recebe a cópia de trabalho, como o editor, e precisa da mesma
        // escala para mostrar o mesmo estilo.
        camada.escala = escala_do_original(largura, altura, lado_original);
        // A que estava no ar começa a se despedir **do alfa em que está**.
        if let Some(anterior) = self.camadas.last_mut() {
            anterior.saindo = Some(Saida {
                em: None,
                de: anterior.alfa,
            });
        }
        // Só duas ficam — atravessar a tira com a seta não pode acumular
        // camadas.
        //
        // 🚨 **Sai a menos visível, e não a mais antiga** — ver
        // `contrato::camada_a_descartar`.
        if self.camadas.len() >= 2 {
            let descartar =
                camada_a_descartar(self.camadas.iter().map(|c| (c.saindo.is_some(), c.alfa)));
            self.camadas.remove(descartar);
        }
        self.camadas.push(camada);
        Ok(())
    }

    /// Troca os ajustes da foto que está entrando — o editor mexendo, ao vivo.
    ///
    /// 🔑 **Não reinicia o cruzamento**: quem está no balcão vê a mesma foto
    /// mudar de tratamento, e não a foto entrar de novo a cada slider.
    pub fn ajustar(&mut self, ajustes: &[f32], corte: &[f32]) -> Result<(), JsValue> {
        let ajustes = ajustes_de(ajustes)?;
        let corte = corte_de(corte)?;
        let Some(camada) = self.camadas.last_mut() else {
            return Ok(());
        };
        camada.ajustes = ajustes;
        camada.corte = corte;
        camada.suja = true;
        Ok(())
    }

    /// Há foto na tela?
    pub fn tem_foto(&self) -> bool {
        !self.camadas.is_empty()
    }

    /// Tira tudo da tela — a galeria mandou "nada em foco".
    pub fn limpar(&mut self) {
        self.camadas.clear();
    }

    /// Desenha um quadro. `agora` é o relógio do `requestAnimationFrame`.
    ///
    /// Devolve `true` enquanto houver animação — é o que diz ao JavaScript para
    /// pedir o próximo quadro. Uma tela parada custa zero.
    pub fn quadro(&mut self, agora: f64) -> Result<bool, JsValue> {
        if self.largura == 0 || self.altura == 0 {
            return Ok(false);
        }

        // 1. Quem mudou de ajuste é revelado de novo, na textura dela.
        let sujas: Vec<usize> = self
            .camadas
            .iter()
            .enumerate()
            .filter(|(_, c)| c.suja)
            .map(|(i, _)| i)
            .collect();
        let mut grades_pendentes = false;
        for i in sujas {
            let (pixels, largura, altura, ajustes, escala, corte) = {
                let c = &self.camadas[i];
                (
                    c.pixels.clone(),
                    c.largura,
                    c.altura,
                    c.ajustes,
                    c.escala,
                    c.corte.clone(),
                )
            };
            self.motor.definir_escala_do_original(escala);
            // 🔑 As vinhetas são do recorte, e o motor revela a foto inteira: sem
            // o corte desta camada, o cliente veria a vinheta centrada na foto
            // inteira, e não a que o editor mostra e o arquivo tem.
            self.motor.definir_corte(&corte);
            // O editor manda um aviso por quadro de arrasto: as grades RGB esperam
            // ele parar, como no editor.
            self.motor.definir_relogio(Some(agora));
            let vista = self.camadas[i]
                .textura
                .create_view(&wgpu::TextureViewDescriptor::default());
            self.motor
                .desenhar(
                    &pixels,
                    largura,
                    altura,
                    &ajustes,
                    &vista,
                    FORMATO_DA_CAMADA,
                )
                .ok_or_else(|| erro("o motor não revelou a foto"))?;
            // 🔑 Com grades de antes, a camada continua suja e o quadro seguinte
            // desenha de novo — é o que as refaz quando o editor parar de mexer.
            if self.motor.grades_pendentes() {
                grades_pendentes = true;
            } else {
                self.camadas[i].suja = false;
            }
        }

        // 2. O tempo de cada camada, e os uniformes que saem dele.
        let mut animando = false;
        let janela = (self.largura as f32, self.altura as f32);
        for camada in self.camadas.iter_mut() {
            let (alfa, zoom) = match &mut camada.saindo {
                Some(saida) => {
                    let comecou = *saida.em.get_or_insert(agora);
                    let t = progresso(agora, comecou);
                    if t < 1.0 {
                        animando = true;
                    }
                    // Só esmaece: dois movimentos cruzados pareceriam empurrão.
                    (alfa_da_saida(saida.de, t), 1.0)
                }
                None => {
                    let entrou = *camada.entrou_em.get_or_insert(agora);
                    let t = progresso(agora, entrou);
                    if t < 1.0 {
                        animando = true;
                    }
                    alfa_e_zoom_da_entrada(t)
                }
            };
            camada.alfa = alfa;
            let uniforme = montar_uniforme(camada, janela, alfa, zoom);
            self.motor
                .fila()
                .write_buffer(&camada.uniforme, 0, bytemuck::bytes_of(&uniforme));
        }
        // Quem terminou de sair vai embora — e nunca a última, que é a foto no
        // ar mesmo quando ela chegou sem cruzamento nenhum.
        let ultima = self.camadas.len().saturating_sub(1);
        let mut i = 0;
        self.camadas.retain(|c| {
            let fica = fica_no_ar(i == ultima, c.alfa, c.saindo.is_some());
            i += 1;
            fica
        });

        // 3. Uma passada: preto, e as camadas por cima.
        let quadro = self
            .superficie
            .get_current_texture()
            .map_err(|e| erro(format!("a superfície não deu um quadro: {e:?}")))?;
        let vista = quadro
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let dispositivo = self.motor.dispositivo();
        let mut encoder = dispositivo.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Tela do cliente"),
        });
        {
            let mut passada = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Tela do cliente"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &vista,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // 🔑 Preto, e não a cor do tema: é onde a cor da foto é
                        // julgada por quem paga por ela.
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            passada.set_pipeline(&self.pipeline);
            for camada in &self.camadas {
                passada.set_bind_group(0, &camada.grupo, &[]);
                passada.draw(0..4, 0..1);
            }
        }
        self.motor.fila().submit(std::iter::once(encoder.finish()));
        quadro.present();
        Ok(animando || grades_pendentes)
    }
}

impl Tela {
    fn nova_camada(
        &self,
        largura: u32,
        altura: u32,
        rgba: &[u8],
        ajustes: Ajustes,
        corte: Corte,
    ) -> Camada {
        let dispositivo = self.motor.dispositivo();
        let textura = dispositivo.create_texture(&wgpu::TextureDescriptor {
            label: Some("Tela do cliente: foto revelada"),
            size: wgpu::Extent3d {
                width: largura,
                height: altura,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMATO_DA_CAMADA,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let vista = textura.create_view(&wgpu::TextureViewDescriptor::default());
        let uniforme = dispositivo.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Tela do cliente: camada"),
            size: std::mem::size_of::<CamadaUniforme>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let grupo = dispositivo.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Tela do cliente: camada"),
            layout: &self.leiaute,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforme.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&vista),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.amostrador),
                },
            ],
        });

        Camada {
            escala: 1.0,
            pixels: Arc::new(rgba.to_vec()),
            largura,
            altura,
            ajustes,
            corte,
            textura,
            grupo,
            uniforme,
            suja: true,
            entrou_em: None,
            saindo: None,
            alfa: 0.0,
        }
    }
}

fn ajustes_de(v: &[f32]) -> Result<Ajustes, JsValue> {
    contrato::ajustes_de(v).map_err(erro)
}

fn corte_de(v: &[f32]) -> Result<Corte, JsValue> {
    contrato::corte_de(v).map_err(erro)
}

/// Onde esta camada fica na janela, e que pedaço da foto ela mostra — a conta
/// é `contrato::montar_uniforme`.
fn montar_uniforme(camada: &Camada, janela: (f32, f32), alfa: f32, zoom: f32) -> CamadaUniforme {
    contrato::montar_uniforme(
        camada.largura,
        camada.altura,
        &camada.corte,
        janela,
        alfa,
        zoom,
    )
}
