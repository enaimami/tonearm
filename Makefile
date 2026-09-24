# headshell — geliştirici kısayolları.
#
# Bu dosya yeni bir kural koymuyor: CLAUDE.md'deki komutları ve ci.yml'nin iki
# işini tek yerden koşulur hâle getiriyor. Üç kapının tanımı PLAN.md §0.4'te,
# komut yüzeyi CLAUDE.md'de — çelişirse onlar geçerli.
#
# Hedef adları İngilizce, yazı Türkçe (D-036).

# Sürüm tek kaynaktan okunuyor: workspace Cargo.toml. Arch sürümünde `-`
# pkgrel ayıracı olduğu için `_` yazılır; aynı ders D-063'te MSI tarafında
# öğrenilmişti — iki yere yazılan sürüm kayar.
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
PKGVER  := $(subst -,_,$(VERSION))

# make cli ARGS="stats --year 2024 --top 10"
ARGS ?=
# make aur-test PKG=headshell-bin
PKG  ?= headshell

# Arch makinesinde `makepkg` doğrudan koşar. Olmayan bir makinede (bu dosya
# Debian'da yazıldı) aynı iş bir Arch konteynerinde yapılır. Zorlamak için:
# `make aur-test ENGINE=container`.
ENGINE    ?= $(if $(shell command -v makepkg 2>/dev/null),native,container)
CONTAINER ?= podman
IMAGE     ?= docker.io/library/archlinux:base-devel
AUR_DIR   := packaging/aur
BUILD_DIR := target/aur

.DEFAULT_GOAL := help

.PHONY: help gates fmt fmt-check clippy test core-features accuracy snapshots \
        cli desktop diag aur-tarball aur-stage aur-test aur-clean clean

# `LC_ALL=C` şart, süs değil: `[a-z]` aralığı yerele göre harmanlama düzenini
# kullanıyor ve tr_TR'de `i` o aralığın dışında kalıyor. Bu satır yerelsiz
# yazıldığında `make help` tam olarak adında `i` geçen hedefleri — `cli`,
# `clippy`, `diag` — sessizce atlıyordu.
help: ## Bu listeyi bas
	@echo "headshell $(VERSION) — hedefler:"
	@LC_ALL=C grep -hE '^[a-zA-Z0-9_-]+:[^#]*## ' $(MAKEFILE_LIST) | LC_ALL=C sort \
	  | awk 'BEGIN{FS=":[^#]*## "}{printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "Örnek: make cli ARGS=\"stats --year 2024 --top 10\""

# --- üç kapı (PLAN.md §0.4) --------------------------------------------------
# Sıra ci.yml ile aynı: biçim en ucuzu, en önce düşsün.

gates: fmt-check clippy test ## Üç kapı: fmt + clippy + test

fmt: ## Biçimi uygula
	cargo fmt --all

fmt-check: ## Biçimi denetle, uygulama
	cargo fmt --all --check

clippy: ## Clippy — uyarılar hata sayılır
	cargo clippy --workspace --all-targets -- -D warnings

test: ## Bütün testler
	cargo test --workspace

# `--workspace` koşumunda headshell-cli ve headshell, headshell-core'un
# feature'larını açıyor ve varsayılan derlemedeki ölü kodu gizliyor (D-054).
# Mobil (Faz 6) çekirdeği o feature'lar olmadan derleyecek, o yüzden ayrı
# bakılır — ci.yml'nin ikinci işi budur.
core-features: ## Çekirdeği feature'lar birleşmeden denetle
	cargo clippy -p headshell-core --all-targets -- -D warnings
	@for f in audio http-client fingerprint render-png plugin-engine; do \
	  echo "--- feature: $$f"; \
	  cargo clippy -p headshell-core --all-targets --features "$$f" -- -D warnings || exit 1; \
	done
	cargo clippy -p headshell-core --all-targets \
	  --features audio,http-client,fingerprint,render-png,plugin-engine -- -D warnings

# --- tek tek sınamalar -------------------------------------------------------

accuracy: ## Kimlik doğruluk kümesi — projenin en önemli sayısı
	cargo test -p headshell-core --test identity_accuracy -- --nocapture

snapshots: ## CLI --json snapshot testleri
	cargo test -p headshell-cli --test cli_json

# --- koşturma ----------------------------------------------------------------

cli: ## CLI'yi koştur (ARGS="...")
	cargo run -p headshell-cli -- $(ARGS)

desktop: ## Masaüstü penceresini aç
	cargo run -p headshell

diag: ## Son çalıştırmanın tanı raporu
	@cargo run -q -p headshell-cli -- diag

# --- AUR ---------------------------------------------------------------------
# Ayrıntı: packaging/aur/README.md.
#
# Buradaki hedefler **yayımlanmamış** bir etiketi sınamak için: PKGBUILD'in
# `source=`'u etikete bakıyor, oysa sınamak istediğin şey çalışma ağacın. Bu
# yüzden ağaçtan etiket-eşi bir arşiv üretiliyor ve konteynerde `updpkgsums`
# toplamları ona göre tazeliyor. Depodaki PKGBUILD'e dokunulmuyor — o gerçek
# etiketin toplamlarını taşımaya devam ediyor.

aur-tarball: ## Çalışma ağacından etiket-eşi kaynak arşivi üret
	@mkdir -p $(BUILD_DIR)
	@git ls-files --cached --others --exclude-standard | sort -u > $(BUILD_DIR)/filelist
	@tar czf $(BUILD_DIR)/headshell-$(PKGVER).tar.gz \
	  --transform 's|^|headshell-$(VERSION)/|' -T $(BUILD_DIR)/filelist
	@echo "$(BUILD_DIR)/headshell-$(PKGVER).tar.gz — $$(wc -l < $(BUILD_DIR)/filelist) dosya"

# PKGBUILD'ler depo kökünde **değil**, `$(AUR_DIR)/<paket>/` altında — kökte
# `makepkg` koşarsan "PKGBUILD mevcut değil" dersin. Hazırlık hep aynı:
# paketi `$(BUILD_DIR)/<paket>/` altına kur, kaynakları yanına koy.
aur-stage: aur-tarball
	@test -d $(AUR_DIR)/$(PKG) || { \
	  echo "ADIM: AUR_STAGE — böyle bir paket yok: $(AUR_DIR)/$(PKG)"; \
	  echo "seçenekler: $$(ls $(AUR_DIR) | grep -v README | tr '\n' ' ')"; exit 1; }
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

aur-test: aur-stage ## Bir AUR paketini derle ve namcap'le (PKG=...)
ifeq ($(ENGINE),native)
	@command -v updpkgsums >/dev/null || { \
	  echo 'ADIM: AUR_TEST — `updpkgsums` yok: pacman -S pacman-contrib'; exit 1; }
	@command -v namcap >/dev/null || { \
	  echo 'ADIM: AUR_TEST — `namcap` yok: pacman -S namcap'; exit 1; }
	cd $(BUILD_DIR)/$(PKG) && updpkgsums \
	  && makepkg --printsrcinfo > .SRCINFO \
	  && makepkg -sf --noconfirm
	@echo; echo "=== namcap: PKGBUILD (boşsa temiz) ==="
	@namcap $(BUILD_DIR)/$(PKG)/PKGBUILD
	@echo "=== namcap: paketler ==="
	@for p in $(BUILD_DIR)/$(PKG)/*.pkg.tar.zst; do namcap "$$p"; done
	@echo "=== içerik ==="
	@for p in $(BUILD_DIR)/$(PKG)/*.pkg.tar.zst; do \
	  echo "-- $$(basename $$p)"; bsdtar tf "$$p" | grep -v "^\." | grep -v "/$$"; \
	done
else
	@command -v $(CONTAINER) >/dev/null || { \
	  echo 'ADIM: AUR_TEST — ne `makepkg` ne `$(CONTAINER)` var.'; \
	  echo "Arch'ta: pacman -S pacman-contrib namcap   —   başka yerde: podman kur"; \
	  exit 1; }
	$(CONTAINER) run --rm -v "$(CURDIR)/$(BUILD_DIR)/$(PKG):/build:z" $(IMAGE) bash -c '\
	  set -e; \
	  extra=""; \
	  if [ "$(PKG)" = headshell ]; then extra="rust pkgconf webkit2gtk-4.1 gtk3 alsa-lib"; fi; \
	  pacman -Syu --noconfirm --needed namcap pacman-contrib $$extra >/dev/null 2>&1; \
	  useradd -m builder; chown -R builder /build; \
	  su builder -c "cd /build && updpkgsums && makepkg --printsrcinfo > .SRCINFO && makepkg --noconfirm --nodeps"; \
	  echo; echo "=== namcap: PKGBUILD (boşsa temiz) ==="; namcap /build/PKGBUILD; \
	  echo "=== namcap: paketler ==="; for p in /build/*.pkg.tar.zst; do namcap "$$p"; done; \
	  echo "=== içerik ==="; \
	  for p in /build/*.pkg.tar.zst; do \
	    echo "-- $$(basename $$p)"; bsdtar tf "$$p" | grep -v "^\." | grep -v "/$$"; \
	  done'
endif

# `fakeroot`un bıraktığı dosyalar ana makinede root'a ait olur; düz `rm` onlara
# yetişemez, `unshare` kullanıcı ad alanının içinden siler.
aur-clean: ## AUR derleme artıklarını sil
	@rm -rf $(BUILD_DIR) 2>/dev/null \
	  || { command -v $(CONTAINER) >/dev/null && $(CONTAINER) unshare rm -rf $(BUILD_DIR); }

clean: aur-clean ## cargo clean + AUR artıkları
	cargo clean
