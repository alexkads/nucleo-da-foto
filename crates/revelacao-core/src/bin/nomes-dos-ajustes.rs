//! Imprime os nomes dos ajustes, na ordem do `uniform`, como JSON.
//!
//! 🔑 É o contrato entre este repositório e o site: o `scripts/construir-web.sh`
//! grava a saída em `public/revelacao/nomes.json`, e um teste do frontend
//! compara a lista dele com esta. Um campo fora de lugar não é erro em lugar
//! nenhum — é a foto saindo com o ajuste errado — e a lista é a única defesa.

fn main() {
    println!(
        "{}",
        serde_json::to_string(&revelacao_core::Ajustes::NOMES[..]).expect("os nomes viram JSON")
    );
}
