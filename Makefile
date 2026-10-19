# Os comandos do nucleo-da-foto. `make` sem argumento lista.
.DEFAULT_GOAL := ajuda
.PHONY: ajuda verificar fmt clippy testar wasm doc deny cobertura semver msrv bancada bancada-testar

WEB := -p conversao-web -p fotolivro-web -p raw-web -p revelacao-web -p tela-do-cliente-web
# O que vai para o navegador: os wrappers e os crates do núcleo que eles levam.
WASM := $(WEB) -p revelacao-core -p foto-codec -p raw-codec -p fotolivro
# O rawler é vendorizado: entra na compilação, não na conta da cobertura.
FORA_DA_COBERTURA := --ignore-filename-regex 'vendor/'
# O piso: a cobertura de linhas do núcleo (sem o vendor) não desce daqui.
PISO_DA_COBERTURA := 90
# 🚨 Um por um: com `--workspace`, o cargo-semver-checks pula os crates com
# `publish = false` — todos aqui — e sai verde sem ter conferido nada.
BIBLIOTECAS := foto-codec raw-codec revelacao-core editor-core recuperacao-core fotolivro visualizador-pdf ia-local preenchimento
MSRV := $(shell sed -n 's/^rust-version *= *"\(.*\)"/\1/p' Cargo.toml)
ULTIMA_TAG := $(shell git describe --tags --abbrev=0 2>/dev/null)

ajuda: ## Lista os alvos
	@grep -hE '^[a-z-]+:.*?## ' $(MAKEFILE_LIST) | awk -F':.*?## ' '{printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

verificar: fmt clippy testar wasm doc deny ## Tudo o que tem de passar antes de lançar

fmt: ## Formatação (rustfmt), sem mudar nada
	cargo fmt --all -- --check

clippy: ## Clippy com -D warnings, nativo
	cargo clippy --workspace --all-targets -- -D warnings

testar: ## Os testes (unidade, integração e doc-tests)
	cargo test --workspace

wasm: ## Os *-web e os crates que eles levam, no alvo wasm32, com clippy -D warnings
	cargo clippy --target wasm32-unknown-unknown $(WASM) -- -D warnings

doc: ## A documentação sem nenhum aviso (link quebrado, item privado citado)
	RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

deny: ## Licenças, vulnerabilidades conhecidas e fontes das dependências (cargo-deny)
	cargo deny check

cobertura: ## Cobertura por arquivo (cargo-llvm-cov), com piso; HTML em target/cobertura
	cargo llvm-cov --workspace $(FORA_DA_COBERTURA) --html --output-dir target/cobertura
	cargo llvm-cov report $(FORA_DA_COBERTURA) --summary-only --fail-under-lines $(PISO_DA_COBERTURA)

semver: ## A API pública de cada crate comparada com a última tag (cargo-semver-checks)
	@for c in $(BIBLIOTECAS); do cargo semver-checks -p $$c --baseline-rev $(ULTIMA_TAG) || exit 1; done

msrv: ## Compila com o Rust mínimo declarado (rust-version)
	rustup toolchain install $(MSRV) --profile minimal
	cargo +$(MSRV) check --workspace --all-targets

# A bancada (ferramentas/bancada) tem o próprio workspace: o GPUI da janela não
# entra no grafo dos apps nem no `verificar`.
bancada: ## A janela que mede o núcleo contra a última tag, com a sua foto
	cd ferramentas/bancada && cargo run --release

bancada-testar: ## Os testes da bancada: todo caso sai igual na tag e aqui; a janela sem tela
	cd ferramentas/bancada && cargo fmt -- --check && cargo clippy --all-targets -- -D warnings && cargo test
