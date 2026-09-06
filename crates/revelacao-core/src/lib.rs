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

pub mod ajustes;
pub mod jpeg;
pub mod motor;
pub mod transformacao;

pub use ajustes::{Ajustes, QUANTIDADE};
pub use motor::{Entrada, Motor};
pub use transformacao::Corte;
