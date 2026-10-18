//! Recuperar as fotos de um cartão formatado.
//!
//! # Por que dá
//!
//! A formatação rápida, que é a que a câmera e o sistema fazem, grava uma
//! tabela de arquivos nova e vazia (FAT32 ou exFAT) e **não toca nos dados**.
//! As fotos continuam nos mesmos setores até alguém gravar por cima. Por isso a
//! primeira regra, que vale para quem chama este crate: **nada é gravado no
//! cartão**, e o destino das fotos recuperadas fica sempre fora dele.
//!
//! # Como
//!
//! Sem tabela, o nome e o tamanho do arquivo se perderam. O que sobrou foram os
//! bytes. [`varredura::varrer`] lê o cartão do começo ao fim e, em cada início
//! de setor (é ali que a FAT e a exFAT começam arquivos), testa a assinatura de
//! cada formato. Quando uma bate, [`formatos`] lê a estrutura do arquivo para
//! descobrir **onde ele termina**: os segmentos do JPEG, os IFDs do TIFF (CR2,
//! NEF, ARW, DNG, ORF, RW2), as caixas do CR3, o cabeçalho do RAF.
//!
//! 🔑 **Medir, e não adivinhar.** Cortar "até a próxima assinatura" devolveria
//! arquivos com lixo no fim, ou cortados. Aqui o fim vem da estrutura, e um
//! arquivo cuja estrutura não fecha é descartado. Nenhuma foto sai pela metade.
//!
//! # O limite
//!
//! ⚠️ **A foto tem de estar contígua no cartão.** É o caso normal: a câmera
//! grava em sequência num cartão recém-formatado, e a foto ocupa clusters
//! seguidos. Uma foto fragmentada (cartão cheio, apagado e regravado muitas
//! vezes pela câmera) é descartada, porque juntar pedaços sem a tabela é
//! adivinhação.
//!
//! # O que ele **não** faz
//!
//! Não abre dispositivo, não pede privilégio, não grava arquivo. Quem faz isso
//! é o `infrastructure` (`recuperacao.rs`). Aqui a entrada é um `Read + Seek`,
//! e é o que permite testar com um `Cursor` de bytes montado no teste.
//!
//! # Exemplo
//!
//! Um "cartão" de 8 KiB em memória, com lixo, um JPEG no setor 2 e zeros
//! depois — a varredura acha a foto e a copia inteira, nem um byte a mais:
//!
//! ```rust
//! use std::io::Cursor;
//! use recuperacao_core::{copiar, varrer, Formato, Leitor};
//!
//! // Um JPEG mínimo com a estrutura que a medida confere: SOI, DQT, SOF0,
//! // SOS, dados comprimidos e EOI — com mais de 1 KiB, para não passar por
//! // miniatura solta.
//! let mut jpeg = vec![0xFF, 0xD8];
//! jpeg.extend([0xFF, 0xDB, 0x00, 0x43]);
//! jpeg.extend([1u8; 65]);
//! jpeg.extend([0xFF, 0xC0, 0x00, 0x0B, 8, 0, 16, 0, 16, 1, 1, 0x11, 0]);
//! jpeg.extend([0xFF, 0xDA, 0x00, 0x08, 1, 1, 0, 0, 63, 0]);
//! jpeg.extend([0x55u8; 2000]);
//! jpeg.extend([0xFF, 0xD9]);
//!
//! let mut cartao = vec![0xA5u8; 1024];
//! cartao.extend(&jpeg);
//! cartao.resize(8192, 0);
//!
//! let total = cartao.len() as u64;
//! let mut leitor = Leitor::novo(Cursor::new(cartao), Some(total));
//! let mut copias = Vec::new();
//! let resumo = varrer(&mut leitor, &|| false, &mut |_| {}, &mut |leitor, achado| {
//!     assert_eq!((achado.inicio, achado.formato), (1024, Formato::Jpeg));
//!     let mut destino = Vec::new();
//!     copiar(leitor, achado, &mut destino)?;
//!     copias.push(destino);
//!     Ok(())
//! })
//! .unwrap();
//!
//! assert_eq!(resumo.achadas, 1);
//! assert_eq!(copias, vec![jpeg]);
//! ```

#![deny(missing_docs)]

pub mod formatos;
pub mod leitor;
pub mod varredura;

pub use formatos::Formato;
pub use leitor::Leitor;
pub use varredura::{copiar, varrer, Achado, Andamento, Resumo};
