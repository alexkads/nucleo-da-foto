//! Escreve na saída o `shaders/darktable_constantes.wgsl`.
//!
//! ```text
//! cargo run -q -p revelacao-core --example darktable-constantes-wgsl \
//!     > crates/revelacao-core/src/shaders/darktable_constantes.wgsl
//! ```
//!
//! O teste `as_constantes_do_wgsl_estao_em_dia` falha quando o arquivo não é
//! exatamente o que este programa escreve.
fn main() {
    print!("{}", revelacao_core::darktable::constantes_wgsl());
}
