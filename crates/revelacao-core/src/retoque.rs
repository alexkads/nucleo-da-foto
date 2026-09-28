//! Clone e Heal na GPU: o pré-passe que troca a entrada da revelação.
//!
//! 🔑 **O bruto e os pixels de origem não mudam.** A textura de entrada é lida,
//! nunca escrita; cada retoque lê uma textura e escreve **outra** (ping-pong
//! entre duas `Rgba8Unorm`), e o shader de revelação passa a ler a última. Sem
//! retoque, ele lê a entrada original, e nada disto é criado.
//!
//! A ordem é a da revelação: cada retoque vê o resultado dos anteriores — o
//! segundo carimbo pode tirar a fonte de onde o primeiro já pintou, como no
//! Lightroom. O que está pronto fica guardado: retoque novo no fim roda só ele,
//! sobre o último resultado; qualquer outra mudança (desfazer, trocar a
//! foto) refaz a cadeia.
//!
//! A matemática e os limites do Heal estão em `shaders/retoque.wgsl`.

use std::sync::Arc;

use crate::locais::{self, Preenchimento, Retoque, AMOSTRAS_DO_ANEL};
use crate::mascaras::{self, MedidasDosLocais, Rasterizador};
use crate::preenchimento::{self, Remendo};

/// O `struct Retoque` do WGSL: dois `vec4` e o anel.
pub(crate) const TAMANHO_DO_UNIFORME: u64 = 32 + 16 * AMOSTRAS_DO_ANEL as u64;
const FORMATO: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// O estado do retoque de um tamanho de imagem.
#[derive(Default)]
pub(crate) struct Retoques {
    /// As duas texturas do ping-pong.
    texturas: Option<[wgpu::Texture; 2]>,
    /// A máscara do destino do retoque em curso.
    mascara: Option<wgpu::Texture>,
    tamanho: (u32, u32),
    /// A cadeia que está pronta, e sobre quais pixels.
    feitos: Vec<Retoque>,
    pixels: usize,
    /// Qual das duas texturas tem o resultado — `None` é a entrada original.
    pub(crate) atual: Option<usize>,
    uniformes: Option<wgpu::Buffer>,
    desenhos: Option<wgpu::Buffer>,
    trechos: Option<wgpu::Buffer>,
    lacos: Option<wgpu::Buffer>,
    /// Os remendos do Content-Aware já sintetizados: `(retoque, tamanho,
    /// pixels, remendo)`. Desfazer e refazer não sintetizam de novo.
    remendos: Vec<RemendoGuardado>,
    /// As texturas dos remendos em uso nesta revelação, e a vazia (1×1) que
    /// ocupa o binding nos retoques que não têm remendo.
    texturas_de_remendo: Vec<wgpu::Texture>,
    vazia: Option<wgpu::Texture>,
}

/// `(retoque, tamanho da imagem, identidade dos pixels, remendo)`.
type RemendoGuardado = (Preenchimento, (u32, u32), usize, Option<Remendo>);

/// O laço de um retoque: onde ele está no buffer, e a caixa para o scissor.
type LacoDoRetoque = (u64, (u32, u32, u32, u32));

/// Quantos remendos ficam guardados.
const REMENDOS_GUARDADOS: usize = 32;

fn textura(
    dispositivo: &wgpu::Device,
    rotulo: &str,
    w: u32,
    h: u32,
    formato: wgpu::TextureFormat,
) -> wgpu::Texture {
    dispositivo.create_texture(&wgpu::TextureDescriptor {
        label: Some(rotulo),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: formato,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

impl Retoques {
    /// A textura que a revelação lê, quando há retoque.
    pub(crate) fn resultado(&self) -> Option<&wgpu::Texture> {
        Some(&self.texturas.as_ref()?[self.atual?])
    }

    pub(crate) fn bytes(&self) -> u64 {
        let (w, h) = self.tamanho;
        let px = w as u64 * h as u64;
        self.texturas.as_ref().map_or(0, |_| 8 * px) + self.mascara.as_ref().map_or(0, |_| px)
    }

    /// Leva o resultado a dizer o mesmo que a revelação. Devolve se a textura
    /// que a revelação lê mudou (e o bind group tem de ser refeito).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn atualizar(
        &mut self,
        dispositivo: &wgpu::Device,
        fila: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        rasterizador: Option<&Rasterizador>,
        entrada: &wgpu::Texture,
        pixels: Option<&Arc<Vec<u8>>>,
        retoques: &[Retoque],
        largura: u32,
        altura: u32,
        medidas: &mut MedidasDosLocais,
    ) -> bool {
        let antes = self.atual;
        let Some(rasterizador) = rasterizador.filter(|_| !retoques.is_empty()) else {
            self.feitos.clear();
            self.atual = None;
            return antes.is_some();
        };

        if self.texturas.is_none() || self.tamanho != (largura, altura) {
            self.texturas = Some([
                textura(dispositivo, "Retoque A", largura, altura, FORMATO),
                textura(dispositivo, "Retoque B", largura, altura, FORMATO),
            ]);
            self.mascara = Some(textura(
                dispositivo,
                "Retoque: máscara do destino",
                largura,
                altura,
                mascaras::FORMATO,
            ));
            self.tamanho = (largura, altura);
            self.feitos.clear();
            self.atual = None;
        }
        let id_dos_pixels = pixels.map_or(0, |p| Arc::as_ptr(p) as usize);
        if self.pixels != id_dos_pixels {
            self.feitos.clear();
            self.atual = None;
            self.pixels = id_dos_pixels;
        }

        let continua = self.atual.is_some()
            && !self.feitos.is_empty()
            && retoques.len() >= self.feitos.len()
            && retoques[..self.feitos.len()] == self.feitos[..];
        let comeco = if continua { self.feitos.len() } else { 0 };
        if !continua {
            self.atual = None;
        }
        let novos = &retoques[comeco..];
        if novos.is_empty() {
            return self.atual != antes;
        }

        // Os uniformes: o do retoque (com o anel) e o `Desenho` das cápsulas
        // da máscara, um de cada por retoque, com offset dinâmico.
        let (w, h) = (largura as f32, altura as f32);
        let lado = w.max(h);
        let alinhamento = rasterizador.alinhamento;
        let passo_r = TAMANHO_DO_UNIFORME.div_ceil(alinhamento) * alinhamento;
        let mut uniformes = vec![0u8; (novos.len() as u64 * passo_r) as usize];
        let mut desenhos = vec![0u8; (novos.len() as u64 * alinhamento) as usize];
        let mut trechos: Vec<[f32; 6]> = Vec::new();
        let mut faixas = Vec::new();
        self.texturas_de_remendo.clear();
        // O índice, em `texturas_de_remendo`, do remendo de cada retoque novo.
        let mut remendo_de = Vec::with_capacity(novos.len());
        // O laço de cada retoque novo (o Content-Aware cercado), quando há.
        let mut lacos: Vec<Vec<f32>> = Vec::new();
        let mut laco_de: Vec<Option<LacoDoRetoque>> = Vec::with_capacity(novos.len());
        for (i, retoque) in novos.iter().enumerate() {
            let traco = retoque.traco();
            laco_de.push(match retoque {
                Retoque::Preencher(p) if !p.laco.is_empty() => {
                    mascaras::uniforme_do_laco(&p.laco, p.feather, largura, altura).map(|u| {
                        let caixa = mascaras::caixa_do_uniforme(&u);
                        lacos.push(u);
                        ((lacos.len() - 1) as u64, caixa)
                    })
                }
                _ => None,
            });
            let mut u = vec![0.0f32; (TAMANHO_DO_UNIFORME / 4) as usize];
            match retoque {
                Retoque::Clone(c) | Retoque::Heal(c) => {
                    let heal = matches!(retoque, Retoque::Heal(_));
                    let [dx, dy] = c.deslocamento();
                    let anel = if heal {
                        locais::anel_do_carimbo(c, largura, altura)
                    } else {
                        Vec::new()
                    };
                    u[..8].copy_from_slice(&[
                        w,
                        h,
                        if heal { 2.0 } else { 1.0 },
                        c.opacidade,
                        dx * w,
                        dy * h,
                        anel.len() as f32,
                        0.0,
                    ]);
                    for (j, q) in anel.iter().enumerate() {
                        u[8 + j * 4] = q[0];
                        u[8 + j * 4 + 1] = q[1];
                    }
                    remendo_de.push(None);
                }
                Retoque::Preencher(p) => {
                    let remendo = self.remendo(p, pixels, largura, altura, medidas);
                    // Sem remendo (buraco fora da foto, ou sem patch para
                    // oferecer), a caixa é vazia e o destino fica como está.
                    let (x0, y0, lw, lh) = remendo
                        .as_ref()
                        .map_or((0, 0, 0, 0), |r| (r.x0, r.y0, r.largura, r.altura));
                    u[..8].copy_from_slice(&[
                        w,
                        h,
                        3.0,
                        p.opacidade,
                        x0 as f32,
                        y0 as f32,
                        lw as f32,
                        lh as f32,
                    ]);
                    remendo_de.push(remendo.map(|r| {
                        self.texturas_de_remendo
                            .push(textura_do_remendo(dispositivo, fila, &r));
                        self.texturas_de_remendo.len() - 1
                    }));
                }
            }
            let inicio = i * passo_r as usize;
            uniformes[inicio..inicio + TAMANHO_DO_UNIFORME as usize]
                .copy_from_slice(bytemuck::cast_slice(&u));

            let mut d = [0.0f32; 16];
            d[..4].copy_from_slice(&[w, h, traco.raio * lado, traco.feather]);
            let inicio = i * alinhamento as usize;
            desenhos[inicio..inicio + 64].copy_from_slice(bytemuck::cast_slice(&d));

            let primeiro = trechos.len() as u32;
            for (a, b) in locais::trechos(&traco.pontos) {
                trechos.push([a[0] * w, a[1] * h, b[0] * w, b[1] * h, a[2], b[2]]);
            }
            faixas.push(primeiro..trechos.len() as u32);
        }
        mascaras::garantir(
            dispositivo,
            &mut self.uniformes,
            uniformes.len() as u64,
            wgpu::BufferUsages::UNIFORM,
            "Retoque: uniformes",
        );
        mascaras::garantir(
            dispositivo,
            &mut self.desenhos,
            desenhos.len() as u64,
            wgpu::BufferUsages::UNIFORM,
            "Retoque: desenhos",
        );
        mascaras::garantir(
            dispositivo,
            &mut self.trechos,
            (trechos.len() as u64 * 24).max(24),
            wgpu::BufferUsages::VERTEX,
            "Retoque: trechos",
        );
        let (buf_u, buf_d, buf_t) = (
            self.uniformes.as_ref().expect("garantido"),
            self.desenhos.as_ref().expect("garantido"),
            self.trechos.as_ref().expect("garantido"),
        );
        fila.write_buffer(buf_u, 0, &uniformes);
        fila.write_buffer(buf_d, 0, &desenhos);
        if !trechos.is_empty() {
            fila.write_buffer(buf_t, 0, bytemuck::cast_slice(&trechos));
        }

        let vazia = self.vazia.get_or_insert_with(|| {
            let t = textura(dispositivo, "Retoque: sem remendo", 1, 1, FORMATO);
            fila.write_texture(
                wgpu::ImageCopyTexture {
                    texture: &t,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &[0, 0, 0, 255],
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            t
        });
        let texturas = self.texturas.as_ref().expect("criadas acima");
        let mascara = self.mascara.as_ref().expect("criada acima");
        let vista_mascara = mascara.create_view(&Default::default());
        let grupo_desenho = dispositivo.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Retoque: cápsulas"),
            layout: &rasterizador.layout_desenho,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: buf_d,
                    offset: 0,
                    size: wgpu::BufferSize::new(64),
                }),
            }],
        });

        let lacos_na_gpu =
            mascaras::subir_lacos(dispositivo, fila, rasterizador, &mut self.lacos, &lacos);
        let mut atual = self.atual;
        for (i, faixa) in faixas.into_iter().enumerate() {
            // A máscara do destino: o laço, quando a área foi cercada; senão,
            // as cápsulas do caminho, com MAX.
            if let Some((k, caixa)) = laco_de[i] {
                let l = lacos_na_gpu.as_ref().expect("há laço, há buffer");
                let mut r = mascaras::passe(encoder, &vista_mascara, true);
                r.set_pipeline(&rasterizador.laco[0]);
                r.set_bind_group(0, &l.grupo, &[(k * l.passo) as u32]);
                r.set_scissor_rect(caixa.0, caixa.1, caixa.2 - caixa.0, caixa.3 - caixa.1);
                r.draw(0..6, 0..1);
            } else {
                let mut r = mascaras::passe(encoder, &vista_mascara, true);
                r.set_pipeline(&rasterizador.capsula);
                r.set_bind_group(0, &grupo_desenho, &[(i as u64 * alinhamento) as u32]);
                r.set_vertex_buffer(0, buf_t.slice(..));
                r.draw(0..6, faixa);
            }
            // Lê uma, escreve a outra.
            let leitura = match atual {
                None => entrada,
                Some(k) => &texturas[k],
            };
            let alvo = match atual {
                None | Some(1) => 0,
                Some(_) => 1,
            };
            let vista_leitura = leitura.create_view(&Default::default());
            let vista_remendo = remendo_de[i]
                .map_or(&*vazia, |k| &self.texturas_de_remendo[k])
                .create_view(&Default::default());
            let grupo = dispositivo.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Retoque"),
                layout: &rasterizador.layout_retoque,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: buf_u,
                            offset: 0,
                            size: wgpu::BufferSize::new(TAMANHO_DO_UNIFORME),
                        }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&vista_leitura),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&vista_mascara),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&vista_remendo),
                    },
                ],
            });
            let vista_alvo = texturas[alvo].create_view(&Default::default());
            let mut r = mascaras::passe(encoder, &vista_alvo, false);
            r.set_pipeline(&rasterizador.retoque);
            r.set_bind_group(0, &grupo, &[(i as u64 * passo_r) as u32]);
            r.draw(0..3, 0..1);
            drop(r);
            atual = Some(alvo);
            medidas.retoques_aplicados += 1;
        }
        self.atual = atual;
        self.feitos = retoques.to_vec();
        self.atual != antes || comeco == 0
    }
}

impl Retoques {
    /// O remendo do Content-Aware — do guardado, ou sintetizado agora.
    fn remendo(
        &mut self,
        p: &Preenchimento,
        pixels: Option<&Arc<Vec<u8>>>,
        largura: u32,
        altura: u32,
        medidas: &mut MedidasDosLocais,
    ) -> Option<Remendo> {
        let pixels = pixels?;
        let id = Arc::as_ptr(pixels) as usize;
        if let Some((_, _, _, r)) = self
            .remendos
            .iter()
            .find(|(q, t, i, _)| q == p && *t == (largura, altura) && *i == id)
        {
            return r.clone();
        }
        let comeco = relogio();
        let remendo = preenchimento::preencher(pixels, largura, altura, p);
        medidas.sintese_ms += relogio() - comeco;
        if self.remendos.len() >= REMENDOS_GUARDADOS {
            self.remendos.remove(0);
        }
        self.remendos
            .push((p.clone(), (largura, altura), id, remendo.clone()));
        remendo
    }
}

fn textura_do_remendo(
    dispositivo: &wgpu::Device,
    fila: &wgpu::Queue,
    r: &Remendo,
) -> wgpu::Texture {
    let t = textura(
        dispositivo,
        "Retoque: remendo",
        r.largura,
        r.altura,
        FORMATO,
    );
    fila.write_texture(
        wgpu::ImageCopyTexture {
            texture: &t,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &r.rgba,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(4 * r.largura),
            rows_per_image: Some(r.altura),
        },
        wgpu::Extent3d {
            width: r.largura,
            height: r.altura,
            depth_or_array_layers: 1,
        },
    );
    t
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

#[cfg(target_arch = "wasm32")]
fn relogio() -> f32 {
    0.0
}
