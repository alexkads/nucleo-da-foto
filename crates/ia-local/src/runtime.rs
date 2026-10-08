//! 🪟 O ONNX Runtime do Windows com MSYS2 (`x86_64-pc-windows-gnu`).
//!
//! O `ort` só traz o runtime pronto para o Windows com MSVC — no `-gnu` a
//! compilação morria em `ort-sys: no prebuilt binaries` (a 0.1.106 a 0.1.120
//! não instalaram em balcão Windows). Ali o `ort` é ligado com `load-dynamic`:
//! o `onnxruntime.dll` oficial da Microsoft é baixado **na primeira vez que a
//! IA roda** (como um modelo: hash conferido, pasta de dados do app) e
//! carregado pelo caminho completo — nunca o `onnxruntime.dll` do System32,
//! que o Windows 11 traz em versão mais velha.
//!
//! O DLL da Microsoft depende do Visual C++ Redistributable (x64). Sem ele, a
//! IA avisa e o resto do app segue igual.
//!
//! Nos outros sistemas o runtime vem estático no executável, e tudo aqui é
//! um "pronto" imediato.

use std::sync::atomic::AtomicBool;

use crate::modelos::ErroDeModelo;

/// Garante o runtime carregado (baixando-o, se faltar). Antes de qualquer
/// sessão. Sem progresso: para mostrar progresso, [`baixar`] antes.
pub fn garantir() -> Result<(), String> {
    #[cfg(all(windows, target_env = "gnu"))]
    {
        gnu::garantir()
    }
    #[cfg(not(all(windows, target_env = "gnu")))]
    {
        Ok(())
    }
}

/// Já dá para usar sem baixar nada? (Barato: não baixa nem carrega.)
pub fn presente() -> bool {
    #[cfg(all(windows, target_env = "gnu"))]
    {
        gnu::dll().is_file()
    }
    #[cfg(not(all(windows, target_env = "gnu")))]
    {
        true
    }
}

/// Baixa o runtime, se esta compilação precisar dele e ele faltar, com o
/// progresso `(recebidos, total)`. Nos outros sistemas não faz nada.
pub fn baixar(progresso: &dyn Fn(u64, u64), cancelado: &AtomicBool) -> Result<(), ErroDeModelo> {
    #[cfg(all(windows, target_env = "gnu"))]
    {
        gnu::baixar(progresso, cancelado)
    }
    #[cfg(not(all(windows, target_env = "gnu")))]
    {
        let _ = (progresso, cancelado);
        Ok(())
    }
}

#[cfg(all(windows, target_env = "gnu"))]
mod gnu {
    use std::io::Read;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    use sha2::{Digest, Sha256};

    use crate::modelos::{self, ErroDeModelo, Modelo};

    /// O pacote oficial (só CPU) da versão que o `ort` =2.0.0-rc.13 usa.
    const PACOTE: Modelo = Modelo {
        id: "onnxruntime-win-x64",
        nome: "ONNX Runtime (Windows x64)",
        arquivo: "onnxruntime-win-x64-1.28.0.zip",
        versao: "microsoft/onnxruntime v1.28.0",
        url: "https://github.com/microsoft/onnxruntime/releases/download/v1.28.0/onnxruntime-win-x64-1.28.0.zip",
        bytes: 78_796_801,
        sha256: "abef733dacbe2f571547a7150b479b5cb9cc0df22f96c24983a42cadb1b4f8bc",
        licenca: "MIT",
        origem: "https://github.com/microsoft/onnxruntime",
    };
    const DENTRO_DO_PACOTE: &str = "onnxruntime-win-x64-1.28.0/lib/onnxruntime.dll";
    const SHA_DO_DLL: &str = "18370c375f07357fa5874344a9d9ac17e6b6fe1eb18b1dd209d79483b4470257";

    fn pasta() -> PathBuf {
        modelos::pasta_padrao().join("onnxruntime-1.28.0")
    }

    pub fn dll() -> PathBuf {
        pasta().join("onnxruntime.dll")
    }

    /// Carregado nesta execução (o `ort` só aceita um `init_from`).
    static CARREGADO: Mutex<bool> = Mutex::new(false);

    pub fn garantir() -> Result<(), String> {
        let mut carregado = CARREGADO.lock().unwrap_or_else(|e| e.into_inner());
        if *carregado {
            return Ok(());
        }
        baixar(&|_, _| {}, &AtomicBool::new(false))
            .map_err(|e| format!("ONNX Runtime: {}", e.mensagem()))?;
        ort::init_from(dll())
            .map_err(|e| {
                format!(
                    "o ONNX Runtime não carregou ({e}) — instale o Microsoft Visual C++ \
                     Redistributable (x64) e tente de novo"
                )
            })?
            .commit();
        *carregado = true;
        Ok(())
    }

    pub fn baixar(
        progresso: &dyn Fn(u64, u64),
        cancelado: &AtomicBool,
    ) -> Result<(), ErroDeModelo> {
        if dll().is_file() {
            return Ok(());
        }
        let pasta = pasta();
        let zip = modelos::baixar(&pasta, &PACOTE, PACOTE.url, progresso, cancelado)?;
        let extraido = extrair(&zip).map_err(ErroDeModelo::Disco);
        let _ = modelos::remover(&pasta, &PACOTE);
        extraido
    }

    /// Tira o DLL do pacote para um `.parte`, confere o hash e só então o
    /// põe no lugar.
    fn extrair(zip: &std::path::Path) -> Result<(), String> {
        let arquivo = std::fs::File::open(zip).map_err(|e| e.to_string())?;
        let mut pacote = zip::ZipArchive::new(arquivo).map_err(|e| e.to_string())?;
        let mut entrada = pacote
            .by_name(DENTRO_DO_PACOTE)
            .map_err(|e| format!("{DENTRO_DO_PACOTE}: {e}"))?;
        let mut bytes = Vec::new();
        entrada.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        let obtido: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if obtido != SHA_DO_DLL {
            return Err("o onnxruntime.dll do pacote não confere com o publicado".into());
        }
        let parte = dll().with_extension("dll.parte");
        std::fs::write(&parte, &bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&parte, dll()).map_err(|e| e.to_string())
    }
}
