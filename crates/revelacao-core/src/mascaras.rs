//! As máscaras locais na GPU: o cache `R8Unorm` que o shader de revelação lê.
//!
//! 🔑 **Tudo aqui é cache reconstruível.** A fonte de verdade é a
//! [`ReceitaLocal`](crate::locais::ReceitaLocal); a textura é refeita a partir
//! dela no tamanho da imagem que está sendo revelada — o preview no preview, o
//! arquivo cheio na exportação. Nenhum bitmap de máscara é ampliado.
//!
//! ## Por que `R8Unorm`, e por que render pass
//!
//! - **É alvo de cor com blend em todo backend que o motor usa**, inclusive o
//!   WebGL2 (GLES 3.0 lista `R8` como color-renderable, e `MAX` é blend do
//!   núcleo). `Motor::abrir_com` confere isso no adaptador
//!   (`get_texture_format_features`) antes de aceitar uma receita com máscara.
//! - **Não é formato de storage no WebGPU**, e por isso o rasterizador é render
//!   pass mesmo no desktop, que revela por compute. Um rasterizador só dá a mesma
//!   máscara nas duas entradas.
//! - **256 níveis bastam**: a ±4 EV o degrau é de ~0,016 EV.
//!
//! ## 🚨 A textura é `texture_2d_array` com pelo menos **duas** camadas
//!
//! O backend GL do wgpu cria uma textura de uma camada só como `TEXTURE_2D`, e
//! ela não pode ser vista como `D2Array` — o bind falharia só no WebGL2. Duas
//! camadas custam `largura × altura` bytes a mais, e o shader é um só.
//!
//! ## Invalidação
//!
//! Cada camada lembra os componentes que já estão nela. Camada igual não é
//! refeita; camada que só ganhou componentes **no fim** (o arrasto do pincel,
//! um stroke novo) recebe só os novos, sobre o que já estava, com o composto
//! limitado à caixa do stroke. Qualquer outra mudança (desfazer, apagar um do
//! meio) refaz só aquela camada.

use crate::locais::{self, Componente, Forma, Modo, ReceitaLocal, MAXIMO_DE_CAMADAS};

const SHADER: &str = include_str!("shaders/mascara.wgsl");
pub(crate) const FORMATO: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
/// O `struct Desenho` do WGSL: quatro `vec4<f32>`.
const TAMANHO_DO_DESENHO: u64 = 64;
/// `ParamsLocais` do `corpo.wgsl`: a contagem e um `vec4` por camada.
pub(crate) const TAMANHO_DOS_PARAMS: u64 = 16 + 16 * MAXIMO_DE_CAMADAS as u64;
/// Um trecho de stroke na GPU: `a.xy b.xy` e as duas pressões.
const TAMANHO_DO_TRECHO: u64 = 24;

/// Se este adaptador desenha máscara: `R8Unorm` como alvo com blend e como
/// textura lida.
pub(crate) fn suportado(adaptador: &wgpu::Adapter) -> bool {
    let recursos = adaptador.get_texture_format_features(FORMATO);
    recursos
        .allowed_usages
        .contains(wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING)
        && recursos
            .flags
            .contains(wgpu::TextureFormatFeatureFlags::BLENDABLE)
}

/// Os pipelines do rasterizador — criados na primeira receita com máscara.
pub(crate) struct Rasterizador {
    pub(crate) layout_desenho: wgpu::BindGroupLayout,
    layout_compor: wgpu::BindGroupLayout,
    pub(crate) capsula: wgpu::RenderPipeline,
    /// `[Somar, Subtrair]`.
    compor: [wgpu::RenderPipeline; 2],
    gradiente: [wgpu::RenderPipeline; 2],
    /// O laço — `shaders/laco.wgsl`. `[Somar, Subtrair]`.
    pub(crate) layout_laco: wgpu::BindGroupLayout,
    pub(crate) laco: [wgpu::RenderPipeline; 2],
    /// Clone e Heal — `shaders/retoque.wgsl`, ver `retoque.rs`.
    pub(crate) layout_retoque: wgpu::BindGroupLayout,
    pub(crate) retoque: wgpu::RenderPipeline,
    pub(crate) alinhamento: u64,
}

fn indice(modo: Modo) -> usize {
    match modo {
        Modo::Somar => 0,
        Modo::Subtrair => 1,
    }
}

impl Rasterizador {
    pub(crate) fn novo(dispositivo: &wgpu::Device) -> Self {
        let modulo = dispositivo.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Máscaras locais"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniforme = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(TAMANHO_DO_DESENHO),
            },
            count: None,
        };
        let layout_desenho =
            dispositivo.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Máscara: desenho"),
                entries: &[uniforme],
            });
        let layout_compor =
            dispositivo.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Máscara: compor"),
                entries: &[
                    uniforme,
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });

        let blend = |src, dst, op| wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: src,
                dst_factor: dst,
                operation: op,
            },
            alpha: wgpu::BlendComponent {
                src_factor: src,
                dst_factor: dst,
                operation: op,
            },
        };
        use wgpu::{BlendFactor as F, BlendOperation as O};
        let maximo = blend(F::One, F::One, O::Max);
        let somar = blend(F::One, F::OneMinusSrc, O::Add);
        let subtrair = blend(F::Zero, F::OneMinusSrc, O::Add);

        let trecho = wgpu::VertexBufferLayout {
            array_stride: TAMANHO_DO_TRECHO,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x2],
        };
        let pipeline = |rotulo: &str,
                        layout: &wgpu::BindGroupLayout,
                        vs: &str,
                        fs: &str,
                        buffers: &[wgpu::VertexBufferLayout],
                        blend: wgpu::BlendState| {
            let layout = dispositivo.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(rotulo),
                bind_group_layouts: &[layout],
                push_constant_ranges: &[],
            });
            dispositivo.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(rotulo),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &modulo,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &modulo,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMATO,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };

        let capsula = pipeline(
            "Máscara: cápsulas (MAX)",
            &layout_desenho,
            "vs_capsula",
            "fs_capsula",
            std::slice::from_ref(&trecho),
            maximo,
        );
        let compor = [
            pipeline(
                "Máscara: somar",
                &layout_compor,
                "vs_retangulo",
                "fs_compor",
                &[],
                somar,
            ),
            pipeline(
                "Máscara: subtrair",
                &layout_compor,
                "vs_retangulo",
                "fs_compor",
                &[],
                subtrair,
            ),
        ];
        let gradiente = [
            pipeline(
                "Máscara: gradiente somar",
                &layout_desenho,
                "vs_retangulo",
                "fs_gradiente",
                &[],
                somar,
            ),
            pipeline(
                "Máscara: gradiente subtrair",
                &layout_desenho,
                "vs_retangulo",
                "fs_gradiente",
                &[],
                subtrair,
            ),
        ];
        let (layout_retoque, retoque) = pipeline_do_retoque(dispositivo);
        let (layout_laco, laco) = pipelines_do_laco(dispositivo, somar, subtrair);
        Self {
            layout_laco,
            laco,
            layout_desenho,
            layout_compor,
            capsula,
            compor,
            gradiente,
            layout_retoque,
            retoque,
            alinhamento: dispositivo
                .limits()
                .min_uniform_buffer_offset_alignment
                .max(TAMANHO_DO_DESENHO as u32) as u64,
        }
    }
}

/// O `struct Laco` do WGSL: dois `vec4` e os vértices, dois por `vec4`.
pub(crate) const TAMANHO_DO_LACO: u64 = 32 + 16 * (locais::VERTICES_DO_LACO as u64 / 2);

/// O uniforme de um laço numa imagem `largura × altura` — `None` quando ele
/// não tem área dentro da foto.
pub(crate) fn uniforme_do_laco(
    pontos: &[[f32; 2]],
    feather: f32,
    largura: u32,
    altura: u32,
) -> Option<Vec<f32>> {
    let poligono = locais::laco_em_pixels(pontos, largura, altura);
    if poligono.len() < 3 {
        return None;
    }
    let (w, h) = (largura as f32, altura as f32);
    let f = feather * w.max(h);
    let folga = f * 0.5 + 1.0;
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in &poligono {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let caixa = [
        (x0 - folga).floor().max(0.0),
        (y0 - folga).floor().max(0.0),
        (x1 + folga).ceil().min(w),
        (y1 + folga).ceil().min(h),
    ];
    if caixa[2] <= caixa[0] || caixa[3] <= caixa[1] {
        return None;
    }
    let mut u = vec![0.0f32; (TAMANHO_DO_LACO / 4) as usize];
    u[..4].copy_from_slice(&[w, h, f, poligono.len() as f32]);
    u[4..8].copy_from_slice(&caixa);
    for (i, p) in poligono.iter().enumerate() {
        u[8 + i * 2] = p[0];
        u[8 + i * 2 + 1] = p[1];
    }
    Some(u)
}

/// A caixa (em pixels) que um uniforme de laço cobre — para o scissor.
pub(crate) fn caixa_do_uniforme(u: &[f32]) -> (u32, u32, u32, u32) {
    (u[4] as u32, u[5] as u32, u[6] as u32, u[7] as u32)
}

fn pipelines_do_laco(
    dispositivo: &wgpu::Device,
    somar: wgpu::BlendState,
    subtrair: wgpu::BlendState,
) -> (wgpu::BindGroupLayout, [wgpu::RenderPipeline; 2]) {
    let modulo = dispositivo.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Laço"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/laco.wgsl").into()),
    });
    let layout_do_grupo = dispositivo.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Laço"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: wgpu::BufferSize::new(TAMANHO_DO_LACO),
            },
            count: None,
        }],
    });
    let layout = dispositivo.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Laço"),
        bind_group_layouts: &[&layout_do_grupo],
        push_constant_ranges: &[],
    });
    let pipeline = |blend| {
        dispositivo.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Laço"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &modulo,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &modulo,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMATO,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    };
    (layout_do_grupo, [pipeline(somar), pipeline(subtrair)])
}

/// Os lacos de uma revelação num buffer só, com offset dinâmico, e o grupo
/// que os liga.
pub(crate) struct LacosNaGpu {
    pub(crate) grupo: wgpu::BindGroup,
    pub(crate) passo: u64,
}

pub(crate) fn subir_lacos(
    dispositivo: &wgpu::Device,
    fila: &wgpu::Queue,
    rasterizador: &Rasterizador,
    guardado: &mut Option<wgpu::Buffer>,
    lacos: &[Vec<f32>],
) -> Option<LacosNaGpu> {
    if lacos.is_empty() {
        return None;
    }
    let passo = TAMANHO_DO_LACO.div_ceil(rasterizador.alinhamento) * rasterizador.alinhamento;
    let mut bytes = vec![0u8; (lacos.len() as u64 * passo) as usize];
    for (i, u) in lacos.iter().enumerate() {
        let inicio = i * passo as usize;
        bytes[inicio..inicio + TAMANHO_DO_LACO as usize].copy_from_slice(bytemuck::cast_slice(u));
    }
    garantir(
        dispositivo,
        guardado,
        bytes.len() as u64,
        wgpu::BufferUsages::UNIFORM,
        "Laços",
    );
    let buffer = guardado.as_ref().expect("garantido");
    fila.write_buffer(buffer, 0, &bytes);
    let grupo = dispositivo.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Laços"),
        layout: &rasterizador.layout_laco,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer,
                offset: 0,
                size: wgpu::BufferSize::new(TAMANHO_DO_LACO),
            }),
        }],
    });
    Some(LacosNaGpu { grupo, passo })
}

/// O pipeline do retoque: lê uma textura e a máscara do destino, escreve outra.
fn pipeline_do_retoque(
    dispositivo: &wgpu::Device,
) -> (wgpu::BindGroupLayout, wgpu::RenderPipeline) {
    let modulo = dispositivo.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Retoque"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shaders/retoque.wgsl").into()),
    });
    let textura = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    };
    let layout_do_grupo = dispositivo.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Retoque"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(crate::retoque::TAMANHO_DO_UNIFORME),
                },
                count: None,
            },
            textura(1),
            textura(2),
            textura(3),
        ],
    });
    let layout = dispositivo.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Retoque"),
        bind_group_layouts: &[&layout_do_grupo],
        push_constant_ranges: &[],
    });
    let pipeline = dispositivo.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Retoque"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &modulo,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &modulo,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });
    (layout_do_grupo, pipeline)
}

/// O que a última atualização fez — é a régua da invalidação e da memória.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MedidasDosLocais {
    /// Camadas limpas e refeitas do zero.
    pub camadas_refeitas: u32,
    /// Componentes rasterizados (strokes e gradientes).
    pub componentes_desenhados: u32,
    /// Bytes das texturas de máscara, do rascunho e do retoque.
    pub bytes: u64,
    /// Retoques (Clone e Heal) aplicados nesta revelação — zero quando a
    /// cadeia pronta foi reaproveitada.
    pub retoques_aplicados: u32,
    /// Milissegundos de CPU sintetizando remendos de Content-Aware nesta
    /// revelação — zero quando todos vieram do guardado.
    pub sintese_ms: f32,
    /// Milissegundos gastos na CPU montando os passes (a GPU roda depois).
    pub montagem_ms: f32,
}

/// As máscaras de um tamanho de imagem — vivem dentro dos recursos do motor.
pub(crate) struct Mascaras {
    pub(crate) textura: wgpu::Texture,
    pub(crate) buffer_params: wgpu::Buffer,
    largura: u32,
    altura: u32,
    camadas_na_textura: u32,
    rascunho: Option<wgpu::Texture>,
    /// O que já está em cada camada da textura.
    feitas: Vec<Option<Vec<Componente>>>,
    desenhos: Option<wgpu::Buffer>,
    trechos: Option<wgpu::Buffer>,
    lacos: Option<wgpu::Buffer>,
}

fn textura(
    dispositivo: &wgpu::Device,
    rotulo: &str,
    largura: u32,
    altura: u32,
    camadas: u32,
) -> wgpu::Texture {
    dispositivo.create_texture(&wgpu::TextureDescriptor {
        label: Some(rotulo),
        size: wgpu::Extent3d {
            width: largura,
            height: altura,
            depth_or_array_layers: camadas,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMATO,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// Um desenho da lista: o que o `struct Desenho` recebe, e como desenhá-lo.
enum Passo {
    Limpar(u32),
    Traco {
        camada: u32,
        modo: Modo,
        desenho: u64,
        trechos: std::ops::Range<u32>,
        caixa: (u32, u32, u32, u32),
    },
    Gradiente {
        camada: u32,
        modo: Modo,
        desenho: u64,
    },
    Laco {
        camada: u32,
        modo: Modo,
        laco: u64,
        caixa: (u32, u32, u32, u32),
    },
}

impl Mascaras {
    pub(crate) fn nova(dispositivo: &wgpu::Device) -> Self {
        Self {
            textura: textura(dispositivo, "Máscaras (vazia)", 1, 1, 2),
            buffer_params: dispositivo.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Params locais"),
                size: TAMANHO_DOS_PARAMS,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            largura: 1,
            altura: 1,
            camadas_na_textura: 2,
            rascunho: None,
            feitas: Vec::new(),
            desenhos: None,
            trechos: None,
            lacos: None,
        }
    }

    pub(crate) fn vista(&self) -> wgpu::TextureView {
        self.textura.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        })
    }

    pub(crate) fn bytes(&self) -> u64 {
        let camadas = self.largura as u64 * self.altura as u64 * self.camadas_na_textura as u64;
        let rascunho = self
            .rascunho
            .as_ref()
            .map(|t| t.width() as u64 * t.height() as u64)
            .unwrap_or(0);
        camadas + rascunho
    }

    /// Leva a textura a dizer o mesmo que a receita. Devolve `true` quando a
    /// textura foi recriada — aí o bind group da revelação tem de ser refeito.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn atualizar(
        &mut self,
        dispositivo: &wgpu::Device,
        fila: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        rasterizador: Option<&Rasterizador>,
        receita: &ReceitaLocal,
        largura: u32,
        altura: u32,
        medidas: &mut MedidasDosLocais,
    ) -> bool {
        let comeco = relogio();
        let camadas = &receita.camadas[..receita.camadas.len().min(MAXIMO_DE_CAMADAS)];
        let mut params = vec![0.0f32; (TAMANHO_DOS_PARAMS / 4) as usize];
        params[0] = camadas.len() as f32;
        for (i, c) in camadas.iter().enumerate() {
            params[4 + i * 4] = c.ajustes.exposicao_ev;
            params[4 + i * 4 + 1] = if c.invertida { 1.0 } else { 0.0 };
        }
        fila.write_buffer(&self.buffer_params, 0, bytemuck::cast_slice(&params));
        *medidas = MedidasDosLocais {
            bytes: self.bytes(),
            ..Default::default()
        };
        let Some(rasterizador) = rasterizador.filter(|_| !camadas.is_empty()) else {
            return false;
        };

        let mut recriou = false;
        let necessarias = (camadas.len() as u32).max(2);
        if (self.largura, self.altura) != (largura, altura) || self.camadas_na_textura < necessarias
        {
            self.textura = textura(dispositivo, "Máscaras locais", largura, altura, necessarias);
            self.largura = largura;
            self.altura = altura;
            self.camadas_na_textura = necessarias;
            self.feitas = vec![None; necessarias as usize];
            recriou = true;
        }

        // O plano: o que cada camada precisa, em desenhos e trechos.
        let (w, h) = (largura as f32, altura as f32);
        let lado = w.max(h);
        let mut desenhos: Vec<[f32; 16]> = Vec::new();
        let mut trechos: Vec<[f32; 6]> = Vec::new();
        let mut lacos: Vec<Vec<f32>> = Vec::new();
        let mut passos = Vec::new();
        for (i, camada) in camadas.iter().enumerate() {
            let componentes = &camada.componentes;
            let novos = match &self.feitas[i] {
                Some(feitos) if feitos == componentes => continue,
                Some(feitos)
                    if !feitos.is_empty()
                        && componentes.len() > feitos.len()
                        && componentes[..feitos.len()] == feitos[..] =>
                {
                    &componentes[feitos.len()..]
                }
                _ => {
                    passos.push(Passo::Limpar(i as u32));
                    medidas.camadas_refeitas += 1;
                    &componentes[..]
                }
            };
            for componente in novos {
                let mut desenho = [0.0f32; 16];
                desenho[0] = w;
                desenho[1] = h;
                match &componente.forma {
                    Forma::Laco(l) => {
                        let Some(u) = uniforme_do_laco(&l.pontos, l.feather, largura, altura)
                        else {
                            continue;
                        };
                        passos.push(Passo::Laco {
                            camada: i as u32,
                            modo: componente.modo,
                            laco: lacos.len() as u64,
                            caixa: caixa_do_uniforme(&u),
                        });
                        lacos.push(u);
                        medidas.componentes_desenhados += 1;
                        continue;
                    }
                    Forma::Pincel(traco) => {
                        let Some(caixa) = traco.caixa_em_pixels(largura, altura) else {
                            continue;
                        };
                        desenho[2] = traco.raio * lado;
                        desenho[3] = traco.feather;
                        desenho[4..8].copy_from_slice(&[
                            caixa.0 as f32,
                            caixa.1 as f32,
                            caixa.2 as f32,
                            caixa.3 as f32,
                        ]);
                        desenho[13] = traco.opacidade;
                        let inicio = trechos.len() as u32;
                        for (a, b) in locais::trechos(&traco.pontos) {
                            trechos.push([a[0] * w, a[1] * h, b[0] * w, b[1] * h, a[2], b[2]]);
                        }
                        passos.push(Passo::Traco {
                            camada: i as u32,
                            modo: componente.modo,
                            desenho: desenhos.len() as u64,
                            trechos: inicio..trechos.len() as u32,
                            caixa,
                        });
                    }
                    Forma::Linear(g) => {
                        desenho[4..8].copy_from_slice(&[0.0, 0.0, w, h]);
                        desenho[8..12].copy_from_slice(&[
                            g.inicio[0] * w,
                            g.inicio[1] * h,
                            g.fim[0] * w,
                            g.fim[1] * h,
                        ]);
                        desenho[12] = 1.0;
                        passos.push(Passo::Gradiente {
                            camada: i as u32,
                            modo: componente.modo,
                            desenho: desenhos.len() as u64,
                        });
                    }
                    Forma::Radial(g) => {
                        desenho[3] = g.feather;
                        desenho[4..8].copy_from_slice(&[0.0, 0.0, w, h]);
                        desenho[8..12].copy_from_slice(&[
                            g.centro[0] * w,
                            g.centro[1] * h,
                            g.raio_x * lado,
                            g.raio_y * lado,
                        ]);
                        desenho[12] = 2.0;
                        desenho[14] = g.angulo.to_radians();
                        desenho[15] = if g.fora { 1.0 } else { 0.0 };
                        passos.push(Passo::Gradiente {
                            camada: i as u32,
                            modo: componente.modo,
                            desenho: desenhos.len() as u64,
                        });
                    }
                }
                desenhos.push(desenho);
                medidas.componentes_desenhados += 1;
            }
            self.feitas[i] = Some(componentes.clone());
        }
        if passos.is_empty() {
            medidas.montagem_ms = relogio() - comeco;
            return recriou;
        }

        // Os uniformes de todos os desenhos num buffer só, com offset dinâmico:
        // escrever o mesmo endereço duas vezes antes do `submit` faria todos
        // os desenhos lerem o último.
        let passo = rasterizador.alinhamento;
        let lacos_na_gpu = subir_lacos(dispositivo, fila, rasterizador, &mut self.lacos, &lacos);
        let mut bytes = vec![0u8; (desenhos.len().max(1) as u64 * passo) as usize];
        for (i, d) in desenhos.iter().enumerate() {
            let inicio = i * passo as usize;
            bytes[inicio..inicio + TAMANHO_DO_DESENHO as usize]
                .copy_from_slice(bytemuck::cast_slice(d));
        }
        garantir(
            dispositivo,
            &mut self.desenhos,
            bytes.len() as u64,
            wgpu::BufferUsages::UNIFORM,
            "Máscara: desenhos",
        );
        let buffer_desenhos = self.desenhos.as_ref().expect("acabou de ser garantido");
        fila.write_buffer(buffer_desenhos, 0, &bytes);
        if !trechos.is_empty() {
            garantir(
                dispositivo,
                &mut self.trechos,
                trechos.len() as u64 * TAMANHO_DO_TRECHO,
                wgpu::BufferUsages::VERTEX,
                "Máscara: trechos",
            );
            let b = self.trechos.as_ref().expect("acabou de ser garantido");
            fila.write_buffer(b, 0, bytemuck::cast_slice(&trechos));
        }
        let buffer_trechos = self.trechos.as_ref().filter(|_| !trechos.is_empty());
        let ha_traco = !trechos.is_empty();
        if ha_traco
            && self
                .rascunho
                .as_ref()
                .is_none_or(|r| (r.width(), r.height()) != (largura, altura))
        {
            self.rascunho = Some(textura(
                dispositivo,
                "Máscara: rascunho",
                largura,
                altura,
                1,
            ));
        }

        let uniforme = wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: buffer_desenhos,
            offset: 0,
            size: wgpu::BufferSize::new(TAMANHO_DO_DESENHO),
        });
        let grupo_desenho = dispositivo.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Máscara: desenho"),
            layout: &rasterizador.layout_desenho,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforme.clone(),
            }],
        });
        let vista_do_rascunho = self
            .rascunho
            .as_ref()
            .filter(|_| ha_traco)
            .map(|r| r.create_view(&Default::default()));
        let grupo_compor = vista_do_rascunho.as_ref().map(|vista| {
            dispositivo.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Máscara: compor"),
                layout: &rasterizador.layout_compor,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniforme.clone(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(vista),
                    },
                ],
            })
        });

        let vista_da_camada = |i: u32| {
            self.textura.create_view(&wgpu::TextureViewDescriptor {
                label: Some("Máscara: uma camada"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: i,
                array_layer_count: Some(1),
                ..Default::default()
            })
        };
        for p in &passos {
            match p {
                Passo::Limpar(i) => {
                    drop(passe(encoder, &vista_da_camada(*i), true));
                }
                Passo::Traco {
                    camada,
                    modo,
                    desenho,
                    trechos,
                    caixa,
                } => {
                    let deslocamento = (*desenho * passo) as u32;
                    let rascunho = vista_do_rascunho.as_ref().expect("há traço, há rascunho");
                    {
                        let mut r = passe(encoder, rascunho, true);
                        r.set_pipeline(&rasterizador.capsula);
                        r.set_bind_group(0, &grupo_desenho, &[deslocamento]);
                        r.set_vertex_buffer(
                            0,
                            buffer_trechos.expect("há traço, há trechos").slice(..),
                        );
                        r.draw(0..6, trechos.clone());
                    }
                    let mut r = passe(encoder, &vista_da_camada(*camada), false);
                    r.set_pipeline(&rasterizador.compor[indice(*modo)]);
                    r.set_bind_group(
                        0,
                        grupo_compor.as_ref().expect("há traço, há grupo"),
                        &[deslocamento],
                    );
                    r.set_scissor_rect(caixa.0, caixa.1, caixa.2 - caixa.0, caixa.3 - caixa.1);
                    r.draw(0..6, 0..1);
                }
                Passo::Laco {
                    camada,
                    modo,
                    laco,
                    caixa,
                } => {
                    let l = lacos_na_gpu.as_ref().expect("há laço, há buffer");
                    let mut r = passe(encoder, &vista_da_camada(*camada), false);
                    r.set_pipeline(&rasterizador.laco[indice(*modo)]);
                    r.set_bind_group(0, &l.grupo, &[(*laco * l.passo) as u32]);
                    r.set_scissor_rect(caixa.0, caixa.1, caixa.2 - caixa.0, caixa.3 - caixa.1);
                    r.draw(0..6, 0..1);
                }
                Passo::Gradiente {
                    camada,
                    modo,
                    desenho,
                } => {
                    let mut r = passe(encoder, &vista_da_camada(*camada), false);
                    r.set_pipeline(&rasterizador.gradiente[indice(*modo)]);
                    r.set_bind_group(0, &grupo_desenho, &[(*desenho * passo) as u32]);
                    r.draw(0..6, 0..1);
                }
            }
        }
        medidas.bytes = self.bytes();
        medidas.montagem_ms = relogio() - comeco;
        recriou
    }
}

/// Um passe de render sobre uma vista da máscara — limpando antes ou não.
pub(crate) fn passe(
    encoder: &mut wgpu::CommandEncoder,
    vista: &wgpu::TextureView,
    limpar: bool,
) -> wgpu::RenderPass<'static> {
    // O cronômetro da GPU carimba a passada, quando há medição em curso
    // (`crate::cronometro`); sem ela, `timestamp_writes` fica `None`.
    crate::cronometro::com_escritas_de_render(|timestamp_writes| {
        encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Máscara"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: vista,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if limpar {
                            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes,
                occlusion_query_set: None,
            })
            .forget_lifetime()
    })
}

/// Um buffer com pelo menos `tamanho` bytes, reaproveitado entre quadros.
pub(crate) fn garantir(
    dispositivo: &wgpu::Device,
    guardado: &mut Option<wgpu::Buffer>,
    tamanho: u64,
    uso: wgpu::BufferUsages,
    rotulo: &str,
) {
    if guardado.as_ref().is_some_and(|b| b.size() >= tamanho) {
        return;
    }
    // Cresce com folga: um arrasto acrescenta trechos a cada quadro.
    *guardado = Some(dispositivo.create_buffer(&wgpu::BufferDescriptor {
        label: Some(rotulo),
        size: tamanho.next_power_of_two().max(256),
        usage: uso | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    }));
}

#[cfg(not(target_arch = "wasm32"))]
fn relogio() -> f32 {
    use std::sync::OnceLock;
    static INICIO: OnceLock<std::time::Instant> = OnceLock::new();
    INICIO
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f32()
        * 1000.0
}

/// No navegador não há `Instant`; a montagem é medida por quem chama, com
/// `performance.now()`.
#[cfg(target_arch = "wasm32")]
fn relogio() -> f32 {
    0.0
}
