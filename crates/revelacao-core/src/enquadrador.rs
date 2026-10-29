//! O enquadramento na GPU — o que a **tela** mostra.
//!
//! # Por que existe
//!
//! O shader de revelação devolve a foto inteira, e o enquadramento (espelhos,
//! giro, perspectiva, endireitamento, recorte) vinha depois, na CPU, por
//! [`crate::transformacao::aplicar`] — **a cada resultado do motor**, na thread
//! da interface. Numa foto com perspectiva isso são 30 a 80 ms por quadro no
//! balcão (medido em produção, Intel Iris Pro, 28/set/2026), gastos enquanto
//! um slider anda; e sem enquadramento nenhum ainda era uma cópia da foto
//! inteira por quadro. O Lightroom não faz isso: a prévia nasce enquadrada na
//! GPU e vai para a janela como está.
//!
//! Aqui é o mesmo: uma passada de compute lê a foto revelada e escreve, já
//! enquadrada e no tamanho da saída, uma textura que volta para a CPU **pronta
//! para desenhar** — inclusive na ordem de canais que a janela quer
//! ([`OrdemDosCanais::Bgra`]), que antes era mais uma passada na CPU.
//!
//! 🔑 **O arquivo que sai não passa por aqui.** A exportação, a impressão e as
//! prévias gravadas continuam no caminho da CPU, byte a byte os de sempre: o
//! filtro bilinear da GPU e o de [`crate::transformacao`] arredondam diferente
//! por um nível aqui e ali, e isso é aceitável na tela, não no arquivo.
//!
//! A matriz é a de [`Corte::mapa`], a mesma que a perspectiva guiada já usava;
//! sem ângulo nem perspectiva o retângulo é o **inteiro** de
//! [`Corte::retangulo`], para a amostra cair no centro do texel e o pixel sair
//! igual ao da cópia de bytes da CPU.

use std::num::NonZeroUsize;

use lru::LruCache;

use crate::transformacao::Corte;

const SHADER: &str = include_str!("shaders/enquadrar.wgsl");

/// Quantos tamanhos de saída ficam guardados: a tela alterna entre o rascunho
/// do arrasto, o tamanho da cópia e o do zoom.
const SAIDAS_GUARDADAS: usize = 4;

/// A ordem dos bytes de cada pixel que volta.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OrdemDosCanais {
    /// `R, G, B, A` — a do crate `image`.
    #[default]
    Rgba,
    /// `B, G, R, A` — a que o Metal, o DirectX e o GPUI desenham sem converter.
    Bgra,
}

/// O que a tela quer de volta de uma revelação — ver [`crate::Motor::revelar_para_tela`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PedidoDeTela {
    /// `true` fora do modo de corte: a foto sai recortada. `false` no
    /// Enquadrar: a foto inteira, girada e endireitada, com os cantos que
    /// ficam de fora transparentes.
    pub recortar: bool,
    /// A ordem dos canais da imagem da tela.
    pub ordem: OrdemDosCanais,
    /// Também a foto inteira, sem enquadramento, em RGBA — a mesma de
    /// [`crate::Motor::revelar`] —, para o cache, a tira e o arquivo. Custa
    /// uma segunda leitura de volta; no rascunho de um arrasto não vale.
    pub inteira: bool,
}

/// A foto enquadrada, no tamanho da saída, pronta para a janela.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImagemDaTela {
    /// Em pixels.
    pub largura: u32,
    /// Em pixels.
    pub altura: u32,
    /// A ordem dos bytes de cada pixel.
    pub ordem: OrdemDosCanais,
    /// `largura × altura × 4` bytes, na ordem de `ordem`, sem enchimento.
    pub bytes: Vec<u8>,
}

/// O que [`crate::Motor::revelar_para_tela`] devolve.
#[derive(Debug)]
pub struct RevelacaoParaTela {
    /// A foto enquadrada, pronta para a janela.
    pub tela: ImagemDaTela,
    /// A foto inteira em RGBA, quando pedida ([`PedidoDeTela::inteira`]).
    pub inteira: Option<image::DynamicImage>,
}

/// O que acontece com o pixel que cai fora da foto.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fora {
    /// Gruda na borda — o `clamp` de `amostrar`: o endireitar recortado e os
    /// caminhos sem ângulo.
    Grudar,
    /// Pinta uma cor: branco no arquivo com perspectiva, transparente no
    /// Enquadrar (o palco aparece por trás).
    Pintar([f32; 4]),
}

/// A conta do enquadramento, sem GPU: o tamanho da saída, a matriz e a borda.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Plano {
    pub(crate) largura: u32,
    pub(crate) altura: u32,
    /// As três linhas de [`Corte::mapa`], normalizadas (`m[2][2] = 1`).
    pub(crate) matriz: [[f32; 3]; 3],
    fora: Fora,
    /// A folga na borda, em meios pixels — ver o WGSL.
    folga: f32,
}

/// O plano do enquadramento de uma foto `largura × altura`.
///
/// 🔑 **Sem ângulo nem perspectiva, o retângulo é o inteiro.** A CPU copia
/// bytes a partir de [`Corte::retangulo`] (arredondado para o pixel); a matriz
/// construída do mesmo retângulo põe cada amostra no centro de um texel, e o
/// filtro bilinear devolve o texel intacto — o mesmo byte da CPU.
pub(crate) fn planejar(corte: &Corte, largura: u32, altura: u32, recortar: bool) -> Plano {
    let (lg, ag) = corte.dimensoes_giradas(largura.max(1), altura.max(1));
    let reto = !corte.tem_perspectiva() && corte.angulo() == 0.0;
    let (saida_l, saida_a, exato) = if !recortar {
        (lg, ag, corte.clone())
    } else if reto {
        let (x, y, w, h) = corte.retangulo(lg, ag);
        let exato = Corte::novo(
            x as f32 / lg as f32,
            y as f32 / ag as f32,
            w as f32 / lg as f32,
            h as f32 / ag as f32,
            corte.giro_90(),
            0.0,
            corte.espelho_h(),
            corte.espelho_v(),
        );
        (w, h, exato)
    } else {
        let (w, h) = corte.dimensoes_de_saida(largura.max(1), altura.max(1));
        (w, h, corte.clone())
    };
    let m = exato.mapa(largura.max(1), altura.max(1), recortar);
    let m = if m[(2, 2)].abs() > 1e-12 {
        m / m[(2, 2)]
    } else {
        m
    };
    let matriz = [0, 1, 2].map(|i| [m[(i, 0)], m[(i, 1)], m[(i, 2)]].map(|v| v as f32));
    // As mesmas bordas de `aplicar_projetiva` e de `inclinar_inteira`.
    let fora = if corte.tem_perspectiva() {
        if recortar {
            Fora::Pintar([1.0, 1.0, 1.0, 1.0])
        } else {
            Fora::Pintar([0.0, 0.0, 0.0, 0.0])
        }
    } else if !recortar && corte.angulo() != 0.0 {
        Fora::Pintar([0.0, 0.0, 0.0, 0.0])
    } else {
        Fora::Grudar
    };
    // `inclinar_inteira` corta no pixel exato; a projetiva dá meio pixel.
    let folga = if !corte.tem_perspectiva() && !recortar && corte.angulo() != 0.0 {
        0.0
    } else {
        1.0
    };
    Plano {
        largura: saida_l.max(1),
        altura: saida_a.max(1),
        matriz,
        fora,
        folga,
    }
}

/// O `struct Enquadramento` do WGSL, byte a byte.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforme {
    linha_x: [f32; 4],
    linha_y: [f32; 4],
    linha_w: [f32; 4],
    saida: [f32; 2],
    origem: [f32; 2],
    fora: [f32; 4],
    modo_fora: u32,
    bgra: u32,
    folga: f32,
    _reservado: u32,
}

/// A textura enquadrada de um tamanho e o buffer que a lê de volta.
pub(crate) struct SaidaDaTela {
    pub(crate) textura: wgpu::Texture,
    pub(crate) buffer: wgpu::Buffer,
    pub(crate) bytes_por_linha: u32,
    pub(crate) bytes_por_linha_alinhado: u32,
}

/// O pipeline do enquadramento e as saídas por tamanho — um por motor,
/// criado na primeira revelação para a tela.
pub(crate) struct Enquadrador {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    amostrador: wgpu::Sampler,
    uniforme: wgpu::Buffer,
    saidas: LruCache<(u32, u32), SaidaDaTela>,
}

impl Enquadrador {
    pub(crate) fn novo(dispositivo: &wgpu::Device) -> Self {
        let modulo = dispositivo.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Enquadramento (compute)"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = dispositivo.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Layout do enquadramento"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let layout_do_pipeline =
            dispositivo.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Layout do pipeline do enquadramento"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
        let pipeline = dispositivo.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Enquadramento"),
            layout: Some(&layout_do_pipeline),
            module: &modulo,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
        // Bilinear, grudando na borda: o `amostrar` de `transformacao.rs`.
        let amostrador = dispositivo.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Amostrador do enquadramento"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let uniforme = dispositivo.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Enquadramento"),
            size: std::mem::size_of::<Uniforme>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            amostrador,
            uniforme,
            saidas: LruCache::new(NonZeroUsize::new(SAIDAS_GUARDADAS).expect("4 não é zero")),
        }
    }

    /// Grava no `encoder` a passada que enquadra `revelada` (`largura ×
    /// altura`) e a cópia da saída para o buffer de leitura. Devolve a saída.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn gravar(
        &mut self,
        dispositivo: &wgpu::Device,
        fila: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        revelada: &wgpu::Texture,
        (largura, altura): (u32, u32),
        corte: &Corte,
        recortar: bool,
        ordem: OrdemDosCanais,
    ) -> &SaidaDaTela {
        let plano = planejar(corte, largura, altura, recortar);
        let linha = |l: [f32; 3]| [l[0], l[1], l[2], 0.0];
        let (modo_fora, fora) = match plano.fora {
            Fora::Grudar => (0u32, [0.0; 4]),
            Fora::Pintar(cor) => (1u32, cor),
        };
        fila.write_buffer(
            &self.uniforme,
            0,
            bytemuck::bytes_of(&Uniforme {
                linha_x: linha(plano.matriz[0]),
                linha_y: linha(plano.matriz[1]),
                linha_w: linha(plano.matriz[2]),
                saida: [plano.largura as f32, plano.altura as f32],
                origem: [largura.max(1) as f32, altura.max(1) as f32],
                fora,
                modo_fora,
                bgra: u32::from(ordem == OrdemDosCanais::Bgra),
                folga: plano.folga,
                _reservado: 0,
            }),
        );

        let chave = (plano.largura, plano.altura);
        if !self.saidas.contains(&chave) {
            self.saidas.put(
                chave,
                saida_da_tela(dispositivo, plano.largura, plano.altura),
            );
        }
        let saida = self.saidas.get(&chave).expect("acabou de entrar");

        let vista_revelada = revelada.create_view(&wgpu::TextureViewDescriptor::default());
        let vista_saida = saida
            .textura
            .create_view(&wgpu::TextureViewDescriptor::default());
        let grupo = dispositivo.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Enquadramento"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&vista_revelada),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.amostrador),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.uniforme.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&vista_saida),
                },
            ],
        });
        crate::cronometro::com_escritas_de_compute(|timestamp_writes| {
            let mut passe = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Enquadramento"),
                timestamp_writes,
            });
            passe.set_pipeline(&self.pipeline);
            passe.set_bind_group(0, &grupo, &[]);
            passe.dispatch_workgroups(plano.largura.div_ceil(16), plano.altura.div_ceil(16), 1);
        });
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &saida.textura,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &saida.buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(saida.bytes_por_linha_alinhado),
                    rows_per_image: Some(plano.altura),
                },
            },
            wgpu::Extent3d {
                width: plano.largura,
                height: plano.altura,
                depth_or_array_layers: 1,
            },
        );
        saida
    }
}

fn saida_da_tela(dispositivo: &wgpu::Device, largura: u32, altura: u32) -> SaidaDaTela {
    const ALINHAMENTO: u32 = 256;
    let bytes_por_linha = 4 * largura;
    let bytes_por_linha_alinhado = bytes_por_linha.div_ceil(ALINHAMENTO) * ALINHAMENTO;
    let textura = dispositivo.create_texture(&wgpu::TextureDescriptor {
        label: Some("Foto enquadrada"),
        size: wgpu::Extent3d {
            width: largura,
            height: altura,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let buffer = dispositivo.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Leitura da foto enquadrada"),
        size: (bytes_por_linha_alinhado * altura) as wgpu::BufferAddress,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    SaidaDaTela {
        textura,
        buffer,
        bytes_por_linha,
        bytes_por_linha_alinhado,
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Sem ângulo nem perspectiva, a saída é o retângulo inteiro da CPU.
    #[test]
    fn o_plano_reto_usa_o_retangulo_inteiro() {
        let corte = Corte::novo(0.1, 0.2, 0.5, 0.5, 0, 0.0, false, false);
        let plano = planejar(&corte, 97, 61, true);
        let (_, _, w, h) = corte.retangulo(97, 61);
        assert_eq!((plano.largura, plano.altura), (w, h));
        assert_eq!(plano.fora, Fora::Grudar);
    }

    /// Girada de 90°, a saída troca os lados; sem recortar, é a foto girada.
    #[test]
    fn o_plano_girado_troca_os_lados() {
        let corte = Corte::novo(0.0, 0.0, 1.0, 1.0, 1, 0.0, false, false);
        let plano = planejar(&corte, 97, 61, false);
        assert_eq!((plano.largura, plano.altura), (61, 97));
    }

    /// Com ângulo, o Enquadrar pinta os cantos de transparente; recortado,
    /// gruda na borda — como `inclinar_inteira` e `endireitar_e_recortar`.
    #[test]
    fn a_borda_segue_o_caminho_da_cpu() {
        let corte = Corte::novo(0.1, 0.1, 0.8, 0.8, 0, 7.0, false, false);
        assert_eq!(
            planejar(&corte, 97, 61, false).fora,
            Fora::Pintar([0.0, 0.0, 0.0, 0.0])
        );
        assert_eq!(planejar(&corte, 97, 61, true).fora, Fora::Grudar);
        assert_eq!(
            planejar(&corte, 97, 61, true).altura,
            corte.dimensoes_de_saida(97, 61).1
        );
    }

    /// O `struct` do WGSL tem 96 bytes, em múltiplos de 16.
    #[test]
    fn o_uniforme_tem_o_tamanho_do_wgsl() {
        assert_eq!(std::mem::size_of::<Uniforme>(), 96);
    }
}
