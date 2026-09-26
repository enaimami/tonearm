# headshell — developer shortcuts.
#
# This file sets no new rule: it makes the commands in CLAUDE.md and the two jobs
# of ci.yml runnable from a single place. The three gates are defined in PLAN.md
# §0.4, the command surface in CLAUDE.md — if they disagree, those win.
#
# Target names and text are both in English (D-036, D-073).

# The version is read from a single source: the workspace Cargo.toml. In an Arch
# version `-` is the pkgrel separator, so `_` is written; the same lesson was learned
# on the MSI side in D-063 — a version written in two places drifts.
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
PKGVER  := $(subst -,_,$(VERSION))

# make cli ARGS="stats --year 2024 --top 10"
ARGS ?=
# make aur-test PKG=headshell-bin
PKG  ?= headshell

# On an Arch machine `makepkg` runs directly. On a machine without it (this file
# was written on Debian) the same job is done in an Arch container. To force it:
# `make aur-test ENGINE=container`.
ENGINE    ?= $(if $(shell command -v makepkg 2>/dev/null),native,container)
CONTAINER ?= podman
IMAGE     ?= docker.io/library/archlinux:base-devel
AUR_DIR   := packaging/aur
BUILD_DIR := target/aur

.DEFAULT_GOAL := help

.PHONY: help gates fmt fmt-check clippy test core-features accuracy snapshots \
        cli desktop diag aur-tarball aur-stage aur-test aur-clean clean

# `LC_ALL=C` is required, not decoration: the `[a-z]` range follows the locale's
# collation order, and in tr_TR `i` falls outside that range. When this line was
# written without a locale, `make help` silently skipped exactly the targets with an
# `i` in their names — `cli`, `clippy`, `diag`.
help: ## Print this list
	@echo "headshell $(VERSION) — targets:"
	@LC_ALL=C grep -hE '^[a-zA-Z0-9_-]+:[^#]*## ' $(MAKEFILE_LIST) | LC_ALL=C sort \
	  | awk 'BEGIN{FS=":[^#]*## "}{printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "Example: make cli ARGS=\"stats --year 2024 --top 10\""

# --- the three gates (PLAN.md §0.4) -----------------------------------------
# The same order as ci.yml: formatting is the cheapest, so it should fail first.

gates: fmt-check clippy test ## The three gates: fmt + clippy + test

fmt: ## Apply the formatting
	cargo fmt --all

fmt-check: ## Check the formatting, do not apply it
	cargo fmt --all --check

clippy: ## Clippy — warnings count as errors
	cargo clippy --workspace --all-targets -- -D warnings

test: ## All the tests
	cargo test --workspace

# In a `--workspace` run headshell-cli and headshell turn on headshell-core's
# features and hide the dead code of the default build (D-054). Mobile (Phase 6)
# will build the core without those features, so it is checked separately —
# that is ci.yml's second job.
core-features: ## Check the core without the features merged
	cargo clippy -p headshell-core --all-targets -- -D warnings
	@for f in audio http-client fingerprint render-png artwork-resize plugin-engine; do \
	  echo "--- feature: $$f"; \
	  cargo clippy -p headshell-core --all-targets --features "$$f" -- -D warnings || exit 1; \
	done
	cargo clippy -p headshell-core --all-targets \
	  --features audio,http-client,fingerprint,render-png,artwork-resize,plugin-engine -- -D warnings

# --- individual checks ------------------------------------------------------

accuracy: ## The identity accuracy set — the project's most important number
	cargo test -p headshell-core --test identity_accuracy -- --nocapture

snapshots: ## The CLI --json snapshot tests
	cargo test -p headshell-cli --test cli_json

# --- running ----------------------------------------------------------------

cli: ## Run the CLI (ARGS="...")
	cargo run -p headshell-cli -- $(ARGS)

desktop: ## Open the desktop window
	cargo run -p headshell

diag: ## The last run's diagnostics report
	@cargo run -q -p headshell-cli -- diag

# --- AUR ---------------------------------------------------------------------
# Details: packaging/aur/README.md.
#
# The targets here are for testing an **unpublished** tag: the PKGBUILD's
# `source=` points at the tag, while what you want to test is your working tree.
# So a tag-like archive is produced from the tree, and `updpkgsums` refreshes the
# checksums for it in the container. The PKGBUILD in the repository is not touched
# — it keeps carrying the real tag's checksums.

aur-tarball: ## Build a tag-like source archive from the working tree
	@mkdir -p $(BUILD_DIR)
	@git ls-files --cached --others --exclude-standard | sort -u > $(BUILD_DIR)/filelist
	@tar czf $(BUILD_DIR)/headshell-$(PKGVER).tar.gz \
	  --transform 's|^|headshell-$(VERSION)/|' -T $(BUILD_DIR)/filelist
	@echo "$(BUILD_DIR)/headshell-$(PKGVER).tar.gz — $$(wc -l < $(BUILD_DIR)/filelist) files"

# The PKGBUILDs are **not** at the repository root but under `$(AUR_DIR)/<package>/`
# — run `makepkg` at the root and it says "PKGBUILD does not exist". The staging is
# always the same: set the package up under `$(BUILD_DIR)/<package>/` and put the
# sources next to it.
aur-stage: aur-tarball
	@test -d $(AUR_DIR)/$(PKG) || { \
	  echo "STEP: AUR_STAGE — no such package: $(AUR_DIR)/$(PKG)"; \
	  echo "options: $$(ls $(AUR_DIR) | grep -v README | tr '\n' ' ')"; exit 1; }
	@rm -rf $(BUILD_DIR)/$(PKG) 2>/dev/null \
	  || { command -v $(CONTAINER) >/dev/null && $(CONTAINER) unshare rm -rf $(BUILD_DIR)/$(PKG); }
	@mkdir -p $(BUILD_DIR)/$(PKG)
	@cp $(AUR_DIR)/$(PKG)/PKGBUILD $(BUILD_DIR)/$(PKG)/
	@cp $(BUILD_DIR)/headshell-$(PKGVER).tar.gz $(BUILD_DIR)/$(PKG)/
	@case "$(PKG)" in \
	  headshell-bin) \
	    gh release download v$(VERSION) --clobber -D $(BUILD_DIR)/$(PKG) \
	      -p 'headshell_$(VERSION)_amd64.deb' ;; \
	  headshell-cli-bin) \
	    gh release download v$(VERSION) --clobber -D $(BUILD_DIR)/$(PKG) \
	      -p 'headshell-cli-linux-x86_64.tar.gz' && \
	    mv $(BUILD_DIR)/$(PKG)/headshell-cli-linux-x86_64.tar.gz \
	       $(BUILD_DIR)/$(PKG)/headshell-cli-$(PKGVER).tar.gz ;; \
	esac

aur-test: aur-stage ## Build an AUR package and run namcap on it (PKG=...)
ifeq ($(ENGINE),native)
	@command -v updpkgsums >/dev/null || { \
	  echo 'STEP: AUR_TEST — no `updpkgsums`: pacman -S pacman-contrib'; exit 1; }
	@command -v namcap >/dev/null || { \
	  echo 'STEP: AUR_TEST — no `namcap`: pacman -S namcap'; exit 1; }
	cd $(BUILD_DIR)/$(PKG) && updpkgsums \
	  && makepkg --printsrcinfo > .SRCINFO \
	  && makepkg -sf --noconfirm
	@echo; echo "=== namcap: PKGBUILD (clean if empty) ==="
	@namcap $(BUILD_DIR)/$(PKG)/PKGBUILD
	@echo "=== namcap: packages ==="
	@for p in $(BUILD_DIR)/$(PKG)/*.pkg.tar.zst; do namcap "$$p"; done
	@echo "=== contents ==="
	@for p in $(BUILD_DIR)/$(PKG)/*.pkg.tar.zst; do \
	  echo "-- $$(basename $$p)"; bsdtar tf "$$p" | grep -v "^\." | grep -v "/$$"; \
	done
else
	@command -v $(CONTAINER) >/dev/null || { \
	  echo 'STEP: AUR_TEST — neither `makepkg` nor `$(CONTAINER)` is available.'; \
	  echo "on Arch: pacman -S pacman-contrib namcap   —   elsewhere: install podman"; \
	  exit 1; }
	$(CONTAINER) run --rm -v "$(CURDIR)/$(BUILD_DIR)/$(PKG):/build:z" $(IMAGE) bash -c '\
	  set -e; \
	  extra=""; \
	  if [ "$(PKG)" = headshell ]; then extra="rust pkgconf webkit2gtk-4.1 gtk3 alsa-lib"; fi; \
	  pacman -Syu --noconfirm --needed namcap pacman-contrib $$extra >/dev/null 2>&1; \
	  useradd -m builder; chown -R builder /build; \
	  su builder -c "cd /build && updpkgsums && makepkg --printsrcinfo > .SRCINFO && makepkg --noconfirm --nodeps"; \
	  echo; echo "=== namcap: PKGBUILD (clean if empty) ==="; namcap /build/PKGBUILD; \
	  echo "=== namcap: packages ==="; for p in /build/*.pkg.tar.zst; do namcap "$$p"; done; \
	  echo "=== contents ==="; \
	  for p in /build/*.pkg.tar.zst; do \
	    echo "-- $$(basename $$p)"; bsdtar tf "$$p" | grep -v "^\." | grep -v "/$$"; \
	  done'
endif

# The files `fakeroot` leaves behind are owned by root on the host; a plain `rm`
# cannot reach them, `unshare` deletes them from inside the user namespace.
aur-clean: ## Delete the AUR build leftovers
	@rm -rf $(BUILD_DIR) 2>/dev/null \
	  || { command -v $(CONTAINER) >/dev/null && $(CONTAINER) unshare rm -rf $(BUILD_DIR); }

clean: aur-clean ## cargo clean + the AUR leftovers
	cargo clean
