//! A bancada: o núcleo desta pasta contra a última tag, caso a caso, com a
//! mesma foto.
//!
//! Cada caso roda o caminho **antes** (o crate da tag, `*-antes` no
//! `Cargo.toml`) e o **depois** (o crate desta pasta) alternados, mede a
//! mediana de cada um e confere se a saída é a mesma byte a byte — a regra da
//! casa: desempenho que muda a foto não é desempenho, é quebra.
//!
//! A janela (`janela`) e o modo de terminal (`--sem-janela`) só mostram o que
//! sai daqui.

pub mod casos;
pub mod foto;
pub mod janela;
pub mod medir;

pub use casos::{casos_para, Caso, Medida};
pub use foto::Foto;
