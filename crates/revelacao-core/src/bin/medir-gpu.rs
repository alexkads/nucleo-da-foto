//! A comparação das APIs gráficas fora do app — para levar a uma máquina que
//! não o tem. Dentro do app ela é **Desempenho → Comparar APIs gráficas**, que
//! salva no banco sozinha. O que se mede e por quê: o módulo
//! [`revelacao_core::comparacao_de_apis`].
//!
//! ```bash
//! cargo run --release -p revelacao-core --bin medir-gpu                # foto sintética de 24 MP
//! cargo run --release -p revelacao-core --bin medir-gpu -- foto.jpg    # uma foto de verdade
//! ```
//!
//! 🔑 **Mora no `revelacao-core`, e não no `ui-gpui`, para sair como `.exe`
//! do Mac.** O GPUI do Windows compila os shaders dele com o `fxc` da
//! Microsoft no build, e isso só roda no Windows; este binário não precisa do
//! GPUI, e assim `cargo build --target x86_64-pc-windows-gnu` o entrega pronto
//! para levar ao balcão. O instalador compila só `-p ui-gpui --bin ui-gpui`:
//! nada daqui entra no app.
//!
//! 📄 **O resultado fica num arquivo**, `medir-gpu-<data>.txt`, ao lado do
//! executável (ou na pasta atual, se lá não der para gravar). A janela espera
//! um Enter antes de fechar, para o `.exe` aberto com dois cliques não sumir
//! com o resultado.
//!
//! ⚠️ **Só vale fora do `debug`**, e com o app fechado: duas coisas na mesma
//! GPU dividem o tempo dela.

use revelacao_core::comparacao_de_apis::{comparar, foto_sintetica};

fn main() {
    if cfg!(debug_assertions) {
        eprintln!("⚠️  perfil `debug`: os números não valem. Use --release.\n");
    }
    let (nome, foto) = match std::env::args().nth(1) {
        Some(caminho) => match image::open(&caminho) {
            Ok(foto) => (caminho, foto.into_rgba8()),
            Err(erro) => {
                eprintln!("⚠️  {caminho} não abriu ({erro}); usando a sintética\n");
                ("sintética".to_string(), foto_sintetica())
            }
        },
        None => ("sintética".to_string(), foto_sintetica()),
    };

    let resultado = comparar(&foto, &nome, |passo| eprintln!("… {passo}"), |_| {});
    let texto = format!(
        "medir-gpu · VintageLightbox {} · {} {} · {}\n{}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        agora(),
        resultado.texto()
    );
    println!("\n{texto}");
    gravar_relatorio(&texto);
}

/// Data e hora em UTC, sem crate: só para nomear e datar o relatório.
fn agora() -> String {
    let segundos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (dias, resto) = ((segundos / 86_400) as i64, segundos % 86_400);
    // Dias desde 1970 → data civil (Howard Hinnant, "chrono-compatible
    // low-level date algorithms").
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = doy - (153 * mp + 2) / 5 + 1;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 };
    let ano = yoe + era * 400 + i64::from(mes <= 2);
    format!(
        "{ano:04}-{mes:02}-{dia:02} {:02}h{:02} UTC",
        resto / 3_600,
        resto % 3_600 / 60
    )
}

/// Grava o relatório ao lado do executável e espera um Enter.
fn gravar_relatorio(texto: &str) {
    let nome = format!("medir-gpu-{}.txt", agora().replace([' ', ':'], "-"));
    let pastas = [
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|p| p.to_path_buf())),
        std::env::current_dir().ok(),
    ];
    let gravado = pastas
        .into_iter()
        .flatten()
        .map(|pasta| pasta.join(&nome))
        .find(|caminho| std::fs::write(caminho, texto).is_ok());
    match gravado {
        Some(caminho) => println!("\n📄 Relatório gravado em {}", caminho.display()),
        None => println!("\n⚠️  Não consegui gravar o relatório; copie o texto acima."),
    }
    if cfg!(target_os = "windows") {
        println!("Enter para fechar.");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}
