# tui-map build targets
# PGO (Profile-Guided Optimization) requires: rustup component add llvm-tools-preview

TARGET := $(shell rustc -vV | grep host | cut -d' ' -f2)
PGO_DIR := /tmp/tui-map-pgo
LLVM_PROFDATA := $(shell rustup run stable which llvm-profdata 2>/dev/null || \
	find $$(rustup run stable rustc --print sysroot) -name llvm-profdata 2>/dev/null | head -1)

# Standard release build with target-cpu=native
release:
	RUSTFLAGS="-Ctarget-cpu=native" cargo build --release --target=$(TARGET)
	@echo "Binary: target/$(TARGET)/release/tui-map"

# Step 1: Build instrumented binary for PGO profiling
pgo-instrument:
	@rm -rf $(PGO_DIR)
	RUSTFLAGS="-Cprofile-generate=$(PGO_DIR) -Ctarget-cpu=native" \
		cargo build --release --target=$(TARGET)
	@echo ""
	@echo "Instrumented binary built. Now run it for ~60 seconds:"
	@echo "  ./target/$(TARGET)/release/tui-map"
	@echo ""
	@echo "Exercise: pan, zoom, launch weapons, let fires spread."
	@echo "Then run: make pgo-optimize"

# Step 2: Merge profiles and build optimized binary
pgo-optimize:
	@if [ -z "$(LLVM_PROFDATA)" ]; then \
		echo "llvm-profdata not found. Run: rustup component add llvm-tools-preview"; \
		exit 1; \
	fi
	$(LLVM_PROFDATA) merge -o $(PGO_DIR)/merged.profdata $(PGO_DIR)
	RUSTFLAGS="-Cprofile-use=$(PGO_DIR)/merged.profdata -Ctarget-cpu=native" \
		cargo build --release --target=$(TARGET)
	@echo ""
	@echo "PGO-optimized binary: target/$(TARGET)/release/tui-map"

# Full PGO pipeline (interactive — pauses for profiling session)
pgo: pgo-instrument
	@echo "Press Enter after your profiling session..."
	@read _
	$(MAKE) pgo-optimize

clean:
	cargo clean
	rm -rf $(PGO_DIR)

.PHONY: release pgo-instrument pgo-optimize pgo clean
