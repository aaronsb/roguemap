# roguemap task runner. `make help` lists targets.

.DEFAULT_GOAL := help
.PHONY: help build run term test test-assets lint format check clean snap screenshots fonts golden golden-check assets-export

BIN      := target/release/roguemap
SEED     ?= 7
SIZE     ?= 32
# Snapshot geometry in cells; matches the Konsole launcher.
COLS     ?= 168
ROWS     ?= 71
SHOTS    := docs/screenshots

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'

build: ## Build the release binary
	cargo build --release

run: build ## Run in the current terminal (SEED=7 SIZE=32)
	./$(BIN) $(SEED) $(SIZE)

term: build ## Open a Konsole window with the Unscii font, sized like the screenshots
	./term.sh $(SEED) $(SIZE)

test: ## Run unit tests
	cargo test --release

test-assets: ## Run the asset loader tests: embedded set, cross-references, round trips
	cargo test --release assets::

lint: ## Run clippy
	cargo clippy --release -- -D warnings

format: ## Format sources
	cargo fmt

check: lint test test-assets ## Run all quality gates

clean: ## Remove build artefacts and generated screenshots
	cargo clean
	rm -f $(SHOTS)/*.png *.cells

golden: build ## Record reference frames in .golden (run before a refactor)
	tools/golden.sh .golden

golden-check: build ## Re-render and compare against .golden byte for byte
	tools/golden.sh .golden-new >/dev/null
	@fail=0; for f in .golden/*.cells; do n=.golden-new/$$(basename $$f); \
	  if ! cmp -s $$f $$n; then echo "DIFF $$(basename $$f)"; fail=1; fi; done; \
	  if [ $$fail = 0 ]; then echo "golden: all frames identical"; else exit 1; fi

assets-export: build ## Write the embedded asset tables to ./assets-export for editing (ROGUEMAP_ASSETS=assets-export to load them)
	./$(BIN) --export-assets assets-export

fonts: ## Report fonts covering the Symbols for Legacy Computing block
	@fc-list ':charset=1fb00' family | sort -u

# One-off snapshot: make snap OUT=shot.png ARGS="fill=1 zoom=2 tod=21 fire=1"
snap: build ## Render one headless frame to OUT (ARGS="key=value ...")
	./snap.sh $(OUT) $(ARGS)

screenshots: build ## Render the documentation screenshots into docs/screenshots
	./snap.sh $(SHOTS)/island.png zoom=1 t=3 tod=12
	./snap.sh $(SHOTS)/rotated.png zoom=1 t=3 tod=12 deg=25
	./snap.sh $(SHOTS)/filled.png fill=1 zoom=2 cx=0 cy=0 t=3 tod=12 player=1
	./snap.sh $(SHOTS)/closeup.png fill=1 zoom=6 cx=0 cy=0 t=3 tod=12 player=1
	./snap.sh $(SHOTS)/night.png fill=1 zoom=4 cx=500 cy=-300 t=3 tod=22 fire=1
	./snap.sh $(SHOTS)/winter.png fill=1 zoom=2 cx=500 cy=-300 t=3 tod=12 season=3 simdays=2
	./snap.sh $(SHOTS)/clouds.png fill=1 zoom=0 cx=0 cy=0 t=3 tod=14 cover=0.4
	./snap.sh $(SHOTS)/steppe.png fill=1 zoom=2 cx=-500 cy=400 t=3 tod=12
	./snap.sh $(SHOTS)/worldmap.png fill=1 worldmap=1 scale=2 cx=0 cy=0
	./snap.sh $(SHOTS)/settings.png popover=1 zoom=1
	./snap.sh $(SHOTS)/ascii.png zoom=2 fill=1 cx=0 cy=0 t=3 tod=12 glyphs=ascii
