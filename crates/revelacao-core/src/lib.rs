//! O motor de revelação do VintageLightbox, sem nada em volta.
//!
//! ## Por que um crate próprio
//!
//! O motor morava em `infrastructure`, que também puxa `sqlx`, `rusqlite`,
//! `reqwest`, `rsraw` e `sysinfo` — nenhum deles compila para `wasm32`. Em
//! 2026-09-04 o dono pediu a revelação **no navegador**, no painel do pós-venda
//! do `recordarfotos.com.br`, e o que o navegador precisa é exatamente o que
//! este crate tem: os 46 ajustes ([`Ajustes`]), o WGSL, quem o executa
//! ([`Motor`]), o enquadramento em CPU ([`transformacao`]) e o JPEG ([`jpeg`]).
//!
//! 🔑 **Ele não depende de `domain`.** As duas funções que liam a entidade
//! `Photo` — `ajustes_da_entidade` e `corte_da_entidade` — ficaram em
//! `infrastructure`, que é quem conhece a entidade. Aqui entram pixels e
//! números, e saem pixels.
//!
//! ## As duas entradas do mesmo shader
//!
//! O corpo da matemática é um arquivo só (`shaders/corpo.wgsl`), concatenado a
//! uma entrada em tempo de compilação: `@compute` para o desktop e
//! `@vertex`/`@fragment` para o navegador, onde o WebGL2 não tem compute. Quem
//! prende que as duas revelam o mesmo pixel é
//! `o_fragmento_revela_o_mesmo_pixel_que_o_compute`, em [`motor`].
//!
//! ## O fluxo típico
//!
//! A revelação é **número entra, pixel sai**: um [`Ajustes`] (o painel do
//! Lightroom, campo a campo — a faixa e o neutro de cada um estão na doc do
//! campo), um [`Corte`] para o enquadramento, e o [`Motor`] para aplicar os
//! dois. Os ajustes nascem no neutro, que não é tudo zero:
//!
//! ```rust
//! use revelacao_core::{Ajustes, QUANTIDADE};
//!
//! // O neutro: a foto sai como entrou. 🔑 O contraste é multiplicador (1,0).
//! let mut ajustes = Ajustes::default();
//! assert!(ajustes.sem_efeito());
//! assert_eq!(ajustes.contrast, 1.0);
//!
//! // Dois controles: +½ EV e o Contraste +20 do Lightroom (1 + 20/100).
//! ajustes.exposure = 0.5;
//! ajustes.contrast = 1.2;
//! assert!(!ajustes.sem_efeito());
//!
//! // Gravado por nome (o site guarda assim); o que falta no JSON é o neutro.
//! let lido: Ajustes = serde_json::from_str(r#"{"exposure": 0.5}"#).unwrap();
//! assert_eq!(lido.contrast, 1.0);
//!
//! // Para a GPU (e para o wasm), por posição: QUANTIDADE `f32` na ordem de NOMES.
//! let vetor = ajustes.como_vetor();
//! assert_eq!(vetor.len(), QUANTIDADE);
//! assert_eq!(Ajustes::de_vetor(&vetor), Some(ajustes));
//! ```
//!
//! Quem aplica os ajustes é a GPU — 🚨 não há caminho de CPU para cair (ver
//! [`Motor::abrir`]). O shader revela a foto **inteira**; o enquadramento vem
//! depois, em CPU, por [`transformacao::aplicar`]:
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use revelacao_core::{transformacao, Ajustes, Corte, Motor};
//!
//! let Some(mut motor) = Motor::abrir() else {
//!     return; // sem GPU, sem revelação: quem chama decide (a exportação falha)
//! };
//! let pixels = Arc::new(vec![128u8; 2 * 2 * 4]); // RGBA de 8 bits, 2×2
//! let ajustes = Ajustes { exposure: 0.5, ..Ajustes::default() };
//! let corte = Corte::inteiro();
//! motor.definir_corte(&corte); // as vinhetas medem no quadro que sai
//! let revelada = motor.revelar(&pixels, 2, 2, &ajustes).expect("a GPU respondeu");
//! let pronta = transformacao::aplicar(&revelada, &corte, true);
//! assert_eq!((pronta.width(), pronta.height()), (2, 2));
//! ```
//!
//! Um ajuste tem gabarito em CPU, a vinheta do darktable
//! ([`vinheta_darktable::aplicar`]) — a mesma conta do shader, que serve de
//! régua nos testes:
//!
//! ```rust
//! use revelacao_core::transformacao::Quadro;
//! use revelacao_core::{vinheta_darktable, Ajustes};
//!
//! let ajustes = Ajustes { darktable_vignette_ativo: 1.0, ..Ajustes::default() };
//! // Uma foto cinza 4×4, RGB em 0–255, sem enquadramento.
//! let mut pixels = vec![[128.0f32; 3]; 4 * 4];
//! vinheta_darktable::aplicar(&mut pixels, 4, 4, &ajustes, &Quadro::inteiro(4, 4));
//! // No padrão do módulo (brilho −0,5) o canto escurece e o centro fica.
//! assert!(pixels[0][0] < 128.0);
//! assert_eq!(pixels[2 * 4 + 2], [128.0; 3]);
//! ```

#![deny(missing_docs)]

pub mod ajustes;
pub mod cronometro;
pub mod enquadrador;
pub mod guia;
pub mod jpeg;
pub mod lightroom;
pub mod locais;
mod mascaras;
pub mod motor;
pub mod nivel;
pub mod perspectiva;
pub mod preenchimento;
mod retoque;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod testes_das_mascaras;
pub mod transformacao;
pub mod vinheta_darktable;

pub use ajustes::{Ajustes, QUANTIDADE};
pub use cronometro::TemposDaGpu;
pub use enquadrador::{ImagemDaTela, OrdemDosCanais, PedidoDeTela, RevelacaoParaTela};
pub use locais::ParametrosLocais;
pub use mascaras::MedidasDosLocais;
#[cfg(not(target_arch = "wasm32"))]
pub use motor::adaptadores_da_maquina;
/// 🚨 O flush do contexto WebGL2 — quem tem o canvas registra, e a espera do
/// mapeamento chama. Sem ele a revelação não termina dentro de um Worker; ver
/// `motor::registrar_flush_da_gpu`.
#[cfg(target_arch = "wasm32")]
pub use motor::registrar_flush_da_gpu;
pub use motor::{Entrada, ErroAoAbrir, ErroDeMascara, InfoDoAdaptador, Motor, TemposDoMotor};
/// A álgebra da perspectiva, para quem converte ponto da tela em ponto da
/// foto pela mesma matriz ([`transformacao::Corte::mapa`]).
pub use nalgebra;
pub use transformacao::Corte;
