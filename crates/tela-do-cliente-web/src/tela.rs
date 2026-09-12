//! A tela: as camadas, o cruzamento e o desenho.

use std::sync::Arc;

use revelacao_core::{Ajustes, Corte, Entrada};
use wasm_bindgen::prelude::*;

/// Quanto dura o cruzamento entre uma foto e a seguinte, em milissegundos.
///
/// 🔑 **Aqui não é o mesmo compromisso da galeria.** Meio segundo era o número
/// da versão em CSS, herdado de uma tela em que o operador atravessa a tira com
/// a seta e a transição vira melado. **Esta tela é a do cliente**: quem olha não
/// está navegando, está decidindo se leva a foto — e o que ele viu foi *"tá
/// muito rápido"* (dono, 2026-09-11). Um segundo dá tempo de a foto que entra
/// ser percebida como apresentação, e não como troca de slide.
///
/// ⚠️ Passar muito disto tem um custo que não aparece parado: o operador que
/// anda três fotos seguidas deixa a tela do cliente cruzando o tempo todo, e a
/// foto nunca assenta. A saída não seria encurtar isto — é [`Tela::mostrar`]
/// descartar a camada do meio, que é o que ele já faz.
const CRUZAMENTO_MS: f64 = 1000.0;

/// De quanto a foto que entra começa maior — o respiro de apresentação.
///
/// 🔑 **Só a que entra se move.** Dois movimentos cruzados dariam a impressão
/// de a foto ter sido empurrada, e o que se quer é que ela **apareça**.
///
/// Cresceu junto com o tempo: num cruzamento de um segundo, 3% é um movimento
/// que quase não se vê — e o movimento é metade do que faz a foto parecer
/// entrar em vez de piscar.
const PASSO_DA_ENTRADA: f32 = 1.06;

/// O formato das texturas em que cada foto é revelada.
///
/// Não-sRGB de propósito, como a superfície: o byte que o shader de revelação
/// escreveu é o byte que vai para a tela, sem uma conversão de gama no meio que
/// o editor não tem.
const FORMATO_DA_CAMADA: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn erro(mensagem: impl Into<String>) -> JsValue {
    JsValue::from_str(&mensagem.into())
}

/// O `uniform` de uma camada — a mesma struct do WGSL.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CamadaUniforme {
    escala: [f32; 2],
    centro: [f32; 2],
    uv_x: [f32; 2],
    uv_y: [f32; 2],
    uv_off: [f32; 2],
    alfa: f32,
    _reservado: f32,
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
        let ajustes = ajustes_de(ajustes)?;
        let corte = corte_de(corte)?;

        let mut camada = self.nova_camada(largura, altura, rgba, ajustes, corte);
        // 🔑 Os módulos locais do darktable medem em pixels da foto original;
        // a tela recebe a cópia de trabalho, como o editor, e precisa da mesma
        // escala para mostrar o mesmo estilo.
        camada.escala = match lado_original {
            Some(lado) if lado > 0 => (largura.max(altura) as f32 / lado as f32).min(1.0),
            _ => 1.0,
        };
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
        // 🚨 **Sai a menos visível, e não a mais antiga.** Com `remove(0)`, o
        // operador que apertasse a seta duas vezes antes do primeiro quadro
        // descartava a foto que estava **inteira** na tela (ela era a mais
        // antiga) e mantinha uma que nunca chegou a ser desenhada, em alfa
        // zero: a tela saltava de uma para a outra sem cruzamento nenhum. A
        // que ainda tem o que mostrar é a que fica.
        if self.camadas.len() >= 2 {
            let descartar = self
                .camadas
                .iter()
                .enumerate()
                .filter(|(_, c)| c.saindo.is_some())
                .min_by(|(_, a), (_, b)| a.alfa.total_cmp(&b.alfa))
                .map(|(i, _)| i)
                .unwrap_or(0);
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
            let (pixels, largura, altura, ajustes, escala) = {
                let c = &self.camadas[i];
                (c.pixels.clone(), c.largura, c.altura, c.ajustes, c.escala)
            };
            self.motor.definir_escala_do_original(escala);
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
                    let t = ((agora - comecou) / CRUZAMENTO_MS).clamp(0.0, 1.0) as f32;
                    if t < 1.0 {
                        animando = true;
                    }
                    // Só esmaece: dois movimentos cruzados pareceriam empurrão.
                    (saida.de * (1.0 - suavizar(t)), 1.0)
                }
                None => {
                    let entrou = *camada.entrou_em.get_or_insert(agora);
                    let t = ((agora - entrou) / CRUZAMENTO_MS).clamp(0.0, 1.0) as f32;
                    if t < 1.0 {
                        animando = true;
                    }
                    let suave = suavizar(t);
                    (suave, PASSO_DA_ENTRADA + (1.0 - PASSO_DA_ENTRADA) * suave)
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
            let fica = i == ultima || c.alfa > 0.0 || c.saindo.is_none();
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

/// A curva do cruzamento: começa devagar, termina devagar.
///
/// 🔑 **`ease-in-out`, e não o `ease-out` de antes.** O `ease-out` sai do zero
/// a toda velocidade — a foto nova "aparece de estalo" e depois demora a
/// assentar, que é exatamente a sensação de rápido demais que o dono relatou,
/// mesmo com o tempo aumentado. Esta é simétrica: os dois primeiros quadros
/// quase não mexem, o meio faz o trabalho, e o fim encosta devagar.
fn suavizar(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        let d = -2.0 * t + 2.0;
        1.0 - d * d * d / 2.0
    }
}

fn ajustes_de(v: &[f32]) -> Result<Ajustes, JsValue> {
    Ajustes::de_vetor(v).ok_or_else(|| {
        erro(format!(
            "esperava {} ajustes, recebi {}",
            revelacao_core::QUANTIDADE,
            v.len()
        ))
    })
}

fn corte_de(v: &[f32]) -> Result<Corte, JsValue> {
    if v.len() != 8 {
        return Err(erro(format!(
            "esperava 8 campos de corte, recebi {}",
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

/// Onde esta camada fica na janela, e que pedaço da foto ela mostra.
///
/// 🚨 **O enquadramento é a mesma conta do arquivo** (`Corte::retangulo` sobre
/// `dimensoes_giradas`), e não uma aproximação: é o que impede o operador de
/// enquadrar uma coisa na tela e o cliente ver outra. O que muda entre os dois é
/// só quem interpola — aqui o amostrador, no arquivo a bilinear do core.
/// Onde esta camada fica na janela, e que pedaço da foto ela mostra.
///
/// 🚨 **O enquadramento é a mesma conta do arquivo** (`Corte::retangulo` sobre
/// `dimensoes_giradas`), e não uma aproximação: é o que impede o operador de
/// enquadrar uma coisa na tela e o cliente ver outra. O que muda entre os dois é
/// só quem interpola — aqui o amostrador, no arquivo a bilinear do core.
/// As UVs do enquadramento — a parte **pura**, e é ela que o teste cobra.
///
/// 🔑 **Extraída de `montar_uniforme` em 2026-09-12**, quando o dono relatou
/// que *"o rotacionamento de fotos na tela do cliente não está funcionando
/// corretamente"* enquanto a revelação, as tiras e a biblioteca funcionavam. O
/// resto daquela função precisa de uma `Camada` com textura de GPU e não se
/// testa fora do navegador; esta parte é aritmética, e agora está presa a um
/// teste que a compara com o `transformacao::aplicar` do core — a mesma conta
/// que produz o arquivo.
///
/// Devolve `(uv_x, uv_y, uv_off)`, que o shader usa como
/// `uv = uv_off + uv_x·s + uv_y·t` para `(s, t)` no quad `0..1`.
///
/// 🚨 **A ordem é a do `transformacao.rs`: espelhos, giro de 90°, ângulo,
/// recorte.** Aqui ela é percorrida ao contrário, porque o caminho é o inverso
/// — do pixel na tela de volta ao pixel da textura.
pub(crate) fn uvs_do_enquadramento(
    largura: u32,
    altura: u32,
    corte: &Corte,
) -> ([f32; 2], [f32; 2], [f32; 2]) {
    let (lg, ag) = corte.dimensoes_giradas(largura, altura);
    let (rx, ry, rw, rh) = corte.retangulo(lg, ag);

    // As UVs: do quad (0..1) para o pedaço da foto que o retângulo marca, no
    // espaço girado — e daí de volta para o espaço da textura.
    let (mut ux, mut uy, mut uoff) = (
        [rw as f32 / lg as f32, 0.0],
        [0.0, rh as f32 / ag as f32],
        [rx as f32 / lg as f32, ry as f32 / ag as f32],
    );

    // O endireitamento gira em torno do centro do espaço girado, como no core.
    if corte.angulo() != 0.0 {
        let r = corte.angulo().to_radians();
        let (sen, cos) = r.sin_cos();
        // Em UV o espaço não é quadrado: gira em pixels e volta.
        let gira = |v: [f32; 2]| {
            let (x, y) = (v[0] * lg as f32, v[1] * ag as f32);
            [
                (x * cos - y * sen) / lg as f32,
                (x * sen + y * cos) / ag as f32,
            ]
        };
        let centro = [0.5, 0.5];
        let canto = [uoff[0] - centro[0], uoff[1] - centro[1]];
        let girado = gira(canto);
        uoff = [girado[0] + centro[0], girado[1] + centro[1]];
        ux = gira(ux);
        uy = gira(uy);
    }

    // Giro de 90° e espelhos: uma troca de eixos e um sinal.
    let quartos = ((corte.giro_90() % 4) + 4) % 4;
    for _ in 0..quartos {
        // (x, y) → (y, 1 - x): um quarto de volta no espaço normalizado.
        let troca = |v: [f32; 2]| [v[1], -v[0]];
        ux = troca(ux);
        uy = troca(uy);
        uoff = [uoff[1], 1.0 - uoff[0]];
    }
    if corte.espelho_h() {
        ux[0] = -ux[0];
        uy[0] = -uy[0];
        uoff[0] = 1.0 - uoff[0];
    }
    if corte.espelho_v() {
        ux[1] = -ux[1];
        uy[1] = -uy[1];
        uoff[1] = 1.0 - uoff[1];
    }

    (ux, uy, uoff)
}

/// Onde esta camada fica na janela, e que pedaço da foto ela mostra.
///
/// 🚨 **O enquadramento é a mesma conta do arquivo** (`Corte::retangulo` sobre
/// `dimensoes_giradas`), e não uma aproximação: é o que impede o operador de
/// enquadrar uma coisa na tela e o cliente ver outra. O que muda entre os dois é
/// só quem interpola — aqui o amostrador, no arquivo a bilinear do core.
fn montar_uniforme(camada: &Camada, janela: (f32, f32), alfa: f32, zoom: f32) -> CamadaUniforme {
    let corte = &camada.corte;

    // 1. Encaixe: a foto enquadrada cabe inteira na janela, sem cortar nada.
    //
    // 🚨 **O tamanho vem de `dimensoes_de_saida`, e não do retângulo.** Com
    // ângulo os dois divergem — o endireitamento reamostra para a saída pedida,
    // sem o `min` da borda —, e usar o retângulo ali deixava a foto endireitada
    // com a proporção errada na tela do cliente enquanto o arquivo saía certo.
    // É a outra metade do defeito de 2026-09-12; a primeira estava nas UVs.
    let (sw, sh) = corte.dimensoes_de_saida(camada.largura, camada.altura);
    let (jw, jh) = janela;
    let escala = (jw / sw.max(1) as f32).min(jh / sh.max(1) as f32) * zoom;
    let largura_na_tela = sw as f32 * escala;
    let altura_na_tela = sh as f32 * escala;

    // 2. As UVs — a conta pura, testada em `uvs_do_enquadramento`.
    let (ux, uy, uoff) =
        revelacao_core::transformacao::uvs_do_enquadramento(camada.largura, camada.altura, corte);

    CamadaUniforme {
        escala: [largura_na_tela / jw, altura_na_tela / jh],
        centro: [0.0, 0.0],
        uv_x: ux,
        uv_y: uy,
        uv_off: uoff,
        alfa,
        _reservado: 0.0,
    }
}
