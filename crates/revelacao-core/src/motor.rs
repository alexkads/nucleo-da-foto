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
    /// Qual API gráfica respondeu — Metal, Vulkan, WebGPU, WebGL2.
    ///
    /// 🔑 **É o selo que o editor do site mostra ao lado do nome do arquivo.**
    /// Lá ele separa WebGPU de WebGL2, que rendem diferente; aqui ele responde
    /// "a GPU está mesmo sendo usada, e por qual caminho" — a pergunta que
    /// aparece toda vez que alguém acha o arrasto lento.
    backend: &'static str,
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
        .ok()
    }

    /// Abre o dispositivo num adaptador que quem chama já escolheu.
    ///
    /// É a porta do navegador: lá o adaptador tem de ser compatível com a
    /// superfície do canvas, e os limites dependem do backend que respondeu
    /// (`Limits::downlevel_webgl2_defaults()` no WebGL2, com a resolução do
    /// adaptador por cima — o padrão sozinho declara 2048 px de textura).
    ///
    /// Devolve o erro do wgpu, e não `None`: no navegador a mensagem é a
    /// única pista de por que um adaptador que respondeu não abriu.
    pub async fn abrir_com(
        adaptador: &wgpu::Adapter,
        entrada: Entrada,
        limites: wgpu::Limits,
    ) -> Result<Self, wgpu::RequestDeviceError> {
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
            .await?;

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

        Ok(Self {
            dispositivo,
            fila,
            pipeline,
            cache: LruCache::new(NonZeroUsize::new(5).expect("5 não é zero")),
            backend: match adaptador.get_info().backend {
                wgpu::Backend::Metal => "Metal",
                wgpu::Backend::Vulkan => "Vulkan",
                wgpu::Backend::Dx12 => "DirectX 12",
                wgpu::Backend::Gl => "OpenGL",
                wgpu::Backend::BrowserWebGpu => "WebGPU",
                wgpu::Backend::Empty => "sem GPU",
            },
        })
    }

    /// Qual API gráfica respondeu.
    pub fn backend(&self) -> &'static str {
        self.backend
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

    /// A fila, para quem submete os **próprios** comandos com este dispositivo.
    ///
    /// 🔑 É o que permite compor o que este motor revelou sem abrir um segundo
    /// dispositivo: a tela do cliente (`tela-do-cliente-web`) revela cada foto numa
    /// textura com [`Self::desenhar`] e compõe as duas na superfície com um
    /// pipeline dela. Dois dispositivos não compartilham textura nenhuma.
    pub fn fila(&self) -> &wgpu::Queue {
        &self.fila
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
            ..
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
            ..
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

    /// Os 256 níveis de cinza, um por pixel, em ordem: o pixel `i` vale `i`.
    ///
    /// 🔑 É a amostra que enxerga o que a `amostra()` não enxerga: com um nível
    /// por pixel e nada de vizinhança ligada, a saída de cada pixel é a curva de
    /// tom inteira, ponto a ponto. Degrau e inversão aparecem como diferença
    /// entre pixels consecutivos.
    fn rampa() -> Arc<Vec<u8>> {
        Arc::new(
            (0u32..256)
                .flat_map(|i| [i as u8, i as u8, i as u8, 255])
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
        Ajustes::de_vetor(&campos).expect("a quantidade certa de campos")
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

    /// Os 21 controles de 2026-09-12 chegam ao shader **e fazem efeito**.
    ///
    /// # Por que este teste, e por que assim
    ///
    /// 🚨 **"Chegar" e "ser aplicado" já foram duas coisas diferentes aqui.** Em
    /// 17/ago/2026 os 46 campos chegavam ao `uniform` e o corpo do shader não
    /// mencionava matiz, luminância nem lente em lugar nenhum — os controles
    /// existiam na tela, o operador os movia, e a foto não mudava. O teste de
    /// paridade de nomes (`o_wgsl_declara_os_mesmos_campos_na_mesma_ordem`)
    /// passa nessa situação: ele confere o contrato, não o efeito.
    ///
    /// Estes 21 entraram porque o operador do estúdio disse que não conseguia
    /// reproduzir os estilos que tem no Lightroom e no darktable. Cada um só
    /// vale se mudar o pixel — e é isso que se cobra aqui, um por um.
    ///
    /// ⚠️ **A amostra é colorida de propósito.** Calibração, mixer P&B e
    /// tonalização por faixa são todos função do **matiz**: num cinza chapado
    /// os três não teriam o que fazer, e o teste passaria verde sobre um shader
    /// vazio.
    #[test]
    fn a_calibracao_o_mixer_pb_e_o_color_grading_chegam_ao_shader() {
        let mut motor = motor_pronto();
        let entrada = amostra();
        let neutro = revelar_e_colher(&mut motor, entrada.clone(), Ajustes::default());

        let posicao = |nome: &str| {
            Ajustes::NOMES
                .iter()
                .position(|n| *n == nome)
                .unwrap_or_else(|| panic!("`{nome}` não está em NOMES"))
        };

        // 🔑 Cada caso leva o **par** que o controle precisa para agir. Matiz
        // sem saturação não pinta nada (a tonalização multiplica um pelo
        // outro), e o mixer não existe com a foto colorida — exatamente como no
        // Lightroom, onde o mixer só aparece depois do B&W.
        let casos: &[(&str, &[(&str, f32)])] = &[
            ("Calibração — matiz do vermelho", &[("calib_red_hue", 60.0)]),
            ("Calibração — saturação do vermelho", &[("calib_red_sat", 80.0)]),
            ("Calibração — matiz do verde", &[("calib_green_hue", 60.0)]),
            ("Calibração — saturação do verde", &[("calib_green_sat", 80.0)]),
            ("Calibração — matiz do azul", &[("calib_blue_hue", 60.0)]),
            ("Calibração — saturação do azul", &[("calib_blue_sat", 80.0)]),
            ("Calibração — matiz das sombras", &[("calib_shadow_tint", 80.0)]),
            (
                "Color Grading — tons médios",
                &[("split_midtone_hue", 40.0), ("split_midtone_sat", 80.0)],
            ),
            (
                "Color Grading — global",
                &[("split_global_hue", 200.0), ("split_global_sat", 80.0)],
            ),
            (
                "Color Grading — a mistura muda a largura das faixas",
                &[
                    ("split_shadow_hue", 30.0),
                    ("split_shadow_sat", 80.0),
                    ("split_highlight_hue", 210.0),
                    ("split_highlight_sat", 80.0),
                    ("split_blending", 0.0),
                ],
            ),
            ("Mixer P&B — vermelho", &[("bw_ativo", 1.0), ("bw_red", 80.0)]),
            ("Mixer P&B — laranja", &[("bw_ativo", 1.0), ("bw_orange", 80.0)]),
            ("Mixer P&B — amarelo", &[("bw_ativo", 1.0), ("bw_yellow", 80.0)]),
            ("Mixer P&B — verde", &[("bw_ativo", 1.0), ("bw_green", 80.0)]),
            ("Mixer P&B — água", &[("bw_ativo", 1.0), ("bw_aqua", 80.0)]),
            ("Mixer P&B — azul", &[("bw_ativo", 1.0), ("bw_blue", 80.0)]),
            ("Mixer P&B — roxo", &[("bw_ativo", 1.0), ("bw_purple", 80.0)]),
            ("Mixer P&B — magenta", &[("bw_ativo", 1.0), ("bw_magenta", 80.0)]),
        ];

        for (rotulo, campos) in casos {
            let indices: Vec<(usize, f32)> =
                campos.iter().map(|(n, v)| (posicao(n), *v)).collect();
            let saida = revelar_e_colher(&mut motor, entrada.clone(), com_campos(&indices));
            assert_ne!(
                saida, neutro,
                "`{rotulo}` devia chegar ao shader e não mudou nada"
            );
        }

        // 🚨 **O mixer desligado não pode fazer nada**, como no Lightroom: os
        // oito sliders existem, o preset os traz, e sem o B&W ligado eles
        // dormem. Sem esta linha, um preset de cor com `GrayMixer` dentro
        // dessaturaria a foto sem ninguém ter pedido.
        let so_os_sliders = com_campos(&[
            (posicao("bw_red"), 100.0),
            (posicao("bw_blue"), -100.0),
        ]);
        assert_eq!(
            revelar_e_colher(&mut motor, entrada.clone(), so_os_sliders),
            neutro,
            "o mixer P&B agiu com `bw_ativo` em zero"
        );

        // 🚨 **E a mistura no neutro (50) tem de devolver a foto de antes.** O
        // valor fixo que estava no shader era 0,35 de meia-largura, e é nele
        // que `0,10 + 0,50 · 0,5` cai: se esta conta mudar, toda revelação já
        // gravada com tonalização sai diferente da que o operador salvou.
        let tonalizada = [
            (posicao("split_shadow_hue"), 30.0),
            (posicao("split_shadow_sat"), 80.0),
        ];
        let com_neutro_explicito = com_campos(&[
            tonalizada[0],
            tonalizada[1],
            (posicao("split_blending"), 50.0),
        ]);
        assert_eq!(
            revelar_e_colher(&mut motor, entrada.clone(), com_campos(&tonalizada)),
            revelar_e_colher(&mut motor, entrada.clone(), com_neutro_explicito),
            "a mistura neutra devia ser a largura de antes"
        );
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

    /// 🚨 **Altas luzes, sombras, brancos e pretos não invertem nem dão degrau.**
    ///
    /// É o teste do defeito de 06/09: os quatro eram `if` sobre a luminância com
    /// um fator só por região, e o dono viu o estrago numa foto de estúdio — o
    /// branco das janelas manchado, contorno duro em volta delas. Duas medidas,
    /// sobre a rampa de 256 níveis:
    ///
    ///   - **Inversão**: um nível de entrada maior nunca pode sair menor. Era o
    ///     que fazia \"brancos\" em -100 zerar o pixel de 255 e deixar o de 190
    ///     intacto.
    ///   - **Degrau**: entre dois níveis vizinhos de entrada a saída não pode
    ///     pular. Era o que punha contorno onde a luminância cruzava 128 ou 192.
    ///
    /// A folga de 4 níveis vem da matemática: a derivada de cada curva fica em
    /// 0..2, então um passo de 1 nível na entrada anda no máximo 2 na saída, e
    /// sobra 1 para o arredondamento de cada lado.
    #[test]
    fn o_tom_por_regiao_nunca_inverte_nem_da_degrau() {
        let mut motor = motor_pronto();

        // 4 a 7 é `highlights`, `shadows`, `whites`, `blacks`.
        let mut casos: Vec<(String, Ajustes)> = Vec::new();
        for (i, nome) in Ajustes::NOMES.iter().enumerate().take(8).skip(4) {
            for valor in [-100.0, -60.0, 60.0, 100.0] {
                casos.push((format!("{nome} em {valor}"), com_campo(i, valor)));
            }
        }
        // 🔑 Os quatro juntos, e nos sinais que se opõem: é a combinação em que
        // somar os deltas em vez de compor as curvas volta a inverter.
        casos.push((
            "os quatro opostos".into(),
            com_campos(&[(4, -100.0), (5, -100.0), (6, 100.0), (7, 100.0)]),
        ));
        casos.push((
            "os quatro opostos, ao contrário".into(),
            com_campos(&[(4, 100.0), (5, 100.0), (6, -100.0), (7, -100.0)]),
        ));

        for (rotulo, ajustes) in casos {
            let saida = revelar_e_colher(&mut motor, rampa(), ajustes);
            let nivel = |entrada: usize| saida[entrada * 4] as i32;

            for entrada in 1..256usize {
                let (antes, agora) = (nivel(entrada - 1), nivel(entrada));
                assert!(
                    agora >= antes,
                    "{rotulo}: a entrada {entrada} saiu em {agora}, ABAIXO do nível \
                     {antes} que a entrada {} devolveu — isso é a inversão que mancha",
                    entrada - 1
                );
                assert!(
                    agora - antes <= 4,
                    "{rotulo}: de {} para {entrada} a saída pulou de {antes} para {agora} — \
                     esse degrau vira contorno duro na foto",
                    entrada - 1
                );
            }
        }
    }

    /// ✅ **Cada um dos quatro age na sua ponta da escala.**
    ///
    /// Sem isto, a correção do degrau passaria com os quatro virando controle
    /// global de brilho — contínuo, monotônico e errado. A medida é a ponta
    /// oposta: \"sombras\" tem de levantar o cinza 32 e quase não tocar o 240.
    #[test]
    fn cada_ajuste_de_tom_age_na_sua_ponta() {
        let mut motor = motor_pronto();
        let nivel = |saida: &[u8], entrada: usize| saida[entrada * 4] as i32;

        let sombras = revelar_e_colher(&mut motor, rampa(), com_campo(5, 100.0));
        assert!(
            nivel(&sombras, 32) > 32 + 15,
            "sombras +100 mal levantou o nível 32: saiu {}",
            nivel(&sombras, 32)
        );
        assert!(
            (nivel(&sombras, 240) - 240).abs() <= 5,
            "sombras +100 mexeu no nível 240 ({}) — isso é brilho, não sombra",
            nivel(&sombras, 240)
        );

        let brancos = revelar_e_colher(&mut motor, rampa(), com_campo(6, -100.0));
        assert!(
            nivel(&brancos, 240) < 240 - 15,
            "brancos -100 mal baixou o nível 240: saiu {}",
            nivel(&brancos, 240)
        );
        assert!(
            (nivel(&brancos, 32) - 32).abs() <= 5,
            "brancos -100 mexeu no nível 32 ({}) — a faixa dele é o topo",
            nivel(&brancos, 32)
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
            split_shadow_hue: 35.0,
            split_shadow_sat: 60.0,
            split_highlight_hue: 210.0,
            split_highlight_sat: 40.0,
            split_balance: -20.0,
            grain_amount: 70.0,
            grain_size: 40.0,
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

    /// 🔑 **Sépia é tonalização sobre foto sem cor — e até 2026-09-06 não dava.**
    ///
    /// Temperatura e matiz agem no passo 3 do shader, antes da saturação: numa
    /// foto com `saturation = -1.0` eles pintam uma cor que o passo 9 apaga em
    /// seguida. Este teste é a prova de que o caminho novo existe: o cinza sai
    /// âmbar (`r > g > b`) **e com o mesmo brilho**, que é o que separa uma sépia
    /// de uma foto amarelada.
    #[test]
    fn a_tonalizacao_pinta_de_sepia_uma_foto_sem_cor() {
        let mut motor = motor_pronto();
        let saida = revelar_e_colher(
            &mut motor,
            cinza(16, 128),
            Ajustes {
                saturation: -1.0,
                split_shadow_hue: 35.0,
                split_shadow_sat: 100.0,
                split_highlight_hue: 35.0,
                split_highlight_sat: 100.0,
                ..Default::default()
            },
        );

        let (r, g, b) = (saida[0] as i32, saida[1] as i32, saida[2] as i32);
        assert!(
            r > g && g > b,
            "35° é âmbar: esperava vermelho > verde > azul, saiu ({r}, {g}, {b})"
        );

        let brilho = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round() as i32;
        assert!(
            (brilho - 128).abs() <= 2,
            "tonalizar não pode mudar o brilho: 128 virou {brilho}"
        );
    }

    /// As duas pontas da escala recebem cores diferentes — e cada pixel a sua.
    ///
    /// A rampa dá um nível de cinza por pixel, então "sombra" e "altas luzes"
    /// aqui são dois pixels concretos: o 20 e o 240.
    #[test]
    fn a_tonalizacao_separa_as_sombras_das_altas_luzes() {
        let mut motor = motor_pronto();
        let saida = revelar_e_colher(
            &mut motor,
            rampa(),
            Ajustes {
                split_shadow_hue: 30.0,
                split_shadow_sat: 100.0,
                split_highlight_hue: 210.0,
                split_highlight_sat: 100.0,
                ..Default::default()
            },
        );

        let quente = |i: usize| saida[i * 4] as i32 - saida[i * 4 + 2] as i32;
        assert!(
            quente(20) > 5,
            "a sombra tinha de puxar para o âmbar, e saiu {}",
            quente(20)
        );
        assert!(
            quente(240) < -5,
            "a alta luz tinha de puxar para o azul, e saiu {}",
            quente(240)
        );
    }

    /// ⚠️ **O balanço move a fronteira, e move para os dois lados.**
    ///
    /// Sem ele a tonalização é uma escolha só; com ele o meio-tom cai para uma
    /// ponta ou para a outra — que é o que decide se a foto lê como "sombra
    /// quente" ou "foto inteira quente".
    #[test]
    fn o_balanco_desloca_a_fronteira_da_tonalizacao() {
        let mut motor = motor_pronto();
        let tonalizada = |motor: &mut Motor, balanco: f32| {
            let saida = revelar_e_colher(
                motor,
                rampa(),
                Ajustes {
                    split_shadow_hue: 30.0,
                    split_shadow_sat: 100.0,
                    split_highlight_hue: 210.0,
                    split_highlight_sat: 100.0,
                    split_balance: balanco,
                    ..Default::default()
                },
            );
            // O meio-tom exato: o pixel 128 da rampa.
            saida[128 * 4] as i32 - saida[128 * 4 + 2] as i32
        };

        let para_a_sombra = tonalizada(&mut motor, -100.0);
        let neutro = tonalizada(&mut motor, 0.0);
        let para_a_luz = tonalizada(&mut motor, 100.0);

        assert!(
            para_a_luz < neutro && neutro < para_a_sombra,
            "o meio-tom tinha de esfriar com o balanço nas altas luzes e esquentar              com ele nas sombras — saiu {para_a_luz}, {neutro}, {para_a_sombra}"
        );
    }

    /// 🚨 **A tonalização não pode manchar o que veio fora da faixa.**
    ///
    /// Contraste, nitidez e as curvas de tom entregam valores abaixo de 0 e
    /// acima de 255 — sempre entregaram, e até 2026-09-06 isso não importava,
    /// porque o `clamp` do fim do shader recolhia tudo. `tonalizar` divide pela
    /// luminância da mistura, e essa divisão inverte de sinal quando a
    /// luminância de entrada é negativa: o pixel explode para dezenas de
    /// milhares e o `clamp` final o deposita num canto puro da roda de cor.
    ///
    /// A rampa é o caso mínimo: os 256 níveis entram lisos, e basta um ajuste
    /// que empurre o escuro abaixo de zero. Os dois aqui vêm de uma varredura
    /// dos 53 controles sobre uma sépia ligada, e são os **extremos do
    /// painel**, não valores de laboratório: contraste 2,0 leva o nível `i` a
    /// `2i - 128`, negativo abaixo do 64; matiz -10 tira 50 do verde. Nos dois
    /// o denominador cruzava o zero em algum nível, e ali dois vizinhos saíam
    /// em cores opostas — salto de 255 num degradê liso.
    #[test]
    fn a_tonalizacao_nao_mancha_o_que_veio_fora_da_faixa() {
        let mut motor = motor_pronto();
        // Uma sépia como a do preset, e o controle que empurra para fora.
        let sepia = Ajustes {
            saturation: -1.0,
            split_shadow_hue: 35.0,
            split_shadow_sat: 60.0,
            split_highlight_hue: 45.0,
            split_highlight_sat: 40.0,
            ..Default::default()
        };
        let casos = [
            (
                "contraste no máximo",
                Ajustes {
                    contrast: 2.0,
                    ..sepia
                },
            ),
            (
                "matiz no mínimo",
                Ajustes {
                    tint: -10.0,
                    ..sepia
                },
            ),
        ];

        for (nome, ajustes) in casos {
            let saida = revelar_e_colher(&mut motor, rampa(), ajustes);

            // A entrada anda de um nível por pixel e todo ajuste aqui é função
            // contínua do nível: nenhum canal tem por que saltar dezenas entre
            // vizinhos.
            for i in 1..256usize {
                for canal in 0..3 {
                    let antes = saida[(i - 1) * 4 + canal] as i32;
                    let agora = saida[i * 4 + canal] as i32;
                    assert!(
                        (agora - antes).abs() <= 24,
                        "{nome}: mancha no pixel {i}, canal {canal} — \
                         {antes} saltou para {agora}"
                    );
                }
            }
        }
    }

    /// O grão muda a foto, é **o mesmo** a cada revelação, e some nas pontas.
    ///
    /// 🚨 **Repetir é requisito, e não detalhe.** O site revela a mesma foto a
    /// cada arrasto de slider e exporta no fim; grão sorteado por revelação daria
    /// uma prévia que nunca é o arquivo, e um "antes/depois" que pisca. Quem
    /// garante é o hash inteiro do shader, que não depende de relógio nem de
    /// quadro.
    #[test]
    fn o_grao_e_sempre_o_mesmo_e_respeita_as_pontas() {
        let mut motor = motor_pronto();
        let com_grao = Ajustes {
            grain_amount: 100.0,
            grain_size: 0.0,
            ..Default::default()
        };

        let meio_tom = cinza(16, 128);
        let primeira = revelar_e_colher(&mut motor, meio_tom.clone(), com_grao);
        let segunda = revelar_e_colher(&mut motor, meio_tom.clone(), com_grao);
        assert_eq!(
            primeira, segunda,
            "o grão tem de ser o mesmo a cada revelação"
        );
        assert_ne!(
            primeira,
            meio_tom.as_slice().to_vec(),
            "o grão tinha de mover o meio-tom"
        );

        // Monocromático: o mesmo delta nos três canais, como prata de filme.
        for pixel in primeira.as_chunks::<4>().0 {
            assert_eq!(
                (pixel[0], pixel[1]),
                (pixel[2], pixel[2]),
                "o grão saiu colorido — é chuvisco de sensor, não prata"
            );
        }

        let preto = cinza(16, 0);
        assert_eq!(
            revelar_e_colher(&mut motor, preto.clone(), com_grao),
            preto.as_slice().to_vec(),
            "no preto fechado não há grão a mostrar — e o clamp o viraria mancha"
        );
    }

    /// O tamanho do grão engrossa o grumo: células maiores, vizinhos iguais.
    ///
    /// A medida é o contraste local — quantos níveis separam pixels vizinhos.
    /// Grão fino muda a cada pixel; grão grosso repete o mesmo valor por vários,
    /// e a soma das diferenças cai.
    #[test]
    fn o_tamanho_do_grao_engrossa_o_grumo() {
        let mut motor = motor_pronto();
        let grao = |motor: &mut Motor, tamanho: f32| {
            contraste_local(&revelar_e_colher(
                motor,
                cinza(16, 128),
                Ajustes {
                    grain_amount: 100.0,
                    grain_size: tamanho,
                    ..Default::default()
                },
            ))
        };

        let fino = grao(&mut motor, 0.0);
        let grosso = grao(&mut motor, 100.0);
        assert!(
            grosso < fino,
            "grão de cinco pixels tinha de ter menos contraste entre vizinhos              que o de um pixel — saiu {grosso} contra {fino}"
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
