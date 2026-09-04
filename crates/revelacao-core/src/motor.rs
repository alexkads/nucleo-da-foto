//! O motor de revelação: wgpu, o WGSL, e a passada que aplica os 46 ajustes.
//!
//! 🔑 **Ele mora aqui, e não no crate de interface, porque wgpu é detalhe
//! técnico.** Estava em `ui-gpui/revelacao/processador.rs` por herança: veio do
//! `gpu_processor.rs` do `crates/ui`, onde nasceu colado na tela. O que o
//! descolou foi a exportação: `ImageExporterImpl` tinha a **própria**
//! implementação dos ajustes, na CPU, com 15 dos 46 e uma matemática que já
//! divergia. Com o motor num lugar só, a tela e o arquivo atravessam o
//! **mesmo** `.wgsl` — e desde 2026-09-04 o navegador também.
//!
//! ## As duas entradas
//!
//! O corpo (`shaders/corpo.wgsl`) é um; o ponto de entrada são dois:
//!
//! | [`Entrada`] | quem usa | por quê |
//! |---|---|---|
//! | `Compute` | desktop (Metal, Vulkan, DX12) | um invocation por pixel, storage texture |
//! | `Fragmento` | navegador (WebGPU **e** WebGL2) | o WebGL2 não tem compute nem storage texture |
//!
//! A concatenação é feita em tempo de compilação do Rust (`concat!` +
//! `include_str!`), então não há como as duas entradas verem corpos diferentes.
//! Quem prende que revelam o mesmo pixel é
//! `o_fragmento_revela_o_mesmo_pixel_que_o_compute`.
//!
//! ## Síncrono no desktop, assíncrono no navegador
//!
//! `request_device` e a leitura de volta (`map_async`) são assíncronos no wgpu.
//! O desktop bloqueia a thread de fundo com `pollster` ([`Motor::abrir`],
//! [`Motor::revelar`]); o navegador não tem thread para bloquear, e usa
//! [`Motor::abrir_com`] e [`Motor::revelar_async`]. É a mesma função por baixo.
//!
//! ## O que ficou do outro lado
//!
//! A fila de pedidos — thread, canal, descarte do pedido velho durante um
//! arrasto — continua em `ui-gpui`: é resposta a um dedo arrastando um slider,
//! e não tem o que fazer numa exportação, que roda uma vez e espera.

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;

use image::DynamicImage;
use lru::LruCache;

use crate::ajustes::{Ajustes, TAMANHO_DO_UNIFORM};

/// O shader do desktop: o corpo mais a entrada por compute.
const SHADER_COMPUTE: &str = concat!(
    include_str!("shaders/corpo.wgsl"),
    include_str!("shaders/entrada_compute.wgsl")
);

/// O shader do navegador: o corpo mais a entrada por vértice e fragmento.
const SHADER_FRAGMENTO: &str = concat!(
    include_str!("shaders/corpo.wgsl"),
    include_str!("shaders/entrada_fragmento.wgsl")
);

/// O formato em que a revelação é lida de volta — o mesmo da textura de entrada.
const FORMATO_DE_LEITURA: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Por onde o shader entra: ver o módulo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entrada {
    /// `@compute` escrevendo numa storage texture. O desktop.
    Compute,
    /// `@vertex` + `@fragment` escrevendo no alvo do render pass. O navegador.
    Fragmento,
}

/// O pipeline de cada entrada.
///
/// O de fragmento é **um por formato de alvo**: a textura de leitura é
/// `Rgba8Unorm`, e a superfície de um canvas é o que o navegador preferir
/// (`Bgra8Unorm` no WebGPU). O render pipeline grava o formato do alvo, então
/// há um para cada, criados quando pedidos e guardados.
enum Pipeline {
    Compute(wgpu::ComputePipeline),
    Fragmento {
        modulo: wgpu::ShaderModule,
        layout_do_grupo: wgpu::BindGroupLayout,
        layout: wgpu::PipelineLayout,
        por_formato: HashMap<wgpu::TextureFormat, wgpu::RenderPipeline>,
    },
}

/// Recursos por tamanho de imagem.
///
/// Trocar de foto no mesmo tamanho reaproveita textura e buffer; trocar de
/// tamanho cria de novo. O cache guarda cinco tamanhos porque uma sessão de
/// revelação alterna entre poucas resoluções (o preview, o full, o recorte), e
/// recriar textura a cada troca aparece como engasgo.
struct Recursos {
    textura_entrada: wgpu::Texture,
    textura_saida: wgpu::Texture,
    buffer_ajustes: wgpu::Buffer,
    buffer_saida: wgpu::Buffer,
    grupo: wgpu::BindGroup,
    bytes_por_linha_alinhado: u32,
    bytes_por_linha: u32,
    /// Os pixels que já estão na textura de entrada — por identidade de `Arc`,
    /// não por conteúdo. Comparar 24 MB byte a byte para decidir se vale subir
    /// 24 MB custaria quase o mesmo que subir.
    ultimos_pixels: Option<Arc<Vec<u8>>>,
}

fn criar_recursos(
    dispositivo: &wgpu::Device,
    pipeline: &Pipeline,
    largura: u32,
    altura: u32,
) -> Recursos {
    /// Exigência do wgpu ao copiar textura para buffer.
    const ALINHAMENTO: u32 = 256;
    let bytes_por_linha = 4 * largura;
    let bytes_por_linha_alinhado = bytes_por_linha.div_ceil(ALINHAMENTO) * ALINHAMENTO;

    let descritor = |rotulo, uso| wgpu::TextureDescriptor {
        label: Some(rotulo),
        size: wgpu::Extent3d {
            width: largura,
            height: altura,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMATO_DE_LEITURA,
        usage: uso,
        view_formats: &[],
    };

    let textura_entrada = dispositivo.create_texture(&descritor(
        "Input Texture",
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
    ));
    // A saída é escrita pelo shader como storage (compute) ou como alvo de
    // render pass (fragmento) — e lida de volta por cópia nos dois casos.
    let uso_da_saida = match pipeline {
        Pipeline::Compute(_) => wgpu::TextureUsages::STORAGE_BINDING,
        Pipeline::Fragmento { .. } => wgpu::TextureUsages::RENDER_ATTACHMENT,
    };
    let textura_saida = dispositivo.create_texture(&descritor(
        "Output Texture",
        uso_da_saida | wgpu::TextureUsages::COPY_SRC,
    ));

    let buffer_ajustes = dispositivo.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Params Buffer"),
        size: TAMANHO_DO_UNIFORM,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let vista_de_entrada = textura_entrada.create_view(&wgpu::TextureViewDescriptor::default());
    let vista_de_saida = textura_saida.create_view(&wgpu::TextureViewDescriptor::default());
    let entrada = wgpu::BindGroupEntry {
        binding: 0,
        resource: wgpu::BindingResource::TextureView(&vista_de_entrada),
    };
    let ajustes = wgpu::BindGroupEntry {
        binding: 2,
        resource: buffer_ajustes.as_entire_binding(),
    };
    let grupo = match pipeline {
        Pipeline::Compute(pipeline) => dispositivo.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Compute Bind Group"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                entrada,
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&vista_de_saida),
                },
                ajustes,
            ],
        }),
        // O fragmento não tem o binding 1: a saída é o alvo do passe.
        Pipeline::Fragmento {
            layout_do_grupo, ..
        } => dispositivo.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Fragment Bind Group"),
            layout: layout_do_grupo,
            entries: &[entrada, ajustes],
        }),
    };

    let buffer_saida = dispositivo.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Output Buffer"),
        size: (bytes_por_linha_alinhado * altura) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    Recursos {
        textura_entrada,
        textura_saida,
        buffer_ajustes,
        buffer_saida,
        grupo,
        bytes_por_linha_alinhado,
        bytes_por_linha,
        ultimos_pixels: None,
    }
}

/// O dispositivo, o pipeline e o cache de recursos — abertos uma vez.
///
/// ⚠️ **Abrir custa**: `request_adapter` e `request_device` são assíncronos e
/// levam dezenas de milissegundos. Quem revela abre um na thread de fundo e o
/// mantém pela sessão inteira; quem exporta abre um por lote, não por foto.
pub struct Motor {
    dispositivo: wgpu::Device,
    fila: wgpu::Queue,
    pipeline: Pipeline,
    /// Recursos por tamanho de imagem — ver [`Recursos`].
    cache: LruCache<(u32, u32), Recursos>,
}

impl Motor {
    /// `None` quando não há adaptador de GPU.
    ///
    /// 🚨 **E aí não há caminho de CPU para cair.** O `crates/ui` tinha um —
    /// uma segunda implementação da mesma matemática, com resultado diferente
    /// do shader — e ele não veio junto de propósito: um motor que responde
    /// "certo por outro caminho" é pior que um que responde "não sei".
    ///
    /// Quem chama decide o que fazer com o `None`: a Revelação mostra a foto sem
    /// ajuste, e a exportação **falha**, porque gravar arquivo com os ajustes
    /// descartados em silêncio é o defeito que ela acabou de deixar de ter.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn abrir() -> Option<Self> {
        Self::abrir_por(Entrada::Compute)
    }

    /// [`Motor::abrir`] com a entrada escolhida — o desktop usa `Compute`; o
    /// `Fragmento` em nativo existe para o teste que compara os dois.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn abrir_por(entrada: Entrada) -> Option<Self> {
        let instancia = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adaptador =
            pollster::block_on(instancia.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            }))?;

        pollster::block_on(Self::abrir_com(
            &adaptador,
            entrada,
            wgpu::Limits::default(),
        ))
    }

    /// Abre o dispositivo num adaptador que quem chama já escolheu.
    ///
    /// É a porta do navegador: lá o adaptador tem de ser compatível com a
    /// superfície do canvas, e os limites dependem do backend que respondeu
    /// (`Limits::downlevel_webgl2_defaults()` no WebGL2, com a resolução do
    /// adaptador por cima — o padrão sozinho declara 2048 px de textura).
    pub async fn abrir_com(
        adaptador: &wgpu::Adapter,
        entrada: Entrada,
        limites: wgpu::Limits,
    ) -> Option<Self> {
        let (dispositivo, fila) = adaptador
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("VintageLightbox GPU"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limites,
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .ok()?;

        // 🚨 O `struct Params` do WGSL tem de casar com o `Ajustes`, campo a
        // campo: o `uniform` viaja como bytes crus e liga por **posição**, não
        // por nome. Quem prende isso é
        // `o_wgsl_declara_os_mesmos_46_campos_na_mesma_ordem`.
        let pipeline = match entrada {
            Entrada::Compute => {
                let modulo = dispositivo.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Image Adjustments Shader (compute)"),
                    source: wgpu::ShaderSource::Wgsl(SHADER_COMPUTE.into()),
                });
                Pipeline::Compute(dispositivo.create_compute_pipeline(
                    &wgpu::ComputePipelineDescriptor {
                        label: Some("Image Processing Pipeline"),
                        layout: None,
                        module: &modulo,
                        entry_point: Some("main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        cache: None,
                    },
                ))
            }
            Entrada::Fragmento => {
                let modulo = dispositivo.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Image Adjustments Shader (fragment)"),
                    source: wgpu::ShaderSource::Wgsl(SHADER_FRAGMENTO.into()),
                });
                // Explícito, e não `layout: None`: o mesmo bind group serve a
                // todos os pipelines por formato, e layouts implícitos são um
                // por pipeline.
                let layout_do_grupo =
                    dispositivo.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("Fragment Bind Group Layout"),
                        entries: &[
                            wgpu::BindGroupLayoutEntry {
                                binding: 0,
                                visibility: wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Texture {
                                    sample_type: wgpu::TextureSampleType::Float {
                                        filterable: false,
                                    },
                                    view_dimension: wgpu::TextureViewDimension::D2,
                                    multisampled: false,
                                },
                                count: None,
                            },
                            wgpu::BindGroupLayoutEntry {
                                binding: 2,
                                visibility: wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Uniform,
                                    has_dynamic_offset: false,
                                    min_binding_size: None,
                                },
                                count: None,
                            },
                        ],
                    });
                let layout = dispositivo.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Fragment Pipeline Layout"),
                    bind_group_layouts: &[&layout_do_grupo],
                    push_constant_ranges: &[],
                });
                Pipeline::Fragmento {
                    modulo,
                    layout_do_grupo,
                    layout,
                    por_formato: HashMap::new(),
                }
            }
        };

        Some(Self {
            dispositivo,
            fila,
            pipeline,
            cache: LruCache::new(NonZeroUsize::new(5).expect("5 não é zero")),
        })
    }

    /// Por onde este motor entra no shader.
    pub fn entrada(&self) -> Entrada {
        match self.pipeline {
            Pipeline::Compute(_) => Entrada::Compute,
            Pipeline::Fragmento { .. } => Entrada::Fragmento,
        }
    }

    /// Os limites do dispositivo aberto — o maior lado de textura, em especial.
    pub fn limites(&self) -> wgpu::Limits {
        self.dispositivo.limits()
    }

    /// O dispositivo, para quem precisa configurar uma superfície com ele.
    pub fn dispositivo(&self) -> &wgpu::Device {
        &self.dispositivo
    }

    /// Uma passada: sobe o que mudou, despacha, lê de volta.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn revelar(
        &mut self,
        pixels: &Arc<Vec<u8>>,
        largura: u32,
        altura: u32,
        ajustes: &Ajustes,
    ) -> Option<DynamicImage> {
        pollster::block_on(self.revelar_async(pixels, largura, altura, ajustes))
    }

    /// [`Motor::revelar`] sem bloquear: é o que o navegador consegue esperar.
    pub async fn revelar_async(
        &mut self,
        pixels: &Arc<Vec<u8>>,
        largura: u32,
        altura: u32,
        ajustes: &Ajustes,
    ) -> Option<DynamicImage> {
        let Motor {
            dispositivo,
            fila,
            pipeline,
            cache,
        } = self;
        let recursos = preparar(
            dispositivo,
            fila,
            pipeline,
            cache,
            pixels,
            largura,
            altura,
            ajustes,
        );

        let mut encoder = dispositivo.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Revelação Encoder"),
        });

        let vista_de_saida = recursos
            .textura_saida
            .create_view(&wgpu::TextureViewDescriptor::default());
        despachar(
            dispositivo,
            pipeline,
            &mut encoder,
            recursos,
            largura,
            altura,
            &vista_de_saida,
            FORMATO_DE_LEITURA,
        );

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &recursos.textura_saida,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &recursos.buffer_saida,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(recursos.bytes_por_linha_alinhado),
                    rows_per_image: Some(altura),
                },
            },
            wgpu::Extent3d {
                width: largura,
                height: altura,
                depth_or_array_layers: 1,
            },
        );

        fila.submit(std::iter::once(encoder.finish()));

        let fatia = recursos.buffer_saida.slice(..);
        let (avisa, espera) = futures_channel::oneshot::channel();
        fatia.map_async(wgpu::MapMode::Read, move |r| {
            let _ = avisa.send(r);
        });
        esperar_o_mapeamento(dispositivo, espera).await?;

        let dados = fatia.get_mapped_range();
        // A GPU devolve cada linha alinhada em 256 bytes; a imagem não tem esse
        // enchimento. Copiar o buffer inteiro daria uma foto com listras
        // deslocadas — e quanto mais estreita, mais torta.
        let mut saida = Vec::with_capacity((recursos.bytes_por_linha * altura) as usize);
        for y in 0..altura {
            let inicio = (y * recursos.bytes_por_linha_alinhado) as usize;
            saida.extend_from_slice(&dados[inicio..inicio + recursos.bytes_por_linha as usize]);
        }
        drop(dados);
        recursos.buffer_saida.unmap();

        Some(DynamicImage::ImageRgba8(image::RgbaImage::from_raw(
            largura, altura, saida,
        )?))
    }

    /// Revela **para um alvo de quem chama**, sem ler de volta — a superfície
    /// de um canvas, no navegador.
    ///
    /// O alvo tem de ter o tamanho da imagem: o triângulo cobre o alvo inteiro
    /// e cada fragmento lê o pixel de mesma coordenada. `None` se o motor for de
    /// compute (não há render pass para escrever no alvo) ou se o formato não
    /// puder ser alvo de cor.
    pub fn desenhar(
        &mut self,
        pixels: &Arc<Vec<u8>>,
        largura: u32,
        altura: u32,
        ajustes: &Ajustes,
        alvo: &wgpu::TextureView,
        formato: wgpu::TextureFormat,
    ) -> Option<()> {
        if !matches!(self.pipeline, Pipeline::Fragmento { .. }) {
            return None;
        }
        let Motor {
            dispositivo,
            fila,
            pipeline,
            cache,
        } = self;
        let recursos = preparar(
            dispositivo,
            fila,
            pipeline,
            cache,
            pixels,
            largura,
            altura,
            ajustes,
        );

        let mut encoder = dispositivo.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Revelação Encoder (superfície)"),
        });
        despachar(
            dispositivo,
            pipeline,
            &mut encoder,
            recursos,
            largura,
            altura,
            alvo,
            formato,
        );
        fila.submit(std::iter::once(encoder.finish()));
        Some(())
    }
}

/// Garante os recursos do tamanho, sobe os pixels se mudaram e grava os ajustes.
#[allow(clippy::too_many_arguments)]
fn preparar<'a>(
    dispositivo: &wgpu::Device,
    fila: &wgpu::Queue,
    pipeline: &Pipeline,
    cache: &'a mut LruCache<(u32, u32), Recursos>,
    pixels: &Arc<Vec<u8>>,
    largura: u32,
    altura: u32,
    ajustes: &Ajustes,
) -> &'a mut Recursos {
    if !cache.contains(&(largura, altura)) {
        cache.put(
            (largura, altura),
            criar_recursos(dispositivo, pipeline, largura, altura),
        );
    }
    let recursos = cache
        .get_mut(&(largura, altura))
        .expect("acabou de entrar no cache");

    let precisa_subir = match &recursos.ultimos_pixels {
        Some(ultimos) => !Arc::ptr_eq(ultimos, pixels),
        None => true,
    };

    if precisa_subir {
        fila.write_texture(
            wgpu::ImageCopyTexture {
                texture: &recursos.textura_entrada,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(recursos.bytes_por_linha),
                rows_per_image: Some(altura),
            },
            wgpu::Extent3d {
                width: largura,
                height: altura,
                depth_or_array_layers: 1,
            },
        );
        recursos.ultimos_pixels = Some(pixels.clone());
    }

    fila.write_buffer(&recursos.buffer_ajustes, 0, bytemuck::bytes_of(ajustes));
    recursos
}

/// Grava no encoder a passada do shader — compute ou render — sobre `alvo`.
///
/// No compute o `alvo` é ignorado: a saída é a storage texture do bind group.
#[allow(clippy::too_many_arguments)]
fn despachar(
    dispositivo: &wgpu::Device,
    pipeline: &mut Pipeline,
    encoder: &mut wgpu::CommandEncoder,
    recursos: &Recursos,
    largura: u32,
    altura: u32,
    alvo: &wgpu::TextureView,
    formato: wgpu::TextureFormat,
) {
    match pipeline {
        Pipeline::Compute(pipeline) => {
            let mut passe = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Image Processing Pass"),
                timestamp_writes: None,
            });
            passe.set_pipeline(pipeline);
            passe.set_bind_group(0, &recursos.grupo, &[]);
            // Grupos de 16×16, como o `@workgroup_size` do WGSL declara. A divisão
            // arredonda para cima: com 17 pixels de largura o último grupo trabalha
            // pela metade, e é o shader que descarta quem cair fora.
            passe.dispatch_workgroups(largura.div_ceil(16), altura.div_ceil(16), 1);
        }
        Pipeline::Fragmento {
            modulo,
            layout,
            por_formato,
            ..
        } => {
            let pipeline = por_formato
                .entry(formato)
                .or_insert_with(|| pipeline_de_fragmento(dispositivo, modulo, layout, formato));
            let mut passe = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Image Processing Pass (fragment)"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: alvo,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            passe.set_pipeline(pipeline);
            passe.set_bind_group(0, &recursos.grupo, &[]);
            // Um triângulo que cobre o alvo inteiro: três vértices, sem buffer.
            passe.draw(0..3, 0..1);
        }
    }
}

fn pipeline_de_fragmento(
    dispositivo: &wgpu::Device,
    modulo: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    formato: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    dispositivo.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Image Processing Pipeline (fragment)"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: modulo,
            entry_point: Some("vs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: modulo,
            entry_point: Some("fs"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: formato,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// O resultado do `map_async`, do jeito que cada lado consegue esperar.
type Mapeamento = Result<(), wgpu::BufferAsyncError>;

/// No desktop, bloqueia até a GPU terminar: o callback dispara dentro do `poll`.
#[cfg(not(target_arch = "wasm32"))]
async fn esperar_o_mapeamento(
    dispositivo: &wgpu::Device,
    espera: futures_channel::oneshot::Receiver<Mapeamento>,
) -> Option<()> {
    dispositivo.poll(wgpu::Maintain::Wait);
    espera.await.ok()?.ok()
}

/// No navegador, não há como bloquear — e o WebGL2 só atualiza o estado dos
/// fences **entre tarefas** do laço de eventos. Então: um `poll` sem esperar,
/// e se o callback ainda não veio, ceder a vez ao navegador e tentar de novo.
/// No WebGPU o callback vem pelo `Promise` do `mapAsync`, e o mesmo laço serve.
#[cfg(target_arch = "wasm32")]
async fn esperar_o_mapeamento(
    dispositivo: &wgpu::Device,
    mut espera: futures_channel::oneshot::Receiver<Mapeamento>,
) -> Option<()> {
    loop {
        dispositivo.poll(wgpu::Maintain::Poll);
        match espera.try_recv() {
            Ok(Some(resultado)) => return resultado.ok(),
            Ok(None) => gloo_timers::future::TimeoutFuture::new(0).await,
            Err(_) => return None,
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod testes {
    use super::*;

    /// Espera a thread abrir o dispositivo, e falha se não houver GPU.
    ///
    /// ⚠️ **Falha, e não pula.** O produto declara macOS e Windows, onde sempre
    /// há adaptador; uma máquina rodando esta suíte sem GPU precisa saber que
    /// não está conferindo o motor de revelação. Teste que se cala quando não
    /// pode rodar é teste que some do placar sem ninguém notar.
    fn motor_pronto() -> Motor {
        Motor::abrir().expect("nenhum adaptador de GPU — o motor de revelação não roda aqui")
    }

    fn cinza(lado: u32, valor: u8) -> Arc<Vec<u8>> {
        Arc::new(
            std::iter::repeat_n([valor, valor, valor, 255], (lado * lado) as usize)
                .flatten()
                .collect(),
        )
    }

    fn revelar_e_colher(motor: &mut Motor, entrada: Arc<Vec<u8>>, ajustes: Ajustes) -> Vec<u8> {
        motor
            .revelar(&entrada, 16, 16, &ajustes)
            .expect("o motor não devolveu imagem")
            .into_rgba8()
            .into_raw()
    }

    /// 🚨 O neutro tem de sair **igual** ao que entrou.
    ///
    /// É o teste que vale mais nesta fase, e não é sobre a GPU: é sobre os 46
    /// valores de `Ajustes::default`. Um só deles fora do neutro faz toda foto
    /// abrir alterada — sem erro, sem aviso, e parecendo decisão de cor de quem
    /// escreveu o shader.
    #[test]
    fn o_neutro_devolve_o_pixel_intacto() {
        let mut motor = motor_pronto();
        let entrada = cinza(16, 100);
        let saida = revelar_e_colher(&mut motor, entrada.clone(), Ajustes::default());

        assert_eq!(
            saida.as_slice(),
            entrada.as_slice(),
            "algum campo de `Ajustes::default` não é neutro"
        );
    }

    /// A exposição atravessou: `+1` dobra o valor, como `pow(2, exposure)` manda.
    ///
    /// Prova que o caminho inteiro está de pé — textura sobe, `uniform` chega no
    /// campo certo, compute despacha, buffer volta sem o enchimento de 256 bytes
    /// por linha. Se o `Ajustes` estivesse deslocado de um campo, aqui apareceria
    /// como exposição que não faz nada.
    #[test]
    fn exposicao_de_um_ponto_dobra_o_valor() {
        let mut motor = motor_pronto();
        let saida = revelar_e_colher(
            &mut motor,
            cinza(16, 100),
            Ajustes {
                exposure: 1.0,
                ..Default::default()
            },
        );

        assert_eq!(&saida[0..4], &[200, 200, 200, 255]);
        // O último pixel também: linha final é onde o desalinhamento de 256
        // bytes apareceria primeiro.
        assert_eq!(&saida[saida.len() - 4..], &[200, 200, 200, 255]);
    }

    /// Uma amostra 16×16 desenhada para que **todo** ajuste do shader tenha onde
    /// agir: rampa de cinza de 0 a 255 (altas luzes, sombras, brancos, pretos e
    /// as quatro zonas da curva), as 8 cores do HSL saturadas, e as mesmas 8
    /// esmaecidas (`vibrance` só age onde `max_diff < 64`). Tudo em xadrez de
    /// 1px, para haver borda dura em toda parte — sem borda, nitidez e redução de
    /// ruído não teriam o que fazer.
    fn amostra() -> Arc<Vec<u8>> {
        const CORES: [[u8; 3]; 8] = [
            [220, 40, 40],
            [230, 140, 30],
            [230, 220, 40],
            [40, 200, 60],
            [40, 210, 200],
            [50, 80, 220],
            [140, 50, 210],
            [220, 50, 180],
        ];
        let mut pixels = Vec::with_capacity(16 * 16 * 4);
        for y in 0u32..16 {
            for x in 0u32..16 {
                let cor = match y {
                    // Rampa de cinza, com um degrau por coluna.
                    0..=3 => [(x * 17) as u8; 3],
                    // As 8 cores, uma por linha, em xadrez com cinza médio.
                    4..=11 if (x + y) % 2 == 0 => CORES[(y - 4) as usize],
                    4..=11 => [128, 128, 128],
                    // As mesmas, puxadas para perto do cinza.
                    _ if (x + y) % 2 == 0 => {
                        let base = CORES[(y - 12) as usize];
                        [
                            (128 + (base[0] as i32 - 128) / 4) as u8,
                            (128 + (base[1] as i32 - 128) / 4) as u8,
                            (128 + (base[2] as i32 - 128) / 4) as u8,
                        ]
                    }
                    _ => [128, 128, 128],
                };
                pixels.extend_from_slice(&[cor[0], cor[1], cor[2], 255]);
            }
        }
        Arc::new(pixels)
    }

    /// Escreve **por posição**, e não por nome: é assim que o `uniform` chega à
    /// GPU, e é a única forma de perguntar "o que o shader faz com o campo *n*"
    /// sem depender de qual nome o Rust deu a ele.
    fn com_campos(alterados: &[(usize, f32)]) -> Ajustes {
        let mut campos = Ajustes::default().como_vetor();
        for (indice, valor) in alterados {
            campos[*indice] = *valor;
        }
        Ajustes::de_vetor(&campos).expect("46 campos")
    }

    fn com_campo(indice: usize, valor: f32) -> Ajustes {
        com_campos(&[(indice, valor)])
    }

    /// Soma das diferenças entre vizinhos horizontais.
    ///
    /// Cai quando a imagem borra, sobe quando ela é afiada — é o que separa
    /// "mudou alguma coisa" de "virou exatamente o ajuste do vizinho".
    fn contraste_local(pixels: &[u8]) -> u64 {
        let mut soma = 0u64;
        for y in 0..16usize {
            for x in 1..16usize {
                for canal in 0..3usize {
                    let atual = pixels[(y * 16 + x) * 4 + canal] as i32;
                    let anterior = pixels[(y * 16 + x - 1) * 4 + canal] as i32;
                    soma += atual.abs_diff(anterior) as u64;
                }
            }
        }
        soma
    }

    /// Os 23 primeiros ajustes e os 4 de Detalhe mudam a foto.
    ///
    /// ✅ **O Detalhe é o que o alinhamento de 17/ago devolveu**: `nr_luminance`,
    /// `nr_color` e `sharpen_amount` sempre tiveram código no corpo do shader —
    /// o que faltava era chegarem lá. Eram quatro sliders que arrastavam,
    /// mostravam número e não moviam um pixel.
    ///
    /// ⚠️ **`sharpen_radius` só conta com `sharpen_amount` junto**: raio sozinho
    /// nunca mudaria nada (`do_sharpen` é `amount > 0.0`), e o teste passaria por
    /// engano ao afirmar o contrário.
    #[test]
    fn o_basico_e_o_detalhe_chegam_ao_shader() {
        let mut motor = motor_pronto();
        let entrada = amostra();
        let neutro = revelar_e_colher(&mut motor, entrada.clone(), Ajustes::default());

        for (i, nome) in Ajustes::NOMES.iter().enumerate().take(23) {
            let saida = revelar_e_colher(&mut motor, entrada.clone(), com_campo(i, 60.0));
            assert_ne!(
                saida, neutro,
                "`{nome}` (campo {i}) devia chegar ao shader e não mudou nada"
            );
        }

        let detalhe: [(&str, &[(usize, f32)]); 3] = [
            ("Ruído (luminância)", &[(42, 60.0)]),
            ("Ruído (cor)", &[(43, 60.0)]),
            ("Nitidez (com raio)", &[(44, 80.0), (45, 2.0)]),
        ];
        for (rotulo, campos) in detalhe {
            let saida = revelar_e_colher(&mut motor, entrada.clone(), com_campos(campos));
            assert_ne!(saida, neutro, "Detalhe — `{rotulo}` não fez efeito nenhum");
        }
    }

    /// 🚨 **O neutro devolve a foto intacta — numa imagem com detalhe.**
    ///
    /// Existe porque `o_neutro_devolve_o_pixel_intacto` **não consegue** ver o
    /// defeito que este vê: ele usa cinza chapado, e interpolar dois pixels
    /// iguais devolve o mesmo valor. Um erro de meio pixel na reamostragem passa
    /// por ele sem tocar em nada.
    ///
    /// Descoberto quebrando de propósito, ao ligar a distorção de lente: um
    /// deslocamento na `amostrar` deixou os 13 testes passando. A `amostra` tem
    /// xadrez de 1px, onde meio pixel de erro vira borrão imediato.
    #[test]
    fn o_neutro_nao_reamostra_uma_imagem_com_detalhe() {
        let mut motor = motor_pronto();
        let entrada = amostra();
        let saida = revelar_e_colher(&mut motor, entrada.clone(), Ajustes::default());

        assert_eq!(
            saida.as_slice(),
            entrada.as_slice(),
            "o neutro moveu pixel — a leitura bilinear deixou de ser exata no inteiro"
        );
    }

    /// ✅ **As quatro zonas da curva de tons movem a foto, cada uma na sua.**
    ///
    /// A prova de que cada uma respeita a própria faixa é feita sobre cinzas de
    /// níveis diferentes: a zona das sombras (centro 0,125) tem de mexer num
    /// cinza escuro e deixar um claro em paz.
    #[test]
    fn cada_zona_da_curva_de_tons_mexe_na_sua_faixa() {
        let mut motor = motor_pronto();

        let escuro = cinza(16, 32); // ~0,125
        let claro = cinza(16, 223); // ~0,875

        let com_sombras = Ajustes {
            tone_curve_shadows: 60.0,
            ..Default::default()
        };
        let com_altas = Ajustes {
            tone_curve_highlights: 60.0,
            ..Default::default()
        };

        let escuro_neutro = revelar_e_colher(&mut motor, escuro.clone(), Ajustes::default());
        let claro_neutro = revelar_e_colher(&mut motor, claro.clone(), Ajustes::default());

        let escuro_com_sombras = revelar_e_colher(&mut motor, escuro.clone(), com_sombras);
        assert_ne!(
            escuro_com_sombras, escuro_neutro,
            "a zona das sombras não alcançou um cinza de nível 32"
        );

        let claro_com_sombras = revelar_e_colher(&mut motor, claro.clone(), com_sombras);
        assert_eq!(
            claro_com_sombras, claro_neutro,
            "a zona das sombras alcançou um cinza de nível 223 — a faixa dela vazou"
        );

        let claro_com_altas = revelar_e_colher(&mut motor, claro, com_altas);
        assert_ne!(
            claro_com_altas, claro_neutro,
            "a zona das altas luzes não alcançou um cinza de nível 223"
        );

        let escuro_com_altas = revelar_e_colher(&mut motor, escuro, com_altas);
        assert_eq!(
            escuro_com_altas, escuro_neutro,
            "a zona das altas luzes alcançou um cinza de nível 32"
        );
    }

    /// ✅ **Os 3 controles de Lente movem a foto.**
    ///
    /// ⚠️ **A vinheta é testada com o meio junto**, e a distorção sozinha: o meio
    /// da vinheta não faz nada sem a intensidade, e um teste que o afirmasse
    /// passaria por engano.
    #[test]
    fn a_lente_move_a_foto() {
        let mut motor = motor_pronto();
        let entrada = amostra();
        let neutro = revelar_e_colher(&mut motor, entrada.clone(), Ajustes::default());

        let distorcida = revelar_e_colher(&mut motor, entrada.clone(), com_campo(39, 60.0));
        assert_ne!(distorcida, neutro, "Lente — a distorção não moveu nada");

        let vinheta = revelar_e_colher(&mut motor, entrada, com_campos(&[(40, -80.0), (41, 30.0)]));
        assert_ne!(vinheta, neutro, "Lente — a vinheta não moveu nada");
    }

    /// 🚨 **A vinheta escurece o canto e deixa o centro em paz.**
    ///
    /// É o que separa "vinheta" de "exposição": um fator aplicado à foto inteira
    /// também mudaria a saída, e `assert_ne!` sozinho não veria a diferença. Aqui
    /// a medida é a razão entre o canto e o centro.
    #[test]
    fn a_vinheta_escurece_o_canto_e_nao_o_centro() {
        let mut motor = motor_pronto();
        let cinza_puro = cinza(16, 200);

        let saida = revelar_e_colher(
            &mut motor,
            cinza_puro,
            Ajustes {
                lens_vignette_amount: -80.0,
                lens_vignette_midpoint: 0.0,
                ..Default::default()
            },
        );

        let em = |x: usize, y: usize| saida[(y * 16 + x) * 4] as i32;
        let centro = em(8, 8);
        let canto = em(0, 0);

        assert!(
            canto < centro - 20,
            "o canto ({canto}) tinha de estar bem mais escuro que o centro ({centro})"
        );
        assert!(
            centro >= 190,
            "o centro ({centro}) mal pode ser tocado — senão isto é exposição, não vinheta"
        );
    }

    /// ⚠️ **A vinheta positiva clareia**, e a negativa escurece — é a convenção
    /// do Lightroom, onde "Vignetting: Amount" negativo é o efeito clássico.
    #[test]
    fn o_sinal_da_vinheta_decide_a_direcao() {
        let mut motor = motor_pronto();
        let cinza_puro = cinza(16, 128);
        let canto = |saida: &[u8]| saida[0] as i32;

        let escura = revelar_e_colher(
            &mut motor,
            cinza_puro.clone(),
            Ajustes {
                lens_vignette_amount: -80.0,
                ..Default::default()
            },
        );
        let clara = revelar_e_colher(
            &mut motor,
            cinza_puro,
            Ajustes {
                lens_vignette_amount: 80.0,
                ..Default::default()
            },
        );

        assert!(canto(&escura) < 128, "vinheta negativa tem de escurecer");
        assert!(canto(&clara) > 128, "vinheta positiva tem de clarear");
    }

    /// ✅ **Os 16 sliders de matiz e luminância do HSL movem a foto.**
    ///
    /// A amostra tem as oito cores do HSL, então cada canal tem onde agir.
    #[test]
    fn o_matiz_e_a_luminancia_do_hsl_movem_a_foto() {
        let mut motor = motor_pronto();
        let entrada = amostra();
        let neutro = revelar_e_colher(&mut motor, entrada.clone(), Ajustes::default());

        // 23–30 é matiz, 31–38 é luminância — um canal por posição.
        for (i, nome) in Ajustes::NOMES.iter().enumerate().take(39).skip(23) {
            let saida = revelar_e_colher(&mut motor, entrada.clone(), com_campo(i, 60.0));
            assert_ne!(
                saida, neutro,
                "`{nome}` (campo {i}) chega ao shader e não moveu um pixel"
            );
        }
    }

    /// 🚨 **A luminância do HSL não pode clarear cinza.**
    ///
    /// Um pixel cinza não tem matiz: `delta == 0` dá `hue == 0`, que cai na
    /// faixa do **vermelho** com peso 1.0. Sem o portão da saturação, arrastar
    /// "HSL / luminância — Vermelho" clarearia **toda** área neutra da foto — e
    /// o slider viraria, calado, um controle global de brilho.
    #[test]
    fn a_luminancia_do_vermelho_nao_mexe_no_cinza() {
        let mut motor = motor_pronto();
        let cinza_puro = cinza(16, 128);

        let neutro = revelar_e_colher(&mut motor, cinza_puro.clone(), Ajustes::default());
        let com_lum = revelar_e_colher(
            &mut motor,
            cinza_puro,
            Ajustes {
                hsl_red_lum: 100.0,
                ..Default::default()
            },
        );

        assert_eq!(
            com_lum, neutro,
            "o cinza não tem matiz — a luminância do vermelho não pode tocá-lo"
        );
    }

    /// 🔑 **O matiz gira a cor; ele não mexe no contraste entre vizinhos.**
    ///
    /// É a mesma medida que acusou o defeito antigo, agora do lado certo: quando
    /// o campo 23 era lido como `nr_luminance`, o contraste local **caía** —
    /// assinatura de borrão. Girando de verdade, ele fica onde estava.
    #[test]
    fn o_matiz_gira_a_cor_sem_borrar() {
        let mut motor = motor_pronto();
        let entrada = amostra();

        let neutro = revelar_e_colher(&mut motor, entrada.clone(), Ajustes::default());
        let girado = revelar_e_colher(
            &mut motor,
            entrada,
            Ajustes {
                hsl_red_hue: 120.0,
                ..Default::default()
            },
        );

        assert_ne!(girado, neutro, "o matiz do vermelho não moveu nada");
        let (antes, depois) = (contraste_local(&neutro), contraste_local(&girado));
        assert!(
            depois > antes / 2,
            "o contraste local caiu de {antes} para {depois} — isso é borrão, não giro de matiz"
        );
    }

    /// 🚨 **A entrada por fragmento revela o mesmo pixel que a por compute.**
    ///
    /// É o teste que autoriza o navegador: o desktop revela por `@compute`, o
    /// site por `@fragment`, e o corpo é um só por construção (`concat!`). O que
    /// este teste prende é o que a construção não prende — que a coordenada do
    /// fragmento (`floor(uv * dims)`) é o mesmo pixel que o `global_id` do
    /// compute, inclusive nas bordas e no último pixel, e que o render pass não
    /// inverte, desloca nem mistura nada.
    ///
    /// Com **todos** os grupos fora do neutro, porque distorção e vinheta são os
    /// únicos que dependem de **onde** o pixel está — e um erro de coordenada só
    /// aparece neles.
    ///
    /// A folga de 1 nível é para a rasterização: a interpolação do `uv` e o
    /// arredondamento do alvo podem diferir do compute no último bit.
    #[test]
    fn o_fragmento_revela_o_mesmo_pixel_que_o_compute() {
        let mut compute = motor_pronto();
        let mut fragmento = Motor::abrir_por(Entrada::Fragmento)
            .expect("nenhum adaptador de GPU para a entrada por fragmento");
        assert_eq!(fragmento.entrada(), Entrada::Fragmento);

        let entrada = amostra();
        let ajustes = Ajustes {
            exposure: 0.4,
            contrast: 1.2,
            temperature: 15.0,
            highlights: -30.0,
            shadows: 25.0,
            clarity: 20.0,
            vibrance: 30.0,
            tone_curve_darks: 15.0,
            hsl_red_hue: 40.0,
            hsl_blue_sat: -30.0,
            hsl_green_lum: 20.0,
            lens_distortion: 35.0,
            lens_vignette_amount: -60.0,
            lens_vignette_midpoint: 20.0,
            nr_luminance: 30.0,
            nr_color: 30.0,
            sharpen_amount: 50.0,
            sharpen_radius: 1.5,
            ..Default::default()
        };

        let pelo_compute = revelar_e_colher(&mut compute, entrada.clone(), ajustes);
        let pelo_fragmento = revelar_e_colher(&mut fragmento, entrada.clone(), ajustes);
        let neutro = revelar_e_colher(&mut compute, entrada, Ajustes::default());
        assert_ne!(
            pelo_compute, neutro,
            "os ajustes escolhidos têm de mover a foto"
        );

        let maior_diferenca = pelo_compute
            .iter()
            .zip(&pelo_fragmento)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(
            maior_diferenca <= 1,
            "compute e fragmento divergem em até {maior_diferenca} níveis — \
             a coordenada do fragmento não é o pixel do compute"
        );
    }

    /// O motor de compute não desenha em alvo alheio, e diz isso com `None`.
    #[test]
    fn o_compute_nao_desenha_em_alvo_de_quem_chama() {
        let mut motor = motor_pronto();
        let textura = motor.dispositivo.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMATO_DE_LEITURA,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let vista = textura.create_view(&Default::default());
        assert_eq!(
            motor.desenhar(
                &cinza(16, 1),
                16,
                16,
                &Ajustes::default(),
                &vista,
                FORMATO_DE_LEITURA
            ),
            None
        );
    }
}
