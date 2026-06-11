{
  description = "lore dev environment — Rust + Dioxus + tooling, pinned via flake.lock";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    # nixpkgs' rustc can't add extra targets; rust-overlay gives us a
    # toolchain with wasm32-unknown-unknown for the Dioxus web build.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    # Stealth Chromium with source-level fingerprint patches, used by
    # lore-worker / lore-e2e via CDP. Linux x86_64 + aarch64 only — its
    # own pin, so we don't `follows` it.
    cloakbrowser.url = "github:CloakHQ/CloakBrowser";
  };

  outputs = { self, nixpkgs, rust-overlay, cloakbrowser }:
    let
      systems = [ "aarch64-linux" "x86_64-linux" "aarch64-darwin" ];

      # --- wasm-bindgen-cli pin --------------------------------------------
      # `dx` refuses a wasm-bindgen CLI whose version differs from the
      # `wasm-bindgen` crate in /Cargo.lock, and on NixOS it can't run the
      # prebuilt binary it would otherwise download — so we build the matching
      # CLI from source. These three values are the ONLY thing that must track
      # Cargo.lock; `dev-env/update-wasm-bindgen.sh` (→ `make update-deps`)
      # rewrites them automatically, so don't hand-edit. The hashes are
      # fixed-output, hence version-specific — they can't be elided.
      wasmBindgenVersion = "0.2.118";
      wasmBindgenSrcHash = "sha256-ve783oYH0TGv8Z8lIPdGjItzeLDQLOT5uv/jbFOlZpI=";
      wasmBindgenCargoHash = "sha256-EYDfuBlH3zmTxACBL+sjicRna84CvoesKSQVcYiG9P0=";
      # Factored so the update script can build the two fixed-output pieces
      # in isolation (no full CLI compile): the src tarball and the vendored
      # cargo deps. The src hash is also obtainable via `nix-prefetch-url
      # --unpack`; the vendor hash is a derived FOD with no prefetch URL, so it
      # must be realized to learn its hash.
      mkWasmBindgenSrc = pkgs: pkgs.fetchCrate {
        pname = "wasm-bindgen-cli";
        version = wasmBindgenVersion;
        hash = wasmBindgenSrcHash;
      };
      mkWasmBindgenCargoDeps = pkgs: pkgs.rustPlatform.fetchCargoVendor {
        src = mkWasmBindgenSrc pkgs;
        pname = "wasm-bindgen-cli";
        version = wasmBindgenVersion;
        hash = wasmBindgenCargoHash;
      };
      mkWasmBindgenCli = pkgs: pkgs.buildWasmBindgenCli {
        src = mkWasmBindgenSrc pkgs;
        cargoDeps = mkWasmBindgenCargoDeps pkgs;
      };
      # ---------------------------------------------------------------------

      forAllSystems = f:
        nixpkgs.lib.genAttrs systems (system:
          f (import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          }) system);
    in
    {
      devShells = forAllSystems (pkgs: system:
        let
          lib = pkgs.lib;

          # CloakBrowser only ships Linux binaries; its flake has no
          # darwin outputs, so guard the reference.
          cloak =
            if pkgs.stdenv.isLinux
            then cloakbrowser.packages.${system}.cloakbrowserChromium
            else null;
          cloakExe = "${cloak}/bin/cloakbrowser-chrome";

          rust = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" "rust-analyzer" ];
            targets = [
              "wasm32-unknown-unknown" # dx web bundle
            ] ++ lib.optionals pkgs.stdenv.isLinux [
              # Cross targets for the headless crates (cli/server/worker).
              # lore-ui (GTK/WebView) is macOS-native and not crossed.
              "x86_64-unknown-linux-gnu"
              "x86_64-pc-windows-gnu"
            ];
          };

          # Cross C toolchains for cc-rs deps (bundled SQLite, ring) +
          # rustc's linker, keyed per target via the env vars in shellHook.
          # Linux-host only; from aarch64-darwin you build the macOS app
          # natively, not cross.
          crossCc = lib.optionals pkgs.stdenv.isLinux [
            pkgs.pkgsCross.gnu64.stdenv.cc      # → x86_64-linux
            pkgs.pkgsCross.mingwW64.stdenv.cc   # → x86_64-windows (gnu ABI)
          ];

          # rust's windows-gnu std links `-l:libpthread.a` (static
          # winpthreads), which lives outside the gcc wrapper's default
          # search path — point the linker at it.
          mingwPthreads = pkgs.pkgsCross.mingwW64.windows.pthreads;

          wasm-bindgen-cli = mkWasmBindgenCli pkgs;

          # Dioxus desktop on Linux = wry/tao on GTK3 + WebKitGTK.
          linuxGui = lib.optionals pkgs.stdenv.isLinux [
            pkgs.gtk3
            pkgs.webkitgtk_4_1
            pkgs.libsoup_3
            pkgs.glib
            pkgs.cairo
            pkgs.pango
            pkgs.gdk-pixbuf
            pkgs.atk
            pkgs.xdotool
            pkgs.glib-networking # TLS for WebKit
            pkgs.gsettings-desktop-schemas
          ];

          # lore-worker / lore-e2e drive headless Chromium over CDP. On
          # Linux we ship CloakBrowser; on darwin the code falls back to
          # /Applications/Chromium.app.
          browser = lib.optionals pkgs.stdenv.isLinux [ cloak ];

          # Base shell: everything for day-to-day dev + the nix CI jobs
          # (clippy/test/web). Deliberately excludes the cross C toolchains —
          # those build mingw/gnu gcc from source (no binary cache for them on
          # this host), so keeping them out makes `nix develop` fast to enter.
          # Cross builds use the `cross` shell below.
          basePackages = [
            rust
            pkgs.dioxus-cli
            wasm-bindgen-cli
            pkgs.binaryen # wasm-opt for dx release builds
            pkgs.cargo-deny
            pkgs.cargo-mutants
            pkgs.nodejs_22 # milkdown.js bundle (make js-build)
            pkgs.gnumake
            pkgs.sqlite # sqlite3 CLI for poking at db.sqlite (rusqlite is bundled)
          ] ++ browser;

          baseShellHook = ''
            export NIX_ENV=1
          '' + lib.optionalString pkgs.stdenv.isLinux ''
            # lore-worker (LORE_BROWSER) + lore-e2e: pinned CloakBrowser
            # instead of a PATH lookup.
            export LORE_BROWSER=${cloakExe}

            # GTK apps outside a NixOS-managed session need schemas + TLS
            # modules wired up by hand. GSETTINGS_SCHEMAS_PATH comes from
            # wrapGAppsHook3.
            export XDG_DATA_DIRS="$GSETTINGS_SCHEMAS_PATH''${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}"
            export GIO_EXTRA_MODULES=${pkgs.glib-networking}/lib/gio/modules

            # WebKitGTK's DMA-BUF renderer shows a blank window in VMs
            # without GPU passthrough.
            export WEBKIT_DISABLE_DMABUF_RENDERER=1
          '';

          # Extra env for the `cross` shell: rustc linker + cc-rs (bundled
          # SQLite, ring) per target. Headless crates only — lore-ui/lore-e2e
          # won't cross-build, and lore-worker stays Linux-only.
          crossShellHook = lib.optionalString pkgs.stdenv.isLinux ''
            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-unknown-linux-gnu-cc
            export CC_x86_64_unknown_linux_gnu=x86_64-unknown-linux-gnu-cc
            export CXX_x86_64_unknown_linux_gnu=x86_64-unknown-linux-gnu-c++
            export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-cc
            export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-cc
            export CXX_x86_64_pc_windows_gnu=x86_64-w64-mingw32-c++
            export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-L native=${mingwPthreads}/lib"
          '';

          commonInputs = {
            nativeBuildInputs = [
              pkgs.pkg-config
            ] ++ lib.optionals pkgs.stdenv.isLinux [ pkgs.wrapGAppsHook3 ];
            buildInputs = [ pkgs.openssl ] ++ linuxGui;
          };
        in
        {
          default = pkgs.mkShell (commonInputs // {
            packages = basePackages;
            shellHook = baseShellHook;
          });

          # `nix develop ./dev-env#cross` — adds the cross gcc toolchains and
          # per-target linker env. Used by `make cross` / `make cross-*`.
          cross = pkgs.mkShell (commonInputs // {
            packages = basePackages ++ crossCc;
            shellHook = baseShellHook + crossShellHook;
          });
        });

      # Compatibility shim for the `nix build .#wrapper` workflow:
      # `dev-env/result/bin/wrapper [cmd...]` enters the dev shell above
      # (interactively with no args, otherwise runs the command). The flake
      # source + lock are baked in via ${self}, so it's fully pinned.
      packages = forAllSystems (pkgs: system: rec {
        wrapper = pkgs.writeShellApplication {
          name = "wrapper";
          text = ''
            if [ "$#" -eq 0 ]; then
              exec nix --extra-experimental-features "nix-command flakes" \
                develop "path:${self}"
            else
              exec nix --extra-experimental-features "nix-command flakes" \
                develop "path:${self}" --command "$@"
            fi
          '';
        };
        default = wrapper;

        # Exposed for `update-wasm-bindgen.sh`: it builds the cargo-deps FOD
        # alone to learn the vendor hash (no full CLI compile needed).
        wasm-bindgen-cli = mkWasmBindgenCli pkgs;
        wasm-bindgen-cargo-deps = mkWasmBindgenCargoDeps pkgs;
      });
    };
}
