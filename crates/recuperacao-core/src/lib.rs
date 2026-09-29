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

pub mod formatos;
pub mod leitor;
pub mod varredura;

pub use formatos::Formato;
pub use leitor::Leitor;
pub use varredura::{copiar, varrer, Achado, Andamento, Resumo};
