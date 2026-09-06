# roguemap task runner. `make help` lists targets.

.DEFAULT_GOAL := help
.PHONY: help build run term test test-assets test-golden lint format fmt-check check clean snap tree-snap screenshots fonts golden golden-bytes golden-check golden-record assets-export edit edit-snap

BIN      := target/release/roguemap
EDIT     := target/release/roguemap-edit
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

test-golden: ## Compare the golden frame set with tests/golden/*.frame (see golden-check)
	cargo test --release --test golden

lint: ## Run clippy
	cargo clippy --release -- -D warnings

format: ## Format sources
	cargo fmt

check: lint test test-assets test-golden ## Run all quality gates

clean: ## Remove build artefacts and generated screenshots
	cargo clean
	rm -f $(SHOTS)/*.png *.cells

golden: build ## Record the frame set as cell dumps in .golden (run before a refactor)
	tools/golden.sh .golden

golden-bytes: build ## Re-render the cell dumps and compare against .golden byte for byte
	tools/golden.sh .golden-new >/dev/null
	@fail=0; for f in .golden/*.cells; do n=.golden-new/$$(basename $$f); \
	  if ! cmp -s $$f $$n; then echo "DIFF $$(basename $$f)"; fail=1; fi; done; \
	  if [ $$fail = 0 ]; then echo "golden: all frames identical"; else exit 1; fi

# Tolerances: GOLDEN_MIN_IDENTICAL=98.0 GOLDEN_MAX_DISTANCE=2.0 GOLDEN_STRICT=1
golden-check: ## Render the golden frames in-process and compare with tests/golden/*.frame, printing each frame's score
	cargo test --release --test golden golden_frames -- --nocapture

golden-record: ## Rewrite tests/golden/*.frame from the current code (say which frames changed and why in the commit)
	GOLDEN_RECORD=1 cargo test --release --test golden golden_frames -- --nocapture

assets-export: build ## Write the embedded asset tables to ./assets-export for editing (ROGUEMAP_ASSETS=assets-export to load them)
	./$(BIN) --export-assets assets-export

edit: build ## Open the asset editor on ./assets-export, exporting the embedded set first if it is missing
	@[ -d assets-export ] || ./$(BIN) --export-assets assets-export
	./$(EDIT) assets-export

# One-off editor screen: make edit-snap OUT=editor.png ARGS="table=species row=oak grid=1"
edit-snap: build ## Render one editor screen to OUT (ARGS="key=value ...": table row biome season tod glyphs tier pattern grid)
	./edit-snap.sh $(OUT) $(ARGS)

fonts: ## Report fonts covering the Symbols for Legacy Computing block
	@fc-list ':charset=1fb00' family | sort -u

# One-off snapshot: make snap OUT=shot.png ARGS="fill=1 zoom=2 tod=21 fire=1"
snap: build ## Render one headless frame to OUT (ARGS="key=value ...")
	./snap.sh $(OUT) $(ARGS)

# One L-system tree side-on: make tree-snap OUT=oak.png NAME="gnarled oak" ARGS="110 55 7 1"
tree-snap: build ## Render one L-system species to OUT (NAME=species ARGS="cols rows seed season [foliage= state=dead]")
	./tree-snap.sh $(OUT) "$(NAME)" $(ARGS)

screenshots: build ## Render the documentation screenshots into docs/screenshots
	./snap.sh $(SHOTS)/island.png zoom=0 t=3 tod=12
	./snap.sh $(SHOTS)/rotated.png zoom=0 t=3 tod=12 deg=25
	./snap.sh $(SHOTS)/filled.png fill=1 zoom=1 cx=0 cy=0 t=3 tod=12 player=1
	./snap.sh $(SHOTS)/closeup.png fill=1 zoom=3 cx=0 cy=0 t=3 tod=12 player=1
	./snap.sh $(SHOTS)/night.png fill=1 zoom=2 cx=500 cy=-300 t=3 tod=22 fire=1
	./snap.sh $(SHOTS)/winter.png fill=1 zoom=1 cx=500 cy=-300 t=3 tod=12 season=3 simdays=2
	./snap.sh $(SHOTS)/clouds.png fill=1 zoom=0 cx=0 cy=0 t=3 tod=14 cover=0.4
	./snap.sh $(SHOTS)/steppe.png fill=1 zoom=1 cx=-500 cy=400 t=3 tod=12
	./snap.sh $(SHOTS)/worldmap.png fill=1 worldmap=1 scale=2 cx=0 cy=0
	./snap.sh $(SHOTS)/settings.png popover=1 zoom=0
	./snap.sh $(SHOTS)/ascii.png zoom=1 fill=1 cx=0 cy=0 t=3 tod=12 glyphs=ascii
	./snap.sh $(SHOTS)/village.png fill=1 zoom=1 cx=130 cy=-8 t=3 tod=12 player=1
	./snap.sh $(SHOTS)/lsystem-stand-near.png fill=1 zoom=2 cx=500 cy=-300 t=3 tod=12
	./snap.sh $(SHOTS)/lsystem-stand-close.png fill=1 zoom=3 cx=500 cy=-300 t=3 tod=12
	./snap.sh $(SHOTS)/scale-zfar.png scene=scale zoom=0 t=3 tod=12
	./snap.sh $(SHOTS)/scale-zmid.png scene=scale zoom=1 t=3 tod=12
	./snap.sh $(SHOTS)/scale-znear.png scene=scale zoom=2 t=3 tod=12
	./snap.sh $(SHOTS)/scale-zclose.png scene=scale zoom=3 t=3 tod=12
	./edit-snap.sh $(SHOTS)/editor.png table=species row=oak
	./tree-snap.sh $(SHOTS)/lsystem-oak.png "gnarled oak" 110 55 7 1
	./tree-snap.sh $(SHOTS)/lsystem-willow.png "weeping willow" 110 55 7 1
	./tree-snap.sh $(SHOTS)/lsystem-birch.png "young birch" 70 55 7 1
	./tree-snap.sh $(SHOTS)/lsystem-conifer.png "spruce" 62 60 7 1
	./tree-snap.sh $(SHOTS)/lsystem-oak-winter.png "gnarled oak" 110 55 7 3
	./tree-snap.sh $(SHOTS)/lsystem-oak-dead.png "gnarled oak" 110 55 7 1 state=dead
