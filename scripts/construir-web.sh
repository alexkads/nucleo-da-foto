#!/usr/bin/env bash
# Compila o motor de revelação para o navegador e entrega ao site.
#
# Uso: scripts/construir-web.sh [caminho do frontend do e-commerce]
#      (padrão: $RECORDARFOTOS_FRONTEND, senão ../recordarfotos-e-commerce/frontend)
#
# O que sai, e para onde:
#
#   frontend/public/revelacao/revelacao_web.js        o glue do wasm-bindgen (--target web)
#   frontend/public/revelacao/revelacao_web_bg.wasm   o motor
#   frontend/public/revelacao/nomes.json              os 46 nomes, na ordem do uniform
#   frontend/public/revelacao/VERSAO                  o commit deste repositório
#   frontend/src/.../revelacao/revelacao_web.d.ts     os tipos do glue, para o TypeScript
#   frontend/src/.../revelacao/versao.ts              a mesma VERSAO, importável
#
# 🔑 `--target web` é o que dispensa configurar o bundler do Next: o glue
# exporta `default(init)` e não tem `import` de nada. O site carrega os dois
# arquivos em runtime, de `public/`, com `?v=VERSAO` para o glue e o `.wasm`
# nunca descasarem no cache do navegador.
#
# Os artefatos são commitados no repositório do site: a Vercel não tem Rust.
# 🚨 **Se a pilha local estiver no ar, reinicie o `web`.**
#
# O `versao.ts` deste script é lido por `import`, e o Turbopack **não o
# recompila** quando ele muda por fora do editor: o contêiner continua servindo
# o `?v=` antigo, o navegador continua carregando o wasm de antes, e o motor
# novo simplesmente não entra. Isso custou uma ida e volta com o dono em
# 2026-09-12 — ele testou uma correção três vezes contra o binário velho.
#
#   docker compose -f docker-compose.dev.yml exec web \
#     sh -c 'rm -rf /app/.next/dev/cache /app/.next/dev/static /app/.next/dev/server'
#   docker compose -f docker-compose.dev.yml restart web
#
# 🚨 **E o `restart` sozinho não basta** — provado em 2026-09-12. Depois dele o
# HTML do servidor já vinha com o `?v=` novo e o **chunk do cliente** continuava
# sendo o antigo, vindo do cache do Turbopack: o selo de desenvolvimento da tela
# do cliente mostrava o commit velho enquanto o log do contêiner mostrava o
# novo. Só apagando `.next/dev` o chunk velho desaparece — e vale um
# `Cmd+Shift+R` na janela, que pode ter o dela em cache.
#
# Em produção não acontece: a Vercel constrói do zero a cada deploy.
set -euo pipefail

cd "$(dirname "$0")/.."

FRONTEND="${1:-${RECORDARFOTOS_FRONTEND:-../recordarfotos-e-commerce/frontend}}"
if [[ ! -d "$FRONTEND/public" ]]; then
    echo "frontend não encontrado em $FRONTEND (passe o caminho ou RECORDARFOTOS_FRONTEND)" >&2
    exit 1
fi
PUBLICO="$FRONTEND/public/revelacao"
FONTE="$FRONTEND/src/app/(dashboard)/dashboard/sessoes-fotograficas/[id]/revelacao"
SAIDA="target/revelacao-web"

for ferramenta in wasm-pack wasm-opt; do
    command -v "$ferramenta" >/dev/null || {
        echo "falta $ferramenta — brew install wasm-pack binaryen" >&2
        exit 1
    }
done
rustup target add wasm32-unknown-unknown >/dev/null

echo "→ wasm-pack build (perfil release-web)"
# `--profile` é o do cargo (`[profile.release-web]` no Cargo.toml do workspace);
# não se passa `--release` junto, o wasm-pack recusa os dois.
RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+simd128" \
wasm-pack build crates/revelacao-web \
    --target web \
    --profile release-web \
    --out-dir "../../$SAIDA" \
    --out-name revelacao_web

echo "→ wasm-opt -O3"
mkdir -p "$PUBLICO" "$FONTE"
wasm-opt -O3 --enable-simd --enable-bulk-memory --enable-nontrapping-float-to-int \
    -o "$PUBLICO/revelacao_web_bg.wasm" "$SAIDA/revelacao_web_bg.wasm"
cp "$SAIDA/revelacao_web.js" "$PUBLICO/revelacao_web.js"
cp "$SAIDA/revelacao_web.d.ts" "$FONTE/revelacao_web.d.ts"

echo "→ nomes.json e VERSAO"
cargo run -q -p revelacao-core --bin nomes-dos-ajustes > "$PUBLICO/nomes.json"
VERSAO="$(git rev-parse --short HEAD)"
# 🚨 **O perfil e este script entram na conta do "sujo".** Antes só os dois
# crates contavam, e isso bastava enquanto a compilação era sempre a mesma. Não
# é mais: o `opt-level` do `[profile.release-web]` e o `+simd128` daqui mudam o
# `.wasm` **sem mudar uma linha de Rust** — e o site agora serve o motor com
# `Cache-Control: immutable`, onde uma VERSAO que não muda é um wasm velho que
# nunca mais se desfaz no navegador de quem já o baixou.
if [[ -n "$(git status --porcelain crates/revelacao-core crates/revelacao-web Cargo.toml "$0")" ]]; then
    VERSAO="$VERSAO-sujo"
fi
echo "$VERSAO" > "$PUBLICO/VERSAO"
cat > "$FONTE/versao.ts" <<TS
// Gerado por scripts/construir-web.sh do VintageLightbox-Rust — não editar.
// É o commit do motor que está em public/revelacao/, e vai na URL dos dois
// arquivos (\`?v=\`) para o glue e o .wasm nunca descasarem no cache.
export const VERSAO = "$VERSAO";
TS

echo
ls -la "$PUBLICO"
echo
echo "motor $VERSAO entregue em $PUBLICO"
