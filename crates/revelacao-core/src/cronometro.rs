//! ⏱️ O cronômetro da GPU: quanto cada passada do motor levou **na GPU**.
//!
//! # O pedido (dono, 2026-09-27)
//!
//! A ferramenta de desempenho do rodapé tem de dizer em qual etapa da GPU o
//! tempo de um arrasto foi gasto — revelação, máscaras, retoques. Um `Instant`
//! em volta do `queue.submit` **não** responde isso: ele mede a CPU gravando e
//! enviando comandos, e a GPU trabalha depois, em paralelo. Quem mede a GPU é a
//! própria GPU, por *timestamp query*.
//!
//! # Como
//!
//! - **Só com `Features::TIMESTAMP_QUERY`**, que o motor pede ao abrir quando o
//!   adaptador oferece ([`suportado`]). Sem ele, [`Cronometro`] não existe e a
//!   ferramenta diz "indisponível" — nada mais muda.
//! - **Os carimbos vão nas próprias passadas** (`timestamp_writes`), que é o que
//!   o `TIMESTAMP_QUERY` sozinho permite. Carimbo no meio do encoder
//!   (`TIMESTAMP_QUERY_INSIDE_ENCODERS`) não existe nas GPUs Apple, e o motor
//!   precisa responder igual nas três plataformas.
//! - 🚨 **Cada carimbo tem índice próprio.** Uma etapa com várias passadas (um
//!   traço de pincel por passada) carimba o começo na primeira e o fim em
//!   todas, cada fim num índice novo: no Vulkan escrever duas vezes a mesma
//!   query sem `reset` é comportamento indefinido.
//! - **A leitura não espera nada a mais.** O buffer dos carimbos é mapeado junto
//!   com o da foto, que a thread do motor já espera; quando a foto volta, os
//!   carimbos voltaram. Se não voltaram, a próxima revelação sai **sem**
//!   medição em vez de esperar — nunca na thread da interface, que não toca em
//!   nada disto.
//!
//! # As passadas que ninguém passa para cá
//!
//! As máscaras e os retoques abrem as passadas em `mascaras::passe`, fundo
//! dentro de `atualizar`. Em vez de atravessar o cronômetro por cinco
//! assinaturas, a medição em curso fica numa `thread_local` enquanto o encoder é
//! gravado ([`na_etapa`]). O motor vive numa thread só, e a variável só existe
//! entre [`Cronometro::comecar`] e [`Cronometro::encerrar`].

use std::cell::RefCell;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

/// Quantos carimbos cabem numa revelação. Um traço de pincel é uma passada; um
/// retrato com vinte traços e cinco retoques usa ~60. Acima disso a etapa que
/// estourou sai como indisponível, e não com um fim adiantado.
const CAPACIDADE: u32 = 256;

/// As etapas da GPU que o motor controla.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EtapaDaGpu {
    /// As passadas que rasterizam as máscaras (pincel, gradientes, laço).
    Mascaras = 0,
    /// Clone, Heal e o remendo do Preencher.
    Retoques = 1,
    /// A passada dos 46 ajustes.
    Revelacao = 2,
}

const ETAPAS: usize = 3;

/// O que a GPU gastou numa revelação, em milissegundos. `None` = a etapa não
/// rodou nesta revelação, ou o carimbo não é confiável.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TemposDaGpu {
    pub mascaras_ms: Option<f32>,
    pub retoques_ms: Option<f32>,
    pub revelacao_ms: Option<f32>,
    /// Do começo da primeira passada ao fim da última — inclui o intervalo
    /// entre passadas, e não inclui a cópia de volta para a CPU.
    pub total_ms: Option<f32>,
}

/// Um relógio em milissegundos para as etapas de CPU do motor. No navegador
/// não há `Instant`: lá quem mede é quem chama, com `performance.now()`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn relogio_ms() -> f64 {
    use std::sync::OnceLock;
    static INICIO: OnceLock<std::time::Instant> = OnceLock::new();
    INICIO
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs_f64()
        * 1000.0
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn relogio_ms() -> f64 {
    0.0
}

/// O adaptador carimba passadas.
pub fn suportado(adaptador: &wgpu::Adapter) -> bool {
    adaptador
        .features()
        .contains(wgpu::Features::TIMESTAMP_QUERY)
}

/// Os carimbos de uma etapa dentro do encoder em gravação.
#[derive(Clone, Copy, Default)]
struct Marcas {
    inicio: Option<u32>,
    fim: Option<u32>,
    estourou: bool,
}

/// A medição em curso — viva só enquanto o encoder é gravado.
struct Medicao {
    conjunto: Arc<wgpu::QuerySet>,
    proximo: u32,
    etapa: Option<EtapaDaGpu>,
    marcas: [Marcas; ETAPAS],
}

thread_local! {
    static MEDICAO: RefCell<Option<Medicao>> = const { RefCell::new(None) };
}

/// Grava as passadas de `f` como da `etapa` (se houver medição em curso).
pub(crate) fn na_etapa<R>(etapa: EtapaDaGpu, f: impl FnOnce() -> R) -> R {
    let anterior = MEDICAO.with(|m| m.borrow_mut().as_mut().map(|m| m.etapa.replace(etapa)));
    let saida = f();
    if let Some(anterior) = anterior {
        MEDICAO.with(|m| {
            if let Some(m) = m.borrow_mut().as_mut() {
                m.etapa = anterior;
            }
        });
    }
    saida
}

/// Reserva os índices da próxima passada: `(conjunto, começo, fim)`.
fn reservar() -> Option<(Arc<wgpu::QuerySet>, Option<u32>, u32)> {
    MEDICAO.with(|m| {
        let mut m = m.borrow_mut();
        let m = m.as_mut()?;
        let etapa = m.etapa? as usize;
        let precisa_de_inicio = m.marcas[etapa].inicio.is_none();
        let precisa = if precisa_de_inicio { 2 } else { 1 };
        if m.proximo + precisa > CAPACIDADE {
            m.marcas[etapa].estourou = true;
            return None;
        }
        let inicio = precisa_de_inicio.then(|| {
            m.proximo += 1;
            m.proximo - 1
        });
        let fim = m.proximo;
        m.proximo += 1;
        if let Some(i) = inicio {
            m.marcas[etapa].inicio = Some(i);
        }
        m.marcas[etapa].fim = Some(fim);
        Some((m.conjunto.clone(), inicio, fim))
    })
}

/// Os carimbos de uma passada de render, se houver medição em curso.
pub(crate) fn com_escritas_de_render<R>(
    f: impl FnOnce(Option<wgpu::RenderPassTimestampWrites<'_>>) -> R,
) -> R {
    match reservar() {
        Some((conjunto, inicio, fim)) => f(Some(wgpu::RenderPassTimestampWrites {
            query_set: &conjunto,
            beginning_of_pass_write_index: inicio,
            end_of_pass_write_index: Some(fim),
        })),
        None => f(None),
    }
}

/// Os carimbos de uma passada de compute, se houver medição em curso.
pub(crate) fn com_escritas_de_compute<R>(
    f: impl FnOnce(Option<wgpu::ComputePassTimestampWrites<'_>>) -> R,
) -> R {
    match reservar() {
        Some((conjunto, inicio, fim)) => f(Some(wgpu::ComputePassTimestampWrites {
            query_set: &conjunto,
            beginning_of_pass_write_index: inicio,
            end_of_pass_write_index: Some(fim),
        })),
        None => f(None),
    }
}

/// O estado do buffer de leitura, compartilhado com o callback do `map_async`.
const LIVRE: u8 = 0;
const PEDIDA: u8 = 1;
const PRONTA: u8 = 2;
const FALHOU: u8 = 3;

/// O conjunto de queries e os buffers de um motor — abertos uma vez.
pub struct Cronometro {
    conjunto: Arc<wgpu::QuerySet>,
    resolvido: wgpu::Buffer,
    leitura: wgpu::Buffer,
    /// Nanossegundos por tique do relógio da GPU.
    periodo_ns: f32,
    estado: Arc<AtomicU8>,
    /// As marcas da revelação cujos carimbos estão no buffer de leitura.
    marcas_em_leitura: Option<([Marcas; ETAPAS], u32)>,
}

impl Cronometro {
    pub fn novo(dispositivo: &wgpu::Device, fila: &wgpu::Queue) -> Self {
        let bytes = u64::from(CAPACIDADE) * 8;
        Self {
            conjunto: Arc::new(dispositivo.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("Cronômetro da revelação"),
                ty: wgpu::QueryType::Timestamp,
                count: CAPACIDADE,
            })),
            resolvido: dispositivo.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Cronômetro: resolvido"),
                size: bytes,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            leitura: dispositivo.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Cronômetro: leitura"),
                size: bytes,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            periodo_ns: fila.get_timestamp_period(),
            estado: Arc::new(AtomicU8::new(LIVRE)),
            marcas_em_leitura: None,
        }
    }

    /// Abre a medição de um encoder. `false` se a leitura anterior ainda não
    /// voltou — esta revelação sai sem medição, e ninguém espera.
    pub(crate) fn comecar(&mut self) -> bool {
        if self.estado.load(Ordering::Acquire) != LIVRE {
            return false;
        }
        MEDICAO.with(|m| {
            *m.borrow_mut() = Some(Medicao {
                conjunto: self.conjunto.clone(),
                proximo: 0,
                etapa: None,
                marcas: [Marcas::default(); ETAPAS],
            })
        });
        true
    }

    /// Fecha a medição: resolve os carimbos escritos e copia para a leitura.
    pub(crate) fn encerrar(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(medicao) = MEDICAO.with(|m| m.borrow_mut().take()) else {
            return;
        };
        if medicao.proximo == 0 {
            return;
        }
        // 🚨 Só o intervalo escrito: resolver query que nunca foi escrita é
        // indefinido no Vulkan (e pode esperar para sempre com `WAIT`).
        encoder.resolve_query_set(&self.conjunto, 0..medicao.proximo, &self.resolvido, 0);
        encoder.copy_buffer_to_buffer(
            &self.resolvido,
            0,
            &self.leitura,
            0,
            u64::from(medicao.proximo) * 8,
        );
        self.marcas_em_leitura = Some((medicao.marcas, medicao.proximo));
    }

    /// Pede o mapeamento, depois do `submit`. Não espera.
    pub(crate) fn pedir_leitura(&mut self) {
        let Some((_, usados)) = self.marcas_em_leitura else {
            return;
        };
        self.estado.store(PEDIDA, Ordering::Release);
        let estado = self.estado.clone();
        self.leitura
            .slice(..u64::from(usados) * 8)
            .map_async(wgpu::MapMode::Read, move |r| {
                estado.store(if r.is_ok() { PRONTA } else { FALHOU }, Ordering::Release);
            });
    }

    /// Os tempos, se a leitura já voltou. Chamado depois do `poll` que o motor
    /// já faz; nunca bloqueia.
    pub(crate) fn colher(&mut self) -> Option<TemposDaGpu> {
        match self.estado.load(Ordering::Acquire) {
            PRONTA => {}
            FALHOU => {
                self.leitura.unmap();
                self.marcas_em_leitura = None;
                self.estado.store(LIVRE, Ordering::Release);
                return None;
            }
            _ => return None,
        }
        let (marcas, usados) = self.marcas_em_leitura.take()?;
        let fatia = self.leitura.slice(..u64::from(usados) * 8);
        let carimbos: Vec<u64> = {
            let dados = fatia.get_mapped_range();
            dados
                .as_chunks::<8>()
                .0
                .iter()
                .map(|c| u64::from_le_bytes(*c))
                .collect()
        };
        self.leitura.unmap();
        self.estado.store(LIVRE, Ordering::Release);
        Some(tempos_dos_carimbos(&marcas, &carimbos, self.periodo_ns))
    }
}

/// A conta dos carimbos, separada para ser testada sem GPU.
fn tempos_dos_carimbos(
    marcas: &[Marcas; ETAPAS],
    carimbos: &[u64],
    periodo_ns: f32,
) -> TemposDaGpu {
    let ler = |i: Option<u32>| i.and_then(|i| carimbos.get(i as usize).copied());
    let em_ms = |tiques: u64| (tiques as f64 * f64::from(periodo_ns) / 1e6) as f32;
    let etapa = |m: &Marcas| -> Option<f32> {
        if m.estourou {
            return None;
        }
        let (inicio, fim) = (ler(m.inicio)?, ler(m.fim)?);
        // Carimbo zerado ou fora de ordem: o driver não carimbou de verdade
        // (acontece com passadas vazias em alguns backends). Melhor
        // indisponível que um número inventado.
        (inicio > 0 && fim >= inicio).then(|| em_ms(fim - inicio))
    };
    let validas = marcas.iter().filter(|m| !m.estourou);
    let primeiro = validas
        .clone()
        .filter_map(|m| ler(m.inicio))
        .filter(|&c| c > 0)
        .min();
    let ultimo = validas.filter_map(|m| ler(m.fim)).max();
    TemposDaGpu {
        mascaras_ms: etapa(&marcas[EtapaDaGpu::Mascaras as usize]),
        retoques_ms: etapa(&marcas[EtapaDaGpu::Retoques as usize]),
        revelacao_ms: etapa(&marcas[EtapaDaGpu::Revelacao as usize]),
        total_ms: match (primeiro, ultimo) {
            (Some(p), Some(u)) if u >= p => Some(em_ms(u - p)),
            _ => None,
        },
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn cada_etapa_mede_do_seu_comeco_ao_seu_ultimo_fim() {
        let mut marcas = [Marcas::default(); ETAPAS];
        // Máscaras: começo 0, três passadas (fins 1, 2, 3).
        marcas[0] = Marcas {
            inicio: Some(0),
            fim: Some(3),
            estourou: false,
        };
        // Revelação: 4 → 5.
        marcas[2] = Marcas {
            inicio: Some(4),
            fim: Some(5),
            estourou: false,
        };
        let carimbos = [1_000, 1_500, 2_000, 3_000, 3_100, 7_100];
        // Período de 1000 ns por tique: 1 tique = 0,001 ms.
        let t = tempos_dos_carimbos(&marcas, &carimbos, 1000.0);
        assert_eq!(t.mascaras_ms, Some(2.0));
        assert_eq!(t.retoques_ms, None, "não rodou");
        assert_eq!(t.revelacao_ms, Some(4.0));
        assert_eq!(t.total_ms, Some(6.1));
    }

    #[test]
    fn carimbo_zerado_ou_estourado_vira_indisponivel() {
        let mut marcas = [Marcas::default(); ETAPAS];
        marcas[0] = Marcas {
            inicio: Some(0),
            fim: Some(1),
            estourou: true,
        };
        marcas[2] = Marcas {
            inicio: Some(2),
            fim: Some(3),
            estourou: false,
        };
        let t = tempos_dos_carimbos(&marcas, &[10, 20, 0, 50], 1.0);
        assert_eq!(t.mascaras_ms, None, "estourou a capacidade");
        assert_eq!(t.revelacao_ms, None, "começo zerado não é medida");
    }

    #[test]
    fn sem_medicao_em_curso_nao_ha_escritas() {
        let usou = com_escritas_de_compute(|tw| tw.is_some());
        assert!(!usou);
        assert!(na_etapa(EtapaDaGpu::Mascaras, reservar).is_none());
    }
}
