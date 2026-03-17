{
  description = "Handy - A free, open source, and extensible speech-to-text application that works completely offline";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    # bun2nix: generates per-package Nix fetchurl expressions from bun.lock,
    # replacing the old FOD approach where a single hash covered the entire
    # node_modules directory (that hash would break on bun version changes).
    # See: https://github.com/nix-community/bun2nix
    bun2nix = {
      url = "github:nix-community/bun2nix/2.0.8";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      bun2nix,
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      # Read version from Cargo.toml
      cargoToml = fromTOML (builtins.readFile ./src-tauri/Cargo.toml);
      version = cargoToml.package.version;

      # Shared native library dependencies for both package build and dev shell.
      # Keep in sync: if a native dep is needed for compilation, add it here.
      commonNativeDeps = pkgs: with pkgs; [
        webkitgtk_4_1
        gtk3
        glib
        libsoup_3
        alsa-lib
        onnxruntime
        libayatana-appindicator
        libevdev
        libxtst
        gtk-layer-shell
        openssl
        vulkan-loader
        vulkan-headers
        shaderc
      ];

      # GStreamer plugins for WebKitGTK audio/video
      gstPlugins = pkgs: with pkgs.gst_all_1; [
        gstreamer
        gst-plugins-base
        gst-plugins-good
        gst-plugins-bad
        gst-plugins-ugly
      ];

      # Shared environment variables for Rust/native builds
      commonEnv = pkgs: let lib = pkgs.lib; in {
        LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.llvmPackages.libclang.lib}/lib/clang/${lib.getVersion pkgs.llvmPackages.libclang}/include -isystem ${pkgs.glibc.dev}/include";
        ORT_LIB_LOCATION = "${pkgs.onnxruntime}/lib";
        ORT_PREFER_DYNAMIC_LINK = "1";
        GST_PLUGIN_SYSTEM_PATH_1_0 = "${lib.makeSearchPathOutput "lib" "lib/gstreamer-1.0" (gstPlugins pkgs)}";
      };

      # TODO: Remove this overlay once nixpkgs ships onnxruntime ≥ 1.24.
      # Tracking PR: https://github.com/NixOS/nixpkgs/pull/499389
      # ort-sys 2.0.0-rc.12 requires ONNX Runtime 1.24 (API v24);
      # nixpkgs only ships 1.23.2, so use MS prebuilt binaries.
      onnxruntimeOverlay = (final: prev: {
        onnxruntime = let
          onnxVersion = "1.24.2";
          platform = {
            x86_64-linux = { name = "linux-x64"; hash = "sha256-Q3JUdLpWY2QuF2hHF5Rmk4UOIAXvvXJKxy2ieP6tJeY="; };
            aarch64-linux = { name = "linux-aarch64"; hash = "sha256-spla8PQ3xOAi/YAcV/tcJf0f5mDNM9JutHGUSQpbRsQ="; };
          }.${final.system};
        in prev.stdenv.mkDerivation {
          pname = "onnxruntime";
          version = onnxVersion;
          src = prev.fetchurl {
            url = "https://github.com/microsoft/onnxruntime/releases/download/v${onnxVersion}/onnxruntime-${platform.name}-${onnxVersion}.tgz";
            hash = platform.hash;
          };
          sourceRoot = "onnxruntime-${platform.name}-${onnxVersion}";
          nativeBuildInputs = [ prev.autoPatchelfHook ];
          buildInputs = [ prev.stdenv.cc.cc.lib ];
          installPhase = ''
            runHook preInstall
            mkdir -p $out/lib $out/include
            cp -r lib/* $out/lib/
            cp -r include/* $out/include/
            runHook postInstall
          '';
          meta = prev.onnxruntime.meta // {
            description = "ONNX Runtime ${onnxVersion} (prebuilt by Microsoft)";
          };
        };
      });
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [
              bun2nix.overlays.default
              onnxruntimeOverlay
            ];
          };
          lib = pkgs.lib;
        in
        {
          handy = pkgs.rustPlatform.buildRustPackage {
            pname = "handy";
            inherit version;
            src = self;

            cargoRoot = "src-tauri";

            cargoLock = {
              lockFile = ./src-tauri/Cargo.lock;
              # Automatically fetch git dependencies using builtins.fetchGit.
              # This eliminates the need for manual outputHashes that had to be
              # updated every time a git dependency changed in Cargo.lock.
              # Safe for standalone flakes (not allowed in nixpkgs, it is needed something like crate2nix).
              allowBuiltinFetchGit = true;
            };

            postPatch = ''
              ${pkgs.jq}/bin/jq 'del(.build.beforeBuildCommand) | .bundle.createUpdaterArtifacts = false' \
                src-tauri/tauri.conf.json > $TMPDIR/tauri.conf.json
              cp $TMPDIR/tauri.conf.json src-tauri/tauri.conf.json

              # Strip postinstall hook — it runs check-nix-deps.ts which is only
              # needed during local development, not inside the Nix sandbox.
              ${pkgs.jq}/bin/jq 'del(.scripts.postinstall)' \
                package.json > $TMPDIR/package.json
              cp $TMPDIR/package.json package.json

              # Point libappindicator-sys to the Nix store path
              substituteInPlace \
                $cargoDepsCopy/libappindicator-sys-*/src/lib.rs \
                --replace-fail \
                  "libayatana-appindicator3.so.1" \
                  "${pkgs.libayatana-appindicator}/lib/libayatana-appindicator3.so.1"

              # Disable cbindgen in ferrous-opencc (calls cargo metadata which fails in sandbox)
              # Upstream removed this call in v0.3.1+
              substituteInPlace $cargoDepsCopy/ferrous-opencc-0.2.3/build.rs \
                --replace-fail '.expect("Unable to generate bindings")' '.ok();'
              substituteInPlace $cargoDepsCopy/ferrous-opencc-0.2.3/build.rs \
                --replace-fail '.write_to_file("opencc.h");' '// skipped'
            '';

            # Bun dependencies: fetched per-package using hashes from .nix/bun.nix.
            # This file is auto-generated by `bunx bun2nix -o .nix/bun.nix` and
            # kept in sync via the postinstall hook in package.json.
            # To regenerate manually: bun scripts/check-nix-deps.ts
            bunDeps = pkgs.bun2nix.fetchBunDeps {
              bunNix = ./.nix/bun.nix;
            };

            nativeBuildInputs = with pkgs; [
              cargo-tauri.hook
              pkg-config
              wrapGAppsHook4
              bun
              # pkgs.bun2nix (from overlay), not the flake input — `with pkgs;`
              # doesn't shadow function arguments in Nix.
              pkgs.bun2nix.hook # Sets up node_modules from pre-fetched bun cache
              jq
              cmake
              llvmPackages.libclang
              shaderc
            ];

            preBuild = ''
              # bun2nix.hook has already set up node_modules from pre-fetched cache.
              # Build the frontend with bun (tsc + vite).
              export HOME=$TMPDIR
              bun run build
            '';

            # Tests require runtime resources (audio devices, model files, GPU/Vulkan)
            # not available in the Nix build sandbox
            doCheck = false;

            # The tauri hook's installPhase expects target/ in cwd, but our
            # cargoRoot puts it under src-tauri/. Override to extract the DEB.
            installPhase = ''
              runHook preInstall
              mkdir -p $out
              cd src-tauri
              mv target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/bundle/deb/*/data/usr/* $out/
              runHook postInstall
            '';

            buildInputs = commonNativeDeps pkgs ++ (with pkgs; [
              glib-networking
              libx11
            ]) ++ gstPlugins pkgs;

            env = commonEnv pkgs // {
              OPENSSL_NO_VENDOR = "1";
            };

            preFixup = ''
              gappsWrapperArgs+=(
                --set WEBKIT_DISABLE_DMABUF_RENDERER 1
                --set ALSA_PLUGIN_DIR "${pkgs.pipewire}/lib/alsa-lib:${pkgs.alsa-plugins}/lib/alsa-lib"
                --prefix LD_LIBRARY_PATH : "${
                  lib.makeLibraryPath [
                    pkgs.vulkan-loader
                    pkgs.onnxruntime
                  ]
                }"
              )
            '';

            meta = {
              description = "A free, open source, and extensible speech-to-text application that works completely offline";
              homepage = "https://github.com/cjpais/Handy";
              license = lib.licenses.mit;
              mainProgram = "handy";
              platforms = supportedSystems;
            };
          };

          default = self.packages.${system}.handy;
        }
      );

      # NixOS module for system-level integration (udev, input group)
      nixosModules.default =
        { lib, pkgs, ... }:
        {
          imports = [ ./nix/module.nix ];
          programs.handy.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.handy;
        };

      # Home-manager module for per-user service
      homeManagerModules.default =
        { lib, pkgs, ... }:
        {
          imports = [ ./nix/hm-module.nix ];
          services.handy.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.handy;
        };

      # Development shell for building from source
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ onnxruntimeOverlay ];
          };
        in
        {
          default = pkgs.mkShell {
            buildInputs = commonNativeDeps pkgs ++ (with pkgs; [
              # Rust toolchain
              rustc
              cargo
              rust-analyzer
              clippy
              # Frontend
              nodejs
              bun
              # Build tools
              cargo-tauri
              pkg-config
              llvmPackages.libclang
              cmake
            ]);

            inherit (commonEnv pkgs)
              LIBCLANG_PATH
              BINDGEN_EXTRA_CLANG_ARGS
              ORT_LIB_LOCATION
              ORT_PREFER_DYNAMIC_LINK
              GST_PLUGIN_SYSTEM_PATH_1_0;

            LD_LIBRARY_PATH = "${pkgs.lib.makeLibraryPath [ pkgs.libayatana-appindicator pkgs.onnxruntime pkgs.vulkan-loader ]}";

            # Same as wrapGAppsHook4
            XDG_DATA_DIRS = "${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:${pkgs.hicolor-icon-theme}/share";

            shellHook = ''
              echo "Handy development environment"
              bun install
              echo "Run 'bun run tauri dev' to start"
            '';
          };
        }
      );
    };
}
