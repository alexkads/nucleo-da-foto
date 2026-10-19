//! O contrato com o JavaScript, **sem GPU e sem canvas**.
//!
//! Tudo o que o site manda e recebe e que não precisa de `wgpu` nem de
//! `web-sys` mora aqui: a forma dos vetores (corte, ajustes), as conferências
//! de tamanho, as matrizes do enquadramento, os codificadores e o decodificador
//! de arquivo. As funções `#[wasm_bindgen]` de `web.rs` são cascas finas sobre
//! estas, que só trocam o `String` do erro por `JsValue`.
//!
//! 🔑 **Por que separado**: `web.rs` só compila em `wasm32`, e o que mora lá não
//! roda em `cargo test`. Este módulo compila em todo alvo, e os testes de baixo
//! provam em nativo o que o site recebe — a ordem dos números, o cabeçalho do
//! RGBA decodificado, as mensagens de erro que a tela mostra.

use revelacao_core::{Ajustes, Corte, ParametrosLocais};

/// A revelação local vinda do JavaScript, como o desktop a grava
/// (`ParametrosLocais::em_json`). `None` ou texto vazio é "sem máscara".
///
/// 🚨 **Versão mais nova ou JSON inválido é erro**, e não revelação vazia: revelar
/// sem as máscaras que a foto tem seria entregar outra foto, calado.
pub(crate) fn locais_de_json(json: Option<&str>) -> Result<ParametrosLocais, String> {
    match json.map(str::trim).filter(|j| !j.is_empty()) {
        None => Ok(ParametrosLocais::default()),
        Some(j) => ParametrosLocais::de_json(j).map_err(|e| e.to_string()),
    }
}

/// Quantos números o enquadramento carrega **sem** perspectiva.
pub(crate) const CAMPOS_DO_CORTE: usize = 8;
/// …e com ela: os oito, mais o vetor de rotação resolvido (3, em graus), o
/// foco e o ajuste manual vertical e horizontal (graus).
pub(crate) const CAMPOS_COM_PERSPECTIVA: usize = CAMPOS_DO_CORTE + 6;

/// O enquadramento vindo do JavaScript: `[x, y, largura, altura, giro_90,
/// angulo, espelho_h, espelho_v]`, na ordem de `Corte::novo` — e, quando a foto
/// tem perspectiva guiada, `[…, rx, ry, rz, foco, vertical, horizontal]`.
///
/// Os dois espelhos viajam como 0 ou 1 e o giro como inteiro num `f32`: um
/// vetor só, do mesmo tipo do dos ajustes, é o que o `wasm-bindgen` passa sem
/// custo. `Corte::novo` limita tudo, então valor fora da faixa entra corrigido
/// em vez de virar erro — o mesmo tratamento que o desktop dá.
///
/// 🔑 **Oito continua valendo**: o site de hoje manda oito, e a foto sem
/// perspectiva é exatamente a de antes. Um tamanho que não é nenhum dos dois é
/// erro — revelar sem a correção que a foto tem seria entregar outra foto.
pub(crate) fn corte_de_vetor(v: &[f32]) -> Result<Corte, String> {
    if v.len() != CAMPOS_DO_CORTE && v.len() != CAMPOS_COM_PERSPECTIVA {
        return Err(format!(
            "esperava {CAMPOS_DO_CORTE} ou {CAMPOS_COM_PERSPECTIVA} campos de corte, recebi {}",
            v.len()
        ));
    }
    let corte = Corte::novo(
        v[0],
        v[1],
        v[2],
        v[3],
        v[4] as i32,
        v[5],
        v[6] != 0.0,
        v[7] != 0.0,
    );
    Ok(match v.get(CAMPOS_DO_CORTE..CAMPOS_COM_PERSPECTIVA) {
        Some(&[rx, ry, rz, foco, vertical, horizontal]) => {
            corte.com_perspectiva(revelacao_core::perspectiva::Perspectiva::nova(
                [rx, ry, rz],
                foco,
                vertical,
                horizontal,
            ))
        }
        _ => corte,
    })
}

/// Os ajustes vindos do JavaScript, na ordem do `uniform`. O tamanho é a única
/// conferência possível — ver `Ajustes::de_vetor`.
pub(crate) fn ajustes_de_vetor(v: &[f32]) -> Result<Ajustes, String> {
    Ajustes::de_vetor(v).ok_or_else(|| {
        format!(
            "esperava {} ajustes, recebi {}",
            revelacao_core::QUANTIDADE,
            v.len()
        )
    })
}

/// Uma matriz 3×3 do `nalgebra` em nove `f32`, por linhas — a ordem que o
/// `matrix3d` do editor lê.
fn por_linhas(m: &revelacao_core::nalgebra::Matrix3<f64>) -> Vec<f32> {
    (0..3)
        .flat_map(|i| (0..3).map(move |j| (i, j)))
        .map(|(i, j)| m[(i, j)] as f32)
        .collect()
}

/// A perspectiva sozinha, em pixels do espaço orientado, por linhas — ver
/// `homografia_no_espaco` em `web.rs`. Sem perspectiva, a identidade.
pub(crate) fn homografia_no_espaco(
    corte: &[f32],
    largura: u32,
    altura: u32,
) -> Result<Vec<f32>, String> {
    let h = corte_de_vetor(corte)?.homografia_em_pixels(largura, altura);
    Ok(por_linhas(&h))
}

/// A matriz do enquadramento, do quadro que sai à foto de pé, normalizada para
/// o canto de baixo valer 1 — ver `matriz_do_enquadramento` em `web.rs`.
pub(crate) fn matriz_do_enquadramento(
    corte: &[f32],
    largura: u32,
    altura: u32,
    recortar: bool,
) -> Result<Vec<f32>, String> {
    let m = corte_de_vetor(corte)?.mapa(largura, altura, recortar);
    let m = m / m[(2, 2)];
    Ok(por_linhas(&m))
}

/// O enquadramento neutro: a foto inteira, sem giro, ângulo ou espelho.
pub(crate) fn corte_inteiro() -> Vec<f32> {
    vec![0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0]
}

/// A geometria do enquadramento: largura e altura do espaço girado, o
/// retângulo (`x, y, w, h`) nesse espaço e as dimensões do arquivo que vai
/// sair — ver `enquadramento` em `web.rs`.
pub(crate) fn enquadramento(corte: &[f32], largura: u32, altura: u32) -> Result<Vec<f32>, String> {
    let corte = corte_de_vetor(corte)?;
    let (lg, ag) = corte.dimensoes_giradas(largura, altura);
    let (x, y, w, h) = corte.retangulo(lg, ag);
    let (sw, sh) = corte.dimensoes_de_saida(largura, altura);
    Ok(vec![
        lg as f32, ag as f32, x as f32, y as f32, w as f32, h as f32, sw as f32, sh as f32,
    ])
}

/// Os pixels RGBA do JavaScript como imagem, conferido o tamanho.
///
/// 🚨 A conta do tamanho é com `checked_mul`: no `wasm32` o `usize` tem 32
/// bits, e 40 000 × 30 000 × 4 já não cabe.
fn imagem_rgba(largura: u32, altura: u32, rgba: &[u8]) -> Result<image::DynamicImage, String> {
    conferir_tamanho(largura, altura, rgba)?;
    let buffer = image::RgbaImage::from_raw(largura, altura, rgba.to_vec())
        .ok_or_else(|| "os pixels não formam uma imagem".to_string())?;
    Ok(image::DynamicImage::ImageRgba8(buffer))
}

/// Os bytes têm exatamente `largura × altura × 4`?
fn conferir_tamanho(largura: u32, altura: u32, rgba: &[u8]) -> Result<(), String> {
    let esperado = (largura as usize)
        .checked_mul(altura as usize)
        .and_then(|p| p.checked_mul(4))
        .ok_or_else(|| format!("{largura}×{altura} não cabe na memória"))?;
    if rgba.len() != esperado {
        return Err(format!(
            "os bytes não batem com largura × altura × 4: {} para {esperado}",
            rgba.len()
        ));
    }
    Ok(())
}

/// O JPEG do codificador de sempre (`foto_codec`), com a qualidade presa em
/// 1–100, **direto dos pixels emprestados**.
///
/// 🔑 Sem montar uma imagem dona deles: eram 96 MB copiados a 24 MP, no
/// celular, só para o codificador ler e jogar fora. O arquivo é o mesmo byte a
/// byte (`foto_codec::codificar_rgba`).
fn jpeg(largura: u32, altura: u32, rgba: &[u8], qualidade: u8) -> Result<Vec<u8>, String> {
    conferir_tamanho(largura, altura, rgba)?;
    foto_codec::codificar_rgba(rgba, largura, altura, qualidade.clamp(1, 100))
        .map_err(|e| format!("o JPEG não codificou: {e}"))
}

/// Codifica pixels RGBA em JPEG, sem GPU e sem canvas — ver `comprimir_jpeg`
/// em `web.rs`.
pub(crate) fn comprimir_jpeg(
    largura: u32,
    altura: u32,
    rgba: &[u8],
    qualidade: u8,
) -> Result<Vec<u8>, String> {
    jpeg(largura, altura, rgba, qualidade)
}

/// Os formatos que este wasm sabe **abrir**, em JSON.
///
/// 🚨 Tem de bater com as features do `image` no Cargo.toml — o teste
/// `todo_formato_de_entrada_anunciado_abre` prova isso.
pub(crate) fn formatos_de_entrada() -> String {
    serde_json::to_string(&["jpeg", "png", "tiff", "webp", "bmp", "gif"]).unwrap_or_default()
}

/// Os formatos que este wasm sabe **gravar**, em JSON.
pub(crate) fn formatos_de_saida() -> String {
    serde_json::to_string(&["jpeg", "png", "tiff", "webp"]).unwrap_or_default()
}

/// Decodifica um arquivo de imagem para `[largura u32 LE, altura u32 LE,
/// ...rgba]`, de pé e em sRGB — ver `decodificar_imagem` em `web.rs`.
pub(crate) fn decodificar_imagem(bytes: &[u8]) -> Result<Vec<u8>, String> {
    // De pé: a mesma regra do desktop e do `createImageBitmap` com
    // `imageOrientation: "from-image"` — `foto_codec::orientacao`.
    let imagem = foto_codec::orientacao::decodificar_de_pe(bytes).map_err(|e| {
        format!("este arquivo não abriu ({e}). Formatos aceitos: JPEG, PNG, TIFF, WebP, BMP e GIF")
    })?;
    let (largura, altura) = (imagem.width(), imagem.height());

    // 🔑 **O RGBA é montado uma vez, já no lugar.** Antes vinha `to_rgba8()`
    // (a foto inteira) e depois a cópia dela para depois do cabeçalho: dois
    // buffers de 96 MB a 24 MP, no celular. O JPEG abre em RGB, que é expandido
    // aqui direto, com o alfa em 255 — o mesmo do `to_rgba8`.
    let pixels = largura as usize * altura as usize;
    let mut saida = Vec::with_capacity(8 + pixels * 4);
    saida.extend_from_slice(&largura.to_le_bytes());
    saida.extend_from_slice(&altura.to_le_bytes());
    match imagem {
        image::DynamicImage::ImageRgb8(rgb) => {
            for p in rgb.as_raw().as_chunks::<3>().0 {
                saida.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        outra => saida.extend_from_slice(outra.into_rgba8().as_raw()),
    }
    Ok(saida)
}

/// O espaço de cor que o arquivo declara: 0 sRGB, 1 Adobe RGB, 2 Display P3.
pub(crate) fn espaco_de_cor_declarado(bytes: &[u8]) -> u8 {
    foto_codec::espaco_de_cor::declarado_no_arquivo(bytes).codigo()
}

/// Converte RGBA do espaço `espaco` (o código de `espaco_de_cor_declarado`)
/// para sRGB, no lugar. Em sRGB — e em código desconhecido — não toca em nada.
pub(crate) fn converter_para_srgb(rgba: &mut [u8], espaco: u8) {
    use foto_codec::espaco_de_cor::{rgba_para_srgb, EspacoDeCor};
    rgba_para_srgb(rgba, EspacoDeCor::do_codigo(espaco));
}

/// Codifica RGBA no formato pedido (`jpeg`, `png`, `tiff` ou `webp`) — ver
/// `codificar_imagem` em `web.rs`.
pub(crate) fn codificar_imagem(
    largura: u32,
    altura: u32,
    rgba: &[u8],
    formato: &str,
    qualidade: u8,
) -> Result<Vec<u8>, String> {
    if formato == "jpeg" {
        return jpeg(largura, altura, rgba, qualidade);
    }
    let imagem = imagem_rgba(largura, altura, rgba)?;

    let saida_formato = match formato {
        "png" => image::ImageFormat::Png,
        "tiff" => image::ImageFormat::Tiff,
        "webp" => image::ImageFormat::WebP,
        outro => return Err(format!("formato desconhecido: {outro}")),
    };
    // O TIFF do `image` não aceita alfa em todo caminho, e o WebP sem perda
    // aceita: RGBA para os dois, RGB só quando o formato exigir.
    let mut bytes = std::io::Cursor::new(Vec::new());
    imagem
        .write_to(&mut bytes, saida_formato)
        .map_err(|e| format!("o arquivo não codificou: {e}"))?;
    Ok(bytes.into_inner())
}

/// Os valores do neutro, na ordem do `uniform`.
pub(crate) fn ajustes_padrao() -> Vec<f32> {
    Ajustes::default().como_vetor().to_vec()
}

/// Os nomes dos ajustes, na ordem do `uniform`, como JSON.
pub(crate) fn nomes_dos_ajustes() -> String {
    serde_json::to_string(&Ajustes::NOMES[..]).expect("os nomes viram JSON")
}

/// A razão entre esta imagem e a foto original, pelo maior lado.
///
/// 🚨 **Os raios do darktable são em pixels da foto original.** O `shadows and
/// highlights` do estilo borra com raio de 100 px: numa exportação reduzida ao
/// limite de textura, ou na cópia de 2048 px, isso é outro pedaço da foto, e o
/// estilo sairia diferente do que o darktable dá. Sem `lado_original` (ou com
/// um valor sem sentido), vale 1 — a imagem é a original.
pub(crate) fn escala_do_original(largura: u32, altura: u32, lado_original: Option<u32>) -> f32 {
    match lado_original {
        Some(lado) if lado > 0 => (largura.max(altura) as f32 / lado as f32).min(1.0),
        _ => 1.0,
    }
}

/// `largura × altura × 4` em 64 bits: no wasm32 o `usize` tem 32, e
/// 32768 × 32768 × 4 dava a volta até 0 — um buffer vazio passaria na conta.
fn bytes_rgba(largura: u32, altura: u32) -> u64 {
    largura as u64 * altura as u64 * 4
}

/// A conferência da cópia de trabalho antes de ela ir para a GPU
/// (`Motor::carregar`): tem tamanho, os bytes batem e cabe numa textura.
pub(crate) fn conferir_copia_de_trabalho(
    largura: u32,
    altura: u32,
    bytes: usize,
    limite: u32,
) -> Result<(), String> {
    if largura == 0 || altura == 0 {
        return Err("imagem sem tamanho".to_string());
    }
    if bytes as u64 != bytes_rgba(largura, altura) {
        return Err(format!(
            "esperava {} bytes RGBA para {largura}×{altura}, recebi {bytes}",
            bytes_rgba(largura, altura),
        ));
    }
    conferir_limite(largura, altura, limite)
}

/// O lado maior que o dispositivo aceita numa textura.
fn conferir_limite(largura: u32, altura: u32, limite: u32) -> Result<(), String> {
    if largura > limite || altura > limite {
        return Err(format!(
            "{largura}×{altura} passa do limite de textura deste dispositivo ({limite} px)"
        ));
    }
    Ok(())
}

/// O que a exportação precisa, lido e conferido **antes** da GPU, na ordem em
/// que o site recebe os erros: corte, locais, ajustes, bytes e limite.
#[allow(clippy::too_many_arguments)]
pub(crate) fn preparar_exportacao(
    largura: u32,
    altura: u32,
    bytes: usize,
    ajustes: &[f32],
    corte: &[f32],
    locais: Option<&str>,
    limite: u32,
) -> Result<(Corte, ParametrosLocais, Ajustes), String> {
    let corte = corte_de_vetor(corte)?;
    let locais = locais_de_json(locais)?;
    let ajustes = ajustes_de_vetor(ajustes)?;
    if bytes as u64 != bytes_rgba(largura, altura) {
        return Err("os bytes não batem com largura × altura × 4".to_string());
    }
    conferir_limite(largura, altura, limite)?;
    Ok((corte, locais, ajustes))
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Uma imagem RGBA pequena com cor em todo canal e alfa variado: o que se
    /// prova com ela é que nenhum byte troca de lugar.
    fn pixels_de_teste(largura: u32, altura: u32) -> Vec<u8> {
        (0..altura)
            .flat_map(|y| {
                (0..largura).flat_map(move |x| {
                    [
                        (x * 37 % 256) as u8,
                        (y * 53 % 256) as u8,
                        ((x + y) * 19 % 256) as u8,
                        (255 - (x * 7 + y * 3) % 128) as u8,
                    ]
                })
            })
            .collect()
    }

    /// O cabeçalho de `decodificar_imagem`: largura e altura em `u32` LE.
    fn cabecalho(decodificado: &[u8]) -> (u32, u32, &[u8]) {
        let largura = u32::from_le_bytes(decodificado[0..4].try_into().unwrap());
        let altura = u32::from_le_bytes(decodificado[4..8].try_into().unwrap());
        (largura, altura, &decodificado[8..])
    }

    fn quase(a: &[f32], b: &[f32], folga: f32) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= folga)
    }

    const IDENTIDADE: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

    // --- O vetor do corte ---------------------------------------------------

    #[test]
    fn o_corte_inteiro_e_a_identidade() {
        let v = corte_inteiro();
        assert_eq!(v.len(), CAMPOS_DO_CORTE);
        assert_eq!(corte_de_vetor(&v).unwrap(), Corte::inteiro());
        assert!(corte_de_vetor(&v).unwrap().e_inteiro());
    }

    #[test]
    fn o_corte_aceita_oito_ou_quatorze_campos() {
        assert_eq!(CAMPOS_COM_PERSPECTIVA, 14);
        assert!(corte_de_vetor(&[0.0; 8].map(|_| 0.5)).is_ok());
        let mut com_perspectiva = corte_inteiro();
        com_perspectiva.extend_from_slice(&[0.0, 0.0, 0.0, 30.0, 0.0, 0.0]);
        assert!(corte_de_vetor(&com_perspectiva).is_ok());
    }

    #[test]
    fn o_corte_de_outro_tamanho_e_erro_com_a_mensagem_do_site() {
        for n in [0, 1, 7, 9, 13, 15, 20] {
            let erro = corte_de_vetor(&vec![0.0; n]).unwrap_err();
            assert_eq!(
                erro,
                format!("esperava 8 ou 14 campos de corte, recebi {n}"),
                "tamanho {n}"
            );
        }
    }

    #[test]
    fn o_corte_le_os_campos_na_ordem_de_corte_novo() {
        let corte = corte_de_vetor(&[0.1, 0.2, 0.5, 0.6, 1.0, 3.5, 1.0, 0.0]).unwrap();
        assert_eq!(corte.x(), 0.1);
        assert_eq!(corte.y(), 0.2);
        assert_eq!(corte.largura(), 0.5);
        assert_eq!(corte.altura(), 0.6);
        assert_eq!(corte.giro_90(), 1);
        assert_eq!(corte.angulo(), 3.5);
        assert!(corte.espelho_h());
        assert!(!corte.espelho_v());
        assert!(!corte.tem_perspectiva());
    }

    #[test]
    fn o_espelho_e_qualquer_numero_diferente_de_zero() {
        let corte = corte_de_vetor(&[0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.3, -2.0]).unwrap();
        assert!(corte.espelho_h());
        assert!(corte.espelho_v());
    }

    #[test]
    fn o_corte_fora_da_faixa_entra_corrigido_e_nao_vira_erro() {
        let corte = corte_de_vetor(&[-1.0, 2.0, 5.0, 0.0, 9.0, 90.0, 0.0, 0.0]).unwrap();
        assert_eq!(corte.x(), 0.0);
        assert_eq!(corte.y(), 1.0);
        assert_eq!(corte.largura(), 1.0);
        assert_eq!(corte.giro_90(), 3);
        assert_eq!(corte.angulo(), 45.0);
    }

    #[test]
    fn o_giro_fracionario_e_truncado_para_inteiro() {
        assert_eq!(
            corte_de_vetor(&[0.0, 0.0, 1.0, 1.0, 2.9, 0.0, 0.0, 0.0])
                .unwrap()
                .giro_90(),
            2
        );
    }

    #[test]
    fn os_seis_campos_a_mais_viram_a_perspectiva_guiada() {
        let mut v = corte_inteiro();
        v.extend_from_slice(&[5.0, 0.0, 0.0, 30.0, 0.0, 0.0]);
        assert!(corte_de_vetor(&v).unwrap().tem_perspectiva());

        // Seis zeros de rotação e de ajuste não corrigem nada: é a foto de antes.
        let mut neutro = corte_inteiro();
        neutro.extend_from_slice(&[0.0, 0.0, 0.0, 30.0, 0.0, 0.0]);
        assert!(!corte_de_vetor(&neutro).unwrap().tem_perspectiva());
    }

    // --- Os ajustes ---------------------------------------------------------

    #[test]
    fn os_ajustes_padrao_voltam_como_o_neutro() {
        let v = ajustes_padrao();
        assert_eq!(v.len(), revelacao_core::QUANTIDADE);
        assert_eq!(
            ajustes_de_vetor(&v).unwrap().como_vetor(),
            Ajustes::default().como_vetor()
        );
    }

    #[test]
    fn os_ajustes_de_outro_tamanho_sao_erro_com_a_mensagem_do_site() {
        let n = revelacao_core::QUANTIDADE;
        for tamanho in [0, n - 1, n + 1] {
            assert_eq!(
                ajustes_de_vetor(&vec![0.0; tamanho]).err(),
                Some(format!("esperava {n} ajustes, recebi {tamanho}"))
            );
        }
    }

    #[test]
    fn os_nomes_dos_ajustes_sao_um_json_com_um_nome_por_valor() {
        let nomes: Vec<String> = serde_json::from_str(&nomes_dos_ajustes()).unwrap();
        assert_eq!(nomes.len(), ajustes_padrao().len());
        assert_eq!(nomes, Ajustes::NOMES.map(String::from).to_vec());
        let unicos: std::collections::HashSet<_> = nomes.iter().collect();
        assert_eq!(unicos.len(), nomes.len(), "nome repetido");
    }

    // --- Os locais ----------------------------------------------------------

    #[test]
    fn locais_ausentes_ou_vazios_sao_sem_mascara() {
        for json in [None, Some(""), Some("   \n\t ")] {
            assert_eq!(locais_de_json(json).unwrap(), ParametrosLocais::default());
        }
    }

    #[test]
    fn locais_com_json_invalido_sao_erro_e_nao_revelacao_vazia() {
        assert!(locais_de_json(Some("{ isto não é json")).is_err());
        assert!(locais_de_json(Some("[1, 2, 3]")).is_err());
    }

    #[test]
    fn locais_de_uma_versao_mais_nova_sao_erro() {
        assert!(locais_de_json(Some(r#"{"versao": 4000000000}"#)).is_err());
    }

    #[test]
    fn locais_gravados_pelo_desktop_voltam_iguais() {
        let json = serde_json::to_string(&ParametrosLocais::default()).unwrap();
        assert_eq!(
            locais_de_json(Some(&format!("  {json}  "))).unwrap(),
            ParametrosLocais::default()
        );
    }

    // --- As matrizes e a geometria ------------------------------------------

    #[test]
    fn a_homografia_sem_perspectiva_e_a_identidade() {
        let h = homografia_no_espaco(&corte_inteiro(), 6000, 4000).unwrap();
        assert!(quase(&h, &IDENTIDADE, 1e-6), "{h:?}");

        // Giro e espelho não são perspectiva: continua a identidade.
        let h =
            homografia_no_espaco(&[0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 0.0], 6000, 4000).unwrap();
        assert!(quase(&h, &IDENTIDADE, 1e-6), "{h:?}");
    }

    #[test]
    fn a_homografia_com_perspectiva_nao_e_a_identidade_e_vem_normalizada() {
        let mut v = corte_inteiro();
        v.extend_from_slice(&[8.0, 0.0, 0.0, 30.0, 0.0, 0.0]);
        let h = homografia_no_espaco(&v, 6000, 4000).unwrap();
        assert_eq!(h.len(), 9);
        assert!(!quase(&h, &IDENTIDADE, 1e-3), "{h:?}");
        assert!((h[8] - 1.0).abs() < 1e-6, "o canto de baixo vale 1: {h:?}");
    }

    #[test]
    fn a_homografia_recusa_o_corte_de_tamanho_errado() {
        assert_eq!(
            homografia_no_espaco(&[0.0; 3], 10, 10).unwrap_err(),
            "esperava 8 ou 14 campos de corte, recebi 3"
        );
    }

    #[test]
    fn a_matriz_do_corte_inteiro_e_a_identidade() {
        for recortar in [true, false] {
            let m = matriz_do_enquadramento(&corte_inteiro(), 6000, 4000, recortar).unwrap();
            assert!(quase(&m, &IDENTIDADE, 1e-6), "recortar={recortar}: {m:?}");
        }
    }

    #[test]
    fn a_matriz_leva_o_quadro_ao_retangulo_do_recorte() {
        // Metade direita de baixo: (s, t) = (0, 0) cai em (0.5, 0.5) da foto.
        let m =
            matriz_do_enquadramento(&[0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0], 6000, 4000, true)
                .unwrap();
        assert!(
            quase(&m, &[0.5, 0.0, 0.5, 0.0, 0.5, 0.5, 0.0, 0.0, 1.0], 1e-6),
            "{m:?}"
        );

        // Sem recortar, o Enquadrar vê o espaço inteiro.
        let m =
            matriz_do_enquadramento(&[0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0], 6000, 4000, false)
                .unwrap();
        assert!(quase(&m, &IDENTIDADE, 1e-6), "{m:?}");
    }

    #[test]
    fn a_matriz_do_espelho_horizontal_inverte_o_eixo_x() {
        let m =
            matriz_do_enquadramento(&[0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0], 6000, 4000, true)
                .unwrap();
        assert!(
            quase(&m, &[-1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0], 1e-6),
            "{m:?}"
        );
    }

    #[test]
    fn a_matriz_sai_com_o_canto_de_baixo_igual_a_um() {
        let mut v = vec![0.1, 0.1, 0.7, 0.8, 1.0, 4.0, 0.0, 1.0];
        v.extend_from_slice(&[6.0, -3.0, 1.0, 35.0, 2.0, -1.0]);
        let m = matriz_do_enquadramento(&v, 4000, 3000, true).unwrap();
        assert_eq!(m.len(), 9);
        assert!((m[8] - 1.0).abs() < 1e-6, "{m:?}");
        assert!(m.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn o_enquadramento_do_corte_inteiro_e_a_foto_toda() {
        assert_eq!(
            enquadramento(&corte_inteiro(), 6000, 4000).unwrap(),
            vec![6000.0, 4000.0, 0.0, 0.0, 6000.0, 4000.0, 6000.0, 4000.0]
        );
    }

    #[test]
    fn o_enquadramento_girado_um_quarto_troca_os_lados() {
        assert_eq!(
            enquadramento(&[0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0], 6000, 4000).unwrap(),
            vec![4000.0, 6000.0, 0.0, 0.0, 4000.0, 6000.0, 4000.0, 6000.0]
        );
        // Meia volta não troca.
        assert_eq!(
            enquadramento(&[0.0, 0.0, 1.0, 1.0, 2.0, 0.0, 0.0, 0.0], 6000, 4000).unwrap()[..2],
            [6000.0, 4000.0]
        );
    }

    #[test]
    fn o_enquadramento_anuncia_o_retangulo_e_a_saida_do_arquivo() {
        let v = enquadramento(&[0.25, 0.1, 0.5, 0.8, 0.0, 0.0, 0.0, 0.0], 6000, 4000).unwrap();
        assert_eq!(
            v,
            vec![6000.0, 4000.0, 1500.0, 400.0, 3000.0, 3200.0, 3000.0, 3200.0]
        );
    }

    #[test]
    fn o_enquadramento_com_angulo_sai_do_tamanho_pedido() {
        let corte = [0.2, 0.2, 0.6, 0.6, 0.0, 10.0, 0.0, 0.0];
        let v = enquadramento(&corte, 1000, 500).unwrap();
        let esperado = corte_de_vetor(&corte)
            .unwrap()
            .dimensoes_de_saida(1000, 500);
        assert_eq!((v[6], v[7]), (esperado.0 as f32, esperado.1 as f32));
        assert_eq!((v[6], v[7]), (600.0, 300.0));
    }

    #[test]
    fn o_enquadramento_recusa_o_corte_de_tamanho_errado() {
        assert!(enquadramento(&[], 10, 10)
            .unwrap_err()
            .contains("campos de corte, recebi 0"));
    }

    // --- Codificar e decodificar --------------------------------------------

    #[test]
    fn o_jpeg_comprimido_e_um_jpeg_do_tamanho_certo() {
        let rgba = pixels_de_teste(24, 16);
        let jpeg = comprimir_jpeg(24, 16, &rgba, 85).unwrap();
        assert_eq!(&jpeg[..3], &[0xFF, 0xD8, 0xFF], "assinatura SOI");
        let (l, a, pixels) = {
            let d = decodificar_imagem(&jpeg).unwrap();
            let (l, a, p) = cabecalho(&d);
            (l, a, p.len())
        };
        assert_eq!((l, a, pixels), (24, 16, 24 * 16 * 4));
    }

    #[test]
    fn a_qualidade_do_jpeg_e_presa_entre_um_e_cem() {
        let rgba = pixels_de_teste(16, 16);
        assert_eq!(
            comprimir_jpeg(16, 16, &rgba, 0).unwrap(),
            comprimir_jpeg(16, 16, &rgba, 1).unwrap()
        );
        assert_eq!(
            comprimir_jpeg(16, 16, &rgba, 255).unwrap(),
            comprimir_jpeg(16, 16, &rgba, 100).unwrap()
        );
        assert_ne!(
            comprimir_jpeg(16, 16, &rgba, 1).unwrap(),
            comprimir_jpeg(16, 16, &rgba, 100).unwrap()
        );
        // E o `codificar_imagem` em JPEG prende do mesmo jeito.
        assert_eq!(
            codificar_imagem(16, 16, &rgba, "jpeg", 0).unwrap(),
            comprimir_jpeg(16, 16, &rgba, 1).unwrap()
        );
    }

    #[test]
    fn o_jpeg_comprimido_e_o_mesmo_do_codificador_do_desktop() {
        let rgba = pixels_de_teste(10, 7);
        let imagem = image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(10, 7, rgba.clone()).unwrap(),
        );
        assert_eq!(
            comprimir_jpeg(10, 7, &rgba, 90).unwrap(),
            revelacao_core::jpeg::codificar(&imagem, 90).unwrap()
        );
        assert_eq!(
            codificar_imagem(10, 7, &rgba, "jpeg", 90).unwrap(),
            comprimir_jpeg(10, 7, &rgba, 90).unwrap()
        );
    }

    #[test]
    fn bytes_que_nao_batem_com_o_tamanho_sao_erro() {
        let rgba = pixels_de_teste(4, 4);
        assert_eq!(
            comprimir_jpeg(4, 5, &rgba, 90).unwrap_err(),
            "os bytes não batem com largura × altura × 4: 64 para 80"
        );
        assert_eq!(
            codificar_imagem(4, 3, &rgba, "png", 90).unwrap_err(),
            "os bytes não batem com largura × altura × 4: 64 para 48"
        );
    }

    #[test]
    fn um_tamanho_que_estoura_a_conta_e_erro_e_nao_panico() {
        // Em 64 bits `u32::MAX²` cabe, mas `× 4` não.
        assert_eq!(
            comprimir_jpeg(u32::MAX, u32::MAX, &[], 90).unwrap_err(),
            format!("{}×{} não cabe na memória", u32::MAX, u32::MAX)
        );
        assert!(codificar_imagem(u32::MAX, u32::MAX, &[], "png", 90)
            .unwrap_err()
            .contains("não cabe na memória"));
    }

    #[test]
    fn imagem_sem_tamanho_e_erro_e_nao_panico() {
        let erro = comprimir_jpeg(0, 0, &[], 90).unwrap_err();
        assert!(erro.starts_with("o JPEG não codificou: "), "{erro}");
        for formato in ["png", "tiff", "webp"] {
            let erro = codificar_imagem(0, 5, &[], formato, 90).unwrap_err();
            assert!(
                erro.starts_with("o arquivo não codificou: "),
                "{formato}: {erro}"
            );
        }
    }

    #[test]
    fn png_tiff_e_webp_voltam_byte_a_byte() {
        let rgba = pixels_de_teste(13, 9);
        for formato in ["png", "tiff", "webp"] {
            let arquivo = codificar_imagem(13, 9, &rgba, formato, 90).unwrap();
            let decodificado = decodificar_imagem(&arquivo).unwrap();
            let (l, a, pixels) = cabecalho(&decodificado);
            assert_eq!((l, a), (13, 9), "{formato}");
            assert_eq!(pixels, rgba.as_slice(), "{formato} perdeu pixel");
        }
    }

    #[test]
    fn cada_formato_de_saida_sai_com_a_assinatura_dele() {
        let rgba = pixels_de_teste(8, 8);
        let assinatura = |formato: &str| {
            let arquivo = codificar_imagem(8, 8, &rgba, formato, 90).unwrap();
            image::guess_format(&arquivo).unwrap()
        };
        assert_eq!(assinatura("jpeg"), image::ImageFormat::Jpeg);
        assert_eq!(assinatura("png"), image::ImageFormat::Png);
        assert_eq!(assinatura("tiff"), image::ImageFormat::Tiff);
        assert_eq!(assinatura("webp"), image::ImageFormat::WebP);
    }

    #[test]
    fn todo_formato_de_saida_anunciado_codifica() {
        let formatos: Vec<String> = serde_json::from_str(&formatos_de_saida()).unwrap();
        assert_eq!(formatos, ["jpeg", "png", "tiff", "webp"]);
        let rgba = pixels_de_teste(5, 5);
        for formato in &formatos {
            assert!(
                codificar_imagem(5, 5, &rgba, formato, 90).is_ok(),
                "{formato}"
            );
        }
    }

    #[test]
    fn formato_desconhecido_e_erro_com_o_nome_dele() {
        let rgba = pixels_de_teste(2, 2);
        for formato in ["avif", "JPEG", "jpg", ""] {
            assert_eq!(
                codificar_imagem(2, 2, &rgba, formato, 90).unwrap_err(),
                format!("formato desconhecido: {formato}")
            );
        }
    }

    #[test]
    fn todo_formato_de_entrada_anunciado_abre() {
        let formatos: Vec<String> = serde_json::from_str(&formatos_de_entrada()).unwrap();
        assert_eq!(formatos, ["jpeg", "png", "tiff", "webp", "bmp", "gif"]);
        let origem = image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(6, 4, pixels_de_teste(6, 4)).unwrap(),
        );
        for nome in &formatos {
            let formato = image::ImageFormat::from_extension(nome).unwrap();
            // O JPEG não tem alfa: vai em RGB, como sai de câmera.
            let imagem = if formato == image::ImageFormat::Jpeg {
                image::DynamicImage::ImageRgb8(origem.to_rgb8())
            } else {
                origem.clone()
            };
            let mut arquivo = std::io::Cursor::new(Vec::new());
            imagem.write_to(&mut arquivo, formato).unwrap();
            let decodificado = decodificar_imagem(arquivo.get_ref())
                .unwrap_or_else(|e| panic!("{nome} não abriu: {e}"));
            let (l, a, pixels) = cabecalho(&decodificado);
            assert_eq!((l, a, pixels.len()), (6, 4, 6 * 4 * 4), "{nome}");
        }
    }

    #[test]
    fn o_decodificado_comeca_com_largura_e_altura_em_little_endian() {
        // Largura e altura acima de 255, para os dois bytes de cada contarem.
        let (largura, altura) = (300u32, 2u32);
        let rgba = pixels_de_teste(largura, altura);
        let png = codificar_imagem(largura, altura, &rgba, "png", 0).unwrap();
        let d = decodificar_imagem(&png).unwrap();
        assert_eq!(&d[0..4], &[0x2C, 0x01, 0, 0]);
        assert_eq!(&d[4..8], &[2, 0, 0, 0]);
        assert_eq!(d.len(), 8 + rgba.len());
        assert_eq!(&d[8..], rgba.as_slice());
    }

    #[test]
    fn o_png_em_rgb_volta_com_alfa_opaco() {
        let rgb =
            image::RgbImage::from_fn(3, 2, |x, y| image::Rgb([x as u8 * 50, y as u8 * 90, 7]));
        let mut arquivo = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(rgb.clone())
            .write_to(&mut arquivo, image::ImageFormat::Png)
            .unwrap();
        let d = decodificar_imagem(arquivo.get_ref()).unwrap();
        let (_, _, pixels) = cabecalho(&d);
        for (p, q) in pixels.chunks(4).zip(rgb.pixels()) {
            assert_eq!(&p[..3], &q.0);
            assert_eq!(p[3], 255);
        }
    }

    #[test]
    fn arquivo_que_nao_abre_e_erro_com_os_formatos_aceitos() {
        for lixo in [
            &b""[..],
            b"isto nao e uma imagem",
            &[0xFF, 0xD8, 0xFF, 0x00],
        ] {
            let erro = decodificar_imagem(lixo).unwrap_err();
            assert!(erro.starts_with("este arquivo não abriu ("), "{erro}");
            assert!(
                erro.ends_with("). Formatos aceitos: JPEG, PNG, TIFF, WebP, BMP e GIF"),
                "{erro}"
            );
        }
    }

    // --- O espaço de cor ----------------------------------------------------

    /// Um PNG com um perfil ICC cuja descrição diz `descricao`. O `foto_codec`
    /// lê o perfil pela descrição, então basta que o texto esteja nele.
    fn png_com_perfil(rgba: &[u8], largura: u32, altura: u32, descricao: &str) -> Vec<u8> {
        use image::ImageEncoder as _;
        let mut perfil = vec![0u8; 128];
        perfil.extend_from_slice(b"desc");
        perfil.extend_from_slice(descricao.as_bytes());
        let mut arquivo = Vec::new();
        let mut codificador = image::codecs::png::PngEncoder::new(&mut arquivo);
        codificador.set_icc_profile(perfil).unwrap();
        codificador
            .write_image(rgba, largura, altura, image::ExtendedColorType::Rgba8)
            .unwrap();
        arquivo
    }

    #[test]
    fn a_imagem_sem_perfil_declara_srgb() {
        let png = codificar_imagem(4, 4, &pixels_de_teste(4, 4), "png", 0).unwrap();
        assert_eq!(espaco_de_cor_declarado(&png), 0);
        let jpeg = comprimir_jpeg(4, 4, &pixels_de_teste(4, 4), 90).unwrap();
        assert_eq!(espaco_de_cor_declarado(&jpeg), 0);
    }

    #[test]
    fn arquivo_que_nao_abre_declara_srgb() {
        assert_eq!(espaco_de_cor_declarado(b""), 0);
        assert_eq!(espaco_de_cor_declarado(b"lixo"), 0);
    }

    #[test]
    fn o_perfil_icc_e_lido_pela_descricao() {
        let rgba = pixels_de_teste(2, 2);
        assert_eq!(
            espaco_de_cor_declarado(&png_com_perfil(&rgba, 2, 2, "Adobe RGB (1998)")),
            1
        );
        assert_eq!(
            espaco_de_cor_declarado(&png_com_perfil(&rgba, 2, 2, "Display P3")),
            2
        );
        assert_eq!(
            espaco_de_cor_declarado(&png_com_perfil(&rgba, 2, 2, "Perfil qualquer")),
            0
        );
    }

    #[test]
    fn em_srgb_e_em_codigo_desconhecido_a_conversao_nao_toca_em_nada() {
        let original = pixels_de_teste(16, 16);
        for codigo in [0, 3, 9, 255] {
            let mut rgba = original.clone();
            converter_para_srgb(&mut rgba, codigo);
            assert_eq!(rgba, original, "código {codigo}");
        }
    }

    #[test]
    fn display_p3_vira_srgb_mais_saturado() {
        // Um laranja que cabe no sRGB depois de convertido.
        let mut rgba = vec![200, 120, 80, 77];
        converter_para_srgb(&mut rgba, 2);
        assert!(rgba[0] > 200, "o vermelho sobe: {rgba:?}");
        assert!(rgba[1] < 120, "o verde desce: {rgba:?}");
        assert!(rgba[2] < 80, "o azul desce: {rgba:?}");
        assert_eq!(rgba[3], 77, "o alfa passa intacto");
    }

    #[test]
    fn adobe_rgb_vira_srgb_com_o_verde_mais_puro() {
        // O Adobe RGB é mais largo no verde: o mesmo número é um verde mais
        // saturado, e em sRGB ele precisa de menos vermelho e menos azul.
        let mut rgba = vec![90, 180, 90, 200];
        converter_para_srgb(&mut rgba, 1);
        assert!(rgba[0] < 90, "o vermelho desce: {rgba:?}");
        assert!(rgba[2] < 90, "o azul desce: {rgba:?}");
        assert!(
            rgba[1].abs_diff(180) <= 8,
            "o verde quase não muda: {rgba:?}"
        );
        assert_eq!(rgba[3], 200, "o alfa passa intacto");
    }

    #[test]
    fn o_cinza_continua_cinza_em_qualquer_espaco() {
        for codigo in [1, 2] {
            for nivel in [0u8, 1, 64, 128, 200, 255] {
                let mut rgba = vec![nivel, nivel, nivel, 255];
                converter_para_srgb(&mut rgba, codigo);
                assert!(
                    rgba[0] == rgba[1] && rgba[1] == rgba[2],
                    "código {codigo}, nível {nivel}: {rgba:?}"
                );
                assert!(
                    rgba[0].abs_diff(nivel) <= 2,
                    "código {codigo}, nível {nivel}: {rgba:?}"
                );
            }
        }
    }

    #[test]
    fn o_decodificador_e_o_navegador_dao_a_mesma_foto_em_display_p3() {
        // O caminho do `createImageBitmap` é: decodificar sem perfil, perguntar
        // o espaço e converter. O caminho daqui decodifica já convertido. Os
        // dois têm de dar os mesmos bytes.
        let original = pixels_de_teste(8, 8);
        let png = png_com_perfil(&original, 8, 8, "Display P3");

        let espaco = espaco_de_cor_declarado(&png);
        let mut do_navegador = original.clone();
        converter_para_srgb(&mut do_navegador, espaco);

        let decodificado = decodificar_imagem(&png).unwrap();
        let (_, _, daqui) = cabecalho(&decodificado);
        assert_ne!(
            daqui,
            original.as_slice(),
            "a foto P3 tinha de ser convertida"
        );
        assert_eq!(daqui, do_navegador.as_slice());
    }

    // --- A escala e as conferências ------------------------------------------

    #[test]
    fn sem_lado_original_a_escala_e_um() {
        assert_eq!(escala_do_original(2048, 1365, None), 1.0);
        assert_eq!(escala_do_original(2048, 1365, Some(0)), 1.0);
    }

    #[test]
    fn a_escala_e_a_razao_dos_lados_maiores() {
        assert_eq!(escala_do_original(2048, 1365, Some(6000)), 2048.0 / 6000.0);
        // Retrato: o maior lado é a altura.
        assert_eq!(escala_do_original(1365, 2048, Some(4096)), 0.5);
    }

    #[test]
    fn a_escala_nunca_passa_de_um() {
        assert_eq!(escala_do_original(6000, 4000, Some(6000)), 1.0);
        assert_eq!(escala_do_original(6000, 4000, Some(3000)), 1.0);
    }

    #[test]
    fn a_copia_de_trabalho_sem_tamanho_e_erro() {
        assert_eq!(
            conferir_copia_de_trabalho(0, 10, 0, 8192).unwrap_err(),
            "imagem sem tamanho"
        );
        assert_eq!(
            conferir_copia_de_trabalho(10, 0, 0, 8192).unwrap_err(),
            "imagem sem tamanho"
        );
    }

    #[test]
    fn a_copia_de_trabalho_confere_os_bytes_antes_do_limite() {
        assert_eq!(
            conferir_copia_de_trabalho(3, 2, 20, 8192).unwrap_err(),
            "esperava 24 bytes RGBA para 3×2, recebi 20"
        );
        assert_eq!(
            conferir_copia_de_trabalho(9000, 10, 5, 8192).unwrap_err(),
            "esperava 360000 bytes RGBA para 9000×10, recebi 5"
        );
    }

    #[test]
    fn a_copia_de_trabalho_maior_que_a_textura_e_erro() {
        assert_eq!(
            conferir_copia_de_trabalho(8193, 1, 8193 * 4, 8192).unwrap_err(),
            "8193×1 passa do limite de textura deste dispositivo (8192 px)"
        );
        assert!(conferir_copia_de_trabalho(1, 8193, 8193 * 4, 8192).is_err());
        assert!(conferir_copia_de_trabalho(8192, 1, 8192 * 4, 8192).is_ok());
    }

    #[test]
    fn a_exportacao_devolve_o_corte_os_locais_e_os_ajustes() {
        let ajustes = ajustes_padrao();
        let (corte, locais, lidos) =
            preparar_exportacao(4, 3, 48, &ajustes, &corte_inteiro(), None, 8192).unwrap();
        assert_eq!(corte, Corte::inteiro());
        assert_eq!(locais, ParametrosLocais::default());
        assert_eq!(lidos.como_vetor(), Ajustes::default().como_vetor());
    }

    #[test]
    fn a_exportacao_confere_na_ordem_corte_locais_ajustes_bytes_limite() {
        let ajustes = ajustes_padrao();
        let corte = corte_inteiro();
        // Tudo errado: o primeiro erro é o do corte.
        assert!(preparar_exportacao(9000, 1, 1, &[], &[], Some("{"), 8192)
            .unwrap_err()
            .contains("campos de corte"));
        // Corte certo: o próximo é o dos locais.
        assert!(preparar_exportacao(9000, 1, 1, &[], &corte, Some("{"), 8192).is_err());
        assert!(
            !preparar_exportacao(9000, 1, 1, &[], &corte, Some("{"), 8192)
                .unwrap_err()
                .contains("ajustes")
        );
        // Locais certos: os ajustes.
        assert_eq!(
            preparar_exportacao(9000, 1, 1, &[], &corte, None, 8192).unwrap_err(),
            format!("esperava {} ajustes, recebi 0", revelacao_core::QUANTIDADE)
        );
        // Ajustes certos: os bytes.
        assert_eq!(
            preparar_exportacao(9000, 1, 1, &ajustes, &corte, None, 8192).unwrap_err(),
            "os bytes não batem com largura × altura × 4"
        );
        // Bytes certos: o limite.
        assert_eq!(
            preparar_exportacao(9000, 1, 36000, &ajustes, &corte, None, 8192).unwrap_err(),
            "9000×1 passa do limite de textura deste dispositivo (8192 px)"
        );
    }

    #[test]
    fn a_conta_dos_bytes_nao_da_a_volta_em_32_bits() {
        // 2³² bytes: no wasm32 a conta em usize virava 0 e o buffer vazio passava.
        assert!(conferir_copia_de_trabalho(32768, 32768, 0, u32::MAX).is_err());
        assert_eq!(bytes_rgba(32768, 32768), 1 << 32);
    }
}
