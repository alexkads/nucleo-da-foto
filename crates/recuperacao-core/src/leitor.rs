//! Leitura em janelas alinhadas.
//!
//! 🚨 **O dispositivo bruto só aceita leitura alinhada ao setor.** O
//! `\\.\PhysicalDriveN` do Windows e o `/dev/rdiskN` do macOS recusam ler 12
//! bytes no deslocamento 8: a posição e o tamanho têm de ser múltiplos do
//! setor. Os formatos, porém, precisam exatamente disso (um cabeçalho TIFF, um
//! campo de 4 bytes no meio de um IFD). O [`Leitor`] é quem concilia: pede ao
//! dispositivo sempre uma janela inteira, alinhada, e entrega dela o pedaço que
//! foi pedido.
//!
//! Uma janela de 1 MiB guardada também poupa o dispositivo: andar pelos IFDs
//! de um RAW são dezenas de leituras pequenas, quase todas no primeiro mega.

use std::io::{self, Read, Seek, SeekFrom};

/// O tamanho de cada leitura no dispositivo. Múltiplo de 4096, que cobre
/// setores de 512 e de 4K.
pub const JANELA: u64 = 1 << 20;

pub struct Leitor<R> {
    fonte: R,
    total: Option<u64>,
    /// Onde começa a janela guardada, e o que veio nela (pode ser menor que
    /// [`JANELA`] no fim do dispositivo).
    inicio_da_janela: u64,
    janela: Vec<u8>,
    valida: bool,
}

impl<R: Read + Seek> Leitor<R> {
    /// `total` é o tamanho do dispositivo, quando se sabe. Com ele, nenhuma
    /// leitura passa do fim (o Windows responde erro, e não "zero bytes", a quem
    /// lê além do último setor). Sem ele, o fim é o primeiro `read` vazio.
    pub fn novo(fonte: R, total: Option<u64>) -> Self {
        Self {
            fonte,
            total,
            inicio_da_janela: 0,
            janela: Vec::new(),
            valida: false,
        }
    }

    pub fn total(&self) -> Option<u64> {
        self.total
    }

    /// Lê a partir de `pos` o que couber em `buf`. Devolve quantos bytes vieram:
    /// menos que `buf.len()` só no fim do dispositivo.
    pub fn ler_em(&mut self, pos: u64, buf: &mut [u8]) -> io::Result<usize> {
        let mut lidos = 0;
        while lidos < buf.len() {
            let atual = pos + lidos as u64;
            self.carregar(atual)?;
            let dentro = (atual - self.inicio_da_janela) as usize;
            if dentro >= self.janela.len() {
                break;
            }
            let n = (self.janela.len() - dentro).min(buf.len() - lidos);
            buf[lidos..lidos + n].copy_from_slice(&self.janela[dentro..dentro + n]);
            lidos += n;
        }
        Ok(lidos)
    }

    /// Lê exatamente `buf.len()` bytes, ou `None` se o dispositivo acabar antes.
    pub fn ler_exato(&mut self, pos: u64, buf: &mut [u8]) -> io::Result<Option<()>> {
        Ok((self.ler_em(pos, buf)? == buf.len()).then_some(()))
    }

    pub fn u8_em(&mut self, pos: u64) -> io::Result<Option<u8>> {
        let mut b = [0u8; 1];
        Ok(self.ler_exato(pos, &mut b)?.map(|_| b[0]))
    }

    pub fn u16_be(&mut self, pos: u64) -> io::Result<Option<u16>> {
        let mut b = [0u8; 2];
        Ok(self.ler_exato(pos, &mut b)?.map(|_| u16::from_be_bytes(b)))
    }

    pub fn u32_be(&mut self, pos: u64) -> io::Result<Option<u32>> {
        let mut b = [0u8; 4];
        Ok(self.ler_exato(pos, &mut b)?.map(|_| u32::from_be_bytes(b)))
    }

    pub fn u64_be(&mut self, pos: u64) -> io::Result<Option<u64>> {
        let mut b = [0u8; 8];
        Ok(self.ler_exato(pos, &mut b)?.map(|_| u64::from_be_bytes(b)))
    }

    /// Garante que a janela guardada cobre `pos`.
    fn carregar(&mut self, pos: u64) -> io::Result<()> {
        let inicio = pos - pos % JANELA;
        if self.valida && self.inicio_da_janela == inicio {
            return Ok(());
        }
        let mut quanto = JANELA;
        if let Some(total) = self.total {
            quanto = quanto.min(total.saturating_sub(inicio));
        }
        self.janela.resize(quanto as usize, 0);
        self.valida = false;
        self.fonte.seek(SeekFrom::Start(inicio))?;
        let mut cheio = 0;
        while cheio < self.janela.len() {
            match self.fonte.read(&mut self.janela[cheio..]) {
                Ok(0) => break,
                Ok(n) => cheio += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        self.janela.truncate(cheio);
        self.inicio_da_janela = inicio;
        self.valida = true;
        Ok(())
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::io::Cursor;

    /// Um `Read + Seek` que recusa o que um dispositivo bruto recusa.
    struct SoAlinhado(Cursor<Vec<u8>>);

    impl Read for SoAlinhado {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if !self.0.position().is_multiple_of(512) || !buf.len().is_multiple_of(512) {
                return Err(io::Error::other("leitura desalinhada"));
            }
            self.0.read(buf)
        }
    }

    impl Seek for SoAlinhado {
        fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
            self.0.seek(pos)
        }
    }

    #[test]
    fn le_pedaco_pequeno_no_meio_sem_ler_desalinhado() {
        let dados: Vec<u8> = (0..3 * JANELA as usize).map(|i| (i % 251) as u8).collect();
        let total = dados.len() as u64;
        let mut leitor = Leitor::novo(SoAlinhado(Cursor::new(dados.clone())), Some(total));

        let mut buf = [0u8; 12];
        leitor.ler_exato(8, &mut buf).unwrap().unwrap();
        assert_eq!(&buf, &dados[8..20]);

        // Atravessa a fronteira de duas janelas.
        let pos = JANELA - 5;
        let mut buf = [0u8; 10];
        leitor.ler_exato(pos, &mut buf).unwrap().unwrap();
        assert_eq!(&buf[..], &dados[pos as usize..pos as usize + 10]);
    }

    #[test]
    fn no_fim_devolve_so_o_que_existe() {
        let mut leitor = Leitor::novo(Cursor::new(vec![7u8; 1000]), Some(1000));
        let mut buf = [0u8; 100];
        assert_eq!(leitor.ler_em(950, &mut buf).unwrap(), 50);
        assert_eq!(leitor.ler_exato(950, &mut buf).unwrap(), None);
        assert_eq!(leitor.ler_em(5000, &mut buf).unwrap(), 0);
    }

    #[test]
    fn sem_total_o_fim_e_o_read_vazio() {
        let mut leitor = Leitor::novo(Cursor::new(vec![1u8; 700]), None);
        let mut buf = [0u8; 1024];
        assert_eq!(leitor.ler_em(0, &mut buf).unwrap(), 700);
    }
}
